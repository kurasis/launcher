//! The "medium" mod set: popular mods from Modrinth for the game version and
//! loader under test, with the mods they require. Each file is checked against
//! Modrinth's SHA-512 and kept in a cache, so later runs download nothing.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

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
    url: String,
    sha512: String,
    size: u64,
}

/// What the resolution chose and what it left out, with why.
#[derive(Debug, Default)]
pub struct Resolved {
    pub files: Vec<ModFile>,
    pub skipped: Vec<String>,
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

/// The newest release (else the newest of any kind) of a project for this game and loader.
async fn newest(
    client: &reqwest::Client,
    project: &str,
    game: &str,
    loader: &str,
) -> Option<Value> {
    let versions = get(
        client,
        &format!("{API}/project/{project}/version"),
        &[
            ("loaders", format!("[\"{loader}\"]")),
            ("game_versions", format!("[\"{game}\"]")),
        ],
    )
    .await?;
    let list = versions.as_array()?;
    list.iter()
        .find(|v| v["version_type"] == "release")
        .or_else(|| list.first())
        .cloned()
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
        url: url.to_owned(),
        sha512: file["hashes"]["sha512"].as_str()?.to_ascii_lowercase(),
        size: file["size"].as_u64().unwrap_or(0),
    })
}

/// Chooses the medium set for this game version and loader.
pub async fn resolve(game: &str, loader: &str) -> Resolved {
    let client = client();
    let mut resolved = Resolved::default();
    let mut chosen: HashMap<String, Value> = HashMap::new();
    let mut queue: Vec<(String, String)> = MEDIUM
        .iter()
        .map(|s| ((*s).to_owned(), (*s).to_owned()))
        .collect();
    queue.reverse();
    let mut seen = HashSet::new();
    while let Some((project, title)) = queue.pop() {
        if !seen.insert(project.clone()) || resolved.files.len() >= MAX_FILES {
            continue;
        }
        let Some(version) = newest(&client, &project, game, loader).await else {
            if MEDIUM.contains(&project.as_str()) {
                resolved
                    .skipped
                    .push(format!("{title}: no file for {game} {loader}"));
            } else {
                resolved
                    .skipped
                    .push(format!("library {project} of another mod: not found"));
            }
            continue;
        };
        let id = version["project_id"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
        if chosen.contains_key(&id) {
            continue;
        }
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
            continue;
        }
        let Some(file) = file_of(&version, &title) else {
            resolved.skipped.push(format!("{title}: no usable file"));
            continue;
        };
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
    resolved
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
