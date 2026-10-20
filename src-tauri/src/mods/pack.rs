//! Modrinth modpacks (`.mrpack`, D-066 stage 3): a pack becomes an
//! installation of its own, with its game version, loader and memory for its
//! mods, and its files from files-mods.lapi.sh.
//!
//! - The catalog reads the pack's index and keeps every file it lists; the
//!   launcher asks for that once, then looks every few seconds until it is
//!   ready ([`PackInfo`]). Everything in the answer is checked again here.
//! - Files go through the download executor, checked against their SHA-256.
//!   The archive is downloaded the same way, and its `overrides/` and then
//!   `client-overrides/` are unpacked by the same path rules ([`pack_path`]),
//!   bounded in count and size, links never followed or made.
//! - `content.lock.json` records the pack and the SHA-256 of every file it
//!   brought ([`PackLock`]). Applying a pack again (an update) changes only
//!   files that are still as the pack left them: a config or a mod the player
//!   changed stays, and a file the player turned off stays off.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::content::{Installer, Progress, Sink, load_lock, save_lock};
use super::{line, our_file, our_id};
use crate::download::{CancelToken, Digest, DownloadRequest, Executor, ExecutorConfig};
use crate::error::{ErrorCode, LauncherError, LauncherResult};
use crate::loader::{LoaderChoice, LoaderKind};
use crate::storage::paths::{DataRoot, InstanceId, Sha256, is_within};

const MAX_FILES: usize = 5000;
/// Entries of the archive looked at, and overrides unpacked.
const MAX_ENTRIES: usize = 200_000;
const MAX_OVERRIDES: usize = 20_000;
const MAX_OVERRIDE: u64 = 1024 * 1024 * 1024;
/// All the overrides together, as unpacked (a zip bomb stops here).
const MAX_OVERRIDES_TOTAL: u64 = 4 * 1024 * 1024 * 1024;
const MAX_PATH: usize = 400;
const MAX_DEPTH: usize = 16;
const DISABLED: &str = ".disabled";

/// The folders of the game folder a pack may put files into; the catalog has
/// the same list.
pub const ROOTS: &[&str] = &[
    "mods",
    "resourcepacks",
    "shaderpacks",
    "config",
    "defaultconfigs",
    "kubejs",
    "scripts",
    "saves",
    "datapacks",
    "global_packs",
    "openloader",
    "packmenu",
    "resources",
    "patchouli_books",
    "schematics",
    "journeymap",
    "local",
    "emotes",
    "customization",
];

/// Files of the game folder itself a pack may bring.
pub const ROOT_FILES: &[&str] = &[
    "options.txt",
    "optionsof.txt",
    "optionsshaders.txt",
    "servers.dat",
];

/// Programs a pack never brings, wherever they would go.
const PROGRAMS: &[&str] = &[
    "exe", "bat", "cmd", "com", "scr", "msi", "ps1", "psm1", "vbs", "vbe", "wsf", "lnk", "sh",
    "command", "dll", "so", "dylib", "jnilib", "reg", "hta", "cpl", "app",
];

fn plain(part: &str) -> bool {
    let upper = part.to_ascii_uppercase();
    let stem = upper.split('.').next().unwrap_or_default();
    let reserved = matches!(stem, "CON" | "PRN" | "AUX" | "NUL")
        || ((stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.len() == 4
            && stem.as_bytes()[3].is_ascii_digit());
    !part.is_empty()
        && part.len() <= 255
        && part != "."
        && part != ".."
        && !part.ends_with([' ', '.'])
        && !part.starts_with(' ')
        && !part.contains(['\\', ':', '<', '>', '"', '|', '?', '*'])
        && !part.chars().any(char::is_control)
        && !reserved
}

/// A path a pack may write, relative to the game folder: `/` between plain
/// names that are safe on every system, in a folder of [`ROOTS`] or one of
/// [`ROOT_FILES`], no program, and a `.jar` only in `mods/`.
pub fn pack_path(path: &str) -> bool {
    if path.is_empty() || path.len() > MAX_PATH || path.starts_with('/') {
        return false;
    }
    let parts: Vec<&str> = path.split('/').collect();
    if parts.len() > MAX_DEPTH || !parts.iter().all(|p| plain(p)) {
        return false;
    }
    let placed = match parts.as_slice() {
        [file] => ROOT_FILES.contains(file),
        [root, ..] => ROOTS.contains(root),
        [] => false,
    };
    let name = parts[parts.len() - 1].to_ascii_lowercase();
    let name = name.strip_suffix(DISABLED).unwrap_or(&name);
    let extension = name.rsplit_once('.').map_or("", |(_, e)| e);
    placed && !PROGRAMS.contains(&extension) && (extension != "jar" || parts[0] == "mods")
}

fn bad_pack() -> LauncherError {
    LauncherError::new(
        ErrorCode::UnsupportedVersionMetadata,
        "This modpack cannot be installed: its files are not what the launcher accepts.",
    )
}

fn not_ready() -> LauncherError {
    LauncherError::invalid_input("The catalog is still preparing this modpack. Try again soon.")
}

fn disk(e: &std::io::Error) -> LauncherError {
    LauncherError::from_io(e, "Putting the modpack's files in place")
}

fn sha256_text(text: &str) -> bool {
    text.len() == 64 && text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

// ----- what the catalog says -----

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackLoader {
    pub kind: String,
    pub version: String,
}

/// A file a pack lists, at the catalog's address.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackFile {
    pub path: String,
    pub size: u64,
    pub sha256: Option<String>,
    pub url: Option<url::Url>,
}

/// The archive, for its overrides.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackArchive {
    pub size: u64,
    pub sha256: String,
    pub url: url::Url,
}

/// A modpack version as the catalog has it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackInfo {
    /// `none`, `working`, `ready` or `failed`.
    pub state: String,
    /// Why it failed: a short code.
    pub error: Option<String>,
    pub game: Option<String>,
    pub loader: Option<PackLoader>,
    /// Files the catalog keeps already, of all.
    pub kept: u64,
    pub total: u64,
    /// Files the catalog left out (not for the client, or a path a pack may
    /// not write).
    pub skipped: u64,
    pub mods: u32,
    /// The archive and every file.
    pub total_bytes: u64,
    /// On a metered connection, why the download asks first (D-051).
    pub approval: Option<crate::state::dto::ApprovalRequest>,
    #[serde(skip)]
    pub files: Vec<PackFile>,
    #[serde(skip)]
    pub archive: Option<PackArchive>,
}

impl PackInfo {
    fn failed(code: &str) -> Self {
        Self {
            state: "failed".to_owned(),
            error: Some(code.to_owned()),
            game: None,
            loader: None,
            kept: 0,
            total: 0,
            skipped: 0,
            mods: 0,
            total_bytes: 0,
            approval: None,
            files: Vec::new(),
            archive: None,
        }
    }

    /// Reads the catalog's answer. Anything that does not fit (a path, a
    /// hash, an address, a loader) makes the pack `failed` with `bad_pack`.
    pub fn from(value: &Value) -> Self {
        Self::read(value).unwrap_or_else(|| Self::failed("bad_pack"))
    }

    fn read(value: &Value) -> Option<Self> {
        let state = match value["state"].as_str()? {
            s @ ("none" | "working" | "ready" | "failed") => s.to_owned(),
            _ => return None,
        };
        let error = value["error"]
            .as_str()
            .filter(|e| {
                !e.is_empty()
                    && e.len() <= 40
                    && e.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
            })
            .map(str::to_owned);
        let game = match &value["game"] {
            Value::Null => None,
            g => Some(
                g.as_str()
                    .filter(|g| crate::loader::valid_version(g))?
                    .to_owned(),
            ),
        };
        let loader = match &value["loader"] {
            Value::Null => None,
            l => {
                let kind = match l["kind"].as_str()? {
                    k @ ("fabric" | "quilt" | "forge" | "neoforge") => k.to_owned(),
                    _ => return None,
                };
                let version = l["version"]
                    .as_str()
                    .filter(|v| crate::loader::valid_version(v))?
                    .to_owned();
                Some(PackLoader { kind, version })
            }
        };
        let list = value["files"].as_array()?;
        if list.len() > MAX_FILES {
            return None;
        }
        let mut files = Vec::with_capacity(list.len());
        let mut seen = HashSet::new();
        for f in list {
            let path = f["path"].as_str().filter(|p| pack_path(p))?.to_owned();
            if !seen.insert(path.to_lowercase()) {
                return None;
            }
            let sha256 = match &f["sha256"] {
                Value::Null => None,
                s => Some(s.as_str().filter(|s| sha256_text(s))?.to_owned()),
            };
            let url = match (&sha256, f["url"].as_str()) {
                (Some(sha), Some(url)) => Some(our_file(url, sha)?),
                (None, _) => None,
                (Some(_), None) => return None,
            };
            files.push(PackFile {
                path,
                size: f["size"].as_u64().filter(|s| *s > 0)?,
                sha256,
                url,
            });
        }
        let a = &value["archive"];
        let archive = match (a["sha256"].as_str(), a["url"].as_str()) {
            (Some(sha), Some(url)) if sha256_text(sha) => Some(PackArchive {
                size: a["size"].as_u64().filter(|s| *s > 0)?,
                sha256: sha.to_owned(),
                url: our_file(url, sha)?,
            }),
            (None, _) | (_, None) => None,
            _ => return None,
        };
        let ready = state == "ready";
        if ready && (game.is_none() || archive.is_none() || files.iter().any(|f| f.url.is_none())) {
            return None;
        }
        let mods = files
            .iter()
            .filter(|f| f.path.starts_with("mods/") && f.path.ends_with(".jar"))
            .count();
        Some(Self {
            state,
            error,
            game,
            loader,
            kept: files.iter().filter(|f| f.url.is_some()).count() as u64,
            total: files.len() as u64,
            skipped: value["skipped"].as_u64().unwrap_or(0),
            mods: u32::try_from(mods).unwrap_or(u32::MAX),
            total_bytes: files
                .iter()
                .map(|f| f.size)
                .chain(archive.iter().map(|a| a.size))
                .fold(0u64, u64::saturating_add),
            approval: None,
            files,
            archive,
        })
    }

    pub fn ready(&self) -> bool {
        self.state == "ready" && self.archive.is_some()
    }

    /// The loader to install with the game: Forge names its versions with
    /// the game's (`1.20.1-47.2.0`), the others on their own.
    pub fn loader_choice(&self) -> LauncherResult<Option<LoaderChoice>> {
        let Some(loader) = &self.loader else {
            return Ok(None);
        };
        let game = self.game.as_deref().ok_or_else(bad_pack)?;
        let kind = match loader.kind.as_str() {
            "fabric" => LoaderKind::Fabric,
            "quilt" => LoaderKind::Quilt,
            "forge" => LoaderKind::Forge,
            "neoforge" => LoaderKind::NeoForge,
            _ => return Err(bad_pack()),
        };
        let version =
            if kind == LoaderKind::Forge && !loader.version.starts_with(&format!("{game}-")) {
                format!("{game}-{}", loader.version)
            } else {
                loader.version.clone()
            };
        let choice = LoaderChoice { kind, version };
        choice.validate().map_err(|_| bad_pack())?;
        Ok(Some(choice))
    }
}

// ----- what the installation records -----

/// A file a pack brought, with the SHA-256 it had.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackEntry {
    pub path: String,
    pub sha256: String,
}

/// The modpack an installation was made from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackLock {
    pub project_id: String,
    pub version_id: String,
    pub title: String,
    pub version_number: String,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub files: Vec<PackEntry>,
    #[serde(default)]
    pub overrides: Vec<PackEntry>,
    pub installed_at: i64,
}

/// Who the pack is, from the catalog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackMeta {
    pub project_id: String,
    pub version_id: String,
    pub title: String,
    pub version_number: String,
    pub icon: Option<String>,
}

impl PackMeta {
    pub fn new(project: &super::Project, version: &super::Version) -> LauncherResult<Self> {
        if !our_id(&project.id) || project.id != version.project_id {
            return Err(bad_pack());
        }
        Ok(Self {
            project_id: project.id.clone(),
            version_id: version.id.clone(),
            title: line(&project.title, 256),
            version_number: line(&version.number, 128),
            icon: project.icon.clone(),
        })
    }
}

/// What applying a pack did.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackReport {
    /// Files put in place or already right.
    pub files: u64,
    /// The player's own changes, left as they are.
    pub kept_yours: u64,
    /// Left out by the catalog or here: not for the client, or a path a pack
    /// may not write.
    pub left_out: u64,
    /// Files of the previous version the pack no longer has, removed.
    pub removed: u64,
}

// ----- applying -----

/// The file at a path or its turned-off form, if it is a plain file.
fn present(game: &Path, path: &str) -> Option<(PathBuf, bool)> {
    let on = game.join(path);
    if fs::symlink_metadata(&on).is_ok_and(|m| m.is_file()) {
        return Some((on, false));
    }
    let off = game.join(format!("{path}{DISABLED}"));
    fs::symlink_metadata(&off)
        .is_ok_and(|m| m.is_file())
        .then_some((off, true))
}

/// Something at a path that is not a plain file: a folder or a link.
fn in_the_way(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|m| !m.is_file())
}

fn sha256_of(path: &Path) -> Option<String> {
    use sha2::Digest as _;
    let mut file = fs::File::open(path).ok()?;
    let mut hasher = sha2::Sha256::new();
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).ok()?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Some(hex::encode(hasher.finalize()))
}

/// What happens to one listed file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Fate {
    /// Already as the pack has it.
    Have,
    /// Downloaded into place; turned off afterwards when it was off.
    Fetch { off: bool },
    /// The player changed it, or something else is there: left alone.
    Yours,
}

pub(super) fn fate(game: &Path, file: &PackFile, before: Option<&String>) -> Fate {
    let Some(sha) = &file.sha256 else {
        return Fate::Yours;
    };
    if in_the_way(&game.join(&file.path)) {
        return Fate::Yours;
    }
    match present(game, &file.path) {
        None => Fate::Fetch { off: false },
        Some((there, off)) => match sha256_of(&there) {
            Some(now) if now == *sha => Fate::Have,
            Some(now) if before == Some(&now) => Fate::Fetch { off },
            _ => Fate::Yours,
        },
    }
}

/// Unpacks the overrides into the game folder; the pack's entries.
struct Unpacked {
    entries: Vec<PackEntry>,
    placed: u64,
    yours: u64,
    left_out: u64,
}

fn unpack(
    archive: &Path,
    game: &Path,
    staging: &Path,
    ours: &HashMap<String, HashSet<String>>,
    done: &AtomicU64,
    total: &AtomicU64,
    cancel: &CancelToken,
) -> LauncherResult<Unpacked> {
    let file = fs::File::open(archive).map_err(|e| disk(&e))?;
    let mut zip = zip::ZipArchive::new(std::io::BufReader::new(file)).map_err(|_| bad_pack())?;
    if zip.len() > MAX_ENTRIES {
        return Err(bad_pack());
    }
    // By path, the later layer winning; the same name in another case is the
    // same file on Windows and macOS.
    let mut chosen: BTreeMap<String, (String, usize, u8)> = BTreeMap::new();
    let mut left_out = 0u64;
    for index in 0..zip.len() {
        let Some(name) = zip.name_for_index(index) else {
            continue;
        };
        let (rest, layer) = if let Some(rest) = name.strip_prefix("overrides/") {
            (rest, 0)
        } else if let Some(rest) = name.strip_prefix("client-overrides/") {
            (rest, 1)
        } else {
            continue;
        };
        if rest.is_empty() || rest.ends_with('/') {
            continue;
        }
        let rest = rest.to_owned();
        let entry = zip.by_index_raw(index).map_err(|_| bad_pack())?;
        if entry.is_dir() {
            continue;
        }
        if entry.is_symlink() || !pack_path(&rest) {
            left_out += 1;
            continue;
        }
        let key = rest.to_lowercase();
        if chosen.get(&key).is_none_or(|(_, _, l)| *l <= layer) {
            chosen.insert(key, (rest, index, layer));
        }
    }
    if chosen.len() > MAX_OVERRIDES {
        return Err(bad_pack());
    }
    total.store(chosen.len() as u64, Ordering::Relaxed);
    fs::create_dir_all(staging).map_err(|e| disk(&e))?;
    let part = staging.join("override.part");
    let mut unpacked = Unpacked {
        entries: Vec::new(),
        placed: 0,
        yours: 0,
        left_out,
    };
    let mut written = 0u64;
    for (path, index, _) in chosen.into_values() {
        if cancel.is_cancelled() {
            return Err(LauncherError::new(
                ErrorCode::OperationCancelled,
                "The download was cancelled.",
            ));
        }
        let target = path.split('/').fold(game.to_path_buf(), |p, s| p.join(s));
        if !is_within(game, &target) {
            unpacked.left_out += 1;
            continue;
        }
        let sha = {
            use sha2::Digest as _;
            let mut entry = zip.by_index(index).map_err(|_| bad_pack())?;
            if entry.size() > MAX_OVERRIDE {
                return Err(bad_pack());
            }
            let mut out = fs::File::create(&part).map_err(|e| disk(&e))?;
            let mut hasher = sha2::Sha256::new();
            let mut buffer = vec![0u8; 64 * 1024];
            let mut size = 0u64;
            let mut limited = entry.by_ref().take(MAX_OVERRIDE + 1);
            loop {
                let read = limited.read(&mut buffer).map_err(|_| bad_pack())?;
                if read == 0 {
                    break;
                }
                size += read as u64;
                written += read as u64;
                if size > MAX_OVERRIDE || written > MAX_OVERRIDES_TOTAL {
                    let _ = fs::remove_file(&part);
                    return Err(bad_pack());
                }
                hasher.update(&buffer[..read]);
                out.write_all(&buffer[..read]).map_err(|e| disk(&e))?;
            }
            out.sync_all().map_err(|e| disk(&e))?;
            hex::encode(hasher.finalize())
        };
        let place = if in_the_way(&target) {
            false
        } else {
            match fs::symlink_metadata(&target) {
                Err(_) => true,
                Ok(_) => match sha256_of(&target) {
                    Some(now) if now == sha => {
                        unpacked.placed += 1;
                        unpacked.entries.push(PackEntry { path, sha256: sha });
                        done.fetch_add(1, Ordering::Relaxed);
                        continue;
                    }
                    Some(now) => ours.get(&path).is_some_and(|hashes| hashes.contains(&now)),
                    None => false,
                },
            }
        };
        let parent_ok = target
            .parent()
            .is_some_and(|parent| fs::create_dir_all(parent).is_ok() && parent.is_dir());
        if place && parent_ok {
            fs::rename(&part, &target).map_err(|e| disk(&e))?;
            unpacked.placed += 1;
        } else {
            unpacked.yours += 1;
        }
        // Recorded as the pack has it, so a later update still sees the
        // player's change.
        unpacked.entries.push(PackEntry { path, sha256: sha });
        done.fetch_add(1, Ordering::Relaxed);
    }
    let _ = fs::remove_file(&part);
    Ok(unpacked)
}

/// Removes what the previous version brought and this one does not, where
/// it is still as that version left it.
fn remove_dropped(game: &Path, before: &[PackEntry], now: &HashSet<String>) -> u64 {
    let mut removed = 0;
    for entry in before.iter().filter(|e| !now.contains(&e.path)) {
        if !pack_path(&entry.path) {
            continue;
        }
        if let Some((there, _)) = present(game, &entry.path)
            && sha256_of(&there).as_deref() == Some(entry.sha256.as_str())
            && fs::remove_file(&there).is_ok()
        {
            removed += 1;
        }
    }
    removed
}

impl Installer {
    /// Puts a ready pack into an installation: its files, then its
    /// overrides, then the record. A new installation and an update alike.
    #[allow(clippy::too_many_arguments)]
    pub async fn apply_pack(
        &self,
        root: &DataRoot,
        id: InstanceId,
        info: &PackInfo,
        meta: PackMeta,
        cancel: &CancelToken,
        emit: &(dyn Fn(Progress) + Send + Sync),
    ) -> LauncherResult<PackReport> {
        if !info.ready() {
            return Err(not_ready());
        }
        let archive = info.archive.clone().ok_or_else(not_ready)?;
        let game = root.game_dir(id);
        fs::create_dir_all(&game).map_err(|e| disk(&e))?;
        let lock = load_lock(root, id);
        let before = lock.pack.clone();
        let old_files: HashMap<String, String> = before
            .iter()
            .flat_map(|p| p.files.iter())
            .map(|e| (e.path.clone(), e.sha256.clone()))
            .collect();
        // An override may replace what the pack brought before, or a file
        // this version lists at the same path.
        let mut ours: HashMap<String, HashSet<String>> = HashMap::new();
        let brought = before
            .iter()
            .flat_map(|p| p.overrides.iter().chain(p.files.iter()))
            .map(|e| (e.path.clone(), e.sha256.clone()));
        let listed = info
            .files
            .iter()
            .filter_map(|f| Some((f.path.clone(), f.sha256.clone()?)));
        for (path, sha) in brought.chain(listed) {
            ours.entry(path).or_default().insert(sha);
        }

        // What each listed file needs.
        let fates: Vec<Fate> = {
            let (game, files) = (game.clone(), info.files.clone());
            tokio::task::spawn_blocking(move || {
                files
                    .iter()
                    .map(|f| fate(&game, f, old_files.get(&f.path)))
                    .collect()
            })
            .await
            .map_err(|_| LauncherError::internal("A file check stopped unexpectedly."))?
        };
        let staging = root.instance_dir(id).join("pack");
        let archive_path = format!("instances/{id}/pack/archive.mrpack");
        let mut requests = vec![DownloadRequest {
            source: crate::net::SourcePolicy::ModsCatalog,
            url: archive.url.clone(),
            path: archive_path.clone(),
            size: archive.size,
            digest: Digest::Sha256(Sha256::parse(&archive.sha256)?),
            label: Some(meta.title.clone()),
        }];
        for (file, fate) in info.files.iter().zip(&fates) {
            if let (Fate::Fetch { .. }, Some(sha), Some(url)) = (fate, &file.sha256, &file.url) {
                requests.push(DownloadRequest {
                    source: crate::net::SourcePolicy::ModsCatalog,
                    url: url.clone(),
                    path: format!("instances/{id}/game/{}", file.path),
                    size: file.size,
                    digest: Digest::Sha256(Sha256::parse(sha)?),
                    label: file.path.rsplit('/').next().map(str::to_owned),
                });
            }
        }
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
            .run(uuid::Uuid::new_v4(), requests, cancel, &sink)
            .await?;

        let mut report = PackReport {
            left_out: info.skipped,
            ..PackReport::default()
        };
        for (file, fate) in info.files.iter().zip(&fates) {
            match fate {
                Fate::Have => report.files += 1,
                Fate::Fetch { off } => {
                    report.files += 1;
                    if *off {
                        // The player had turned the old one off.
                        let on = game.join(&file.path);
                        let off = game.join(format!("{}{DISABLED}", file.path));
                        let _ = fs::remove_file(&off);
                        let _ = fs::rename(&on, &off);
                    }
                }
                Fate::Yours => report.kept_yours += 1,
            }
        }

        // The overrides, with a count the interface sees.
        let done = Arc::new(AtomicU64::new(0));
        let total = Arc::new(AtomicU64::new(0));
        let unpacking = {
            let (archive, game, staging) = (
                root.instance_dir(id).join("pack").join("archive.mrpack"),
                game.clone(),
                staging.clone(),
            );
            let (done, total, cancel) = (Arc::clone(&done), Arc::clone(&total), cancel.clone());
            tokio::task::spawn_blocking(move || {
                unpack(&archive, &game, &staging, &ours, &done, &total, &cancel)
            })
        };
        tokio::pin!(unpacking);
        let mut ticker = tokio::time::interval(std::time::Duration::from_millis(250));
        let unpacked = loop {
            tokio::select! {
                result = &mut unpacking => break result,
                _ = ticker.tick() => emit(Progress {
                    instance_id: id.to_string(),
                    phase: "unpacking",
                    files: done.load(Ordering::Relaxed),
                    total_files: total.load(Ordering::Relaxed),
                    bytes: 0,
                    total_bytes: 0,
                }),
            }
        };
        let _ = fs::remove_dir_all(&staging);
        let unpacked =
            unpacked.map_err(|_| LauncherError::internal("Unpacking stopped unexpectedly."))??;
        report.files += unpacked.placed;
        report.kept_yours += unpacked.yours;
        report.left_out += unpacked.left_out;

        // What the previous version brought and this one no longer has.
        let now: HashSet<String> = info
            .files
            .iter()
            .map(|f| f.path.clone())
            .chain(unpacked.entries.iter().map(|e| e.path.clone()))
            .collect();
        if let Some(before) = &before {
            report.removed += remove_dropped(&game, &before.files, &now);
            report.removed += remove_dropped(&game, &before.overrides, &now);
        }

        let mut lock = load_lock(root, id);
        lock.pack = Some(PackLock {
            project_id: meta.project_id,
            version_id: meta.version_id,
            title: meta.title,
            version_number: meta.version_number,
            icon: meta.icon,
            files: info
                .files
                .iter()
                .filter_map(|f| {
                    Some(PackEntry {
                        path: f.path.clone(),
                        sha256: f.sha256.clone()?,
                    })
                })
                .collect(),
            overrides: unpacked.entries,
            installed_at: chrono::Utc::now().timestamp(),
        });
        lock.schema_version = crate::storage::document::SCHEMA_VERSION;
        save_lock(root, id, &lock)?;
        emit(Progress {
            instance_id: id.to_string(),
            phase: "done",
            files: report.files,
            total_files: report.files,
            bytes: 0,
            total_bytes: 0,
        });
        Ok(report)
    }
}
