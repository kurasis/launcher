//! The "medium" mod set: popular mods from Modrinth for the game version and
//! loader under test, with the mods they require. Each file is checked against
//! Modrinth's SHA-512 and kept in a cache, so later runs download nothing.
//!
//! Each mod gets the newest version that fits the loader build under test: one
//! whose jar accepts that build (`versionRange` of `neoforge`/`forge` in its
//! `mods.toml`, `depends.fabricloader` in its `fabric.mod.json`; on Quilt, what
//! Quilt provides in Fabric's place, see [`quilt_provides`]), and that this
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
/// Our mod catalog (docs/DECISIONS.md D-066) keeps the files of the mods players
/// use: a file Modrinth does not give is taken from there.
const CATALOG: &str = "https://mods.lapi.sh/v1";
/// Where the catalog hands out its files.
const CATALOG_FILES: &[&str] = &["https://files-mods.lapi.sh/", "https://cdn-mods.lapi.sh/"];

/// The locked medium sets (docs/GAMETEST.md): for each "<game> <loader>", the mod
/// versions that last passed when the newest were taken. The Game test workflow
/// writes it; the nightly test plays these, so another mod's new release does not
/// change what is tested.
const LOCK: &str = include_str!("medium.lock.json");

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
    /// Our catalog finds the file by it; empty when Modrinth gave none.
    sha1: String,
    size: u64,
}

impl ModFile {
    /// A file at Modrinth's own CDN with a plain jar name and its hashes, as
    /// Modrinth or the lock gives it; `None` for anything else.
    #[allow(clippy::too_many_arguments)]
    fn checked(
        title: &str,
        version: &str,
        file_name: &str,
        project: &str,
        version_id: &str,
        url: &str,
        sha512: &str,
        sha1: &str,
        size: u64,
    ) -> Option<Self> {
        let hex = |h: &str, len: usize| h.len() == len && h.bytes().all(|b| b.is_ascii_hexdigit());
        let id = |i: &str| {
            !i.is_empty() && i.len() <= 32 && i.bytes().all(|b| b.is_ascii_alphanumeric())
        };
        let ok_name = file_name.ends_with(".jar")
            && file_name.len() <= 128
            && file_name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._+-".contains(&b));
        let ok = url.starts_with("https://cdn.modrinth.com/")
            && url.len() <= 512
            && ok_name
            && id(project)
            && id(version_id)
            && hex(sha512, 128)
            && (sha1.is_empty() || hex(sha1, 40))
            && !title.is_empty()
            && title.len() <= 200
            && version.len() <= 100;
        ok.then(|| Self {
            title: title.to_owned(),
            version: version.to_owned(),
            file_name: file_name.to_owned(),
            project: project.to_owned(),
            version_id: version_id.to_owned(),
            mod_ids: Vec::new(),
            url: url.to_owned(),
            sha512: sha512.to_ascii_lowercase(),
            sha1: sha1.to_ascii_lowercase(),
            size,
        })
    }
}

/// What the resolution chose and what it left out, with why.
#[derive(Debug, Default)]
pub struct Resolved {
    pub files: Vec<ModFile>,
    pub skipped: Vec<String>,
    /// Versions passed over because their jar does not accept the loader build.
    pub older: Vec<String>,
    /// Versions passed over, or mods left out, because they crashed on this loader
    /// build here before (or, from the lock, could not be downloaded or refuse the
    /// loader build): the scenario then passes only with a warning.
    pub replaced: Vec<String>,
    /// Modrinth answered every question: a set chosen without all answers is not
    /// locked.
    pub complete: bool,
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

/// The JSON at `url`: `None` when there is nothing there (404), an error when
/// there was no answer.
async fn answer(
    client: &reqwest::Client,
    url: &str,
    query: &[(&str, String)],
) -> Result<Option<Value>, String> {
    let mut url = url::Url::parse(url).map_err(|e| e.to_string())?;
    if !query.is_empty() {
        url.query_pairs_mut().extend_pairs(query);
    }
    let response = client.get(url).send().await.map_err(|e| e.to_string())?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    if !response.status().is_success() {
        return Err(format!("answered {}", response.status()));
    }
    response.json().await.map(Some).map_err(|e| e.to_string())
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
/// pre-releases newer than that release, newest first. `None` when it has none,
/// an error when Modrinth did not answer.
async fn candidates(
    client: &reqwest::Client,
    project: &str,
    game: &str,
    loader: &str,
) -> Result<Option<Vec<Value>>, String> {
    let versions = answer(
        client,
        &format!("{API}/project/{project}/version"),
        &[
            ("loaders", loaders(loader)),
            ("game_versions", format!("[\"{game}\"]")),
        ],
    )
    .await?;
    Ok(versions
        .as_ref()
        .and_then(Value::as_array)
        .filter(|list| !list.is_empty())
        .map(|list| order(list)))
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
    ModFile::checked(
        title,
        version["version_number"].as_str().unwrap_or("?"),
        file["filename"].as_str()?,
        version["project_id"].as_str()?,
        version["id"].as_str()?,
        file["url"].as_str()?,
        file["hashes"]["sha512"].as_str()?,
        file["hashes"]["sha1"].as_str().unwrap_or(""),
        file["size"].as_u64().unwrap_or(0),
    )
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LockedSet {
    loader: String,
    files: Vec<LockedFile>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LockedFile {
    title: String,
    version: String,
    file_name: String,
    project: String,
    version_id: String,
    url: String,
    sha512: String,
    #[serde(default)]
    sha1: String,
    #[serde(default)]
    size: u64,
}

/// The locked set for this game version and loader: the loader build it passed
/// on and its files. `None` when there is none, or when an entry is not a plain
/// Modrinth file.
pub fn locked(game: &str, loader: &str) -> Option<(String, Vec<ModFile>)> {
    locked_in(LOCK, game, loader)
}

fn locked_in(text: &str, game: &str, loader: &str) -> Option<(String, Vec<ModFile>)> {
    let mut all: HashMap<String, LockedSet> = serde_json::from_str(text).ok()?;
    let set = all.remove(&format!("{game} {loader}"))?;
    if set.files.is_empty() || set.files.len() > MAX_FILES {
        return None;
    }
    let files = set
        .files
        .iter()
        .map(|f| {
            ModFile::checked(
                &f.title,
                &f.version,
                &f.file_name,
                &f.project,
                &f.version_id,
                &f.url,
                &f.sha512,
                &f.sha1,
                f.size,
            )
        })
        .collect::<Option<Vec<_>>>()?;
    Some((set.loader, files))
}

/// The loader under test, as the mods' jars name it.
pub struct Loader<'a> {
    /// `fabric`, `quilt`, `forge` or `neoforge`.
    pub kind: &'a str,
    /// The build, as the launcher names it (`52.1.0`, `26.3.0.95`, `0.16.10`).
    pub version: &'a str,
    /// On Quilt, the mods it provides to Fabric's mods with their versions
    /// (`fabricloader`, its own `mixinextras`).
    pub provides: &'a [(String, String)],
}

/// Chooses the medium set for this game version and loader build; the jars it
/// looks into stay in `cache`.
pub async fn resolve(game: &str, loader: &Loader<'_>, avoid: &Avoid, cache: &Path) -> Resolved {
    let client = client();
    let mut resolved = Resolved {
        complete: true,
        ..Resolved::default()
    };
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
        let found = match candidates(&client, &project, game, loader.kind).await {
            Ok(found) => found,
            Err(e) => {
                resolved
                    .skipped
                    .push(format!("{title}: Modrinth did not answer ({e})"));
                resolved.complete = false;
                left_out.push(title);
                continue;
            }
        };
        let Some(list) = found else {
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
        let mut not_downloaded = 0;
        for version in list.iter().take(MAX_TRIED) {
            let Some(mut file) = file_of(version, &title) else {
                continue;
            };
            if avoid.has(game, loader.kind, loader.version, &file.version_id) {
                crashed.push(file.version.clone());
                continue;
            }
            let Ok(path) = cached(&client, cache, &file).await else {
                not_downloaded += 1;
                continue;
            };
            let meta = jar_meta(&path);
            if let Some(wants) = meta.refuses(loader.kind, &build, loader.provides) {
                older.push(wants);
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
        if not_downloaded > 0 {
            // A version not looked into may have been the one to take.
            resolved.complete = false;
        }
        let Some((version, file)) = picked else {
            if not_downloaded > 0 {
                resolved.skipped.push(format!(
                    "{title}: {not_downloaded} versions could not be downloaded"
                ));
            } else if crashed.is_empty() {
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
        // Sodium before 0.6 has no Fabric rendering API: mods that draw through
        // it (Supplementaries stops the game without one) need Indium beside it,
        // as players add it.
        if project == "sodium"
            && matches!(loader.kind, "fabric" | "quilt")
            && without_renderer(&file.version)
        {
            queue.push(("indium".to_owned(), "indium (for Sodium)".to_owned()));
        }
        chosen.insert(id, version);
        resolved.files.push(file);
    }
    drop_needing(&mut resolved, left_out);
    resolved
}

/// Drops the files that need a mod left out (by the "(needed by …)" of their
/// titles): a library left out leaves the mods that need it unable to start, and
/// the mods that need those.
fn drop_needing(resolved: &mut Resolved, left_out: Vec<String>) {
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
    let mut dropped = Vec::new();
    resolved.files.retain(|f| {
        let keep = !gone.contains(&f.title);
        if !keep {
            dropped.push(f.title.clone());
        }
        keep
    });
    if !dropped.is_empty() {
        resolved.skipped.push(format!(
            "{}: a library they need is left out",
            dropped.join(", ")
        ));
    }
}

/// The locked set, as it was chosen: every file downloaded and looked into. A file
/// that crashed on this loader build here before, cannot be downloaded or refuses
/// the build is left out with the mods that need it, and noted in `replaced`.
pub async fn from_lock(
    game: &str,
    loader: &Loader<'_>,
    files: Vec<ModFile>,
    avoid: &Avoid,
    cache: &Path,
) -> Resolved {
    let client = client();
    let build = loader_build(game, loader.version);
    let mut resolved = Resolved {
        complete: true,
        ..Resolved::default()
    };
    let mut left_out = Vec::new();
    for mut file in files {
        let shown = format!("{} {}", file.title, file.version);
        let why = if avoid.has(game, loader.kind, loader.version, &file.version_id) {
            Some("crashed on this loader build before".to_owned())
        } else {
            match cached(&client, cache, &file).await {
                Err(e) => Some(format!("could not be downloaded ({e})")),
                Ok(path) => {
                    let meta = jar_meta(&path);
                    match meta.refuses(loader.kind, &build, loader.provides) {
                        Some(wants) => Some(format!("needs {wants}")),
                        None => {
                            file.mod_ids = meta.ids;
                            None
                        }
                    }
                }
            }
        };
        match why {
            Some(why) => {
                resolved.replaced.push(format!("{shown} {why}: left out"));
                left_out.push(file.title);
            }
            None => resolved.files.push(file),
        }
    }
    drop_needing(&mut resolved, left_out);
    resolved
}

/// Whether a Sodium version (`mc1.20.1-0.5.13-fabric`, `0.5.8+mc1.20.1`) comes
/// before 0.6, the first with Fabric's rendering API of its own.
fn without_renderer(version: &str) -> bool {
    version
        .split(['-', '+'])
        .find(|part| part.starts_with(|c: char| c.is_ascii_digit()))
        .is_some_and(|v| compare(v, "0.6") == Ordering::Less)
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
    /// Its other `depends` of `fabric.mod.json`, as mod id and predicates.
    fabric_depends: Vec<(String, Vec<String>)>,
}

/// No metadata file larger than this is read.
const MAX_META: u64 = 256 * 1024;
const MAX_IDS: usize = 16;
const MAX_DEPENDS: usize = 64;
/// A jar inside the Quilt Loader jar larger than this is not looked into.
const MAX_NESTED_JAR: u64 = 8 * 1024 * 1024;

impl JarMeta {
    /// What a jar asks for when it refuses this loader build (`forge [52.1,)`,
    /// `fabricloader >=0.19.5`); a range that cannot be read counts as accepting.
    /// Quilt answers for Fabric's mods with what it provides in Fabric's place
    /// (`provides`), so there each of those the jar depends on is checked.
    fn refuses(&self, kind: &str, build: &str, provides: &[(String, String)]) -> Option<String> {
        // None of the predicates takes `version`, when there are any.
        let refused = |ranges: &[String], version: &str| {
            !ranges.is_empty()
                && ranges
                    .iter()
                    .all(|r| fabric_accepts(r, version) == Some(false))
        };
        match kind {
            "forge" | "neoforge" => {
                let range = self.forge_range.as_ref()?;
                (maven_accepts(range, build) == Some(false)).then(|| format!("{kind} {range}"))
            }
            "fabric" => refused(&self.fabric_ranges, build)
                .then(|| format!("fabricloader {}", self.fabric_ranges.join(" or "))),
            "quilt" => provides.iter().find_map(|(id, version)| {
                let ranges = if id == "fabricloader" {
                    &self.fabric_ranges
                } else {
                    &self.fabric_depends.iter().find(|(d, _)| d == id)?.1
                };
                refused(ranges, version).then(|| format!("{id} {}", ranges.join(" or ")))
            }),
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
        let predicates = |v: &Value| -> Vec<String> {
            match v {
                Value::String(s) => vec![s.clone()],
                Value::Array(list) => list
                    .iter()
                    .filter_map(|v| v.as_str().map(str::to_owned))
                    .take(MAX_IDS)
                    .collect(),
                _ => Vec::new(),
            }
        };
        if let Some(depends) = value["depends"].as_object() {
            for (id, v) in depends.iter().take(MAX_DEPENDS) {
                if id == "fabricloader" {
                    meta.fabric_ranges = predicates(v);
                } else {
                    meta.fabric_depends.push((id.clone(), predicates(v)));
                }
            }
        }
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

/// The mod of the set whose jar holds the first class of a stack trace that
/// belongs to a mod (`at com.example.Mod.run(Mod.java:10)`), unless that class
/// is one of ours (`app.launcher.`). Forge for 1.12.2 lists no suspects in its
/// crash reports, and an exception that Fabric only logs comes with none; the
/// stack still names the mod. The jars are read from `mods`, the game's folder.
pub fn blamed_by_stack<'a>(stack: &str, set: &'a [ModFile], mods: &Path) -> Option<&'a ModFile> {
    let classes: Vec<HashSet<String>> = set
        .iter()
        .map(|file| {
            std::fs::File::open(mods.join(&file.file_name))
                .ok()
                .and_then(|f| zip::ZipArchive::new(f).ok())
                .map(|zip| {
                    zip.file_names()
                        .filter_map(|n| n.strip_suffix(".class"))
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_default()
        })
        .collect();
    let frames = stack.lines().filter_map(|line| {
        let frame = line.trim().strip_prefix("at ")?;
        let call = frame.split('(').next()?;
        let (class, _method) = call.rsplit_once('.')?;
        // "java.base/java.lang.Thread": the module goes.
        Some(class.rsplit('/').next().unwrap_or(class).to_owned())
    });
    for class in frames.take(MAX_FRAMES) {
        if class.starts_with("app.launcher.") {
            return None;
        }
        let path = class.replace('.', "/");
        if let Some(i) = classes.iter().position(|c| c.contains(&path)) {
            return set.get(i);
        }
    }
    None
}

/// Frames of a stack trace looked at for the mod to blame.
const MAX_FRAMES: usize = 200;

/// What a Quilt Loader jar provides to Fabric's mods: the ids in its
/// `quilt.mod.json` `provides` (`fabricloader` at the version Quilt stands in
/// for) and the mods it carries inside (`mixinextras`), each with its version.
/// Nothing when the jar cannot be read.
pub fn quilt_provides(jar: &Path) -> Vec<(String, String)> {
    fn entry(
        zip: &mut zip::ZipArchive<impl std::io::Read + std::io::Seek>,
        name: &str,
        max: u64,
    ) -> Option<Vec<u8>> {
        let entry = zip.by_name(name).ok()?;
        if entry.size() > max {
            return None;
        }
        let mut out = Vec::new();
        entry.take(max).read_to_end(&mut out).ok()?;
        Some(out)
    }
    // A mod's id and version from its `fabric.mod.json` or `quilt.mod.json`.
    fn id_version(
        zip: &mut zip::ZipArchive<impl std::io::Read + std::io::Seek>,
    ) -> Option<(String, String)> {
        let fabric = entry(zip, "fabric.mod.json", MAX_META)
            .and_then(|b| serde_json::from_slice::<Value>(&b).ok());
        if let Some(v) = fabric {
            return Some((
                v["id"].as_str()?.to_owned(),
                v["version"].as_str()?.to_owned(),
            ));
        }
        let quilt: Value = serde_json::from_slice(&entry(zip, "quilt.mod.json", MAX_META)?).ok()?;
        let q = &quilt["quilt_loader"];
        Some((
            q["id"].as_str()?.to_owned(),
            q["version"].as_str()?.to_owned(),
        ))
    }
    let Ok(file) = std::fs::File::open(jar) else {
        return Vec::new();
    };
    let Ok(mut zip) = zip::ZipArchive::new(file) else {
        return Vec::new();
    };
    let Some(json) = entry(&mut zip, "quilt.mod.json", MAX_META)
        .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
    else {
        return Vec::new();
    };
    let q = &json["quilt_loader"];
    let mut out: Vec<(String, String)> = q["provides"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|p| {
            Some((
                p["id"].as_str()?.to_owned(),
                p["version"].as_str()?.to_owned(),
            ))
        })
        .collect();
    for name in q["jars"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
    {
        let inner = entry(&mut zip, name, MAX_NESTED_JAR)
            .and_then(|bytes| zip::ZipArchive::new(std::io::Cursor::new(bytes)).ok());
        if let Some(found) = inner.and_then(|mut inner| id_version(&mut inner)) {
            out.push(found);
        }
    }
    out.truncate(MAX_IDS);
    out
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

/// No mod file larger than this is downloaded.
const MAX_JAR: u64 = 256 * 1024 * 1024;

/// The verified file in `cache`, downloading it first when it is missing: from
/// Modrinth, else from our catalog.
async fn cached(client: &reqwest::Client, cache: &Path, file: &ModFile) -> Result<PathBuf, String> {
    let path = cache.join(format!("{}.jar", &file.sha512[..32]));
    if let Ok(bytes) = tokio::fs::read(&path).await
        && hex::encode(Sha512::digest(&bytes)) == file.sha512
    {
        return Ok(path);
    }
    let bytes = match fetch(client, &file.url, file).await {
        Ok(bytes) => bytes,
        Err(modrinth) => from_catalog(client, file)
            .await
            .map_err(|ours| format!("{}: {modrinth}; our catalog: {ours}", file.title))?,
    };
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

/// The file at `url`, when it is the one wanted: its size and SHA-512.
async fn fetch(client: &reqwest::Client, url: &str, file: &ModFile) -> Result<Vec<u8>, String> {
    let mut response = client.get(url).send().await.map_err(|e| e.to_string())?;
    if !response.status().is_success() {
        return Err(format!("answered {}", response.status()));
    }
    let limit = if file.size > 0 { file.size } else { MAX_JAR };
    if response.content_length().is_some_and(|len| len > limit) {
        return Err("larger than the file".to_owned());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|e| e.to_string())? {
        bytes.extend_from_slice(&chunk);
        if bytes.len() as u64 > limit {
            return Err("larger than the file".to_owned());
        }
    }
    if (file.size > 0 && bytes.len() as u64 != file.size)
        || hex::encode(Sha512::digest(&bytes)) != file.sha512
    {
        return Err("the download does not match its hash".to_owned());
    }
    Ok(bytes)
}

/// The file from our catalog, found by its SHA-1.
async fn from_catalog(client: &reqwest::Client, file: &ModFile) -> Result<Vec<u8>, String> {
    if file.sha1.is_empty() {
        return Err("no SHA-1 to find it by".to_owned());
    }
    let found: Value = client
        .post(format!("{CATALOG}/lookup"))
        .json(&serde_json::json!({ "hashes": [file.sha1] }))
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    let id = found["found"][0]["versionId"]
        .as_str()
        .filter(|id| catalog_id(id))
        .ok_or("not in the catalog")?;
    let version = answer(client, &format!("{CATALOG}/versions/{id}"), &[])
        .await?
        .ok_or("not in the catalog")?;
    let url = catalog_url(&version, &file.sha1).ok_or("no file to download")?;
    fetch(client, url, file).await
}

/// A catalog version id: a UUID.
fn catalog_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-')
}

/// Where the catalog hands out the file with this SHA-1 of a version, when that
/// is one of its file hosts.
fn catalog_url<'a>(version: &'a Value, sha1: &str) -> Option<&'a str> {
    version["files"].as_array()?.iter().find(|f| {
        f["sha1"]
            .as_str()
            .is_some_and(|h| h.eq_ignore_ascii_case(sha1))
    })?["url"]
        .as_str()
        .filter(|url| url.len() <= 512 && CATALOG_FILES.iter().any(|host| url.starts_with(host)))
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
        forge.refuses("forge", "52.0.40", &[]).as_deref(),
        Some("forge [52.1,)")
    );
    assert_eq!(forge.refuses("forge", "52.1.0", &[]), None);
    let fabric = JarMeta {
        fabric_ranges: vec![">=0.17".to_owned(), "0.16.x".to_owned()],
        fabric_depends: vec![("mixinextras".to_owned(), vec![">=0.5.5".to_owned()])],
        ..JarMeta::default()
    };
    assert_eq!(fabric.refuses("fabric", "0.16.14", &[]), None);
    assert_eq!(
        fabric.refuses("fabric", "0.15.11", &[]).as_deref(),
        Some("fabricloader >=0.17 or 0.16.x")
    );
    // Quilt: what it provides counts, not its own build.
    let quilt = |fabric: &str, mixinextras: &str| {
        vec![
            ("fabricloader".to_owned(), fabric.to_owned()),
            ("mixinextras".to_owned(), mixinextras.to_owned()),
        ]
    };
    assert_eq!(
        fabric.refuses("quilt", "0.30.1", &quilt("0.17.2", "0.5.5")),
        None
    );
    assert_eq!(
        fabric
            .refuses("quilt", "0.30.1", &quilt("0.15.11", "0.5.5"))
            .as_deref(),
        Some("fabricloader >=0.17 or 0.16.x")
    );
    assert_eq!(
        fabric
            .refuses("quilt", "0.30.1", &quilt("0.17.2", "0.5.4"))
            .as_deref(),
        Some("mixinextras >=0.5.5")
    );
    assert_eq!(fabric.refuses("quilt", "0.30.1", &[]), None);
    assert_eq!(JarMeta::default().refuses("neoforge", "1", &[]), None);
}

#[test]
fn quilt_provides_fabric_loader_and_the_mods_it_carries() {
    use std::io::Write as _;
    let zip_of = |files: &[(&str, &[u8])]| -> Vec<u8> {
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        for (name, bytes) in files {
            zip.start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(bytes).unwrap();
        }
        zip.finish().unwrap().into_inner()
    };
    let mixinextras = zip_of(&[(
        "fabric.mod.json",
        br#"{"id": "mixinextras", "version": "0.5.4"}"#,
    )]);
    let loader = zip_of(&[
        (
            "quilt.mod.json",
            br#"{"quilt_loader": {"id": "quilt_loader", "version": "0.30.1",
                "provides": [{"id": "fabricloader", "version": "0.19.3"}, "junk"],
                "jars": ["META-INF/jars/mixinextras-fabric-0.5.4.jar", "META-INF/jars/missing.jar"]}}"#,
        ),
        ("META-INF/jars/mixinextras-fabric-0.5.4.jar", &mixinextras),
    ]);
    let dir = tempfile::tempdir().unwrap();
    let jar = dir.path().join("quilt-loader-0.30.1.jar");
    std::fs::write(&jar, loader).unwrap();
    assert_eq!(
        quilt_provides(&jar),
        [
            ("fabricloader".to_owned(), "0.19.3".to_owned()),
            ("mixinextras".to_owned(), "0.5.4".to_owned()),
        ]
    );
    std::fs::write(&jar, b"not a zip").unwrap();
    assert!(quilt_provides(&jar).is_empty());
    assert!(quilt_provides(&dir.path().join("none.jar")).is_empty());
}

#[test]
fn a_jars_depends_are_kept_apart_from_its_loader_range() {
    use std::io::Write as _;
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    zip.start_file("fabric.mod.json", zip::write::SimpleFileOptions::default())
        .unwrap();
    zip.write_all(
        br#"{"id": "lithium", "depends": {"fabricloader": ">=0.19.5", "mixinextras": [">=0.5.5"], "minecraft": "26.3"}}"#,
    )
    .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let jar = dir.path().join("lithium.jar");
    std::fs::write(&jar, zip.finish().unwrap().into_inner()).unwrap();
    let meta = jar_meta(&jar);
    assert_eq!(meta.ids, ["lithium"]);
    assert_eq!(meta.fabric_ranges, [">=0.19.5"]);
    assert!(
        meta.fabric_depends
            .contains(&("mixinextras".to_owned(), vec![">=0.5.5".to_owned()]))
    );
}

#[test]
fn the_stack_names_the_mod_whose_class_comes_first() {
    use std::io::Write as _;
    let dir = tempfile::tempdir().unwrap();
    let jar = |name: &str, classes: &[&str]| {
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        for class in classes {
            zip.start_file(
                format!("{class}.class"),
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
            zip.write_all(b"\xca\xfe").unwrap();
        }
        std::fs::write(dir.path().join(name), zip.finish().unwrap().into_inner()).unwrap();
    };
    jar("chameleon.jar", &["com/jaquadro/chameleon/DefaultRegister"]);
    jar(
        "drawers.jar",
        &["com/jaquadro/drawers/ModBlocks$Registration"],
    );
    let file = |title: &str, name: &str| ModFile {
        title: title.to_owned(),
        version: "1".to_owned(),
        file_name: name.to_owned(),
        project: title.to_owned(),
        version_id: title.to_owned(),
        mod_ids: Vec::new(),
        url: String::new(),
        sha512: String::new(),
        sha1: String::new(),
        size: 0,
    };
    let set = [
        file("Storage Drawers", "drawers.jar"),
        file("Chameleon", "chameleon.jar"),
    ];
    let report = "java.lang.NoSuchMethodError: net.minecraft.item.Item.getItemFromBlock\n\
        \tat com.jaquadro.chameleon.DefaultRegister.getItem(DefaultRegister.java:25)\n\
        \tat com.jaquadro.drawers.ModBlocks$Registration.registerModels(ModBlocks.java:215)\n";
    let blamed = blamed_by_stack(report, &set, dir.path()).map(|f| f.title.as_str());
    assert_eq!(blamed, Some("Chameleon"));
    // Frames of the game, the JDK or missing jars are passed over.
    let later = "\tat java.base/java.lang.Thread.run(Thread.java:1)\n\
        \tat net.minecraft.class_310.method_1514(class_310.java:781)\n\
        \tat com.jaquadro.drawers.ModBlocks$Registration.registerModels(ModBlocks.java:215)\n";
    let blamed = blamed_by_stack(later, &set, dir.path()).map(|f| f.title.as_str());
    assert_eq!(blamed, Some("Storage Drawers"));
    // Our own class first: no mod of the set is blamed.
    let ours = "\tat app.launcher.gametest.core.Bot.tick(Bot.java:1)\n\
        \tat com.jaquadro.chameleon.DefaultRegister.getItem(DefaultRegister.java:25)\n";
    assert!(blamed_by_stack(ours, &set, dir.path()).is_none());
    assert!(blamed_by_stack("no stack at all", &set, dir.path()).is_none());
}

#[test]
fn sodium_before_0_6_has_no_renderer() {
    assert!(without_renderer("mc1.20.1-0.5.13-fabric"));
    assert!(without_renderer("0.5.8+mc1.20.1"));
    assert!(!without_renderer("mc1.21.1-0.6.13-fabric"));
    assert!(!without_renderer("mc26.3-0.9.2-fabric"));
    assert!(!without_renderer("mc1.21.1-0.6.0-beta.2-fabric"));
    assert!(!without_renderer("unknown"));
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

#[test]
fn a_locked_set_takes_only_plain_modrinth_files() {
    let file = |url: &str, name: &str, sha512: &str| {
        serde_json::json!({
            "title": "rZrAKGe1 (needed by storagedrawers)",
            "version": "1.21.1-2.0.0",
            "fileName": name,
            "project": "rZrAKGe1",
            "versionId": "Ab3dEf9h",
            "url": url,
            "sha512": sha512,
            "sha1": "A".repeat(40),
            "size": 12,
        })
    };
    let good = file(
        "https://cdn.modrinth.com/data/rZrAKGe1/versions/Ab3dEf9h/lib-2.0.0.jar",
        "lib-2.0.0.jar",
        &"ab".repeat(64),
    );
    let lock = |files: Vec<Value>| {
        serde_json::json!({ "1.21.1 neoforge": { "loader": "21.1.200", "files": files } })
            .to_string()
    };
    let (loader, files) = locked_in(&lock(vec![good.clone()]), "1.21.1", "neoforge").unwrap();
    assert_eq!(loader, "21.1.200");
    assert_eq!(files[0].version_id, "Ab3dEf9h");
    assert_eq!(files[0].sha1, "a".repeat(40));
    assert!(locked_in(&lock(vec![good.clone()]), "1.21.1", "forge").is_none());
    // One bad entry spoils the set: another host, a path in the name, a short hash.
    for bad in [
        file(
            "https://evil.example/lib.jar",
            "lib-2.0.0.jar",
            &"ab".repeat(64),
        ),
        file(
            "http://cdn.modrinth.com/lib.jar",
            "lib-2.0.0.jar",
            &"ab".repeat(64),
        ),
        file(
            "https://cdn.modrinth.com/x.jar",
            "../../lib.jar",
            &"ab".repeat(64),
        ),
        file(
            "https://cdn.modrinth.com/x.jar",
            "lib.exe",
            &"ab".repeat(64),
        ),
        file("https://cdn.modrinth.com/x.jar", "lib-2.0.0.jar", "abcd"),
        file(
            "https://cdn.modrinth.com/x.jar",
            "lib-2.0.0.jar",
            &"zz".repeat(64),
        ),
    ] {
        assert!(locked_in(&lock(vec![good.clone(), bad]), "1.21.1", "neoforge").is_none());
    }
    assert!(locked_in(&lock(Vec::new()), "1.21.1", "neoforge").is_none());
    assert!(locked_in(&lock(vec![good; MAX_FILES + 1]), "1.21.1", "neoforge").is_none());
    assert!(locked_in("{not json", "1.21.1", "neoforge").is_none());
    assert!(locked_in("[]", "1.21.1", "neoforge").is_none());
    // Every set in the repository's file reads.
    let all: HashMap<String, Value> = serde_json::from_str(LOCK).unwrap();
    for key in all.keys() {
        let (game, loader) = key.split_once(' ').unwrap();
        assert!(locked(game, loader).is_some(), "{key} reads");
    }
}

#[test]
fn the_catalog_gives_files_only_from_its_hosts() {
    let sha1 = "0a".repeat(20);
    let version = |url: &str| {
        serde_json::json!({ "files": [
            { "sha1": "ff".repeat(20), "url": "https://files-mods.lapi.sh/f/other/x.jar" },
            { "sha1": sha1.to_ascii_uppercase(), "url": url },
        ] })
    };
    let ok = "https://cdn-mods.lapi.sh/f/abc";
    assert_eq!(catalog_url(&version(ok), &sha1), Some(ok));
    let ok = "https://files-mods.lapi.sh/f/abc/lib.jar";
    assert_eq!(catalog_url(&version(ok), &sha1), Some(ok));
    for bad in [
        "https://evil.example/f/abc",
        "http://cdn-mods.lapi.sh/f/abc",
        "https://cdn-mods.lapi.sh.evil.example/f/abc",
    ] {
        assert_eq!(catalog_url(&version(bad), &sha1), None);
    }
    let long = format!("https://cdn-mods.lapi.sh/{}", "a".repeat(600));
    assert_eq!(catalog_url(&version(&long), &sha1), None);
    assert_eq!(catalog_url(&version(ok), &"11".repeat(20)), None);
    assert_eq!(
        catalog_url(&serde_json::json!({ "files": "junk" }), &sha1),
        None
    );
    assert!(catalog_id("0b3e5a1c-7f2d-4e8a-9c6b-1d2e3f4a5b6c"));
    assert!(!catalog_id("../versions"));
    assert!(!catalog_id(""));
    assert!(!catalog_id(&"a".repeat(65)));
}
