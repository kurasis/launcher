//! Modrinth modpacks (`.mrpack`, stage 3 of D-066): the catalog reads a
//! kept pack's `modrinth.index.json` and keeps every file it lists, so the
//! launcher installs a whole pack from files-mods.lapi.sh.
//!
//! CurseForge modpacks (a `.zip` with `manifest.json`, stage 4) list their
//! files by project and file number only, with no name, folder or hash: the
//! catalog asks the site for each file's name, keeps the file, and places it
//! by what it is ([`folder_of`]): a `.jar` in `mods/`, a zip with `shaders/`
//! in `shaderpacks/`, one with `pack.mcmeta` in `resourcepacks/`. Anything
//! else is left out and counted.
//!
//! Everything in the index is checked: the game and one loader with plain
//! version words, at most `MAX_FILES` files of at most `MAX_TOTAL` bytes, both
//! hashes of each, a download from a host Modrinth allows. A path must stay
//! in the game folder and in a folder a pack may fill ([`pack_path`]); a file
//! with any other path, or one the client does not use, is left out and
//! counted. The overrides in the archive are checked by the launcher by the
//! same rules when it unpacks them.

use std::collections::HashMap;
use std::io::Read as _;
use std::path::Path;

use serde::Deserialize;

use crate::model::word;

pub const INDEX: &str = "modrinth.index.json";
/// A CurseForge pack's list.
pub const MANIFEST: &str = "manifest.json";
const MAX_INDEX: u64 = 16 * 1024 * 1024;
pub const MAX_FILES: usize = 5000;
/// All the files a pack lists, together.
pub const MAX_TOTAL: u64 = 16 * 1024 * 1024 * 1024;
const MAX_PATH: usize = 400;
const MAX_DEPTH: usize = 16;

/// The folders of the game folder a pack may put files into; the launcher
/// has the same list.
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
pub const PROGRAMS: &[&str] = &[
    "exe", "bat", "cmd", "com", "scr", "msi", "ps1", "psm1", "vbs", "vbe", "wsf", "lnk", "sh",
    "command", "dll", "so", "dylib", "jnilib", "reg", "hta", "cpl", "app",
];

/// The hosts a pack's files may come from, as Modrinth allows them.
const DOWNLOAD_HOSTS: &[&str] = &[
    "cdn.modrinth.com",
    "github.com",
    "raw.githubusercontent.com",
    "gitlab.com",
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
    let name = name.strip_suffix(".disabled").unwrap_or(&name);
    let extension = name.rsplit_once('.').map_or("", |(_, e)| e);
    placed && !PROGRAMS.contains(&extension) && (extension != "jar" || parts[0] == "mods")
}

/// A download address of a pack's file we may fetch.
pub fn download_host(url: &str) -> bool {
    let Ok(parsed) = reqwest::Url::parse(url) else {
        return false;
    };
    url.len() <= 2048
        && parsed.scheme() == "https"
        && parsed.port().is_none()
        && parsed.username().is_empty()
        && parsed.password().is_none()
        && (parsed
            .host_str()
            .is_some_and(|h| DOWNLOAD_HOSTS.contains(&h))
            // A packwiz pack's CurseForge mod, by its project and file.
            || crate::curseforge::download_ids(url).is_some())
}

fn hex(text: &str, len: usize) -> bool {
    text.len() == len
        && text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// A file of a pack as we keep it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackFile {
    pub path: String,
    pub size: u64,
    pub sha1: String,
    pub sha512: String,
    pub url: String,
    /// The pack marks it optional for the client.
    pub optional: bool,
}

/// What a pack's index says, checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Index {
    pub game: String,
    /// fabric, quilt, forge or neoforge, with its version.
    pub loader: Option<(String, String)>,
    pub files: Vec<PackFile>,
    /// A CurseForge pack's files, found once each is asked for.
    pub curseforge: Vec<CurseForgeFile>,
    /// Files left out: a path a pack may not write, or not for the client.
    pub skipped: u64,
}

/// A file a CurseForge pack lists: the site's project and file numbers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurseForgeFile {
    pub project: u64,
    pub file: u64,
    /// The pack has it turned off (`required: false`).
    pub optional: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawIndex {
    format_version: u64,
    game: String,
    #[serde(default)]
    files: Vec<RawFile>,
    #[serde(default)]
    dependencies: HashMap<String, String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawFile {
    path: String,
    #[serde(default)]
    hashes: HashMap<String, String>,
    #[serde(default)]
    env: Option<RawEnv>,
    #[serde(default)]
    downloads: Vec<String>,
    file_size: u64,
}

#[derive(Deserialize)]
struct RawEnv {
    #[serde(default)]
    client: String,
}

/// Reads an index's text; a short code when it is not one we take.
pub fn index(text: &[u8]) -> Result<Index, &'static str> {
    parse_index(text, false)
}

/// The index of a player's own pack (D-066, stage 6): its files are found
/// here by their hashes, so where it says they download from is not used;
/// a file it would leave out refuses the pack instead.
pub fn own_index(text: &[u8]) -> Result<Index, &'static str> {
    let index = parse_index(text, true)?;
    if index.skipped > 0 {
        return Err("bad_pack");
    }
    Ok(index)
}

fn parse_index(text: &[u8], own: bool) -> Result<Index, &'static str> {
    let raw: RawIndex = serde_json::from_slice(text).map_err(|_| "bad_pack")?;
    if raw.format_version != 1 || raw.game != "minecraft" {
        return Err("unsupported_pack");
    }
    if raw.files.len() > MAX_FILES || raw.dependencies.len() > 4 {
        return Err("too_big");
    }
    let mut game = None;
    let mut loader = None;
    for (name, version) in &raw.dependencies {
        let version = word(version).ok_or("bad_pack")?;
        let kind = match name.as_str() {
            "minecraft" => {
                game = Some(version);
                continue;
            }
            "fabric-loader" => "fabric",
            "quilt-loader" => "quilt",
            "forge" => "forge",
            "neoforge" => "neoforge",
            _ => return Err("unsupported_pack"),
        };
        if loader.is_some() {
            return Err("unsupported_pack");
        }
        loader = Some((kind.to_owned(), version));
    }
    let game = game.ok_or("bad_pack")?;
    let mut files = Vec::new();
    let mut skipped = 0;
    let mut total = 0u64;
    let mut paths = std::collections::HashSet::new();
    for file in raw.files {
        let client = file.env.as_ref().map_or("required", |e| e.client.as_str());
        if client == "unsupported" || !pack_path(&file.path) {
            skipped += 1;
            continue;
        }
        if !paths.insert(file.path.to_ascii_lowercase()) {
            return Err("bad_pack");
        }
        let (Some(sha1), Some(sha512)) = (file.hashes.get("sha1"), file.hashes.get("sha512"))
        else {
            return Err("bad_pack");
        };
        if !hex(sha1, 40) || !hex(sha512, 128) || file.file_size == 0 {
            return Err("bad_pack");
        }
        if file.file_size > crate::import::MAX_FILE {
            return Err("too_big");
        }
        total = total.saturating_add(file.file_size);
        if total > MAX_TOTAL {
            return Err("too_big");
        }
        let url = if own {
            String::new()
        } else {
            file.downloads
                .iter()
                .take(8)
                .find(|u| download_host(u))
                .ok_or("unsupported_host")?
                .clone()
        };
        files.push(PackFile {
            path: file.path,
            size: file.file_size,
            sha1: sha1.clone(),
            sha512: sha512.clone(),
            url,
            optional: client == "optional",
        });
    }
    Ok(Index {
        game,
        loader,
        files,
        curseforge: Vec::new(),
        skipped,
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawManifest {
    #[serde(default)]
    manifest_type: String,
    manifest_version: u64,
    minecraft: RawMinecraft,
    #[serde(default)]
    files: Vec<RawCurseForgeFile>,
    #[serde(default)]
    overrides: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawMinecraft {
    version: String,
    #[serde(default)]
    mod_loaders: Vec<RawLoader>,
}

#[derive(Deserialize)]
struct RawLoader {
    id: String,
    #[serde(default)]
    primary: bool,
}

#[derive(Deserialize)]
struct RawCurseForgeFile {
    #[serde(rename = "projectID")]
    project: u64,
    #[serde(rename = "fileID")]
    file: u64,
    #[serde(default = "yes")]
    required: bool,
}

fn yes() -> bool {
    true
}

/// A CurseForge loader name (`forge-47.2.0`, `neoforge-21.1.77`,
/// `fabric-0.15.7`, `quilt-0.26.0`) as a kind and a version.
fn curseforge_loader(id: &str) -> Option<(String, String)> {
    let (name, version) = id.split_once('-')?;
    let kind = match name.to_ascii_lowercase().as_str() {
        "forge" => "forge",
        "neoforge" => "neoforge",
        "fabric" => "fabric",
        "quilt" => "quilt",
        _ => return None,
    };
    Some((kind.to_owned(), word(version)?))
}

/// Reads a CurseForge pack's `manifest.json`; a short code when it is not
/// one we take. Its overrides must be in `overrides/`, where the launcher
/// looks for them.
pub fn manifest(text: &[u8]) -> Result<Index, &'static str> {
    let raw: RawManifest = serde_json::from_slice(text).map_err(|_| "bad_pack")?;
    if raw.manifest_type != "minecraftModpack" || raw.manifest_version != 1 {
        return Err("unsupported_pack");
    }
    if raw.overrides.as_deref().is_some_and(|o| o != "overrides") {
        return Err("unsupported_pack");
    }
    if raw.files.len() > MAX_FILES || raw.minecraft.mod_loaders.len() > 4 {
        return Err("too_big");
    }
    let game = word(&raw.minecraft.version).ok_or("bad_pack")?;
    let loader = match raw
        .minecraft
        .mod_loaders
        .iter()
        .find(|l| l.primary)
        .or_else(|| raw.minecraft.mod_loaders.first())
    {
        Some(l) => Some(curseforge_loader(&l.id).ok_or("unsupported_pack")?),
        None => None,
    };
    let mut seen = std::collections::HashSet::new();
    let mut curseforge = Vec::new();
    for f in raw.files {
        if f.project == 0 || f.file == 0 || f.project > MAX_ID || f.file > MAX_ID {
            return Err("bad_pack");
        }
        // The same file twice is one file.
        if seen.insert(f.file) {
            curseforge.push(CurseForgeFile {
                project: f.project,
                file: f.file,
                optional: !f.required,
            });
        }
    }
    Ok(Index {
        game,
        loader,
        files: Vec::new(),
        curseforge,
        skipped: 0,
    })
}

/// The largest project or file number taken (the site's are far smaller).
const MAX_ID: u64 = 1 << 40;

/// Where a CurseForge pack's file goes, by what it is: a `.jar` in `mods/`;
/// a zip with `shaders/` in `shaderpacks/`, one with `pack.mcmeta` and
/// `assets/` in `resourcepacks/`. `None`: left out (a data pack, or
/// something else).
pub fn folder_of(path: &Path, name: &str) -> Option<&'static str> {
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(".jar") {
        return Some("mods");
    }
    if !lower.ends_with(".zip") {
        return None;
    }
    let file = std::fs::File::open(path).ok()?;
    let zip = zip::ZipArchive::new(std::io::BufReader::new(file)).ok()?;
    let (mut shaders, mut mcmeta, mut assets) = (false, false, false);
    for name in zip.file_names().take(200_000) {
        // At the top, or one folder down (packs zipped with their folder).
        let parts: Vec<&str> = name.splitn(3, '/').collect();
        let holds = |dir: &str| {
            (parts.len() > 1 && parts[0] == dir) || (parts.len() > 2 && parts[1] == dir)
        };
        shaders |= holds("shaders");
        assets |= holds("assets");
        mcmeta |= name == "pack.mcmeta" || (parts.len() == 2 && parts[1] == "pack.mcmeta");
    }
    if shaders {
        Some("shaderpacks")
    } else if mcmeta && assets {
        Some("resourcepacks")
    } else {
        None
    }
}

/// Reads the list out of a kept pack: a `.mrpack`'s index, or a CurseForge
/// pack's manifest.
pub fn read(archive: &Path) -> Result<Index, &'static str> {
    let file = std::fs::File::open(archive).map_err(|_| "disk_error")?;
    let mut zip = zip::ZipArchive::new(std::io::BufReader::new(file)).map_err(|_| "bad_pack")?;
    let curseforge = zip.index_for_name(INDEX).is_none();
    let mut entry = zip
        .by_name(if curseforge { MANIFEST } else { INDEX })
        .map_err(|_| "bad_pack")?;
    if entry.size() > MAX_INDEX {
        return Err("too_big");
    }
    let mut text = Vec::new();
    entry
        .by_ref()
        .take(MAX_INDEX + 1)
        .read_to_end(&mut text)
        .map_err(|_| "bad_pack")?;
    if text.len() as u64 > MAX_INDEX {
        return Err("too_big");
    }
    if curseforge {
        manifest(&text)
    } else {
        index(&text)
    }
}
