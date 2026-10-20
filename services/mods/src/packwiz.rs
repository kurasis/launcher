//! packwiz packs (D-066 stage 5): a `pack.toml` at an address the player
//! gives, with its `index.toml` and a `.pw.toml` for each mod. The catalog
//! reads them, keeps every file and writes the pack out as a `.mrpack`, so
//! it is a modpack like any other here and in the launcher.
//!
//! Everything a pack names is checked: addresses are only of the hosts we
//! fetch from ([`crate::fetch::site`]), paths only where a pack may write
//! ([`crate::pack::pack_path`]), hashes of every file it lists, and counts
//! and sizes are bounded.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;

use serde::Deserialize;
use serde_json::json;

use crate::blobs::Hashed;
use crate::model::{clean, word};

pub const SOURCE: &str = "packwiz";

/// `pack.toml` and `index.toml` at most.
pub const MAX_TOML: u64 = 4 * 1024 * 1024;
/// A `.pw.toml`.
pub const MAX_META: u64 = 64 * 1024;
/// Files a pack lists.
pub const MAX_FILES: usize = 5000;
/// A file of a pack that is not a mod (a config), and all of them.
pub const MAX_OVERRIDE: u64 = 64 * 1024 * 1024;
pub const MAX_OVERRIDES: u64 = 512 * 1024 * 1024;

/// The address to fetch a pack's `pack.toml` from: on GitHub a file's page
/// becomes its raw address, on GitLab `/-/blob/` becomes `/-/raw/`. `None`
/// for an address that is not a `pack.toml` on a host we fetch from.
pub fn pack_url(url: &reqwest::Url) -> Option<String> {
    if url.scheme() != "https" || url.port().is_some() || !url.username().is_empty() {
        return None;
    }
    if url.query().is_some() || url.fragment().is_some() {
        return None;
    }
    let host = url.host_str()?.to_ascii_lowercase();
    let path = url.path();
    if !path.ends_with("/pack.toml") || path.contains("/../") || path.contains("//") {
        return None;
    }
    let raw = match host.as_str() {
        "github.com" => {
            // /<owner>/<repo>/blob/<ref>/<path>/pack.toml
            let parts: Vec<&str> = path.trim_start_matches('/').splitn(4, '/').collect();
            match parts.as_slice() {
                [owner, repo, "blob" | "raw", rest] => {
                    format!("https://raw.githubusercontent.com/{owner}/{repo}/{rest}")
                }
                _ => return None,
            }
        }
        "gitlab.com" => format!(
            "https://gitlab.com{}",
            path.replacen("/-/blob/", "/-/raw/", 1)
        ),
        "raw.githubusercontent.com" => format!("https://{host}{path}"),
        _ => return None,
    };
    crate::fetch::site(&raw).is_some().then_some(raw)
}

/// An address relative to another (a file of a pack next to its index).
pub fn relative(base: &str, path: &str) -> Option<String> {
    if path.is_empty()
        || path.starts_with('/')
        || path.contains('\\')
        || path.contains("://")
        || path
            .split('/')
            .any(|p| p == ".." || p == "." || p.is_empty())
    {
        return None;
    }
    let base = reqwest::Url::parse(base).ok()?;
    let joined = base.join(path).ok()?;
    let joined = joined.to_string();
    crate::fetch::site(&joined).is_some().then_some(joined)
}

/// A hash a pack gives, by its format.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Hash {
    Sha1(String),
    Sha256(String),
    Sha512(String),
    /// md5 or CurseForge's murmur2: not checked here; such a file is taken
    /// only from CurseForge, over TLS, as CurseForge's files are.
    Other,
}

impl Hash {
    pub fn of(format: &str, value: &str) -> Option<Self> {
        let value = value.trim().to_ascii_lowercase();
        let hex = |len: usize| value.len() == len && value.bytes().all(|b| b.is_ascii_hexdigit());
        match format {
            "sha1" if hex(40) => Some(Self::Sha1(value)),
            "sha256" if hex(64) => Some(Self::Sha256(value)),
            "sha512" if hex(128) => Some(Self::Sha512(value)),
            "md5" | "murmur2" => Some(Self::Other),
            _ => None,
        }
    }

    /// Whether `hashed` is this; `None` when it cannot be told.
    pub fn matches(&self, hashed: &Hashed) -> Option<bool> {
        match self {
            Self::Sha1(h) => Some(*h == hashed.sha1),
            Self::Sha256(h) => Some(*h == hashed.sha256),
            Self::Sha512(h) => Some(*h == hashed.sha512),
            Self::Other => None,
        }
    }

    /// Whether bytes are this.
    pub fn matches_bytes(&self, bytes: &[u8]) -> Option<bool> {
        use sha2::Digest as _;
        match self {
            Self::Sha1(h) => Some(*h == hex::encode(sha1::Sha1::digest(bytes))),
            Self::Sha256(h) => Some(*h == hex::encode(sha2::Sha256::digest(bytes))),
            Self::Sha512(h) => Some(*h == hex::encode(sha2::Sha512::digest(bytes))),
            Self::Other => None,
        }
    }
}

#[derive(Deserialize)]
struct RawPack {
    #[serde(default)]
    name: String,
    #[serde(default)]
    author: String,
    #[serde(default)]
    version: String,
    index: RawIndexRef,
    #[serde(default)]
    versions: BTreeMap<String, String>,
}

#[derive(Deserialize)]
struct RawIndexRef {
    file: String,
    #[serde(rename = "hash-format")]
    hash_format: String,
    hash: String,
}

/// What a `pack.toml` says, checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pack {
    pub name: String,
    pub author: String,
    pub version: String,
    pub game: String,
    /// fabric, quilt, forge or neoforge, with its version.
    pub loader: Option<(String, String)>,
    pub index: String,
    pub index_hash: Hash,
}

/// Reads a `pack.toml`; a short code when it is not one we take.
pub fn pack(text: &str) -> Result<Pack, &'static str> {
    let raw: RawPack = toml::from_str(text).map_err(|_| "bad_pack")?;
    let mut game = None;
    let mut loader = None;
    for (name, version) in &raw.versions {
        let version = word(version).ok_or("bad_pack")?;
        let kind = match name.as_str() {
            "minecraft" => {
                game = Some(version);
                continue;
            }
            "fabric" => "fabric",
            "quilt" => "quilt",
            "forge" => "forge",
            "neoforge" => "neoforge",
            // Other entries (liteloader, a pack format) are not ours to install.
            _ => return Err("unsupported_pack"),
        };
        if loader.is_some() {
            return Err("unsupported_pack");
        }
        loader = Some((kind.to_owned(), version));
    }
    let index_hash = Hash::of(&raw.index.hash_format, &raw.index.hash).ok_or("bad_pack")?;
    if index_hash == Hash::Other {
        return Err("bad_pack");
    }
    Ok(Pack {
        name: clean(&raw.name, 200, false),
        author: clean(&raw.author, 100, false),
        version: clean(&raw.version, 64, false),
        game: game.ok_or("bad_pack")?,
        loader,
        index: raw.index.file,
        index_hash,
    })
}

#[derive(Deserialize)]
struct RawIndex {
    #[serde(rename = "hash-format")]
    hash_format: String,
    #[serde(default)]
    files: Vec<RawIndexFile>,
}

#[derive(Deserialize)]
struct RawIndexFile {
    file: String,
    hash: String,
    #[serde(rename = "hash-format")]
    hash_format: Option<String>,
    alias: Option<String>,
    #[serde(default)]
    metafile: bool,
}

/// A file an index lists: where it is next to the index, where it goes,
/// and its hash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexFile {
    pub file: String,
    pub path: String,
    pub hash: Hash,
    pub metafile: bool,
}

/// Reads an `index.toml`. Files that go nowhere a pack may write are
/// counted, not listed.
pub fn index(text: &str) -> Result<(Vec<IndexFile>, u64), &'static str> {
    let raw: RawIndex = toml::from_str(text).map_err(|_| "bad_pack")?;
    if raw.files.len() > MAX_FILES {
        return Err("too_big");
    }
    let mut files = Vec::new();
    let mut skipped = 0;
    for file in raw.files {
        let format = file.hash_format.as_deref().unwrap_or(&raw.hash_format);
        let hash = Hash::of(format, &file.hash).ok_or("bad_pack")?;
        let path = file.alias.clone().unwrap_or_else(|| file.file.clone());
        // A metafile's own path is not where its mod goes: its folder is.
        let placed = if file.metafile {
            folder(&path).is_some()
        } else {
            crate::pack::pack_path(&path)
        };
        if !placed || hash == Hash::Other {
            skipped += 1;
            continue;
        }
        files.push(IndexFile {
            file: file.file,
            path,
            hash,
            metafile: file.metafile,
        });
    }
    Ok((files, skipped))
}

/// The folder of a path (`mods` of `mods/x.pw.toml`); `None` at the top.
fn folder(path: &str) -> Option<&str> {
    path.rsplit_once('/')
        .map(|(dir, _)| dir)
        .filter(|d| !d.is_empty())
}

#[derive(Deserialize)]
struct RawMeta {
    filename: String,
    #[serde(default)]
    side: String,
    download: RawDownload,
    #[serde(default)]
    option: Option<RawOption>,
    #[serde(default)]
    update: Option<RawUpdate>,
}

#[derive(Deserialize)]
struct RawDownload {
    url: Option<String>,
    #[serde(rename = "hash-format")]
    hash_format: String,
    hash: String,
    #[serde(default)]
    mode: String,
}

#[derive(Deserialize)]
struct RawOption {
    #[serde(default)]
    optional: bool,
}

#[derive(Deserialize)]
struct RawUpdate {
    curseforge: Option<RawCurseForge>,
}

#[derive(Deserialize)]
struct RawCurseForge {
    #[serde(rename = "file-id")]
    file_id: u64,
    #[serde(rename = "project-id")]
    project_id: u64,
}

/// A mod a `.pw.toml` names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Meta {
    /// Where it goes in the game folder.
    pub path: String,
    /// Where it is fetched from.
    pub url: String,
    pub hash: Hash,
    pub optional: bool,
}

/// Reads a `.pw.toml` that sits at `at` (its folder is the mod's). `Ok(None)`
/// for a mod only for servers, or one that goes nowhere a pack may write.
pub fn meta(text: &str, at: &str) -> Result<Option<Meta>, &'static str> {
    let raw: RawMeta = toml::from_str(text).map_err(|_| "bad_pack")?;
    if raw.side == "server" {
        return Ok(None);
    }
    let Some(dir) = folder(at) else {
        return Ok(None);
    };
    if raw.filename.contains('/') || raw.filename.contains('\\') {
        return Err("bad_pack");
    }
    let path = format!("{dir}/{}", raw.filename);
    if !crate::pack::pack_path(&path) {
        return Ok(None);
    }
    let hash = Hash::of(&raw.download.hash_format, &raw.download.hash).ok_or("bad_pack")?;
    let url = match (raw.download.mode.as_str(), raw.download.url) {
        ("metadata:curseforge", _) => {
            let cf = raw.update.and_then(|u| u.curseforge).ok_or("bad_pack")?;
            if cf.project_id == 0 || cf.file_id == 0 {
                return Err("bad_pack");
            }
            crate::curseforge::download_url(cf.project_id, cf.file_id)
        }
        ("" | "url", Some(url)) if crate::pack::download_host(&url) => url,
        ("" | "url", Some(_)) => return Err("unsupported_host"),
        _ => return Err("bad_pack"),
    };
    // A hash we cannot check is taken only from CurseForge.
    if hash == Hash::Other && crate::curseforge::download_ids(&url).is_none() {
        return Err("bad_pack");
    }
    Ok(Some(Meta {
        path,
        url,
        hash,
        optional: raw.option.is_some_and(|o| o.optional),
    }))
}

/// A slug for a pack's name: lowercase letters, digits and dashes.
pub fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars().flat_map(char::to_lowercase) {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
        if out.len() >= 48 {
            break;
        }
    }
    let out = out.trim_end_matches('-').to_owned();
    if out.is_empty() {
        "pack".to_owned()
    } else {
        out
    }
}

/// A mod of the pack as written into the `.mrpack`.
pub struct Written<'a> {
    pub path: &'a str,
    pub hashed: &'a Hashed,
    pub url: &'a str,
    pub optional: bool,
}

/// Writes the pack as a `.mrpack`: its mods in the index with their hashes
/// and addresses, its other files under `overrides/`.
pub fn write_mrpack(
    to: &Path,
    pack: &Pack,
    mods: &[Written<'_>],
    overrides: &[(String, std::path::PathBuf)],
) -> std::io::Result<()> {
    let mut dependencies = serde_json::Map::new();
    dependencies.insert("minecraft".into(), pack.game.clone().into());
    if let Some((kind, version)) = &pack.loader {
        let name = match kind.as_str() {
            "fabric" => "fabric-loader",
            "quilt" => "quilt-loader",
            other => other,
        };
        dependencies.insert(name.into(), version.clone().into());
    }
    let files: Vec<serde_json::Value> = mods
        .iter()
        .map(|m| {
            json!({
                "path": m.path,
                "hashes": { "sha1": m.hashed.sha1, "sha512": m.hashed.sha512 },
                "env": {
                    "client": if m.optional { "optional" } else { "required" },
                    "server": "unsupported",
                },
                "downloads": [m.url],
                "fileSize": m.hashed.size,
            })
        })
        .collect();
    let index = json!({
        "formatVersion": 1,
        "game": "minecraft",
        "versionId": pack.version,
        "name": pack.name,
        "files": files,
        "dependencies": dependencies,
    });
    let file = std::fs::File::create(to)?;
    let mut zip = zip::ZipWriter::new(std::io::BufWriter::new(file));
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    zip.start_file(crate::pack::INDEX, options)?;
    zip.write_all(index.to_string().as_bytes())?;
    for (path, from) in overrides {
        zip.start_file(format!("overrides/{path}"), options)?;
        let mut source = std::fs::File::open(from)?;
        std::io::copy(&mut source, &mut zip)?;
    }
    zip.finish()?.flush()
}
