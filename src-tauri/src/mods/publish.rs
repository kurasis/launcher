//! Publishing an installation as the player's own pack of the catalog
//! (D-066, stage 6). Mods, resource packs and shaders the catalog keeps are
//! named by their hashes; the player's settings go into `overrides/` by the
//! catalog's rules (no mod files, no code or programs, bounded in size); a
//! mod the catalog does not have stops the publish and is named, to be added
//! to the catalog first. The archive carries `catalog.json` (title, summary,
//! description, version number, changes) and the icon the player chose.
//! The catalog checks all of it again; the installation remembers the pack,
//! so the next publish is a new version of it.

use std::collections::HashSet;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::pack::{self, ExportChoice, Gathered, LINKABLE, disk};
use crate::download::CancelToken;
use crate::error::{ErrorCode, LauncherError, LauncherResult};
use crate::loader::LoaderChoice;

/// The catalog's limits (services/mods/src/own.rs).
pub const MAX_ARCHIVE: u64 = 512 * 1024 * 1024;
/// The largest part the catalog may ask for (it asks for 64 MiB).
pub const MAX_PART: u64 = 90 * 1024 * 1024;
pub const MAX_OVERRIDE: u64 = 64 * 1024 * 1024;
pub const MAX_OVERRIDES: u64 = 512 * 1024 * 1024;
pub const MAX_ICON: u64 = 512 * 1024;
const MAX_TITLE: usize = 80;
const MAX_SUMMARY: usize = 300;
const MAX_BODY: usize = 65_536;
const MAX_CHANGES: usize = 16_384;
/// Paths named in a check.
const MAX_NAMED: usize = 50;
const META: &str = "catalog.json";
/// What a player may report a pack for.
pub const REASONS: &[&str] = &["malware", "stolen", "inappropriate", "broken", "other"];
pub const MAX_REPORT: usize = 500;

/// Never in an own pack's overrides beyond programs: code the game loads.
const CODE: &[&str] = &["jar", "class", "jnilib", "litemod", "zip-mod"];

/// A path the catalog takes in an own pack's overrides: one a pack may
/// write, not in `mods/`, and no code or program under any extension.
pub fn overridable(path: &str) -> bool {
    if !pack::pack_path(path) || path.split('/').next() == Some("mods") {
        return false;
    }
    let name = path
        .rsplit('/')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    !name
        .split('.')
        .skip(1)
        .any(|e| CODE.contains(&e) || pack::PROGRAMS.contains(&e))
}

/// What publishing would take, for the player to see first.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublishCheck {
    /// Files named by their hashes in the catalog.
    pub linked: u64,
    /// The player's own files, copied in.
    pub bundled: u64,
    pub bytes: u64,
    /// Mods the catalog does not have: to be added first.
    pub missing: Vec<String>,
    /// Files the catalog never takes in a pack (code, programs).
    pub refused: Vec<String>,
    /// Files over the size a pack may bring.
    pub too_big: Vec<String>,
    /// The catalog's pack this installation was published as.
    pub published: Option<String>,
}

impl PublishCheck {
    pub fn ready(&self) -> bool {
        self.missing.is_empty() && self.refused.is_empty() && self.too_big.is_empty()
    }
}

/// What the player fills in.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PublishForm {
    pub title: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub body: String,
    pub version: String,
    #[serde(default)]
    pub changes: String,
    #[serde(default)]
    pub choice: ExportChoice,
}

/// One line of the player's text: no control characters.
fn line(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_control())
        .collect::<String>()
        .trim()
        .to_owned()
}

fn multiline(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
        .collect::<String>()
        .trim()
        .to_owned()
}

/// The form as it goes into `catalog.json`, checked as the catalog checks it.
pub fn meta(form: &PublishForm) -> LauncherResult<Value> {
    let title = line(&form.title);
    if title.chars().filter(|c| c.is_alphanumeric()).count() < 2
        || title.chars().count() > MAX_TITLE
    {
        return Err(LauncherError::invalid_input(
            "Give the pack a title of 2 to 80 characters.",
        ));
    }
    let summary = line(&form.summary);
    let body = multiline(&form.body);
    let changes = multiline(&form.changes);
    if summary.chars().count() > MAX_SUMMARY
        || body.chars().count() > MAX_BODY
        || changes.chars().count() > MAX_CHANGES
    {
        return Err(LauncherError::invalid_input(
            "A text of the pack is too long.",
        ));
    }
    let version = form.version.trim();
    let plain = !version.is_empty()
        && version.len() <= 48
        && version
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_' | b'+'));
    if !plain {
        return Err(LauncherError::invalid_input(
            "A version number is letters, digits and . - _ + only, such as 1.0.2.",
        ));
    }
    Ok(json!({
        "title": title, "summary": summary, "body": body,
        "version": version, "changes": changes,
    }))
}

/// An icon the player chose: a PNG, JPEG or WebP picture of at most 512 KB.
/// Returns its name in the archive.
pub fn icon_name(bytes: &[u8]) -> LauncherResult<&'static str> {
    if bytes.len() as u64 > MAX_ICON {
        return Err(LauncherError::invalid_input(
            "The icon may be 512 KB at most.",
        ));
    }
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Ok("icon.png")
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Ok("icon.jpg")
    } else if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Ok("icon.webp")
    } else {
        Err(LauncherError::invalid_input(
            "The icon must be a PNG, JPEG or WebP picture.",
        ))
    }
}

/// The files of an installation sorted for publishing.
pub struct Sorted {
    pub check: PublishCheck,
    linked: Vec<Gathered>,
    bundled: Vec<Gathered>,
}

fn name_it(list: &mut Vec<String>, path: &str) {
    if list.len() < MAX_NAMED {
        list.push(path.to_owned());
    }
}

/// Sorts an installation's files: what the catalog has (by SHA-256), the
/// player's own settings, and what stops the publish.
pub async fn sort<C: super::Catalog>(
    catalog: &C,
    game: &Path,
    choice: ExportChoice,
) -> LauncherResult<Sorted> {
    let gathered = {
        let game = game.to_path_buf();
        tokio::task::spawn_blocking(move || pack::gather(&game, choice))
            .await
            .map_err(|_| LauncherError::internal("A file check stopped unexpectedly."))??
    };
    let shas: Vec<String> = gathered
        .iter()
        .filter_map(|g| g.hashes.as_ref().map(|h| h.2.clone()))
        .collect();
    let mut known: HashSet<String> = HashSet::new();
    for chunk in shas.chunks(200) {
        for found in catalog.lookup(chunk).await? {
            known.insert(found.hash);
        }
    }
    let mut check = PublishCheck::default();
    let (mut linked, mut bundled) = (Vec::new(), Vec::new());
    for g in gathered {
        let top = g.path.split('/').next().unwrap_or_default();
        let in_catalog = g.hashes.as_ref().is_some_and(|h| known.contains(&h.2));
        if in_catalog && LINKABLE.contains(&top) {
            check.linked += 1;
            linked.push(g);
        } else if top == "mods" {
            name_it(&mut check.missing, &g.path);
        } else if !overridable(&g.path) {
            name_it(&mut check.refused, &g.path);
        } else if g.size > MAX_OVERRIDE {
            name_it(&mut check.too_big, &g.path);
        } else {
            check.bundled += 1;
            check.bytes += g.size;
            bundled.push(g);
        }
    }
    if check.bytes > MAX_OVERRIDES {
        name_it(&mut check.too_big, "*");
    }
    Ok(Sorted {
        check,
        linked,
        bundled,
    })
}

fn index_json(
    game: &str,
    loader: Option<&LoaderChoice>,
    version: &str,
    linked: &[Gathered],
) -> Value {
    let mut dependencies = serde_json::Map::new();
    dependencies.insert("minecraft".to_owned(), game.into());
    if let Some(loader) = loader {
        let (name, version) = pack::loader_dependency(game, loader);
        dependencies.insert(name.to_owned(), version.into());
    }
    let files: Vec<Value> = linked
        .iter()
        .filter_map(|g| {
            let (sha1, sha512, _) = g.hashes.as_ref()?;
            Some(json!({
                "path": g.path,
                "hashes": { "sha1": sha1, "sha512": sha512 },
                "env": { "client": "required", "server": "required" },
                "downloads": [],
                "fileSize": g.size,
            }))
        })
        .collect();
    json!({
        "formatVersion": 1,
        "game": "minecraft",
        "versionId": version,
        "files": files,
        "dependencies": dependencies,
    })
}

/// Who the pack is for: the game and loader of the installation.
pub struct Target {
    pub game: String,
    pub loader: Option<LoaderChoice>,
}

/// Writes the archive to publish (blocking); fails past the catalog's size.
pub fn write(
    to: &Path,
    sorted: &Sorted,
    target: &Target,
    meta: &Value,
    icon: Option<(&'static str, Vec<u8>)>,
    cancel: &CancelToken,
) -> LauncherResult<()> {
    let write = |e: std::io::Error| LauncherError::from_io(&e, "Writing the modpack");
    let zipped = |_| LauncherError::internal("The modpack could not be written.");
    let version = meta["version"].as_str().unwrap_or_default();
    let index = index_json(
        &target.game,
        target.loader.as_ref(),
        version,
        &sorted.linked,
    );
    let result = (|| {
        let file = fs::File::create(to).map_err(write)?;
        let mut zip = zip::ZipWriter::new(std::io::BufWriter::new(file));
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        zip.start_file(pack::INDEX, options).map_err(zipped)?;
        zip.write_all(index.to_string().as_bytes()).map_err(write)?;
        zip.start_file(META, options).map_err(zipped)?;
        zip.write_all(meta.to_string().as_bytes()).map_err(write)?;
        if let Some((name, bytes)) = icon {
            zip.start_file(name, options).map_err(zipped)?;
            zip.write_all(&bytes).map_err(write)?;
        }
        for file in &sorted.bundled {
            if cancel.is_cancelled() {
                return Err(LauncherError::new(
                    ErrorCode::OperationCancelled,
                    "Publishing was cancelled.",
                ));
            }
            zip.start_file(format!("overrides/{}", file.path), options)
                .map_err(zipped)?;
            let mut source = fs::File::open(&file.full).map_err(write)?;
            std::io::copy(&mut source, &mut zip).map_err(write)?;
        }
        let mut out = zip.finish().map_err(zipped)?;
        out.flush().map_err(write)?;
        Ok(())
    })();
    let size = fs::metadata(to).map(|m| m.len()).unwrap_or(0);
    match result {
        Ok(()) if size <= MAX_ARCHIVE => Ok(()),
        Ok(()) => {
            let _ = fs::remove_file(to);
            Err(LauncherError::invalid_input(
                "The pack is larger than 512 MB packed. Leave out worlds or large files.",
            ))
        }
        Err(e) => {
            let _ = fs::remove_file(to);
            Err(e)
        }
    }
}

/// An archive's size and SHA-256, for the catalog to check it by.
pub fn size_and_sha256(path: &Path) -> LauncherResult<(u64, String)> {
    use sha2::Digest as _;
    use std::io::Read as _;
    let read = |e: std::io::Error| LauncherError::from_io(&e, "Reading the modpack");
    let mut file = fs::File::open(path).map_err(read)?;
    let mut hasher = sha2::Sha256::new();
    let mut buffer = vec![0u8; 256 * 1024];
    let mut size = 0u64;
    loop {
        let n = file.read(&mut buffer).map_err(read)?;
        if n == 0 {
            break;
        }
        size += n as u64;
        hasher.update(&buffer[..n]);
    }
    Ok((size, hex::encode(hasher.finalize())))
}

/// `len` bytes of an archive from `offset`, fewer at its end.
pub fn read_part(path: &Path, offset: u64, len: u64) -> LauncherResult<Vec<u8>> {
    use std::io::{Read as _, Seek as _};
    let read = |e: std::io::Error| LauncherError::from_io(&e, "Reading the modpack");
    let mut file = fs::File::open(path).map_err(read)?;
    file.seek(std::io::SeekFrom::Start(offset)).map_err(read)?;
    let mut bytes = Vec::new();
    file.take(len).read_to_end(&mut bytes).map_err(read)?;
    Ok(bytes)
}

/// Reads the icon the player chose.
pub fn read_icon(path: &Path) -> LauncherResult<(&'static str, Vec<u8>)> {
    let meta = fs::metadata(path).map_err(|e| disk(&e))?;
    if !meta.is_file() || meta.len() > MAX_ICON {
        return Err(LauncherError::invalid_input(
            "The icon may be 512 KB at most.",
        ));
    }
    let bytes = fs::read(path).map_err(|e| disk(&e))?;
    Ok((icon_name(&bytes)?, bytes))
}

/// What the catalog answered to a publish.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", tag = "outcome")]
pub enum Published {
    /// The pack is in the catalog.
    Published {
        project_id: String,
        slug: String,
        version_id: String,
    },
    /// Some files stopped it: `code` says why (`not_in_catalog`, `held`,
    /// `not_allowed`, `too_big`), `paths` which.
    Refused { code: String, paths: Vec<String> },
}

/// A refusal the catalog names files in.
pub fn refused(status: u16, value: &Value) -> Option<Published> {
    let code = value["error"].as_str()?;
    (status == 422 && matches!(code, "not_in_catalog" | "held" | "not_allowed" | "too_big")).then(
        || Published::Refused {
            code: code.to_owned(),
            paths: value["paths"]
                .as_array()
                .into_iter()
                .flatten()
                .take(MAX_NAMED)
                .filter_map(Value::as_str)
                .map(|p| line(p).chars().take(400).collect())
                .collect(),
        },
    )
}

/// What a refused publish means, for the codes that name no files.
/// The catalog lost or refused the parts of an archive.
pub fn lost() -> LauncherError {
    LauncherError::new(
        ErrorCode::NetworkError,
        "The pack did not reach the catalog whole. Publish it again.",
    )
}

pub fn problem(status: u16, code: &str) -> Option<LauncherError> {
    let error = match (status, code) {
        (409, "version_exists") => {
            LauncherError::invalid_input("This version number is published already. Raise it.")
        }
        (410, _) => LauncherError::invalid_input(
            "The catalog removed this pack. It cannot get new versions.",
        ),
        (413, _) => LauncherError::invalid_input(
            "The pack is larger than 512 MB packed. Leave out worlds or large files.",
        ),
        (429, "too_many_packs") => LauncherError::new(
            ErrorCode::RateLimited,
            "Your account published 5 new packs today. Try again tomorrow.",
        ),
        (429, "too_many_versions") => LauncherError::new(
            ErrorCode::RateLimited,
            "Your account published 20 versions today. Try again tomorrow.",
        ),
        (429, "too_much") => LauncherError::new(
            ErrorCode::RateLimited,
            "Your packs take 2 GB in the catalog, as much as an account may have.",
        ),
        (400, "bad_title") => {
            LauncherError::invalid_input("Give the pack a title of 2 to 80 characters.")
        }
        (400, "bad_version") => LauncherError::invalid_input(
            "A version number is letters, digits and . - _ + only, such as 1.0.2.",
        ),
        (400, "too_long") => LauncherError::invalid_input("A text of the pack is too long."),
        (400, "bad_icon") => LauncherError::invalid_input(
            "The icon must be a PNG, JPEG or WebP picture of 2048 pixels at most.",
        ),
        (429, "busy") => LauncherError::new(
            ErrorCode::RateLimited,
            "The catalog is taking many packs right now. Try again in a few minutes.",
        ),
        (400, "bad_upload" | "bad_part" | "not_complete") => lost(),
        (400, "refused_by_scanner") => LauncherError::invalid_input(
            "The catalog's check of the pack found something it does not allow, and refused it.",
        ),
        _ => return None,
    };
    Some(error)
}

/// The archive's place while it is made and sent.
pub fn part_path(instance_dir: &Path) -> PathBuf {
    instance_dir.join("publish.mrpack.part")
}
