//! Maps from the catalog (CurseForge's worlds, D-066 stage 4): a map's zip
//! becomes a new world in an installation's `saves/`.
//!
//! - The world is the folder with `level.dat` at the top of the zip or one
//!   folder down; the shallowest one when there are several.
//! - Every entry under it is checked like a pack's overrides: plain names
//!   safe on every system, no links, at most `MAX_ENTRIES` entries and
//!   `MAX_UNPACKED` bytes in all (a zip bomb stops there).
//! - It is unpacked into a hidden folder beside the others and renamed into
//!   place, under a name no world has yet, so nothing of the player's is
//!   ever overwritten. A map is not recorded in the content lock: once
//!   played, it is the player's world.

use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use serde::Serialize;

use super::content::{Installer, Progress, Sink, plain_name};
use super::{Catalog, our_file};
use crate::download::{CancelToken, Digest, DownloadRequest, Executor, ExecutorConfig};
use crate::error::{ErrorCode, LauncherError, LauncherResult};
use crate::storage::paths::{DataRoot, InstanceId, Sha256, is_within};

/// Entries of a map's zip looked at.
const MAX_ENTRIES: usize = 200_000;
/// A map unpacked, in all.
const MAX_UNPACKED: u64 = 16 * 1024 * 1024 * 1024;
/// A map's zip.
const MAX_ARCHIVE: u64 = 8 * 1024 * 1024 * 1024;
/// How long to wait for the catalog to bring the map from its source.
const FETCH_WAIT: std::time::Duration = std::time::Duration::from_secs(20 * 60);
const FETCH_POLL: std::time::Duration = std::time::Duration::from_secs(3);

fn bad_map() -> LauncherError {
    LauncherError::invalid_input("This map's archive has no world the launcher can add.")
}

fn disk(e: &std::io::Error) -> LauncherError {
    LauncherError::from_io(e, "Adding the map")
}

fn cancelled() -> LauncherError {
    LauncherError::new(ErrorCode::OperationCancelled, "The download was cancelled.")
}

/// A path component safe on every system: plain, no characters Windows
/// refuses, no reserved device name, not ending in a dot or space.
fn safe_part(part: &str) -> bool {
    let stem = part
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ((stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.len() == 4
            && stem.as_bytes()[3].is_ascii_digit());
    plain_name(part)
        && !part.ends_with([' ', '.'])
        && !part.starts_with(' ')
        && !part.contains(['<', '>', '"', '|', '?', '*'])
        && !reserved
}

/// A world folder name from a map's title: what is safe of it, at most 64
/// characters, `Map` when nothing is left.
pub fn world_name(title: &str) -> String {
    let cleaned: String = title
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, '/' | '\\' | ':' | '<' | '>' | '"' | '|' | '?' | '*') {
                ' '
            } else {
                c
            }
        })
        .collect();
    let name: String = cleaned
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim_matches(['.', ' '])
        .chars()
        .take(64)
        .collect();
    let name = name.trim_end_matches(['.', ' ']).to_owned();
    if safe_part(&name) {
        name
    } else {
        "Map".to_owned()
    }
}

/// `base`, or `base (2)`, `base (3)`… whichever no world in `saves` has.
pub fn free_name(saves: &Path, base: &str) -> String {
    let taken = |name: &str| {
        fs::symlink_metadata(saves.join(name)).is_ok()
            || fs::symlink_metadata(saves.join(format!(".{name}.part"))).is_ok()
    };
    if !taken(base) {
        return base.to_owned();
    }
    (2..10_000)
        .map(|n| format!("{base} ({n})"))
        .find(|name| !taken(name))
        .unwrap_or_else(|| format!("{base} ({})", uuid::Uuid::new_v4().simple()))
}

/// The folder in the zip that holds the world: `""` for the top, or one
/// folder with a slash after it.
fn world_root(names: &[String]) -> Option<String> {
    names
        .iter()
        .filter_map(|n| {
            let n = n.replace('\\', "/");
            if n == "level.dat" {
                return Some(String::new());
            }
            let (folder, file) = n.split_once('/')?;
            (file == "level.dat" && safe_part(folder) && folder != "__MACOSX")
                .then(|| format!("{folder}/"))
        })
        .min_by_key(|r| (r.len(), r.clone()))
}

/// Unpacks the world in `archive` into `saves/<name>`; returns the name
/// used (one no world has yet).
pub fn unpack(
    archive: &Path,
    saves: &Path,
    name: &str,
    cancel: &CancelToken,
) -> LauncherResult<String> {
    let file = fs::File::open(archive).map_err(|e| disk(&e))?;
    let mut zip = zip::ZipArchive::new(std::io::BufReader::new(file)).map_err(|_| bad_map())?;
    if zip.len() > MAX_ENTRIES {
        return Err(bad_map());
    }
    let names: Vec<String> = zip.file_names().map(str::to_owned).collect();
    let root = world_root(&names).ok_or_else(bad_map)?;
    fs::create_dir_all(saves).map_err(|e| disk(&e))?;
    let name = free_name(saves, name);
    let staging = saves.join(format!(".{name}.part"));
    let result = unpack_into(&mut zip, &root, &staging, cancel);
    if let Err(e) = result {
        let _ = fs::remove_dir_all(&staging);
        return Err(e);
    }
    let target = saves.join(&name);
    if let Err(e) = fs::rename(&staging, &target) {
        let _ = fs::remove_dir_all(&staging);
        return Err(disk(&e));
    }
    Ok(name)
}

fn unpack_into<R: Read + std::io::Seek>(
    zip: &mut zip::ZipArchive<R>,
    root: &str,
    staging: &Path,
    cancel: &CancelToken,
) -> LauncherResult<()> {
    fs::create_dir_all(staging).map_err(|e| disk(&e))?;
    let mut written = 0u64;
    let mut buffer = vec![0u8; 64 * 1024];
    for index in 0..zip.len() {
        if cancel.is_cancelled() {
            return Err(cancelled());
        }
        let mut entry = zip.by_index(index).map_err(|_| bad_map())?;
        let name = entry.name().replace('\\', "/");
        let Some(rest) = name.strip_prefix(root) else {
            continue;
        };
        let rest = rest.trim_end_matches('/');
        if rest.is_empty() || rest.split('/').next() == Some("__MACOSX") {
            continue;
        }
        // Links are never made; a name that is not plain is left out.
        if entry.is_symlink() || !rest.split('/').all(safe_part) || rest.split('/').count() > 32 {
            continue;
        }
        let target: PathBuf = rest
            .split('/')
            .fold(staging.to_path_buf(), |p, s| p.join(s));
        if !is_within(staging, &target) {
            continue;
        }
        if entry.is_dir() {
            fs::create_dir_all(&target).map_err(|e| disk(&e))?;
            continue;
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|e| disk(&e))?;
        }
        let mut out = fs::File::create(&target).map_err(|e| disk(&e))?;
        loop {
            let read = entry.read(&mut buffer).map_err(|_| bad_map())?;
            if read == 0 {
                break;
            }
            written += read as u64;
            if written > MAX_UNPACKED {
                return Err(bad_map());
            }
            out.write_all(&buffer[..read]).map_err(|e| disk(&e))?;
        }
    }
    if !staging.join("level.dat").is_file() {
        return Err(bad_map());
    }
    Ok(())
}

/// What adding a map will do, before it is done.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorldPlan {
    /// The world's folder name in `saves/`.
    pub name: String,
    pub size: u64,
    /// The catalog keeps the file already.
    pub kept: bool,
    /// On a metered connection, why the download asks first (D-051).
    pub approval: Option<crate::state::dto::ApprovalRequest>,
}

/// The map's file of a version: its zip.
fn map_file(version: &super::Version) -> LauncherResult<&super::VersionFile> {
    version
        .main_file()
        .filter(|f| f.name.to_ascii_lowercase().ends_with(".zip") && f.size <= MAX_ARCHIVE)
        .ok_or_else(|| {
            LauncherError::invalid_input("This version has no map the launcher can add.")
        })
}

/// Plans adding the map of a catalog version to an installation.
pub async fn plan<C: Catalog>(
    catalog: &C,
    game: &Path,
    version: &str,
) -> LauncherResult<WorldPlan> {
    let version = catalog.version(version).await?;
    let project = catalog.project(&version.project_id).await?;
    if project.kind != "world" {
        return Err(LauncherError::invalid_input("This is not a map."));
    }
    let file = map_file(&version)?;
    Ok(WorldPlan {
        name: free_name(&game.join("saves"), &world_name(&project.title)),
        size: file.size,
        kept: file.sha256.is_some() && file.url.is_some(),
        approval: None,
    })
}

impl Installer {
    /// Adds the map of a catalog version to an installation as a new world;
    /// its folder name.
    pub async fn install_world<C: Catalog>(
        &self,
        catalog: &C,
        root: &DataRoot,
        id: InstanceId,
        version_id: &str,
        cancel: &CancelToken,
        emit: &(dyn Fn(Progress) + Send + Sync),
    ) -> LauncherResult<String> {
        let say = |phase: &'static str, files: u64, total_files: u64| {
            emit(Progress {
                instance_id: id.to_string(),
                phase,
                files,
                total_files,
                bytes: 0,
                total_bytes: 0,
            });
        };
        let mut version = catalog.version(version_id).await?;
        let project = catalog.project(&version.project_id).await?;
        if project.kind != "world" {
            return Err(LauncherError::invalid_input("This is not a map."));
        }
        let started = tokio::time::Instant::now();
        let mut asked = false;
        let (sha, url, size, name) = loop {
            let file = map_file(&version)?;
            if let (Some(sha), Some(url)) = (&file.sha256, file.url.as_deref()) {
                let url = our_file(url, sha).ok_or_else(|| {
                    LauncherError::new(
                        ErrorCode::UnsupportedVersionMetadata,
                        "Metadata points to a download location the launcher does not trust.",
                    )
                })?;
                break (sha.clone(), url, file.size, file.name.clone());
            }
            if !asked {
                catalog.fetch(version_id).await?;
                asked = true;
            }
            say("fetching", 1, 1);
            if started.elapsed() > FETCH_WAIT {
                return Err(LauncherError::new(
                    ErrorCode::ServiceUnavailable,
                    "The catalog could not bring the files from their source in time. Try again later.",
                ));
            }
            tokio::select! {
                () = tokio::time::sleep(FETCH_POLL) => {}
                () = cancel.cancelled() => return Err(cancelled()),
            }
            version = catalog.version(version_id).await?;
        };

        let archive = format!("instances/{id}/pack/map.zip");
        let executor = Executor::new(
            self.transport.clone(),
            root.clone(),
            ExecutorConfig::default(),
        );
        let sink = Sink {
            instance: id.to_string(),
            emit,
        };
        executor
            .run(
                uuid::Uuid::new_v4(),
                vec![DownloadRequest {
                    source: crate::net::SourcePolicy::ModsCatalog,
                    url,
                    path: archive.clone(),
                    size,
                    digest: Digest::Sha256(Sha256::parse(&sha)?),
                    label: Some(name),
                }],
                cancel,
                &sink,
            )
            .await?;

        say("unpacking", 0, 1);
        let archive = root.instance_dir(id).join("pack").join("map.zip");
        let saves = root.game_dir(id).join("saves");
        let base = world_name(&project.title);
        let unpacked = {
            let (archive, cancel) = (archive.clone(), cancel.clone());
            tokio::task::spawn_blocking(move || unpack(&archive, &saves, &base, &cancel))
                .await
                .map_err(|_| LauncherError::internal("A file operation stopped unexpectedly."))?
        };
        let _ = fs::remove_file(&archive);
        let name = unpacked?;
        say("done", 1, 1);
        Ok(name)
    }
}
