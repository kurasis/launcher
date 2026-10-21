//! The "medium" mod set: popular mods from Modrinth for the game version and
//! loader under test, with the mods they require. Each file is checked against
//! Modrinth's SHA-512 and kept in a cache, so later runs download nothing.
//!
//! Each mod gets the newest version that fits the loader build under test: one
//! whose jar accepts that build (`versionRange` of `neoforge`/`forge` in its
//! `mods.toml`, `depends.fabricloader` in its `fabric.mod.json`), and that this
//! machine has not seen crash on it ([`Avoid`]). When the newest does not fit,
//! older releases come next, then newer pre-releases; with none, the mod is left out.

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::io::Read as _;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest as _, Sha512};

/// Asked for in this order; a mod with no file for the version and loader is
/// skipped. Libraries they need (Fabric API, Balm, Moonlight, …) come along.
/// No Iris: each release needs one exact Sodium, and the newest of both often
/// do not fit (Iris 1.8.12 and Sodium 0.8.13 on NeoForge 1.21.1 crash).
pub const MEDIUM: &[&str] = &[
    "fabric-api",
    "sodium",
    "lithium",
    "modmenu",
    "immediatelyfast",
    "entityculling",
    "ferrite-core",
    "jei",
    "jade",
    "appleskin",
    "journeymap",
    "xaeros-minimap",
    "mouse-tweaks",
    "controlling",
    "waystones",
    "travelersbackpack",
    "farmers-delight",
    "supplementaries",
    "storagedrawers",
    "sophisticated-backpacks",
    "create",
];

/// No more than this many files, libraries included.
const MAX_FILES: usize = 40;
const API: &str = "https://api.modrinth.com/v2";

/// One chosen file.
#[derive(Debug, Clone)]
pub struct ModFile {
    pub title: String,
    pub version: String,
    pub file_name: String,
    /// Modrinth's project and version ids.
    pub project: String,
    pub version_id: String,
    /// The mod ids its jar declares, as a crash names them.
    pub mod_ids: Vec<String>,
    url: String,
    sha512: String,
    size: u64,
}

/// What the resolution chose and what it left out, with why.
#[derive(Debug, Default)]
pub struct Resolved {
    pub files: Vec<ModFile>,
    pub skipped: Vec<String>,
    /// Versions passed over because their jar does not accept the loader build.
    pub older: Vec<String>,
    /// Versions passed over, or mods left out, because they crashed on this loader
    /// build here before: the scenario then passes only with a warning.
    pub replaced: Vec<String>,
}

/// A mod version that crashed the game on a loader build, kept on the test machine
/// (`<cache>/incompatible.json`) so later runs take another version at once.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bad {
    pub game: String,
    pub loader: String,
    pub loader_version: String,
    pub project: String,
    pub version_id: String,
    /// For the summary: "Sodium 0.9.2".
    pub shown: String,
}

/// The mod versions not to take for this game, loader and loader build.
#[derive(Debug, Default)]
pub struct Avoid {
    path: PathBuf,
    all: Vec<Bad>,
}

/// No more than this many remembered pairs; the oldest go first.
const MAX_BAD: usize = 500;

impl Avoid {
    /// The list kept in `cache`; a missing or damaged file is an empty list.
    pub fn load(cache: &Path) -> Self {
        let path = cache.join("incompatible.json");
        let all = std::fs::read(&path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        Self { path, all }
    }

    fn has(&self, game: &str, loader: &str, loader_version: &str, version_id: &str) -> bool {
        self.all.iter().any(|b| {
            b.game == game
                && b.loader == loader
                && b.loader_version == loader_version
                && b.version_id == version_id
        })
    }

    /// Remembers a pair and writes the list.
    pub fn add(&mut self, bad: Bad) {
        if !self.all.contains(&bad) {
            self.all.push(bad);
        }
        let over = self.all.len().saturating_sub(MAX_BAD);
        self.all.drain(..over);
        if let Some(dir) = self.path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let tmp = self.path.with_extension("part");
        if std::fs::write(
            &tmp,
            serde_json::to_vec_pretty(&self.all).unwrap_or_default(),
        )
        .is_ok()
        {
            let _ = std::fs::rename(&tmp, &self.path);
        }
    }
}

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent("kurasis/launcher game test (github.com/kurasis/launcher)")
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .expect("an HTTP client")
}

async fn get(client: &reqwest::Client, url: &str, query: &[(&str, String)]) -> Option<Value> {
    let mut url = url::Url::parse(url).ok()?;
    url.query_pairs_mut().extend_pairs(query);
    let response = client.get(url).send().await.ok()?;
    if !response.status().is_success() {
        return None;
    }
    response.json().await.ok()
}

/// Modrinth's loader filter: Quilt runs Fabric's mods, and most are published for
/// Fabric only.
fn loaders(loader: &str) -> String {
    if loader == "quilt" {
        "[\"quilt\",\"fabric\"]".to_owned()
    } else {
        format!("[\"{loader}\"]")
    }
}

/// A project's versions for this game and loader in the order they are tried: the
/// newest release (else the newest of any kind), older releases, then the
/// pre-releases newer than that release, newest first.
async fn candidates(
    client: &reqwest::Client,
    project: &str,
    game: &str,
    loader: &str,
) -> Option<Vec<Value>> {
    let versions = get(
        client,
        &format!("{API}/project/{project}/version"),
        &[
            ("loaders", loaders(loader)),
            ("game_versions", format!("[\"{game}\"]")),
        ],
    )
    .await?;
    let list = versions.as_array()?;
    if list.is_empty() {
        return None;
    }
    Some(order(list))
}

fn order(list: &[Value]) -> Vec<Value> {
    let release = |v: &Value| v["version_type"] == "release";
    let Some(first) = list.iter().position(release) else {
        return list.to_vec();
    };
    let mut out: Vec<Value> = list[first..]
        .iter()
        .filter(|v| release(v))
        .cloned()
        .collect();
    out.extend(list[..first].iter().cloned());
    out.extend(list[first..].iter().filter(|v| !release(v)).cloned());
    out
}

fn file_of(version: &Value, title: &str) -> Option<ModFile> {
    let files = version["files"].as_array()?;
    let file = files
        .iter()
        .find(|f| f["primary"] == true)
        .or_else(|| files.first())?;
    let url = file["url"].as_str()?;
    let name = file["filename"].as_str()?;
    // Only Modrinth's own CDN, and a plain jar name.
    let ok_name = name.ends_with(".jar")
        && name.len() <= 128
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._+-".contains(&b));
    if !url.starts_with("https://cdn.modrinth.com/") || !ok_name {
        return None;
    }
    Some(ModFile {
        title: title.to_owned(),
        version: version["version_number"].as_str().unwrap_or("?").to_owned(),
        file_name: name.to_owned(),
        project: version["project_id"].as_str()?.to_owned(),
        version_id: version["id"].as_str()?.to_owned(),
        mod_ids: Vec::new(),
        url: url.to_owned(),
        sha512: file["hashes"]["sha512"].as_str()?.to_ascii_lowercase(),
        size: file["size"].as_u64().unwrap_or(0),
    })
}

/// The loader under test, as the mods' jars name it.
pub struct Loader<'a> {
    /// `fabric`, `quilt`, `forge` or `neoforge`.
    pub kind: &'a str,
    /// The build, as the launcher names it (`52.1.0`, `26.3.0.95`, `0.16.10`).
    pub version: &'a str,
}

/// Chooses the medium set for this game version and loader build; the jars it
/// looks into stay in `cache`.
pub async fn resolve(game: &str, loader: &Loader<'_>, avoid: &Avoid, cache: &Path) -> Resolved {
    let client = client();
    let mut resolved = Resolved::default();
    let mut chosen: HashMap<String, Value> = HashMap::new();
    let mut queue: Vec<(String, String)> = MEDIUM
        .iter()
        .map(|s| ((*s).to_owned(), (*s).to_owned()))
        .collect();
    queue.reverse();
    let mut seen = HashSet::new();
    let build = loader_build(game, loader.version);
    // The titles of mods not taken, so the mods that need them go too.
    let mut left_out: Vec<String> = Vec::new();
    while let Some((project, title)) = queue.pop() {
        if !seen.insert(project.clone()) {
            continue;
        }
        if resolved.files.len() >= MAX_FILES {
            left_out.push(title);
            continue;
        }
        let Some(list) = candidates(&client, &project, game, loader.kind).await else {
            if MEDIUM.contains(&project.as_str()) {
                resolved
                    .skipped
                    .push(format!("{title}: no file for {game} {}", loader.kind));
            } else {
                resolved
                    .skipped
                    .push(format!("library {project} of another mod: not found"));
            }
            left_out.push(title);
            continue;
        };
        let id = list[0]["project_id"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
        if chosen.contains_key(&id) {
            continue;
        }
        // The first version that fits, noting each one passed over.
        let mut picked = None;
        let mut crashed: Vec<String> = Vec::new();
        let mut older: Vec<String> = Vec::new();
        for version in list.iter().take(MAX_TRIED) {
            let Some(mut file) = file_of(version, &title) else {
                continue;
            };
            if avoid.has(game, loader.kind, loader.version, &file.version_id) {
                crashed.push(file.version.clone());
                continue;
            }
            let Ok(path) = cached(&client, cache, &file).await else {
                continue;
            };
            let meta = jar_meta(&path);
            if let Some(range) = meta.refuses(loader.kind, &build) {
                older.push(format!("{} {range}", loader.kind));
                continue;
            }
            file.mod_ids = meta.ids;
            picked = Some((version.clone(), file));
            break;
        }
        let name = title.split(' ').next().unwrap_or(&title).to_owned();
        if !crashed.is_empty() {
            resolved.replaced.push(match &picked {
                Some((_, file)) => format!(
                    "{name} {} crashed on this loader build before: took {}",
                    crashed.join(", "),
                    file.version
                ),
                None => format!(
                    "{name} {} crashed on this loader build before and no other version fits: left out",
                    crashed.join(", ")
                ),
            });
        }
        // The newest refusal says what the mod wants.
        if let (Some(wants), Some((_, file))) = (older.first(), &picked) {
            resolved.older.push(format!(
                "{name} {} ({} newer need {wants})",
                file.version,
                older.len()
            ));
        }
        let Some((version, file)) = picked else {
            if crashed.is_empty() {
                resolved.skipped.push(format!(
                    "{title}: no version accepts {} {}{}",
                    loader.kind,
                    loader.version,
                    older.first().map_or(String::new(), |wants| format!(
                        " (the newest needs {wants})"
                    ))
                ));
            }
            left_out.push(title);
            continue;
        };
        // Mods that refuse each other: the one asked for first stays.
        let clash = |a: &Value, b_id: &str| {
            a["dependencies"].as_array().into_iter().flatten().any(|d| {
                d["dependency_type"] == "incompatible" && d["project_id"].as_str() == Some(b_id)
            })
        };
        if chosen
            .iter()
            .any(|(other, v)| clash(v, &id) || clash(&version, other))
        {
            resolved
                .skipped
                .push(format!("{title}: incompatible with a mod already chosen"));
            left_out.push(title);
            continue;
        }
        for dep in version["dependencies"].as_array().into_iter().flatten() {
            if dep["dependency_type"] == "required"
                && let Some(dep_project) = dep["project_id"].as_str()
            {
                queue.push((
                    dep_project.to_owned(),
                    format!("{dep_project} (needed by {title})"),
                ));
            }
        }
        chosen.insert(id, version);
        resolved.files.push(file);
    }
    // A library left out leaves the mods that need it unable to start: they go
    // too, and the mods that need those.
    let mut gone: Vec<String> = left_out;
    let mut i = 0;
    while i < gone.len() {
        if let Some(parent) = needed_by(&gone[i])
            && !gone.iter().any(|g| g == parent)
        {
            gone.push(parent.to_owned());
        }
        i += 1;
    }
    let before = resolved.files.len();
    let mut dropped = Vec::new();
    resolved.files.retain(|f| {
        let keep = !gone.contains(&f.title);
        if !keep {
            dropped.push(f.title.clone());
        }
        keep
    });
    if resolved.files.len() < before {
        resolved.skipped.push(format!(
            "{}: a library they need is left out",
            dropped.join(", ")
        ));
    }
    resolved
}

/// "lib (needed by Sodium)" → "Sodium"; the parent may be a library itself.
fn needed_by(title: &str) -> Option<&str> {
    let (_, rest) = title.split_once(" (needed by ")?;
    rest.strip_suffix(')')
}

/// Versions looked at per mod before it is left out.
const MAX_TRIED: usize = 8;

/// The loader build as the mods' jars name it: Forge's ids carry the game version
/// (`1.21.1-52.1.0`, `1.7.10-10.13.4.1614-1.7.10`), the jars name only `52.1.0`.
fn loader_build(game: &str, version: &str) -> String {
    version
        .split('-')
        .filter(|part| *part != game)
        .collect::<Vec<_>>()
        .join("-")
}

/// What a jar says about itself, as far as choosing a version needs.
#[derive(Debug, Default, PartialEq, Eq)]
struct JarMeta {
    /// The mod ids it declares.
    ids: Vec<String>,
    /// Its `forge`/`neoforge` dependency's `versionRange`.
    forge_range: Option<String>,
    /// Its `depends.fabricloader`: any one of them will do.
    fabric_ranges: Vec<String>,
}

/// No metadata file larger than this is read.
const MAX_META: u64 = 256 * 1024;
const MAX_IDS: usize = 16;

impl JarMeta {
    /// The range a jar asks for when it refuses this loader build; a range it
    /// cannot be read counts as accepting.
    fn refuses(&self, kind: &str, build: &str) -> Option<String> {
        match kind {
            "forge" | "neoforge" => {
                let range = self.forge_range.as_ref()?;
                (maven_accepts(range, build) == Some(false)).then(|| range.clone())
            }
            "fabric" => {
                if self.fabric_ranges.is_empty()
                    || self
                        .fabric_ranges
                        .iter()
                        .any(|r| fabric_accepts(r, build) != Some(false))
                {
                    None
                } else {
                    Some(self.fabric_ranges.join(" or "))
                }
            }
            // Quilt answers for Fabric's mods with its own idea of their versions.
            _ => None,
        }
    }
}

fn jar_meta(path: &Path) -> JarMeta {
    let Ok(file) = std::fs::File::open(path) else {
        return JarMeta::default();
    };
    let Ok(mut zip) = zip::ZipArchive::new(file) else {
        return JarMeta::default();
    };
    let mut text = |name: &str| -> Option<String> {
        let entry = zip.by_name(name).ok()?;
        if entry.size() > MAX_META {
            return None;
        }
        let mut out = String::new();
        entry.take(MAX_META).read_to_string(&mut out).ok()?;
        Some(out)
    };
    let mut meta = JarMeta::default();
    if let Some(json) = text("fabric.mod.json") {
        let value: Value = serde_json::from_str(&json).unwrap_or(Value::Null);
        meta.ids.extend(value["id"].as_str().map(str::to_owned));
        meta.fabric_ranges = match &value["depends"]["fabricloader"] {
            Value::String(s) => vec![s.clone()],
            Value::Array(list) => list
                .iter()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect(),
            _ => Vec::new(),
        };
    }
    if let Some(json) = text("quilt.mod.json") {
        let value: Value = serde_json::from_str(&json).unwrap_or(Value::Null);
        meta.ids
            .extend(value["quilt_loader"]["id"].as_str().map(str::to_owned));
    }
    let toml = text("META-INF/neoforge.mods.toml").or_else(|| text("META-INF/mods.toml"));
    if let Some(toml) = toml {
        let (ids, range) = toml_meta(&toml);
        meta.ids.extend(ids);
        meta.forge_range = range;
    }
    if let Some(json) = text("mcmod.info") {
        let value: Value = serde_json::from_str(&json).unwrap_or(Value::Null);
        let list = value
            .as_array()
            .or_else(|| value["modList"].as_array())
            .cloned()
            .unwrap_or_default();
        meta.ids.extend(
            list.iter()
                .filter_map(|m| m["modid"].as_str().map(str::to_owned)),
        );
    }
    meta.ids.retain(|id| {
        !id.is_empty()
            && id.len() <= 64
            && id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
    });
    meta.ids.dedup();
    meta.ids.truncate(MAX_IDS);
    meta
}

/// The mod ids of a `mods.toml` and the `versionRange` it asks of the loader.
fn toml_meta(toml: &str) -> (Vec<String>, Option<String>) {
    let mut ids = Vec::new();
    let mut range = None;
    // Each table's keys, read once the next table starts.
    let mut table = String::new();
    let mut keys: HashMap<String, String> = HashMap::new();
    let mut finish = |table: &str, keys: &mut HashMap<String, String>| {
        let id = keys.get("modId").cloned();
        if table == "[[mods]]" {
            ids.extend(id);
        } else if table.starts_with("[[dependencies.")
            && matches!(id.as_deref(), Some("forge" | "neoforge"))
            && range.is_none()
        {
            range = keys.get("versionRange").cloned();
        }
        keys.clear();
    };
    for line in toml.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            finish(&table, &mut keys);
            table = line.split('#').next().unwrap_or("").replace(' ', "");
        } else if let Some((key, value)) = line.split_once('=')
            && let Some(value) = toml_string(value.trim())
        {
            keys.insert(key.trim().to_owned(), value);
        }
    }
    finish(&table, &mut keys);
    (ids, range)
}

/// A quoted TOML string value, without its quotes and any comment after it.
fn toml_string(value: &str) -> Option<String> {
    let quote = value.chars().next().filter(|c| *c == '"' || *c == '\'')?;
    let rest = &value[1..];
    let end = rest.find(quote)?;
    Some(rest[..end].trim().to_owned())
}

/// Compares versions such as `52.1.0`, `26.1.2.10-beta` and `0.16.14+build.1`:
/// numbers part by part, missing parts as 0; with equal numbers, a version with
/// a qualifier (`-beta`) comes before the one without.
fn compare(a: &str, b: &str) -> Ordering {
    let split = |v: &str| -> (Vec<u64>, String) {
        let v = v.split('+').next().unwrap_or("").trim();
        let (core, qualifier) = v.split_once('-').unwrap_or((v, ""));
        let numbers = core
            .split('.')
            .map(|p| {
                p.chars()
                    .take_while(char::is_ascii_digit)
                    .take(18)
                    .collect::<String>()
                    .parse()
                    .unwrap_or(0)
            })
            .collect();
        (numbers, qualifier.to_owned())
    };
    let (an, aq) = split(a);
    let (bn, bq) = split(b);
    for i in 0..an.len().max(bn.len()) {
        let x = an.get(i).copied().unwrap_or(0);
        let y = bn.get(i).copied().unwrap_or(0);
        if x != y {
            return x.cmp(&y);
        }
    }
    match (aq.is_empty(), bq.is_empty()) {
        (true, true) => Ordering::Equal,
        (true, false) => Ordering::Greater,
        (false, true) => Ordering::Less,
        (false, false) => aq.cmp(&bq),
    }
}

/// Whether a Maven version range (`[47,)`, `[20.4,20.5)`, `[1.0]`, several joined
/// by commas) takes `build`; `None` when the range cannot be read. A bare version
/// is only a recommendation and takes any build, as in Forge.
fn maven_accepts(range: &str, build: &str) -> Option<bool> {
    let range: String = range.chars().filter(|c| !c.is_whitespace()).collect();
    if range.is_empty() || range == "*" || range.contains('$') || range.len() > 200 {
        return None;
    }
    if !range.starts_with(['[', '(']) {
        return Some(true);
    }
    let mut rest = range.as_str();
    let mut any = false;
    while !rest.is_empty() {
        let open = rest.chars().next()?;
        let close_at = rest.find([']', ')'])?;
        let close = rest[close_at..].chars().next()?;
        let inner = &rest[1..close_at];
        if !matches!(open, '[' | '(') {
            return None;
        }
        let takes = match inner.split_once(',') {
            None => open == '[' && close == ']' && compare(build, inner) == Ordering::Equal,
            Some((low, high)) => {
                let above = low.is_empty() || {
                    let o = compare(build, low);
                    o == Ordering::Greater || (open == '[' && o == Ordering::Equal)
                };
                let below = high.is_empty() || {
                    let o = compare(build, high);
                    o == Ordering::Less || (close == ']' && o == Ordering::Equal)
                };
                above && below
            }
        };
        any |= takes;
        rest = rest[close_at + 1..].trim_start_matches(',');
    }
    Some(any)
}

/// Whether a Fabric version predicate (`>=0.15.0`, `>=0.14 <0.17`, `^0.16`,
/// `~0.15.7`, `0.16.x`, `*`) takes `build`; `None` when it cannot be read.
fn fabric_accepts(predicate: &str, build: &str) -> Option<bool> {
    if predicate.len() > 200 {
        return None;
    }
    let mut all = true;
    for part in predicate.split_whitespace() {
        let (op, version) = ["<=", ">=", "<", ">", "=", "^", "~"]
            .iter()
            .find_map(|op| part.strip_prefix(op).map(|v| (*op, v)))
            .unwrap_or(("", part));
        if version.is_empty() {
            return None;
        }
        if version == "*" {
            continue;
        }
        let numbers = |v: &str| -> Vec<u64> {
            v.split(['-', '+'])
                .next()
                .unwrap_or("")
                .split('.')
                .map(|p| p.parse().unwrap_or(0))
                .collect()
        };
        let o = compare(build, version);
        let takes = match op {
            "<=" => o != Ordering::Greater,
            ">=" => o != Ordering::Less,
            "<" => o == Ordering::Less,
            ">" => o == Ordering::Greater,
            "^" | "~" => {
                // ^ keeps the first number, ~ the first two.
                let keep = if op == "^" { 1 } else { 2 };
                let (b, v) = (numbers(build), numbers(version));
                o != Ordering::Less
                    && (0..keep).all(|i| b.get(i).unwrap_or(&0) == v.get(i).unwrap_or(&0))
            }
            _ => {
                // An exact version, or one with x for any number: 0.16.x.
                let parts: Vec<&str> = version.split('.').collect();
                if parts.iter().any(|p| matches!(*p, "x" | "X" | "*")) {
                    let b: Vec<String> = numbers(build).iter().map(u64::to_string).collect();
                    parts.iter().enumerate().all(|(i, p)| {
                        matches!(*p, "x" | "X" | "*") || b.get(i).map(String::as_str) == Some(*p)
                    })
                } else {
                    o == Ordering::Equal
                }
            }
        };
        all &= takes;
    }
    Some(all)
}

/// The verified file in `cache`, downloading it first when it is missing.
async fn cached(client: &reqwest::Client, cache: &Path, file: &ModFile) -> Result<PathBuf, String> {
    let path = cache.join(format!("{}.jar", &file.sha512[..32]));
    let matches = |bytes: &[u8]| hex::encode(Sha512::digest(bytes)) == file.sha512;
    if let Ok(bytes) = tokio::fs::read(&path).await
        && matches(&bytes)
    {
        return Ok(path);
    }
    let response = client
        .get(&file.url)
        .send()
        .await
        .map_err(|e| format!("{}: {e}", file.title))?;
    if !response.status().is_success() {
        return Err(format!("{}: answered {}", file.title, response.status()));
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|e| format!("{}: {e}", file.title))?;
    if (file.size > 0 && bytes.len() as u64 != file.size) || !matches(&bytes) {
        return Err(format!(
            "{}: the download does not match its hash",
            file.title
        ));
    }
    tokio::fs::create_dir_all(cache)
        .await
        .map_err(|e| e.to_string())?;
    let tmp = path.with_extension("part");
    tokio::fs::write(&tmp, &bytes)
        .await
        .map_err(|e| e.to_string())?;
    tokio::fs::rename(&tmp, &path)
        .await
        .map_err(|e| e.to_string())?;
    Ok(path)
}

/// Copies every chosen file into `mods`.
pub async fn install(files: &[ModFile], cache: &Path, mods: &Path) -> Result<(), String> {
    let client = client();
    tokio::fs::create_dir_all(mods)
        .await
        .map_err(|e| e.to_string())?;
    for file in files {
        let from = cached(&client, cache, file).await?;
        tokio::fs::copy(&from, mods.join(&file.file_name))
            .await
            .map_err(|e| format!("{}: {e}", file.title))?;
    }
    Ok(())
}

#[test]
fn versions_are_tried_newest_release_first_then_older_then_newer_betas() {
    let v = |id: &str, kind: &str| serde_json::json!({ "id": id, "version_type": kind });
    let list = [
        v("b3", "beta"),
        v("r2", "release"),
        v("b1", "alpha"),
        v("r1", "release"),
    ];
    let ids: Vec<_> = order(&list)
        .iter()
        .map(|v| v["id"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(ids, ["r2", "r1", "b3", "b1"]);
    let betas = [v("b2", "beta"), v("b1", "beta")];
    assert_eq!(order(&betas), betas.to_vec());
}

#[test]
fn loader_builds_lose_the_game_version() {
    assert_eq!(loader_build("1.21.1", "1.21.1-52.1.0"), "52.1.0");
    assert_eq!(
        loader_build("1.7.10", "1.7.10-10.13.4.1614-1.7.10"),
        "10.13.4.1614"
    );
    assert_eq!(loader_build("26.3", "26.3.0.95-beta"), "26.3.0.95-beta");
    assert_eq!(loader_build("1.21.1", "0.16.14"), "0.16.14");
}

#[test]
fn versions_compare_by_number_then_qualifier() {
    assert_eq!(compare("52.1.0", "52.1"), Ordering::Equal);
    assert_eq!(compare("52.10.0", "52.9.9"), Ordering::Greater);
    assert_eq!(compare("26.1.2.10-beta", "26.1.2.10"), Ordering::Less);
    assert_eq!(compare("0.16.14+build.3", "0.16.14"), Ordering::Equal);
    assert_eq!(compare("junk", "0"), Ordering::Equal);
    assert_eq!(compare("99999999999999999999999", "1"), Ordering::Greater);
}

#[test]
fn maven_ranges_take_the_builds_inside() {
    assert_eq!(maven_accepts("[47,)", "47.3.0"), Some(true));
    assert_eq!(maven_accepts("[47,)", "46.0.14"), Some(false));
    assert_eq!(maven_accepts("[20.4,20.5)", "20.5.0"), Some(false));
    assert_eq!(maven_accepts("[20.4,20.5)", "20.4.237"), Some(true));
    assert_eq!(maven_accepts("(,1.0]", "1.0"), Some(true));
    assert_eq!(maven_accepts("[1.0]", "1.0.0"), Some(true));
    assert_eq!(maven_accepts("[1,2),[3,)", "3.1"), Some(true));
    assert_eq!(maven_accepts("[1,2),[3,)", "2.5"), Some(false));
    // Sodium on NeoForge 26.3 asks for any newer build: the range alone cannot help.
    assert_eq!(
        maven_accepts("[26.1.2.10-beta,)", "26.3.0.95-beta"),
        Some(true)
    );
    // A bare version only recommends; junk and placeholders count as unread.
    assert_eq!(maven_accepts("47", "1"), Some(true));
    assert_eq!(maven_accepts("[${forge_version},)", "47"), None);
    assert_eq!(maven_accepts("[47", "47"), None);
    assert_eq!(maven_accepts("", "47"), None);
}

#[test]
fn fabric_predicates_take_the_builds_they_name() {
    assert_eq!(fabric_accepts(">=0.15.0", "0.16.14"), Some(true));
    assert_eq!(fabric_accepts(">=0.17.0", "0.16.14"), Some(false));
    assert_eq!(fabric_accepts(">=0.14 <0.16", "0.16.0"), Some(false));
    assert_eq!(fabric_accepts("^0.16.2", "0.17.0"), Some(true));
    assert_eq!(fabric_accepts("^0.16.2", "1.0.0"), Some(false));
    assert_eq!(fabric_accepts("~0.15.7", "0.16.0"), Some(false));
    assert_eq!(fabric_accepts("0.16.x", "0.16.14"), Some(true));
    assert_eq!(fabric_accepts("0.15.x", "0.16.14"), Some(false));
    assert_eq!(fabric_accepts("*", "0.16.14"), Some(true));
    assert_eq!(fabric_accepts(">=", "0.16.14"), None);
}

#[test]
fn mods_toml_gives_the_ids_and_the_loader_range() {
    let toml = r#"
modLoader="javafml" # comment
loaderVersion="[4,)"
[[mods]]
version="1.0"
modId = "sodium"
[[mods]]
modId='sodium_extra'
[[dependencies.sodium]]
    versionRange="[1.21.1,1.21.2)"
    modId="minecraft"
[[dependencies.sodium]]
    type="required"
    versionRange = "[21.1.100,)" # the loader
    modId="neoforge"
"#;
    let (ids, range) = toml_meta(toml);
    assert_eq!(ids, ["sodium", "sodium_extra"]);
    assert_eq!(range.as_deref(), Some("[21.1.100,)"));
    assert_eq!(toml_meta("[[mods]\nmodId=\"x"), (Vec::new(), None));
}

#[test]
fn a_jar_refuses_a_build_only_when_its_range_says_so() {
    let forge = JarMeta {
        forge_range: Some("[52.1,)".to_owned()),
        ..JarMeta::default()
    };
    assert_eq!(
        forge.refuses("forge", "52.0.40").as_deref(),
        Some("[52.1,)")
    );
    assert_eq!(forge.refuses("forge", "52.1.0"), None);
    let fabric = JarMeta {
        fabric_ranges: vec![">=0.17".to_owned(), "0.16.x".to_owned()],
        ..JarMeta::default()
    };
    assert_eq!(fabric.refuses("fabric", "0.16.14"), None);
    assert!(fabric.refuses("fabric", "0.15.11").is_some());
    assert_eq!(fabric.refuses("quilt", "0.15.11"), None);
    assert_eq!(JarMeta::default().refuses("neoforge", "1"), None);
}

#[test]
fn libraries_name_the_mod_that_needs_them() {
    assert_eq!(needed_by("balm (needed by Waystones)"), Some("Waystones"));
    assert_eq!(
        needed_by("a (needed by b (needed by Create))"),
        Some("b (needed by Create)")
    );
    assert_eq!(needed_by("Sodium"), None);
}

#[test]
fn the_avoid_list_survives_junk_and_stays_bounded() {
    let dir = std::env::temp_dir().join(format!("gametest-avoid-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("incompatible.json"), b"{not json").unwrap();
    let mut avoid = Avoid::load(&dir);
    let bad = |n: usize| Bad {
        game: "26.3".into(),
        loader: "neoforge".into(),
        loader_version: "26.3.0.95-beta".into(),
        project: "AANobbMI".into(),
        version_id: format!("v{n}"),
        shown: "Sodium 0.9.2".into(),
    };
    avoid.add(bad(0));
    avoid.add(bad(0));
    assert_eq!(Avoid::load(&dir).all.len(), 1);
    assert!(Avoid::load(&dir).has("26.3", "neoforge", "26.3.0.95-beta", "v0"));
    assert!(!Avoid::load(&dir).has("26.3", "neoforge", "26.3.0.96-beta", "v0"));
    for n in 1..=MAX_BAD + 5 {
        avoid.add(bad(n));
    }
    let kept = Avoid::load(&dir);
    assert_eq!(kept.all.len(), MAX_BAD);
    assert!(!kept.has("26.3", "neoforge", "26.3.0.95-beta", "v0"));
    let _ = std::fs::remove_dir_all(&dir);
}
