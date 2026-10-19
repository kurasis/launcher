//! Minecraft: Java Edition news from Mojang without minecraft.net: the same
//! patch notes and news the official launcher shows
//! (`launchercontent.mojang.com`) and the version list (`piston-meta`).

use serde::{Deserialize, Serialize};

use crate::html::{self, Block};
use crate::net;

pub const CONTENT: &str = "https://launchercontent.mojang.com/v2/";
pub const MANIFEST: &str = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";
/// Patch notes and news kept in the feed.
pub const PATCH_NOTES: usize = 40;
pub const NEWS: usize = 25;

/// An article the launcher can open: a patch note (`j-…`) or a news item
/// (`n-…`), cleaned.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Note {
    pub id: String,
    pub title: String,
    /// The game version of a patch note.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// release, snapshot, pre or rc (patch notes).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    pub date: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    pub summary: String,
    /// The article on minecraft.net (news).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    pub blocks: Vec<Block>,
}

/// Our ids: a prefix and Mojang's id, lower case letters, digits and dashes.
pub fn note_id(id: &str) -> bool {
    (3..=80).contains(&id.len())
        && (id.starts_with("j-") || id.starts_with("n-"))
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

fn mojang_id(id: &str) -> Option<String> {
    let id = id.trim().to_ascii_lowercase();
    let id: String = id
        .chars()
        .map(|c| if c == '.' || c == '_' { '-' } else { c })
        .take(64)
        .collect();
    (!id.is_empty()
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-'))
    .then_some(id)
}

/// A version's kind by its name: `26.4-snapshot-4`, `25w14a` → snapshot,
/// `26.3-pre-1` → pre, `26.3-rc-2` → rc, anything else of type release →
/// release.
pub fn version_kind(id: &str, release: bool) -> &'static str {
    if id.contains("-pre") || id.contains(" Pre-Release") {
        "pre"
    } else if id.contains("-rc") || id.contains(" Release Candidate") {
        "rc"
    } else if release {
        "release"
    } else {
        "snapshot"
    }
}

#[derive(Deserialize)]
struct PatchList {
    entries: Vec<PatchEntry>,
}

#[derive(Deserialize)]
struct Picture {
    url: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PatchEntry {
    title: String,
    version: String,
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    image: Option<Picture>,
    content_path: String,
    id: String,
    date: String,
}

#[derive(Deserialize)]
struct PatchBody {
    body: String,
}

#[derive(Deserialize)]
struct NewsList {
    entries: Vec<NewsEntry>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct NewsEntry {
    title: String,
    #[serde(default)]
    date: String,
    #[serde(default)]
    text: String,
    #[serde(default)]
    news_page_image: Option<Picture>,
    #[serde(default)]
    play_page_image: Option<Picture>,
    #[serde(default)]
    read_more_link: Option<String>,
    #[serde(default)]
    news_type: Vec<String>,
    #[serde(default)]
    article_body: Option<String>,
    id: String,
}

/// A patch note or news item before its pictures are fetched.
pub struct Found {
    pub note: Note,
    /// The address of its main picture.
    pub image_url: Option<String>,
}

fn picture_url(path: &str) -> Option<String> {
    let url = url::Url::parse(CONTENT).ok()?.join(path).ok()?;
    (url.scheme() == "https").then(|| url.to_string())
}

/// A date as `YYYY-MM-DDTHH:MM:SSZ`, from Mojang's forms.
fn date(raw: &str) -> Option<String> {
    if let Ok(t) = chrono::DateTime::parse_from_rfc3339(raw) {
        return Some(
            t.with_timezone(&chrono::Utc)
                .format("%Y-%m-%dT%H:%M:%SZ")
                .to_string(),
        );
    }
    chrono::NaiveDate::parse_from_str(raw.get(..10)?, "%Y-%m-%d")
        .ok()
        .map(|d| format!("{}T12:00:00Z", d.format("%Y-%m-%d")))
}

/// Ids and dates of the newest patch notes, newest first; `known` says which
/// are fetched already (their body is not fetched again).
pub async fn patch_notes(
    http: &reqwest::Client,
    known: &(dyn Fn(&str) -> bool + Sync),
) -> Result<(Vec<String>, Vec<Found>), String> {
    let list: PatchList =
        net::get_json(http, &format!("{CONTENT}javaPatchNotes.json"), 4 << 20).await?;
    let mut entries: Vec<PatchEntry> = list
        .entries
        .into_iter()
        .filter(|e| date(&e.date).is_some())
        .collect();
    entries.sort_by_key(|e| std::cmp::Reverse(date(&e.date)));
    entries.truncate(PATCH_NOTES);
    let mut order = Vec::new();
    let mut found = Vec::new();
    for entry in entries {
        let Some(id) = mojang_id(&entry.id).map(|id| format!("j-{id}")) else {
            continue;
        };
        order.push(id.clone());
        if known(&id) {
            continue;
        }
        if !entry.content_path.ends_with(".json") || entry.content_path.contains("..") {
            continue;
        }
        let body: PatchBody =
            match net::get_json(http, &format!("{CONTENT}{}", entry.content_path), 4 << 20).await {
                Ok(body) => body,
                Err(e) => {
                    eprintln!("a patch note could not be fetched: {e}");
                    continue;
                }
            };
        let blocks = html::parse(&body.body, Some(CONTENT));
        let version = html::clean(&entry.version, 64);
        let kind = version_kind(&version, entry.kind == "release");
        found.push(Found {
            image_url: entry.image.as_ref().and_then(|p| picture_url(&p.url)),
            note: Note {
                id,
                title: html::clean(&entry.title, 200),
                kind: Some(kind.to_owned()),
                version: Some(version),
                date: date(&entry.date).unwrap_or_default(),
                image: None,
                summary: html::summary(&blocks, 600),
                url: None,
                blocks,
            },
        });
    }
    Ok((order, found))
}

/// Java news, newest first, as for patch notes.
pub async fn news(
    http: &reqwest::Client,
    known: &(dyn Fn(&str) -> bool + Sync),
) -> Result<(Vec<String>, Vec<Found>), String> {
    let list: NewsList = net::get_json(http, &format!("{CONTENT}news.json"), 8 << 20).await?;
    let mut entries: Vec<NewsEntry> = list
        .entries
        .into_iter()
        .filter(|e| e.news_type.iter().any(|t| t == "Java") && date(&e.date).is_some())
        .collect();
    entries.sort_by_key(|e| std::cmp::Reverse(date(&e.date)));
    entries.truncate(NEWS);
    let mut order = Vec::new();
    let mut found = Vec::new();
    for entry in entries {
        let Some(id) = mojang_id(&entry.id).map(|id| format!("n-{}", &id[..id.len().min(40)]))
        else {
            continue;
        };
        order.push(id.clone());
        if known(&id) {
            continue;
        }
        let blocks = entry
            .article_body
            .as_deref()
            .map(|b| html::parse(b, Some(CONTENT)))
            .unwrap_or_default();
        let summary = if entry.text.trim().is_empty() {
            html::summary(&blocks, 600)
        } else {
            html::shorten(&entry.text, 600)
        };
        let url = entry
            .read_more_link
            .as_deref()
            .and_then(|u| url::Url::parse(u).ok())
            .filter(|u| u.scheme() == "https" && u.host_str() == Some("www.minecraft.net"))
            .map(|mut u| {
                u.set_query(None);
                u.to_string()
            });
        found.push(Found {
            image_url: entry
                .news_page_image
                .as_ref()
                .or(entry.play_page_image.as_ref())
                .and_then(|p| picture_url(&p.url)),
            note: Note {
                id,
                title: html::clean(&entry.title, 200),
                version: None,
                kind: None,
                date: date(&entry.date).unwrap_or_default(),
                image: None,
                summary,
                url,
                blocks,
            },
        });
    }
    Ok((order, found))
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Version {
    pub id: String,
    pub kind: String,
    pub date: String,
}

#[derive(Deserialize)]
struct Manifest {
    versions: Vec<ManifestVersion>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ManifestVersion {
    id: String,
    #[serde(rename = "type")]
    kind: String,
    release_time: String,
}

/// The newest 300 versions, newest first.
pub async fn versions(http: &reqwest::Client) -> Result<Vec<Version>, String> {
    let manifest: Manifest = net::get_json(http, MANIFEST, 4 << 20).await?;
    let mut out: Vec<Version> = manifest
        .versions
        .into_iter()
        .filter(|v| v.kind == "release" || v.kind == "snapshot")
        .filter_map(|v| {
            let id = html::clean(&v.id, 64);
            Some(Version {
                kind: version_kind(&id, v.kind == "release").to_owned(),
                date: date(&v.release_time)?,
                id,
            })
        })
        .collect();
    out.sort_by(|a, b| b.date.cmp(&a.date));
    out.truncate(300);
    Ok(out)
}

/// Where the current update cycle is: its target version, how many
/// snapshots, pre-releases and release candidates it had, and whether the
/// release is out.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Cycle {
    pub target: String,
    pub snapshots: u32,
    pub pre: u32,
    pub rc: u32,
    pub released: bool,
    pub stage: String,
    pub latest: String,
    pub date: String,
}

/// The cycle of the newest version, when its name says its target
/// (`26.4-snapshot-4` → 26.4); older names (`25w14a`) give none.
pub fn cycle(versions: &[Version]) -> Option<Cycle> {
    let newest = versions.first()?;
    let target = match newest.kind.as_str() {
        "release" => newest.id.clone(),
        _ => newest.id.split_once('-')?.0.to_owned(),
    };
    if target.is_empty() || !target.chars().next()?.is_ascii_digit() {
        return None;
    }
    let count = |kind: &str| {
        versions
            .iter()
            .filter(|v| v.kind == kind && v.id.starts_with(&format!("{target}-")))
            .count() as u32
    };
    Some(Cycle {
        snapshots: count("snapshot"),
        pre: count("pre"),
        rc: count("rc"),
        released: versions
            .iter()
            .any(|v| v.kind == "release" && v.id == target),
        stage: newest.kind.clone(),
        latest: newest.id.clone(),
        date: newest.date.clone(),
        target,
    })
}
