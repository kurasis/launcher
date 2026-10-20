//! Modrinth's API v2 (api.modrinth.com): the addresses we ask and what we
//! make of the answers. Every field is checked and bounded; anything that does
//! not fit is left out rather than trusted.

use serde_json::Value;

use crate::model::{
    SourceDependency, SourceFile, SourceImage, SourceProject, SourceVersion, clean, word,
};

pub const SOURCE: &str = "modrinth";
const API: &str = "https://api.modrinth.com/v2";

/// Project IDs and slugs: Modrinth's IDs are 8 base62 characters, slugs
/// letters, digits and `-_.!` (at most 64).
pub fn is_ref(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= 64
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'!'))
}

fn path_part(text: &str) -> String {
    percent_encoding::utf8_percent_encode(text, percent_encoding::NON_ALPHANUMERIC).to_string()
}

pub fn project_url(reference: &str) -> String {
    format!("{API}/project/{}", path_part(reference))
}

pub fn members_url(id: &str) -> String {
    format!("{API}/project/{}/members", path_part(id))
}

pub fn versions_url(id: &str) -> String {
    format!("{API}/project/{}/version", path_part(id))
}

pub fn version_url(id: &str) -> String {
    format!("{API}/version/{}", path_part(id))
}

/// Search for a name, our kinds only, five hits.
pub fn search_url(query: &str) -> String {
    let facets = r#"[["project_type:mod","project_type:modpack","project_type:resourcepack","project_type:shader","project_type:datapack"]]"#;
    format!(
        "{API}/search?limit=5&query={}&facets={}",
        path_part(query),
        path_part(facets)
    )
}

fn text(value: &Value, key: &str, max: usize) -> String {
    clean(
        value.get(key).and_then(Value::as_str).unwrap_or_default(),
        max,
        false,
    )
}

fn time(value: &Value, key: &str) -> i64 {
    value
        .get(key)
        .and_then(Value::as_str)
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|t| t.timestamp())
        .unwrap_or(0)
}

fn kind(project_type: &str) -> Option<&'static str> {
    Some(match project_type {
        "mod" => "mod",
        "modpack" => "modpack",
        "resourcepack" => "resourcepack",
        "shader" => "shader",
        "datapack" => "datapack",
        _ => return None,
    })
}

fn side(value: &Value, key: &str) -> String {
    match value.get(key).and_then(Value::as_str) {
        Some(s @ ("required" | "optional" | "unsupported")) => s.to_owned(),
        _ => "unknown".to_owned(),
    }
}

fn hex_of(value: Option<&Value>, len: usize) -> Option<String> {
    let text = value?.as_str()?.to_ascii_lowercase();
    (text.len() == len && text.bytes().all(|b| b.is_ascii_hexdigit())).then_some(text)
}

fn https(value: Option<&Value>) -> Option<String> {
    let url = value?.as_str()?;
    (url.starts_with("https://")
        && url.len() <= 2048
        && !url.chars().any(|c| c.is_control() || c.is_whitespace()))
    .then(|| url.to_owned())
}

/// A project (`/project/{id}`) and its members (`/project/{id}/members`).
pub fn project(value: &Value, members: Option<&Value>) -> Option<SourceProject> {
    let id = value.get("id")?.as_str().filter(|s| is_ref(s))?.to_owned();
    let slug = value
        .get("slug")
        .and_then(Value::as_str)
        .filter(|s| is_ref(s))
        .unwrap_or(&id)
        .to_ascii_lowercase();
    let project_type = value.get("project_type")?.as_str()?;
    let kind = kind(project_type)?;
    let mut categories: Vec<String> = ["categories", "additional_categories"]
        .iter()
        .flat_map(|k| {
            value
                .get(*k)
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default()
        })
        .filter_map(|c| c.as_str().and_then(word))
        .take(32)
        .collect();
    categories.sort();
    categories.dedup();
    let mut authors: Vec<(i64, String, String)> = members
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|m| {
            let name = clean(m.get("user")?.get("username")?.as_str()?, 64, false);
            (!name.is_empty()).then(|| {
                (
                    m.get("ordering").and_then(Value::as_i64).unwrap_or(0),
                    name,
                    text(m, "role", 64),
                )
            })
        })
        .take(50)
        .collect();
    authors.sort_by_key(|(ordering, _, _)| *ordering);
    let mut gallery: Vec<(i64, SourceImage)> = value
        .get("gallery")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|g| {
            Some((
                g.get("ordering").and_then(Value::as_i64).unwrap_or(0),
                SourceImage {
                    url: https(g.get("raw_url").or_else(|| g.get("url")))?,
                    title: text(g, "title", 200),
                    description: text(g, "description", 1000),
                },
            ))
        })
        .take(40)
        .collect();
    gallery.sort_by_key(|(ordering, _)| *ordering);
    let license = value
        .get("license")
        .map(|l| text(l, "id", 64))
        .unwrap_or_default();
    Some(SourceProject {
        source: SOURCE,
        url: format!("https://modrinth.com/{project_type}/{slug}"),
        source_id: id,
        slug,
        kind,
        title: text(value, "title", 256),
        summary: text(value, "description", 1000),
        body: clean(
            value
                .get("body")
                .and_then(Value::as_str)
                .unwrap_or_default(),
            64 * 1024,
            true,
        ),
        license,
        icon_url: https(value.get("icon_url")),
        client_side: side(value, "client_side"),
        server_side: side(value, "server_side"),
        downloads: value
            .get("downloads")
            .and_then(Value::as_i64)
            .unwrap_or(0)
            .max(0),
        followers: value
            .get("followers")
            .and_then(Value::as_i64)
            .unwrap_or(0)
            .max(0),
        categories,
        authors: authors.into_iter().map(|(_, n, r)| (n, r)).collect(),
        gallery: gallery.into_iter().map(|(_, g)| g).collect(),
        published: time(value, "published"),
        updated: time(value, "updated"),
    })
}

/// One version (`/version/{id}` or an item of `/project/{id}/version`).
pub fn version(value: &Value) -> Option<SourceVersion> {
    let id = value.get("id")?.as_str().filter(|s| is_ref(s))?.to_owned();
    let channel = match value.get("version_type").and_then(Value::as_str) {
        Some(c @ ("release" | "beta" | "alpha")) => c.to_owned(),
        _ => "release".to_owned(),
    };
    let words = |key: &str| -> Vec<String> {
        let mut list: Vec<String> = value
            .get(key)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|v| v.as_str().and_then(word))
            .take(500)
            .collect();
        list.sort();
        list.dedup();
        list
    };
    let files: Vec<SourceFile> = value
        .get("files")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|f| {
            let hashes = f.get("hashes");
            let name = clean(f.get("filename")?.as_str()?, 255, false);
            if name.is_empty() || name.contains(['/', '\\']) || name.starts_with('.') {
                return None;
            }
            Some(SourceFile {
                name,
                size: f.get("size").and_then(Value::as_i64).unwrap_or(0).max(0),
                sha1: hex_of(hashes.and_then(|h| h.get("sha1")), 40),
                sha512: hex_of(hashes.and_then(|h| h.get("sha512")), 128),
                url: https(f.get("url")).filter(|u| crate::fetch::site(u).is_some())?,
                primary: f.get("primary").and_then(Value::as_bool).unwrap_or(false),
                role: match f.get("file_type").and_then(Value::as_str) {
                    Some("required-resource-pack") => "resourcepack-required",
                    Some("optional-resource-pack") => "resourcepack-optional",
                    _ => "file",
                }
                .to_owned(),
            })
        })
        .take(20)
        .collect();
    let dependencies = value
        .get("dependencies")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|d| {
            let kind = match d.get("dependency_type").and_then(Value::as_str) {
                Some(k @ ("required" | "optional" | "incompatible" | "embedded")) => k.to_owned(),
                _ => return None,
            };
            let reference = |key: &str| {
                d.get(key)
                    .and_then(Value::as_str)
                    .filter(|s| is_ref(s))
                    .map(str::to_owned)
            };
            let (project, version) = (reference("project_id"), reference("version_id"));
            (project.is_some() || version.is_some()).then_some(SourceDependency {
                kind,
                project,
                version,
            })
        })
        .take(100)
        .collect();
    Some(SourceVersion {
        source_id: id,
        number: text(value, "version_number", 128),
        name: text(value, "name", 256),
        changelog: clean(
            value
                .get("changelog")
                .and_then(Value::as_str)
                .unwrap_or_default(),
            32 * 1024,
            true,
        ),
        channel,
        published: time(value, "date_published"),
        downloads: value
            .get("downloads")
            .and_then(Value::as_i64)
            .unwrap_or(0)
            .max(0),
        games: words("game_versions"),
        loaders: words("loaders"),
        files,
        dependencies,
        details: true,
    })
}

/// A search hit, offered to the player who typed a name. Its pictures are
/// kept here first: the launcher shows nothing from another site, so players'
/// addresses stay here.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    pub source: &'static str,
    pub id: String,
    pub slug: String,
    pub kind: &'static str,
    pub title: String,
    pub summary: String,
    pub author: String,
    pub downloads: i64,
    /// How the import names the project, when it is not `id` (CurseForge's
    /// `<id>/<class>/<slug>`). Kept with the request, never taken from the
    /// player.
    #[serde(rename = "ref", skip_serializing_if = "Option::is_none")]
    pub reference: Option<String>,
    /// Its icon and its poster (the picture its source shows on a card), at
    /// our address once kept.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub banner: Option<String>,
    /// Where they are at the source; never sent on.
    #[serde(skip)]
    pub icon_url: Option<String>,
    #[serde(skip)]
    pub banner_url: Option<String>,
}

/// A picture address of a hit we may fetch.
fn picture_url(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .filter(|u| crate::fetch::site(u).is_some())
        .map(str::to_owned)
}

pub fn candidates(value: &Value) -> Vec<Candidate> {
    value
        .get("hits")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|hit| {
            Some(Candidate {
                source: SOURCE,
                id: hit
                    .get("project_id")?
                    .as_str()
                    .filter(|s| is_ref(s))?
                    .to_owned(),
                slug: hit.get("slug")?.as_str().filter(|s| is_ref(s))?.to_owned(),
                kind: kind(hit.get("project_type")?.as_str()?)?,
                title: text(hit, "title", 256),
                summary: text(hit, "description", 500),
                author: text(hit, "author", 64),
                downloads: hit
                    .get("downloads")
                    .and_then(Value::as_i64)
                    .unwrap_or(0)
                    .max(0),
                icon_url: picture_url(hit.get("icon_url")),
                banner_url: picture_url(hit.get("featured_gallery"))
                    .or_else(|| picture_url(hit.get("gallery").and_then(|g| g.get(0)))),
                ..Candidate::default()
            })
        })
        .take(5)
        .collect()
}
