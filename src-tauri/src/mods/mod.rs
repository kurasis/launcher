//! The catalog of mods, modpacks, resource packs, shaders and worlds at
//! mods.lapi.sh (D-066): search, project pages, "Add to the catalog", and the
//! content of an installation's game folder.
//!
//! The interface reaches the catalog only through these commands. Answers are
//! bounded in size and checked: identifiers, kinds and loaders from closed
//! lists, pictures only from files-mods.lapi.sh, descriptions as a tree of
//! safe nodes ([`markdown`]). Files come from files-mods.lapi.sh and are
//! checked against the SHA-256 the catalog gives before they are put in place
//! ([`content`]).

pub mod content;
pub mod import;
pub mod markdown;
pub mod pack;
pub mod publish;
pub mod world;

use std::collections::HashSet;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tauri::{AppHandle, Manager, Runtime};

use crate::error::{ErrorCode, LauncherError, LauncherResult};
use crate::launcher_account::{AccountProblem, LauncherAccount};
use crate::state::AppState;

const BASE: &str = "https://mods.lapi.sh";
/// Where the catalog's files and pictures are served from our server.
pub const FILES_HOST: &str = "files-mods.lapi.sh";
/// Where its files are served from Cloudflare R2 (D-066, stage 6), by hash:
/// `/f/<sha256>`. Our server has each at the same path, as the reserve.
pub const CDN_HOST: &str = "cdn-mods.lapi.sh";
pub const PROGRESS_EVENT: &str = "mods://progress";
const MAX_ANSWER_BYTES: usize = 4 * 1024 * 1024;
/// A project's version list (every version of a large mod, without
/// changelogs).
const MAX_VERSIONS_BYTES: usize = 32 * 1024 * 1024;

/// What the catalog holds.
pub const KINDS: &[&str] = &[
    "mod",
    "modpack",
    "resourcepack",
    "shader",
    "datapack",
    "world",
];
/// The loaders and platforms versions name.
const LOADERS: &[&str] = &[
    "fabric",
    "quilt",
    "forge",
    "neoforge",
    "liteloader",
    "rift",
    "minecraft",
    "datapack",
    "iris",
    "optifine",
    "canvas",
    "vanilla",
    "bukkit",
    "paper",
    "spigot",
    "folia",
    "purpur",
    "sponge",
    "velocity",
    "bungeecord",
    "waterfall",
    "modloader",
];
/// Where the catalog's projects come from; `packwiz` is a pack read from
/// its `pack.toml` (stage 5), `own` a player's own pack (stage 6).
const SOURCES: &[&str] = &["modrinth", "curseforge", "packwiz", "own"];

/// mods.lapi.sh, or the server a test build was made for
/// (`LAUNCHER_MODS_URL` at build time; release builds never set it).
fn base() -> &'static str {
    option_env!("LAUNCHER_MODS_URL")
        .filter(|u| !u.trim().is_empty())
        .unwrap_or(BASE)
}

fn network() -> LauncherError {
    LauncherError::new(
        ErrorCode::NetworkError,
        "The catalog of mods could not be reached. Check the internet connection.",
    )
}

fn server() -> LauncherError {
    LauncherError::new(
        ErrorCode::ServiceUnavailable,
        "The catalog of mods is not working right now. Try again later.",
    )
}

fn not_found() -> LauncherError {
    LauncherError::new(
        ErrorCode::InvalidInput,
        "This project or version is not in the catalog.",
    )
}

fn signed_out() -> LauncherError {
    LauncherError::new(
        ErrorCode::AuthReauthRequired,
        "Sign in to your launcher account to add to the catalog.",
    )
}

/// What an answer that is not a success means.
/// What went wrong with a publish, in the player's words where the catalog
/// named it.
fn publish_problem(status: u16, body: &Value) -> LauncherError {
    publish::problem(status, body["error"].as_str().unwrap_or_default())
        .unwrap_or_else(|| problem_of(status, body))
}

fn problem_of(status: u16, body: &Value) -> LauncherError {
    match (status, body["error"].as_str().unwrap_or_default()) {
        (401, _) => signed_out(),
        (404, _) => not_found(),
        (429, "too_much_today") => LauncherError::new(
            ErrorCode::RateLimited,
            "Your account brought a lot of files into the catalog today. Try again tomorrow.",
        ),
        (429, _) => LauncherError::new(
            ErrorCode::RateLimited,
            "Too many requests to the catalog in a short time. Try again later.",
        ),
        (400, "bad_input") => LauncherError::invalid_input(
            "Paste the address of a project on Modrinth or CurseForge, or type its name.",
        ),
        (400, "not_offered") | (409, "not_choosing") => {
            LauncherError::invalid_input("This request no longer waits for a choice.")
        }
        (400, _) => LauncherError::invalid_input("The catalog did not accept this request."),
        _ => server(),
    }
}

// ----- checks of what goes out and what comes back -----

/// One of our identifiers (a UUID in its usual form).
pub fn our_id(text: &str) -> bool {
    text.len() == 36
        && text.bytes().enumerate().all(|(i, b)| match i {
            8 | 13 | 18 | 23 => b == b'-',
            _ => b.is_ascii_hexdigit(),
        })
}

/// A project named by our ID or its slug.
pub fn project_ref(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= 64
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}

/// A game version as the catalog writes it (`1.21.1`, `26.1`, `24w14a`).
pub fn game_version(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= 40
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_' | b'+' | b' '))
}

pub fn loader(text: &str) -> bool {
    LOADERS.contains(&text)
}

/// Text from the catalog: no control characters but line breaks, at most
/// `max` characters.
fn clean(text: &str, max: usize) -> String {
    text.chars()
        .filter(|c| !c.is_control() || *c == '\n')
        .take(max)
        .collect()
}

/// One line of text.
fn line(text: &str, max: usize) -> String {
    clean(text, max).replace('\n', " ").trim().to_owned()
}

/// A picture the catalog serves: `https://files-mods.lapi.sh/i/<sha256>.<ext>`.
pub fn our_picture(raw: &str) -> Option<String> {
    let rest = raw.strip_prefix("https://")?.strip_prefix(FILES_HOST)?;
    let name = rest.strip_prefix("/i/")?;
    let (hash, ext) = name.split_once('.')?;
    (hash.len() == 64
        && hash.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
        && matches!(ext, "png" | "jpg" | "gif" | "webp"))
    .then(|| raw.to_owned())
}

/// A file the catalog serves for this hash: from our server,
/// `https://files-mods.lapi.sh/f/<sha256>/<name>`, or from R2,
/// `https://cdn-mods.lapi.sh/f/<sha256>`.
pub fn our_file(raw: &str, sha256: &str) -> Option<url::Url> {
    let url = url::Url::parse(raw).ok()?;
    let path = url.path().strip_prefix("/f/")?;
    let hash = match (url.host_str()?, path.split_once('/')) {
        (FILES_HOST, Some((hash, name))) if !name.is_empty() && !name.contains('/') => hash,
        (FILES_HOST | CDN_HOST, None) => path,
        _ => return None,
    };
    (crate::net::SourcePolicy::ModsCatalog.allows(&url)
        && url.query().is_none()
        && url.fragment().is_none()
        && hash == sha256)
        .then_some(url)
}

/// The reserve of a file served from R2: the same path on our server.
pub fn reserve_url(url: &url::Url) -> Option<url::Url> {
    if url.host_str() != Some(CDN_HOST) || url.scheme() != "https" {
        return None;
    }
    let mut reserve = url.clone();
    reserve.set_host(Some(FILES_HOST)).ok()?;
    Some(reserve)
}

/// A page about the project on its source's site.
fn source_page(raw: &str) -> Option<String> {
    let link = markdown::link(raw)?;
    let url = url::Url::parse(&link).ok()?;
    matches!(
        url.host_str(),
        Some("modrinth.com" | "www.curseforge.com" | "curseforge.com")
    )
    .then_some(link)
}

fn description(markdown: &str) -> Vec<markdown::Node> {
    markdown::parse(markdown, &our_picture)
}

/// A project's description: CurseForge's is HTML, Modrinth's Markdown.
fn project_description(value: &Value) -> Vec<markdown::Node> {
    let body = value["body"].as_str().unwrap_or_default();
    let from_curseforge = value["sources"]
        .as_array()
        .and_then(|s| s.first())
        .is_some_and(|s| s["source"] == "curseforge");
    if from_curseforge {
        markdown::parse_html(body, &our_picture)
    } else {
        description(body)
    }
}

fn kind(text: &str) -> Option<String> {
    KINDS.contains(&text).then(|| text.to_owned())
}

fn sha256(text: Option<&str>) -> Option<String> {
    text.filter(|h| h.len() == 64 && h.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')))
        .map(str::to_owned)
}

fn list_of(value: &Value, check: impl Fn(&str) -> bool) -> Vec<String> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .filter(|s| check(s))
        .take(500)
        .map(str::to_owned)
        .collect()
}

// ----- what the interface gets -----

/// A project in a list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Hit {
    pub id: String,
    pub kind: String,
    pub slug: String,
    pub title: String,
    pub summary: String,
    pub icon: Option<String>,
    pub downloads: i64,
    /// `importing` (still being brought in) or `ready`.
    pub status: String,
    pub updated: i64,
    pub author: Option<String>,
    pub sources: Vec<String>,
}

impl Hit {
    fn from(value: &Value) -> Option<Self> {
        let id = value["id"].as_str().filter(|i| our_id(i))?.to_owned();
        Some(Self {
            id,
            kind: kind(value["kind"].as_str()?)?,
            slug: line(value["slug"].as_str().unwrap_or_default(), 64),
            title: line(value["title"].as_str().unwrap_or_default(), 256),
            summary: line(value["summary"].as_str().unwrap_or_default(), 512),
            icon: value["iconThumb"].as_str().and_then(our_picture),
            downloads: value["downloads"].as_i64().unwrap_or(0).max(0),
            status: status(&value["status"]),
            updated: value["updated"].as_i64().unwrap_or(0),
            author: value["author"].as_str().map(|a| line(a, 128)),
            sources: list_of(&value["sources"], |s| SOURCES.contains(&s)),
        })
    }
}

fn status(value: &Value) -> String {
    match value.as_str() {
        Some("ready") => "ready".to_owned(),
        _ => "importing".to_owned(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchPage {
    pub total: i64,
    pub hits: Vec<Hit>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Author {
    pub name: String,
    pub role: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceLink {
    pub source: String,
    /// The project's page on that site.
    pub url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Picture {
    pub url: String,
    pub thumb: Option<String>,
    pub title: String,
    pub description: String,
}

/// A project's page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: String,
    pub kind: String,
    pub slug: String,
    pub title: String,
    pub summary: String,
    pub body: Vec<markdown::Node>,
    pub license: String,
    pub icon: Option<String>,
    pub client_side: String,
    pub server_side: String,
    pub status: String,
    pub downloads: i64,
    pub followers: i64,
    pub published: i64,
    pub updated: i64,
    /// When the source stopped having it; it stays here.
    pub gone_at: Option<i64>,
    pub authors: Vec<Author>,
    pub categories: Vec<String>,
    pub sources: Vec<SourceLink>,
    pub gallery: Vec<Picture>,
}

fn side(value: &Value) -> String {
    match value.as_str() {
        Some(s @ ("required" | "optional" | "unsupported")) => s.to_owned(),
        _ => "unknown".to_owned(),
    }
}

impl Project {
    /// A project as the catalog's API gives it, for tests outside this module.
    #[cfg(test)]
    pub(crate) fn of_json(value: &Value) -> Option<Self> {
        Self::from(value)
    }

    fn from(value: &Value) -> Option<Self> {
        let id = value["id"].as_str().filter(|i| our_id(i))?.to_owned();
        Some(Self {
            id,
            kind: kind(value["kind"].as_str()?)?,
            slug: line(value["slug"].as_str().unwrap_or_default(), 64),
            title: line(value["title"].as_str().unwrap_or_default(), 256),
            summary: line(value["summary"].as_str().unwrap_or_default(), 512),
            body: project_description(value),
            license: line(value["license"].as_str().unwrap_or_default(), 128),
            icon: value["iconThumb"]
                .as_str()
                .or_else(|| value["icon"].as_str())
                .and_then(our_picture),
            client_side: side(&value["clientSide"]),
            server_side: side(&value["serverSide"]),
            status: status(&value["status"]),
            downloads: value["downloads"].as_i64().unwrap_or(0).max(0),
            followers: value["followers"].as_i64().unwrap_or(0).max(0),
            published: value["published"].as_i64().unwrap_or(0),
            updated: value["updated"].as_i64().unwrap_or(0),
            gone_at: value["goneAt"].as_i64(),
            authors: value["authors"]
                .as_array()
                .into_iter()
                .flatten()
                .take(50)
                .map(|a| Author {
                    name: line(a["name"].as_str().unwrap_or_default(), 128),
                    role: line(a["role"].as_str().unwrap_or_default(), 64),
                })
                .filter(|a| !a.name.is_empty())
                .collect(),
            categories: list_of(&value["categories"], |c| {
                c.len() <= 32
                    && c.bytes()
                        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
            }),
            sources: value["sources"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|s| {
                    let source = s["source"].as_str().filter(|s| SOURCES.contains(s))?;
                    Some(SourceLink {
                        source: source.to_owned(),
                        url: s["url"].as_str().and_then(source_page),
                    })
                })
                .collect(),
            gallery: value["gallery"]
                .as_array()
                .into_iter()
                .flatten()
                .take(64)
                .filter_map(|p| {
                    Some(Picture {
                        url: p["url"].as_str().and_then(our_picture)?,
                        thumb: p["thumb"].as_str().and_then(our_picture),
                        title: line(p["title"].as_str().unwrap_or_default(), 256),
                        description: line(p["description"].as_str().unwrap_or_default(), 1024),
                    })
                })
                .collect(),
        })
    }
}

/// A file of a version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionFile {
    pub name: String,
    pub size: u64,
    /// Known once the catalog keeps the file.
    pub sha256: Option<String>,
    pub primary: bool,
    /// Where the catalog serves it, once kept.
    pub url: Option<String>,
    /// Held back by the catalog's malware check (D-066, stage 6): it has no
    /// address and is not installed.
    pub held: bool,
    /// Its address on Modrinth's CDN, for a modpack's index (an export).
    #[serde(skip)]
    pub origin: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Dependency {
    /// `required`, `optional`, `incompatible` or `embedded`.
    pub kind: String,
    /// The project, when the catalog has it.
    pub project_id: Option<String>,
    /// A version the dependency names exactly.
    pub version_id: Option<String>,
}

/// A version of a project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Version {
    pub id: String,
    pub project_id: String,
    pub number: String,
    pub name: String,
    /// Empty in lists; a version on its own has it.
    pub changelog: Vec<markdown::Node>,
    /// `release`, `beta` or `alpha`.
    pub channel: String,
    pub published: i64,
    pub downloads: i64,
    /// `all`: kept here; otherwise the catalog fetches it on the first install.
    pub saved: String,
    pub games: Vec<String>,
    pub loaders: Vec<String>,
    pub files: Vec<VersionFile>,
    pub dependencies: Vec<Dependency>,
    pub gone_at: Option<i64>,
}

impl Version {
    fn from(value: &Value, changelog: bool) -> Option<Self> {
        let id = value["id"].as_str().filter(|i| our_id(i))?.to_owned();
        let project_id = value["projectId"]
            .as_str()
            .filter(|i| our_id(i))?
            .to_owned();
        Some(Self {
            id,
            project_id,
            number: line(value["number"].as_str().unwrap_or_default(), 128),
            name: line(value["name"].as_str().unwrap_or_default(), 256),
            changelog: if changelog {
                description(value["changelog"].as_str().unwrap_or_default())
            } else {
                Vec::new()
            },
            channel: match value["channel"].as_str() {
                Some(c @ ("release" | "beta" | "alpha")) => c.to_owned(),
                _ => "release".to_owned(),
            },
            published: value["published"].as_i64().unwrap_or(0),
            downloads: value["downloads"].as_i64().unwrap_or(0).max(0),
            saved: match value["saved"].as_str() {
                Some(s @ ("all" | "partial")) => s.to_owned(),
                _ => "none".to_owned(),
            },
            games: list_of(&value["games"], game_version),
            loaders: list_of(&value["loaders"], loader),
            files: value["files"]
                .as_array()
                .into_iter()
                .flatten()
                .take(32)
                .map(|f| {
                    let sha = sha256(f["sha256"].as_str());
                    let held = f["held"].as_bool() == Some(true);
                    VersionFile {
                        name: line(f["name"].as_str().unwrap_or_default(), 255),
                        size: f["size"].as_u64().unwrap_or(0),
                        url: sha.as_deref().filter(|_| !held).and_then(|s| {
                            f["url"]
                                .as_str()
                                .and_then(|u| our_file(u, s))
                                .map(|u| u.to_string())
                        }),
                        sha256: sha,
                        held,
                        primary: f["primary"].as_bool().unwrap_or(false),
                        origin: f["originUrl"]
                            .as_str()
                            .filter(|u| pack::modrinth_download(u))
                            .map(str::to_owned),
                    }
                })
                .collect(),
            dependencies: value["dependencies"]
                .as_array()
                .into_iter()
                .flatten()
                .take(200)
                .filter_map(|d| {
                    Some(Dependency {
                        kind: match d["kind"].as_str()? {
                            k @ ("required" | "optional" | "incompatible" | "embedded") => {
                                k.to_owned()
                            }
                            _ => return None,
                        },
                        project_id: d["projectId"]
                            .as_str()
                            .filter(|i| our_id(i))
                            .map(str::to_owned),
                        version_id: d["versionId"]
                            .as_str()
                            .filter(|i| our_id(i))
                            .map(str::to_owned),
                    })
                })
                .collect(),
            gone_at: value["goneAt"].as_i64(),
        })
    }

    /// The file an installation takes: the primary one, else the first.
    pub fn main_file(&self) -> Option<&VersionFile> {
        self.files
            .iter()
            .find(|f| f.primary)
            .or_else(|| self.files.first())
    }
}

/// A project a name found, to choose from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    pub source: String,
    pub id: String,
    pub kind: Option<String>,
    pub title: String,
    pub summary: String,
    pub author: String,
    pub downloads: i64,
    /// Already in the catalog.
    pub project_id: Option<String>,
    /// Its icon and poster as the catalog keeps them.
    pub icon: Option<String>,
    pub banner: Option<String>,
}

/// A request to add to the catalog.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportRequest {
    pub id: String,
    pub input: String,
    /// `queued`, `choosing`, `importing`, `ready` or `failed`.
    pub state: String,
    pub error: Option<String>,
    pub project_id: Option<String>,
    pub candidates: Vec<Candidate>,
    pub created: i64,
    pub finished: Option<i64>,
}

impl ImportRequest {
    fn from(value: &Value) -> Option<Self> {
        let id = value["id"].as_str().filter(|i| our_id(i))?.to_owned();
        Some(Self {
            id,
            input: line(value["input"].as_str().unwrap_or_default(), 512),
            state: match value["state"].as_str()? {
                s @ ("queued" | "choosing" | "importing" | "ready" | "failed") => s.to_owned(),
                _ => return None,
            },
            error: value["error"].as_str().map(|e| {
                e.chars()
                    .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
                    .take(64)
                    .collect()
            }),
            project_id: value["projectId"]
                .as_str()
                .filter(|i| our_id(i))
                .map(str::to_owned),
            candidates: value["candidates"]
                .as_array()
                .into_iter()
                .flatten()
                .take(20)
                .filter_map(|c| {
                    Some(Candidate {
                        source: c["source"]
                            .as_str()
                            .filter(|s| SOURCES.contains(s))?
                            .to_owned(),
                        id: c["id"].as_str().filter(|i| project_ref(i))?.to_owned(),
                        kind: c["kind"].as_str().and_then(kind),
                        title: line(c["title"].as_str().unwrap_or_default(), 256),
                        summary: line(c["summary"].as_str().unwrap_or_default(), 512),
                        author: line(c["author"].as_str().unwrap_or_default(), 128),
                        downloads: c["downloads"].as_i64().unwrap_or(0).max(0),
                        project_id: c["projectId"]
                            .as_str()
                            .filter(|i| our_id(i))
                            .map(str::to_owned),
                        icon: c["icon"].as_str().and_then(our_picture),
                        banner: c["banner"].as_str().and_then(our_picture),
                    })
                })
                .collect(),
            created: value["created"].as_i64().unwrap_or(0),
            finished: value["finished"].as_i64(),
        })
    }
}

/// A kept file the catalog recognized by its hash.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Found {
    pub hash: String,
    pub project_id: String,
    pub version_id: String,
}

/// A search as the interface asks it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SearchQuery {
    #[serde(default)]
    pub text: String,
    pub kind: Option<String>,
    pub game: Option<String>,
    pub loader: Option<String>,
    pub category: Option<String>,
    /// `relevance`, `downloads`, `updated`, `new` or `title`.
    pub sort: Option<String>,
    #[serde(default)]
    pub offset: u32,
    pub limit: Option<u32>,
}

impl SearchQuery {
    /// The query string, or why it is refused.
    fn url(&self) -> LauncherResult<url::Url> {
        let bad = || LauncherError::invalid_input("This search cannot be made.");
        let mut url = url::Url::parse(&format!("{}/v1/search", base())).map_err(|_| bad())?;
        {
            let mut q = url.query_pairs_mut();
            let text = line(&self.text, 200);
            if !text.is_empty() {
                q.append_pair("q", &text);
            }
            if let Some(kind) = &self.kind {
                if !KINDS.contains(&kind.as_str()) {
                    return Err(bad());
                }
                q.append_pair("kind", kind);
            }
            if let Some(game) = &self.game {
                if !game_version(game) {
                    return Err(bad());
                }
                q.append_pair("game", game);
            }
            if let Some(l) = &self.loader {
                if !loader(l) {
                    return Err(bad());
                }
                q.append_pair("loader", l);
            }
            if let Some(category) = &self.category {
                if !project_ref(category) || category.len() > 32 {
                    return Err(bad());
                }
                q.append_pair("category", category);
            }
            if let Some(sort) = &self.sort {
                if !matches!(
                    sort.as_str(),
                    "relevance" | "downloads" | "updated" | "new" | "title"
                ) {
                    return Err(bad());
                }
                q.append_pair("sort", sort);
            }
            q.append_pair("offset", &self.offset.min(10_000).to_string());
            q.append_pair("limit", &self.limit.unwrap_or(20).clamp(1, 50).to_string());
        }
        Ok(url)
    }
}

/// Text the player typed into "Add to the catalog": one line, at most 512
/// characters.
pub fn import_input(text: &str) -> LauncherResult<String> {
    let text = text.trim();
    if text.is_empty() || text.chars().count() > 512 || text.chars().any(char::is_control) {
        return Err(LauncherError::invalid_input(
            "Paste the address of a project on Modrinth or CurseForge, or type its name.",
        ));
    }
    Ok(text.to_owned())
}

// ----- the client -----

/// What reads the catalog: the service, or a stand-in in tests.
pub trait Catalog: Send + Sync {
    fn version(&self, id: &str) -> impl Future<Output = LauncherResult<Version>> + Send;
    fn project(&self, id: &str) -> impl Future<Output = LauncherResult<Project>> + Send;
    /// Newest first.
    fn versions(
        &self,
        project: &str,
        game: Option<&str>,
        loader: Option<&str>,
    ) -> impl Future<Output = LauncherResult<Vec<Version>>> + Send;
    /// Asks for a version's files to be kept.
    fn fetch(&self, id: &str) -> impl Future<Output = LauncherResult<()>> + Send;
    /// Kept files by SHA-1 or SHA-256 (at most 200).
    fn lookup(&self, hashes: &[String]) -> impl Future<Output = LauncherResult<Vec<Found>>> + Send;
}

/// The catalog's API.
pub struct Client {
    http: reqwest::Client,
}

impl Client {
    pub fn new(app_version: &str) -> Result<Self, String> {
        Ok(Self {
            http: crate::launcher_account::our_client(app_version, base())?,
        })
    }

    async fn send(&self, request: reqwest::RequestBuilder, max: usize) -> LauncherResult<Value> {
        let response = request
            .header(reqwest::header::ACCEPT, "application/json")
            .send()
            .await
            .map_err(|_| network())?;
        let status = response.status().as_u16();
        let bytes = crate::launcher_account::body_within(response, max)
            .await
            .map_err(|e| match e {
                crate::launcher_account::BodyError::Network => network(),
                crate::launcher_account::BodyError::TooLarge => server(),
            })?;
        let value: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        if (200..300).contains(&status) {
            Ok(value)
        } else {
            Err(problem_of(status, &value))
        }
    }

    fn url(path: &str) -> String {
        format!("{}{path}", base())
    }

    async fn get(&self, path: &str, max: usize) -> LauncherResult<Value> {
        self.send(self.http.get(Self::url(path)), max).await
    }

    pub async fn search(&self, query: &SearchQuery) -> LauncherResult<SearchPage> {
        let value = self
            .send(self.http.get(query.url()?), MAX_ANSWER_BYTES)
            .await?;
        Ok(SearchPage {
            total: value["total"].as_i64().unwrap_or(0).max(0),
            hits: value["hits"]
                .as_array()
                .into_iter()
                .flatten()
                .take(50)
                .filter_map(Hit::from)
                .collect(),
        })
    }

    /// The launcher account's token; signing in is the player's to do.
    async fn token<R: Runtime>(&self, app: &AppHandle<R>) -> LauncherResult<String> {
        let account = Arc::clone(&app.state::<Arc<LauncherAccount>>());
        match account.access_token(&app.state::<AppState>()).await {
            Ok(token) => Ok(token.expose().to_owned()),
            Err(AccountProblem::Network) => Err(network()),
            Err(AccountProblem::Server | AccountProblem::TooMany) => Err(server()),
            Err(_) => Err(signed_out()),
        }
    }

    async fn with_token<R: Runtime>(
        &self,
        app: &AppHandle<R>,
        request: reqwest::RequestBuilder,
    ) -> LauncherResult<Value> {
        let token = self.token(app).await?;
        self.send(request.bearer_auth(token), MAX_ANSWER_BYTES)
            .await
    }

    pub async fn imports<R: Runtime>(
        &self,
        app: &AppHandle<R>,
    ) -> LauncherResult<Vec<ImportRequest>> {
        let value = self
            .with_token(app, self.http.get(Self::url("/v1/imports")))
            .await?;
        Ok(value["requests"]
            .as_array()
            .into_iter()
            .flatten()
            .take(50)
            .filter_map(ImportRequest::from)
            .collect())
    }

    pub async fn import<R: Runtime>(
        &self,
        app: &AppHandle<R>,
        input: &str,
    ) -> LauncherResult<ImportRequest> {
        let input = import_input(input)?;
        let value = self
            .with_token(
                app,
                self.http
                    .post(Self::url("/v1/imports"))
                    .json(&json!({ "input": input })),
            )
            .await?;
        ImportRequest::from(&value).ok_or_else(server)
    }

    pub async fn import_state<R: Runtime>(
        &self,
        app: &AppHandle<R>,
        id: &str,
    ) -> LauncherResult<ImportRequest> {
        if !our_id(id) {
            return Err(not_found());
        }
        let value = self
            .with_token(app, self.http.get(Self::url(&format!("/v1/imports/{id}"))))
            .await?;
        ImportRequest::from(&value).ok_or_else(server)
    }

    pub async fn choose<R: Runtime>(
        &self,
        app: &AppHandle<R>,
        id: &str,
        source: &str,
        project: &str,
    ) -> LauncherResult<ImportRequest> {
        if !our_id(id) || !SOURCES.contains(&source) || !project_ref(project) {
            return Err(LauncherError::invalid_input(
                "This request no longer waits for a choice.",
            ));
        }
        let value = self
            .with_token(
                app,
                self.http
                    .post(Self::url(&format!("/v1/imports/{id}/choose")))
                    .json(&json!({ "source": source, "id": project })),
            )
            .await?;
        ImportRequest::from(&value).ok_or_else(server)
    }
}

/// One of the player's own packs, as the catalog lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OwnPack {
    pub id: String,
    pub slug: String,
    pub title: String,
    /// ready, hidden (by its author) or removed (by the catalog's owner).
    pub status: String,
    pub versions: u64,
    pub latest: Option<String>,
}

impl OwnPack {
    fn from(value: &Value) -> Option<Self> {
        let id = value["id"].as_str().filter(|i| our_id(i))?;
        let status = value["status"]
            .as_str()
            .filter(|s| matches!(*s, "ready" | "hidden" | "removed" | "importing"))?;
        Some(Self {
            id: id.to_owned(),
            slug: value["slug"]
                .as_str()
                .filter(|s| project_ref(s))
                .unwrap_or_default()
                .to_owned(),
            title: line(value["title"].as_str().unwrap_or_default(), 200),
            status: status.to_owned(),
            versions: value["versions"].as_u64().unwrap_or(0),
            latest: value["latest"].as_str().map(|v| line(v, 48)),
        })
    }
}

impl Client {
    /// Publishes an archive as an own pack, or a new version of `project`.
    /// It goes in parts (Cloudflare takes requests up to 100 MB): the
    /// catalog is told its size and SHA-256, takes each part, and puts them
    /// together. `sent` hears the bytes sent so far and the total.
    pub async fn publish<R: Runtime>(
        &self,
        app: &AppHandle<R>,
        archive: &std::path::Path,
        project: Option<&str>,
        cancel: &crate::download::CancelToken,
        sent: &(dyn Fn(u64, u64) + Send + Sync),
    ) -> LauncherResult<publish::Published> {
        let mut url = url::Url::parse(&Self::url("/v1/packs/uploads")).map_err(|_| server())?;
        if let Some(project) = project {
            if !our_id(project) {
                return Err(not_found());
            }
            url.query_pairs_mut().append_pair("project", project);
        }
        let (size, sha256) = {
            let path = archive.to_path_buf();
            tauri::async_runtime::spawn_blocking(move || publish::size_and_sha256(&path))
                .await
                .map_err(|_| LauncherError::internal("The modpack could not be read."))??
        };
        let token = self.token(app).await?;
        let (status, value) = Self::answer(
            self.http
                .post(url)
                .bearer_auth(token)
                .json(&json!({ "size": size, "sha256": sha256 })),
            MAX_ANSWER_BYTES,
        )
        .await?;
        if status != 201 {
            return Err(publish_problem(status, &value));
        }
        let upload = value["upload"]
            .as_str()
            .filter(|u| our_id(u))
            .ok_or_else(server)?
            .to_owned();
        let part_size = value["partSize"]
            .as_u64()
            .filter(|n| (1..=publish::MAX_PART).contains(n))
            .ok_or_else(server)?;
        let parts = size.div_ceil(part_size);
        if value["parts"].as_u64() != Some(parts) {
            return Err(server());
        }
        sent(0, size);
        for n in 0..parts {
            if cancel.is_cancelled() {
                return Err(LauncherError::new(
                    ErrorCode::OperationCancelled,
                    "Publishing was cancelled.",
                ));
            }
            let bytes = {
                let path = archive.to_path_buf();
                tauri::async_runtime::spawn_blocking(move || {
                    publish::read_part(&path, n * part_size, part_size)
                })
                .await
                .map_err(|_| LauncherError::internal("The modpack could not be read."))??
            };
            let len = bytes.len() as u64;
            let uri = Self::url(&format!("/v1/packs/uploads/{upload}/{n}"));
            // A part that breaks off is sent again, twice at most.
            let mut tries = 0;
            loop {
                tries += 1;
                let token = self.token(app).await?;
                let result = Self::answer(
                    self.http
                        .put(&uri)
                        .bearer_auth(token)
                        .header(reqwest::header::CONTENT_TYPE, "application/octet-stream")
                        .timeout(std::time::Duration::from_secs(600))
                        .body(bytes.clone()),
                    MAX_ANSWER_BYTES,
                )
                .await;
                match result {
                    Ok((204, _)) => break,
                    // The catalog restarted, or the upload waited too long.
                    Ok((404, _)) => return Err(publish::lost()),
                    Ok((status, value)) if status < 500 || tries >= 3 => {
                        return Err(publish_problem(status, &value));
                    }
                    Err(error) if tries >= 3 => return Err(error),
                    _ => {}
                }
            }
            sent(n * part_size + len, size);
        }
        let token = self.token(app).await?;
        let (status, value) = Self::answer(
            self.http
                .post(Self::url(&format!("/v1/packs/uploads/{upload}/finish")))
                .bearer_auth(token)
                .timeout(std::time::Duration::from_secs(900)),
            MAX_ANSWER_BYTES,
        )
        .await?;
        if status == 201 {
            let project = Project::from(&value["project"]).ok_or_else(server)?;
            return Ok(publish::Published::Published {
                project_id: project.id,
                slug: project.slug,
            });
        }
        if let Some(refused) = publish::refused(status, &value) {
            return Ok(refused);
        }
        if status == 404 && value["error"] == "not_found" {
            return Err(publish::lost());
        }
        Err(publish_problem(status, &value))
    }

    /// Sends a request; its status and JSON answer, whatever the status.
    async fn answer(request: reqwest::RequestBuilder, max: usize) -> LauncherResult<(u16, Value)> {
        let response = request
            .header(reqwest::header::ACCEPT, "application/json")
            .send()
            .await
            .map_err(|_| network())?;
        let status = response.status().as_u16();
        let bytes = crate::launcher_account::body_within(response, max)
            .await
            .map_err(|_| network())?;
        Ok((
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        ))
    }

    /// The account's own packs.
    pub async fn own_packs<R: Runtime>(&self, app: &AppHandle<R>) -> LauncherResult<Vec<OwnPack>> {
        let value = self
            .with_token(app, self.http.get(Self::url("/v1/packs/mine")))
            .await?;
        Ok(value["packs"]
            .as_array()
            .into_iter()
            .flatten()
            .take(200)
            .filter_map(OwnPack::from)
            .collect())
    }

    /// Hides an own pack from the catalog or shows it again; the status now.
    pub async fn set_hidden<R: Runtime>(
        &self,
        app: &AppHandle<R>,
        project: &str,
        hidden: bool,
    ) -> LauncherResult<String> {
        if !our_id(project) {
            return Err(not_found());
        }
        let value = self
            .with_token(
                app,
                self.http
                    .post(Self::url(&format!("/v1/packs/{project}/hidden")))
                    .json(&json!({ "hidden": hidden })),
            )
            .await?;
        value["status"]
            .as_str()
            .filter(|s| matches!(*s, "ready" | "hidden" | "removed" | "importing"))
            .map(str::to_owned)
            .ok_or_else(server)
    }

    /// Reports a player's own pack to the catalog's owner.
    pub async fn report<R: Runtime>(
        &self,
        app: &AppHandle<R>,
        project: &str,
        reason: &str,
        text: &str,
    ) -> LauncherResult<()> {
        let text = clean(text, publish::MAX_REPORT + 1);
        if !our_id(project)
            || !publish::REASONS.contains(&reason)
            || text.chars().count() > publish::MAX_REPORT
        {
            return Err(LauncherError::invalid_input("This report cannot be sent."));
        }
        self.with_token(
            app,
            self.http
                .post(Self::url(&format!("/v1/projects/{project}/report")))
                .json(&json!({ "reason": reason, "text": text.replace('\n', " ").trim() })),
        )
        .await
        .map(|_| ())
    }
}

impl Client {
    /// A modpack version as the catalog has it; `prepare` asks the catalog to
    /// keep its files (once: it is counted, so looking again goes without).
    pub async fn pack(&self, id: &str, prepare: bool) -> LauncherResult<pack::PackInfo> {
        if !our_id(id) {
            return Err(not_found());
        }
        let url = Self::url(&format!("/v1/versions/{id}/pack"));
        let request = if prepare {
            self.http.post(url)
        } else {
            self.http.get(url)
        };
        let value = self.send(request, MAX_VERSIONS_BYTES).await?;
        Ok(pack::PackInfo::from(&value))
    }
}

impl Catalog for Client {
    async fn version(&self, id: &str) -> LauncherResult<Version> {
        if !our_id(id) {
            return Err(not_found());
        }
        let value = self
            .get(&format!("/v1/versions/{id}"), MAX_ANSWER_BYTES)
            .await?;
        Version::from(&value, true).ok_or_else(server)
    }

    async fn project(&self, id: &str) -> LauncherResult<Project> {
        if !project_ref(id) {
            return Err(not_found());
        }
        let value = self
            .get(&format!("/v1/projects/{id}"), MAX_ANSWER_BYTES)
            .await?;
        Project::from(&value).ok_or_else(server)
    }

    async fn versions(
        &self,
        project: &str,
        game: Option<&str>,
        loader_name: Option<&str>,
    ) -> LauncherResult<Vec<Version>> {
        if !project_ref(project) {
            return Err(not_found());
        }
        let mut url = url::Url::parse(&Self::url(&format!("/v1/projects/{project}/versions")))
            .map_err(|_| not_found())?;
        {
            let mut q = url.query_pairs_mut();
            if let Some(game) = game.filter(|g| game_version(g)) {
                q.append_pair("game", game);
            }
            if let Some(l) = loader_name.filter(|l| loader(l)) {
                q.append_pair("loader", l);
            }
        }
        let value = self.send(self.http.get(url), MAX_VERSIONS_BYTES).await?;
        Ok(value["versions"]
            .as_array()
            .into_iter()
            .flatten()
            .take(5000)
            .filter_map(|v| Version::from(v, false))
            .collect())
    }

    async fn fetch(&self, id: &str) -> LauncherResult<()> {
        if !our_id(id) {
            return Err(not_found());
        }
        self.send(
            self.http
                .post(Self::url(&format!("/v1/versions/{id}/fetch"))),
            MAX_ANSWER_BYTES,
        )
        .await
        .map(|_| ())
    }

    async fn lookup(&self, hashes: &[String]) -> LauncherResult<Vec<Found>> {
        if hashes.is_empty() {
            return Ok(Vec::new());
        }
        let hashes: Vec<&String> = hashes
            .iter()
            .filter(|h| {
                (h.len() == 40 || h.len() == 64)
                    && h.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
            })
            .take(200)
            .collect();
        let value = self
            .send(
                self.http
                    .post(Self::url("/v1/lookup"))
                    .json(&json!({ "hashes": hashes })),
                MAX_ANSWER_BYTES,
            )
            .await?;
        Ok(value["found"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|f| {
                Some(Found {
                    hash: f["hash"]
                        .as_str()
                        .filter(|h| hashes.iter().any(|x| x == h))?
                        .to_owned(),
                    project_id: f["projectId"].as_str().filter(|i| our_id(i))?.to_owned(),
                    version_id: f["versionId"].as_str().filter(|i| our_id(i))?.to_owned(),
                })
            })
            .collect())
    }
}

/// Pages of harmful files read at most; 500 a page.
const MAX_HARMFUL_PAGES: usize = 40;
/// How long the list of harmful files is kept before it is read again.
const HARMFUL_FRESH: std::time::Duration = std::time::Duration::from_secs(3600);

impl Client {
    /// The SHA-1 of every file the catalog holds back as harmful (D-066,
    /// stage 6), to warn about one installed before it was held.
    pub async fn harmful(&self) -> LauncherResult<HashSet<String>> {
        let mut out = HashSet::new();
        let (mut since, mut after) = (0i64, String::new());
        for _ in 0..MAX_HARMFUL_PAGES {
            let mut url = url::Url::parse(&Self::url("/v1/harmful")).map_err(|_| server())?;
            {
                let mut q = url.query_pairs_mut();
                q.append_pair("since", &since.to_string());
                if !after.is_empty() {
                    q.append_pair("after", &after);
                }
            }
            let value = self.send(self.http.get(url), MAX_ANSWER_BYTES).await?;
            for f in value["files"].as_array().into_iter().flatten().take(1000) {
                if let Some(sha1) = f["sha1"].as_str().filter(|h| {
                    h.len() == 40 && h.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
                }) {
                    out.insert(sha1.to_owned());
                }
            }
            let next = &value["next"];
            match (
                next["since"].as_i64(),
                next["after"].as_str().filter(|a| sha256(Some(a)).is_some()),
            ) {
                // Only forward: a page that points back ends the reading.
                (Some(s), Some(a)) if (s, a) > (since, after.as_str()) => {
                    since = s;
                    after = a.to_owned();
                }
                _ => break,
            }
        }
        Ok(out)
    }
}

/// The catalog's client and what installs into game folders; one per launcher.
pub struct Mods {
    pub client: Client,
    pub content: content::Installer,
    /// What the player picked to bring in (stage 5), until it is in.
    pub import: std::sync::Mutex<Option<import::Picked>>,
    /// The harmful files' SHA-1 and when they were read.
    harmful: std::sync::Mutex<Option<(std::time::Instant, Arc<HashSet<String>>)>>,
    /// The icon the player picked for publishing a pack (stage 6): its name
    /// in the archive and its bytes, checked.
    pub icon: std::sync::Mutex<Option<(&'static str, Vec<u8>)>>,
}

impl Mods {
    pub fn new(app_version: &str) -> Result<Self, String> {
        Ok(Self {
            client: Client::new(app_version)?,
            content: content::Installer::new(app_version).map_err(|e| e.message)?,
            import: std::sync::Mutex::new(None),
            harmful: std::sync::Mutex::new(None),
            icon: std::sync::Mutex::new(None),
        })
    }

    /// The harmful files' SHA-1, read again once an hour.
    pub async fn harmful(&self) -> LauncherResult<Arc<HashSet<String>>> {
        let kept = self
            .harmful
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        if let Some((read, list)) = kept
            && read.elapsed() < HARMFUL_FRESH
        {
            return Ok(list);
        }
        let list = Arc::new(self.client.harmful().await?);
        *self
            .harmful
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) =
            Some((std::time::Instant::now(), Arc::clone(&list)));
        Ok(list)
    }
}

/// Fails at once while the launcher knows it is offline (D-052).
pub fn online(state: &AppState) -> LauncherResult<()> {
    if state.network() == crate::state::dto::NetworkStatus::Offline {
        Err(network())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests;
