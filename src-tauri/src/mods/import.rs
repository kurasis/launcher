//! Installations from other launchers and from folders (D-066 stage 5): a
//! Prism Launcher or MultiMC instance (exported as a zip, or its folder), or
//! a folder or zip that holds a game folder with `mods/`, or just `.jar`
//! files. It becomes a new installation of ours with the files copied in.
//!
//! - The player picks the file or folder in the native dialog; the core keeps
//!   what was picked, so the window never names a path, and copies only into
//!   the installation made for it.
//! - Only what [`pack_path`] accepts is copied (content folders, options,
//!   no programs, a `.jar` only in `mods/`); links are never followed or
//!   made, and counts, depth and sizes are bounded (a zip bomb stops there).
//! - The game version and loader come from Prism's `mmc-pack.json`; for a
//!   plain folder they are guessed from the mods' own metadata
//!   (`fabric.mod.json`, `quilt.mod.json`, `META-INF/mods.toml`,
//!   `META-INF/neoforge.mods.toml`, `mcmod.info`), and the player can change
//!   them before the installation is made.
//! - The catalog knows the copied mods by their hash afterwards, as it does
//!   any file of the player's.

use std::collections::HashMap;
use std::fs;
use std::io::{Read, Seek, Write};
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::Value;

use super::content::Progress;
use super::pack::pack_path;
use crate::download::CancelToken;
use crate::error::{ErrorCode, LauncherError, LauncherResult};
use crate::loader::LoaderKind;
use crate::storage::paths::{InstanceId, is_within};

/// Entries of a zip or folder looked at.
const MAX_ENTRIES: usize = 200_000;
/// Files copied.
const MAX_FILES: usize = 50_000;
/// Everything copied, in all (worlds can be large).
const MAX_TOTAL: u64 = 32 * 1024 * 1024 * 1024;
/// Folders looked into below the game folder.
const MAX_DEPTH: usize = 16;
/// Mods read for the game version and loader.
const MAX_JARS: usize = 400;
/// A mod inside a zip read whole for that.
const MAX_JAR: u64 = 64 * 1024 * 1024;
/// A metadata file inside a mod.
const MAX_META: u64 = 1024 * 1024;
/// Prism's own files.
const MAX_CONFIG: u64 = 1024 * 1024;

fn unreadable() -> LauncherError {
    LauncherError::invalid_input(
        "The launcher found no installation it can bring in here: no Prism Launcher or MultiMC instance, mods folder or game folder.",
    )
}

fn too_large() -> LauncherError {
    LauncherError::invalid_input(
        "This has too many files to bring in. Choose the instance or its mods folder.",
    )
}

fn disk(e: &std::io::Error) -> LauncherError {
    LauncherError::from_io(e, "Bringing in the installation")
}

fn cancelled() -> LauncherError {
    LauncherError::new(ErrorCode::OperationCancelled, "The download was cancelled.")
}

/// What was found, for the window.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Found {
    /// `prism` (Prism Launcher or MultiMC) or `folder`.
    pub format: &'static str,
    /// A name for the installation: the instance's, or the folder's.
    pub name: String,
    /// The Minecraft version, when known or guessed.
    pub game: Option<String>,
    pub loader: Option<LoaderKind>,
    /// True when the version and loader were guessed from the mods.
    pub guessed: bool,
    pub files: u64,
    pub bytes: u64,
    pub mods: u32,
    /// Files that are not game content (logs, programs, the launcher's own).
    pub left_out: u64,
}

/// Where a file comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Origin {
    Zip(usize),
    Path(PathBuf),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Item {
    from: Origin,
    /// Its path in the game folder.
    to: String,
    size: u64,
}

/// A source picked and read: kept by the core until an installation is made
/// from it.
#[derive(Debug, Clone)]
pub struct Picked {
    path: PathBuf,
    zip: bool,
    items: Vec<Item>,
    pub found: Found,
    /// The loader's version as the source names it (Prism's components).
    pub loader_hint: Option<String>,
    /// The installation made for it; only that one is filled.
    pub instance: Option<InstanceId>,
}

/// An entry of the source: its path with `/`, size, and whether it is a file.
struct Entry {
    name: String,
    size: u64,
    file: bool,
    from: Origin,
}

/// Every entry of a zip, links left out.
fn zip_entries<R: Read + Seek>(zip: &mut zip::ZipArchive<R>) -> LauncherResult<Vec<Entry>> {
    if zip.len() > MAX_ENTRIES {
        return Err(too_large());
    }
    let mut out = Vec::with_capacity(zip.len());
    for index in 0..zip.len() {
        let entry = zip.by_index_raw(index).map_err(|_| unreadable())?;
        if entry.is_symlink() {
            continue;
        }
        let name = entry.name().replace('\\', "/");
        let file = !entry.is_dir();
        out.push(Entry {
            name: name.trim_end_matches('/').to_owned(),
            size: entry.size(),
            file,
            from: Origin::Zip(index),
        });
    }
    Ok(out)
}

/// The entries of a folder that could matter: its top two levels, and below
/// a game folder only the folders content goes in. Links are never followed.
fn folder_entries(base: &Path) -> LauncherResult<Vec<Entry>> {
    let mut out = Vec::new();
    let mut stack: Vec<(PathBuf, String, usize)> = vec![(base.to_path_buf(), String::new(), 0)];
    while let Some((dir, rel, depth)) = stack.pop() {
        let Ok(read) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in read.flatten() {
            if out.len() >= MAX_ENTRIES {
                return Err(too_large());
            }
            let Ok(meta) = fs::symlink_metadata(entry.path()) else {
                continue;
            };
            if meta.file_type().is_symlink() {
                continue;
            }
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            let path = if rel.is_empty() {
                name.clone()
            } else {
                format!("{rel}/{name}")
            };
            if meta.is_dir() {
                // Deeper only where content can be: the first levels, and
                // the content folders of a game folder at any of them.
                let parts: Vec<&str> = path.split('/').collect();
                let content = parts.iter().any(|p| super::pack::ROOTS.contains(p));
                if depth < 3 || (content && depth < MAX_DEPTH + 3) {
                    stack.push((entry.path(), path.clone(), depth + 1));
                }
            }
            out.push(Entry {
                name: path,
                size: meta.len(),
                file: meta.is_file(),
                from: Origin::Path(entry.path()),
            });
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

/// How the source holds the game's files.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Layout {
    /// A Prism or MultiMC instance: its folder (`""` or `x/`), and its game
    /// folder within.
    Prism { instance: String, game: String },
    /// A game folder (`""` or `x/`).
    Game(String),
    /// `.jar` files in a folder (`""` or `x/`): they go into `mods/`.
    Mods(String),
}

fn layout(entries: &[Entry]) -> Option<Layout> {
    let has = |path: &str| entries.iter().any(|e| e.name == path);
    let has_under = |prefix: &str| entries.iter().any(|e| e.name.starts_with(prefix));
    // Folders at the top, the shallowest first.
    let mut tops: Vec<String> = vec![String::new()];
    // Zips need not list their folders: they are the first part of a path.
    let mut firsts: Vec<String> = entries
        .iter()
        .filter_map(|e| e.name.split_once('/').map(|(first, _)| first.to_owned()))
        .chain(
            entries
                .iter()
                .filter(|e| !e.file && !e.name.contains('/'))
                .map(|e| e.name.clone()),
        )
        .filter(|f| f != "__MACOSX")
        .collect();
    firsts.sort();
    firsts.dedup();
    tops.extend(firsts.into_iter().map(|f| format!("{f}/")));
    for top in &tops {
        if has(&format!("{top}instance.cfg")) || has(&format!("{top}mmc-pack.json")) {
            let game = [".minecraft/", "minecraft/"]
                .iter()
                .map(|g| format!("{top}{g}"))
                .find(|g| has_under(g))
                .unwrap_or_else(|| format!("{top}.minecraft/"));
            return Some(Layout::Prism {
                instance: top.clone(),
                game,
            });
        }
    }
    let mut games: Vec<String> = Vec::new();
    for top in &tops {
        games.push(top.clone());
        games.push(format!("{top}.minecraft/"));
        games.push(format!("{top}minecraft/"));
    }
    if let Some(game) = games.into_iter().find(|g| has_under(&format!("{g}mods/"))) {
        return Some(Layout::Game(game));
    }
    let jars = |prefix: &str| {
        entries.iter().any(|e| {
            e.file
                && e.name
                    .strip_prefix(prefix)
                    .is_some_and(|rest| !rest.contains('/') && rest.ends_with(".jar"))
        })
    };
    tops.iter()
        .find(|t| jars(t))
        .map(|t| Layout::Mods(t.clone()))
}

/// Where an entry goes in the game folder, if it is game content.
fn destination(layout: &Layout, name: &str) -> Option<String> {
    let to = match layout {
        Layout::Prism { game, .. } | Layout::Game(game) => {
            name.strip_prefix(game.as_str())?.to_owned()
        }
        Layout::Mods(prefix) => {
            let rest = name.strip_prefix(prefix.as_str())?;
            let lower = rest.to_ascii_lowercase();
            if rest.contains('/') || !(lower.ends_with(".jar") || lower.ends_with(".jar.disabled"))
            {
                return None;
            }
            format!("mods/{rest}")
        }
    };
    pack_path(&to).then_some(to)
}

/// Whether an entry belongs to what is brought in at all (to count what is
/// left out of it).
fn inside(layout: &Layout, name: &str) -> bool {
    match layout {
        Layout::Prism { game, .. } | Layout::Game(game) => name.starts_with(game.as_str()),
        Layout::Mods(prefix) => name
            .strip_prefix(prefix.as_str())
            .is_some_and(|rest| !rest.contains('/')),
    }
}

/// The name for the installation: what is left of `raw` after unsafe
/// characters, at most 64 characters.
fn clean_name(raw: &str) -> String {
    raw.chars()
        .filter(|c| !c.is_control())
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(64)
        .collect()
}

/// The `name=` of a Prism `instance.cfg`.
fn instance_name(cfg: &str) -> Option<String> {
    cfg.lines()
        .filter_map(|l| l.trim().strip_prefix("name="))
        .map(clean_name)
        .find(|n| !n.is_empty())
}

/// The game version, loader and its version of a Prism `mmc-pack.json`.
pub(super) fn prism_components(text: &str) -> (Option<String>, Option<LoaderKind>, Option<String>) {
    let value: Value = serde_json::from_str(text).unwrap_or(Value::Null);
    let mut game = None;
    let mut loader = None;
    for component in value["components"].as_array().into_iter().flatten() {
        let uid = component["uid"].as_str().unwrap_or_default();
        let version = component["version"]
            .as_str()
            .filter(|v| crate::loader::valid_version(v))
            .map(str::to_owned);
        let kind = match uid {
            "net.minecraft" => {
                game = version.filter(|v| crate::loader::valid_version(v));
                continue;
            }
            "net.fabricmc.fabric-loader" => LoaderKind::Fabric,
            "org.quiltmc.quilt-loader" => LoaderKind::Quilt,
            "net.minecraftforge" => LoaderKind::Forge,
            "net.neoforged" => LoaderKind::NeoForge,
            _ => continue,
        };
        // Quilt runs Fabric's mods; an instance with both is Quilt's.
        if loader.as_ref().is_none_or(|(k, _)| *k != LoaderKind::Quilt) {
            loader = Some((kind, version));
        }
    }
    match loader {
        Some((kind, hint)) => (game, Some(kind), hint),
        None => (game, None, None),
    }
}

/// The first game version in a dependency range: `1.20.1` of `>=1.20.1`,
/// `[1.20.1,1.21)` or `~1.20.1`; `26.1` of `26.1.x`.
pub(super) fn first_version(range: &str) -> Option<String> {
    let bytes = range.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i].is_ascii_digit() && (i == 0 || !bytes[i - 1].is_ascii_alphanumeric()) {
            let start = i;
            let mut parts = 0;
            let mut end = i;
            loop {
                let digits = bytes[end..]
                    .iter()
                    .take_while(|b| b.is_ascii_digit())
                    .count();
                if digits == 0 {
                    break;
                }
                end += digits;
                parts += 1;
                if bytes.get(end) == Some(&b'.')
                    && bytes.get(end + 1).is_some_and(u8::is_ascii_digit)
                {
                    end += 1;
                } else {
                    break;
                }
            }
            let text = &range[start..end];
            let major: u32 = text.split('.').next()?.parse().ok()?;
            if parts >= 2 && (major == 1 || (20..100).contains(&major)) {
                return Some(text.to_owned());
            }
            i = end.max(i + 1);
        } else {
            i += 1;
        }
    }
    None
}

/// What a mod says it runs on.
fn read_mod<R: Read + Seek>(jar: R) -> Option<(LoaderKind, Option<String>)> {
    let mut zip = zip::ZipArchive::new(jar).ok()?;
    let mut text = |name: &str| -> Option<String> {
        let entry = zip.by_name(name).ok()?;
        if entry.size() > MAX_META {
            return None;
        }
        let mut out = String::new();
        entry.take(MAX_META).read_to_string(&mut out).ok()?;
        Some(out)
    };
    if let Some(json) = text("quilt.mod.json") {
        let value: Value = serde_json::from_str(&json).unwrap_or(Value::Null);
        let game = value["quilt_loader"]["depends"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|d| d["id"] == "minecraft")
            .and_then(|d| first_version(&d["versions"].to_string()));
        return Some((LoaderKind::Quilt, game));
    }
    if let Some(json) = text("fabric.mod.json") {
        let value: Value = serde_json::from_str(&json).unwrap_or(Value::Null);
        let game = first_version(&value["depends"]["minecraft"].to_string());
        return Some((LoaderKind::Fabric, game));
    }
    if let Some(toml) = text("META-INF/neoforge.mods.toml") {
        return Some((LoaderKind::NeoForge, toml_game(&toml)));
    }
    if let Some(toml) = text("META-INF/mods.toml") {
        let neo = toml.lines().any(|l| {
            let l: String = l.chars().filter(|c| !c.is_whitespace()).collect();
            l == "modId=\"neoforge\""
        });
        let kind = if neo {
            LoaderKind::NeoForge
        } else {
            LoaderKind::Forge
        };
        return Some((kind, toml_game(&toml)));
    }
    if let Some(json) = text("mcmod.info") {
        let value: Value = serde_json::from_str(&json).unwrap_or(Value::Null);
        let list = value
            .as_array()
            .or_else(|| value["modList"].as_array())
            .cloned()
            .unwrap_or_default();
        let game = list
            .iter()
            .find_map(|m| m["mcversion"].as_str().and_then(first_version));
        return Some((LoaderKind::Forge, game));
    }
    None
}

/// The `versionRange` of the `minecraft` dependency of a `mods.toml`.
pub(super) fn toml_game(toml: &str) -> Option<String> {
    let mut in_minecraft = false;
    for line in toml.lines() {
        let line: String = line.chars().filter(|c| !c.is_whitespace()).collect();
        if line.starts_with('[') {
            in_minecraft = false;
        } else if line == "modId=\"minecraft\"" || line == "modId='minecraft'" {
            in_minecraft = true;
        } else if in_minecraft && let Some(range) = line.strip_prefix("versionRange=") {
            return first_version(range);
        }
    }
    None
}

/// The game version and loader most of the mods name.
fn guess(votes: &[(LoaderKind, Option<String>)]) -> (Option<String>, Option<LoaderKind>) {
    let mut kinds: HashMap<LoaderKind, usize> = HashMap::new();
    let mut games: HashMap<&str, usize> = HashMap::new();
    for (kind, game) in votes {
        *kinds.entry(*kind).or_default() += 1;
        if let Some(game) = game {
            *games.entry(game.as_str()).or_default() += 1;
        }
    }
    // Quilt runs Fabric's mods: one Quilt mod makes it Quilt.
    let kind = if kinds.contains_key(&LoaderKind::Quilt) {
        Some(LoaderKind::Quilt)
    } else {
        kinds
            .into_iter()
            .max_by_key(|(k, n)| (*n, k.as_str()))
            .map(|(k, _)| k)
    };
    let game = games
        .into_iter()
        .max_by(|(a, x), (b, y)| x.cmp(y).then_with(|| b.cmp(a)))
        .map(|(g, _)| g.to_owned());
    (game, kind)
}

/// Reads a picked file (a zip) or folder.
pub fn read(path: &Path) -> LauncherResult<Picked> {
    let meta = fs::metadata(path).map_err(|e| disk(&e))?;
    let zip = meta.is_file();
    let mut archive = if zip {
        let file = fs::File::open(path).map_err(|e| disk(&e))?;
        Some(zip::ZipArchive::new(std::io::BufReader::new(file)).map_err(|_| unreadable())?)
    } else {
        None
    };
    let entries = match archive.as_mut() {
        Some(archive) => zip_entries(archive)?,
        None => folder_entries(path)?,
    };
    let layout = layout(&entries).ok_or_else(unreadable)?;

    let mut items = Vec::new();
    let mut left_out = 0u64;
    let mut bytes = 0u64;
    for entry in entries.iter().filter(|e| e.file) {
        if !inside(&layout, &entry.name) {
            continue;
        }
        match destination(&layout, &entry.name) {
            Some(to) => {
                bytes = bytes.saturating_add(entry.size);
                items.push(Item {
                    from: entry.from.clone(),
                    to,
                    size: entry.size,
                });
                if items.len() > MAX_FILES || bytes > MAX_TOTAL {
                    return Err(too_large());
                }
            }
            None => left_out += 1,
        }
    }
    if items.is_empty() {
        return Err(unreadable());
    }

    // Prism's own files.
    let mut config = |name: &str| -> Option<String> {
        let entry = entries.iter().find(|e| e.file && e.name == name)?;
        if entry.size > MAX_CONFIG {
            return None;
        }
        let mut out = String::new();
        match (&entry.from, archive.as_mut()) {
            (Origin::Zip(index), Some(archive)) => {
                let file = archive.by_index(*index).ok()?;
                file.take(MAX_CONFIG).read_to_string(&mut out).ok()?;
            }
            (Origin::Path(p), _) => {
                fs::File::open(p)
                    .ok()?
                    .take(MAX_CONFIG)
                    .read_to_string(&mut out)
                    .ok()?;
            }
            _ => return None,
        }
        Some(out)
    };
    let fallback_name = path
        .file_stem()
        .and_then(|s| s.to_str())
        .map(clean_name)
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| "Imported".to_owned());
    let (format, name, mut game, mut loader, loader_hint) = match &layout {
        Layout::Prism { instance, .. } => {
            let name = config(&format!("{instance}instance.cfg"))
                .and_then(|c| instance_name(&c))
                .or_else(|| {
                    let folder = instance.trim_end_matches('/');
                    (!folder.is_empty()).then(|| clean_name(folder))
                })
                .unwrap_or(fallback_name);
            let (game, loader, hint) = config(&format!("{instance}mmc-pack.json"))
                .map(|t| prism_components(&t))
                .unwrap_or_default();
            ("prism", name, game, loader, hint)
        }
        Layout::Game(top) | Layout::Mods(top) => {
            let folder = top.split('/').next().unwrap_or_default();
            let name = if folder.is_empty() || folder.starts_with('.') || folder == "minecraft" {
                fallback_name
            } else {
                clean_name(folder)
            };
            ("folder", name, None, None, None)
        }
    };

    let jars: Vec<&Item> = items
        .iter()
        .filter(|i| i.to.starts_with("mods/") && i.to.to_ascii_lowercase().ends_with(".jar"))
        .collect();
    let mods = u32::try_from(jars.len()).unwrap_or(u32::MAX);
    let mut guessed = false;
    if game.is_none() || (loader.is_none() && format == "folder") {
        let mut votes = Vec::new();
        for item in jars.iter().take(MAX_JARS) {
            let vote = match (&item.from, archive.as_mut()) {
                (Origin::Zip(index), Some(archive)) if item.size <= MAX_JAR => {
                    let mut bytes = Vec::new();
                    archive
                        .by_index(*index)
                        .ok()
                        .and_then(|f| f.take(MAX_JAR).read_to_end(&mut bytes).ok())
                        .and_then(|_| read_mod(std::io::Cursor::new(bytes)))
                }
                (Origin::Path(p), _) => fs::File::open(p)
                    .ok()
                    .and_then(|f| read_mod(std::io::BufReader::new(f))),
                _ => None,
            };
            votes.extend(vote);
        }
        let (by_mods_game, by_mods_loader) = guess(&votes);
        if game.is_none() {
            game = by_mods_game.filter(|g| crate::loader::valid_version(g));
            guessed = true;
        }
        if loader.is_none() && format == "folder" {
            loader = by_mods_loader;
            guessed = true;
        }
    }

    Ok(Picked {
        path: path.to_path_buf(),
        zip,
        found: Found {
            format,
            name,
            game,
            loader,
            guessed,
            files: items.len() as u64,
            bytes,
            mods,
            left_out,
        },
        items,
        loader_hint,
        instance: None,
    })
}

/// The loader version to install: the one the source names when the list
/// has it (Forge's `47.2.0` is `1.20.1-47.2.0` there), else the recommended
/// one, else the newest stable one, else the newest.
pub fn pick_loader_version(
    list: &[crate::loader::meta::LoaderVersion],
    hint: Option<&str>,
) -> Option<String> {
    if let Some(hint) = hint
        && let Some(found) = list
            .iter()
            .find(|v| v.id == hint || v.label == hint || v.id.ends_with(&format!("-{hint}")))
    {
        return Some(found.id.clone());
    }
    list.iter()
        .find(|v| v.recommended)
        .or_else(|| list.iter().find(|v| v.stable))
        .or_else(|| list.first())
        .map(|v| v.id.clone())
}

/// What bringing in copied.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub files: u64,
    /// Files already there (from an earlier try) that were kept.
    pub kept: u64,
    pub left_out: u64,
}

/// Copies the picked files into a game folder. A file already there with
/// the same size (an earlier try that stopped) is kept; each file is
/// written beside its place and renamed in.
pub fn copy(
    picked: &Picked,
    game: &Path,
    id: InstanceId,
    cancel: &CancelToken,
    emit: &(dyn Fn(Progress) + Send + Sync),
) -> LauncherResult<Report> {
    let total = picked.items.len() as u64;
    let say = |files: u64, bytes: u64| {
        emit(Progress {
            instance_id: id.to_string(),
            phase: "unpacking",
            files,
            total_files: total,
            bytes,
            total_bytes: picked.found.bytes,
        });
    };
    let mut archive = if picked.zip {
        let file = fs::File::open(&picked.path).map_err(|e| disk(&e))?;
        Some(zip::ZipArchive::new(std::io::BufReader::new(file)).map_err(|_| unreadable())?)
    } else {
        None
    };
    fs::create_dir_all(game).map_err(|e| disk(&e))?;
    let mut report = Report {
        files: 0,
        kept: 0,
        left_out: picked.found.left_out,
    };
    let mut copied = 0u64;
    let mut buffer = vec![0u8; 64 * 1024];
    for (n, item) in picked.items.iter().enumerate() {
        if cancel.is_cancelled() {
            return Err(cancelled());
        }
        if n % 50 == 0 {
            say(n as u64, copied);
        }
        let target: PathBuf = item
            .to
            .split('/')
            .fold(game.to_path_buf(), |p, s| p.join(s));
        if !is_within(game, &target) {
            report.left_out += 1;
            continue;
        }
        if let Ok(meta) = fs::symlink_metadata(&target) {
            if meta.is_file() && meta.len() == item.size {
                report.kept += 1;
                copied += item.size;
                continue;
            }
            // Something else in the way: left as it is.
            report.left_out += 1;
            continue;
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|e| disk(&e))?;
            // A link in the way is never written through.
            if fs::symlink_metadata(parent).is_ok_and(|m| m.file_type().is_symlink()) {
                report.left_out += 1;
                continue;
            }
        }
        let part = target.with_file_name(format!(
            ".{}.part",
            target
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("file")
        ));
        let result = (|| -> LauncherResult<u64> {
            let mut out = fs::File::create(&part).map_err(|e| disk(&e))?;
            let mut reader: Box<dyn Read> = match (&item.from, archive.as_mut()) {
                (Origin::Zip(index), Some(archive)) => {
                    let mut entry = archive.by_index(*index).map_err(|_| unreadable())?;
                    // Read here: the entry borrows the archive.
                    let mut written = 0u64;
                    loop {
                        if cancel.is_cancelled() {
                            return Err(cancelled());
                        }
                        let read = entry.read(&mut buffer).map_err(|_| unreadable())?;
                        if read == 0 {
                            break;
                        }
                        written += read as u64;
                        if written > item.size.max(1) * 2 + 1024 || written > MAX_TOTAL {
                            return Err(too_large());
                        }
                        out.write_all(&buffer[..read]).map_err(|e| disk(&e))?;
                    }
                    out.sync_all().map_err(|e| disk(&e))?;
                    return Ok(written);
                }
                (Origin::Path(p), _) => {
                    // Not through a link that appeared since it was read.
                    let meta = fs::symlink_metadata(p).map_err(|e| disk(&e))?;
                    if !meta.is_file() {
                        return Err(unreadable());
                    }
                    Box::new(fs::File::open(p).map_err(|e| disk(&e))?)
                }
                _ => return Err(unreadable()),
            };
            let mut written = 0u64;
            loop {
                if cancel.is_cancelled() {
                    return Err(cancelled());
                }
                let read = reader.read(&mut buffer).map_err(|e| disk(&e))?;
                if read == 0 {
                    break;
                }
                written += read as u64;
                if written > MAX_TOTAL {
                    return Err(too_large());
                }
                out.write_all(&buffer[..read]).map_err(|e| disk(&e))?;
            }
            out.sync_all().map_err(|e| disk(&e))?;
            Ok(written)
        })();
        match result {
            Ok(written) => {
                fs::rename(&part, &target).map_err(|e| {
                    let _ = fs::remove_file(&part);
                    disk(&e)
                })?;
                copied += written;
                report.files += 1;
            }
            Err(e) => {
                let _ = fs::remove_file(&part);
                return Err(e);
            }
        }
    }
    say(total, copied);
    Ok(report)
}
