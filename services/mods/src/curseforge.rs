//! CurseForge without a key (D-066 stage 4): the site's own API at
//! www.curseforge.com/api/v1 for files and dependencies, forgecdn for the
//! files themselves, and the project's page for its card. What the site
//! sends is checked and bounded like Modrinth's answers; anything that does
//! not fit is left out.
//!
//! The site's API answers the server for a project's files, one file, its
//! changelog and dependencies, and its download link (a redirect to
//! forgecdn); the project itself and search are refused (403). The project's
//! page answers with its data in the Next.js App Router stream
//! (`self.__next_f`), JSON-LD and Open Graph tags (`lapi-mods probe`).

use serde_json::Value;

use crate::model::{SourceDependency, SourceFile, SourceImage, SourceVersion, clean, word};

pub const SOURCE: &str = "curseforge";
const SITE: &str = "https://www.curseforge.com";
const API: &str = "https://www.curseforge.com/api/v1/mods";
/// Minecraft on CurseForge.
const GAME: u64 = 432;
/// Files asked for at once.
pub const PAGE_SIZE: usize = 50;

/// The site's classes: the path word and our kind.
pub const CLASSES: &[(&str, &str)] = &[
    ("mc-mods", "mod"),
    ("modpacks", "modpack"),
    ("texture-packs", "resourcepack"),
    ("shaders", "shader"),
    ("data-packs", "datapack"),
    ("worlds", "world"),
];

/// Our kind of a class's path word.
pub fn kind_of_class(class: &str) -> Option<&'static str> {
    CLASSES.iter().find(|(c, _)| *c == class).map(|(_, k)| *k)
}

/// A project's or a file's ID: a positive number of at most 10 digits.
pub fn id(text: &str) -> Option<u64> {
    (!text.is_empty() && text.len() <= 10 && text.bytes().all(|b| b.is_ascii_digit()))
        .then(|| text.parse::<u64>().ok())
        .flatten()
        .filter(|n| *n > 0)
}

/// A project's slug: lowercase letters, digits, `-` and `_`, at most 128.
pub fn is_slug(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= 128
        && text
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'-' | b'_'))
}

/// The site's class IDs for the classes we keep.
const CLASS_IDS: &[(u64, &str)] = &[
    (6, "mc-mods"),
    (4471, "modpacks"),
    (12, "texture-packs"),
    (6552, "shaders"),
    (6945, "data-packs"),
    (17, "worlds"),
];

/// The site's search page for a name (Minecraft only).
pub fn search_url(words: &str) -> String {
    format!(
        "{SITE}/minecraft/search?search={}",
        percent_encoding::utf8_percent_encode(words, percent_encoding::NON_ALPHANUMERIC)
    )
}

pub fn page_url(class: &str, slug: &str) -> String {
    format!("{SITE}/minecraft/{class}/{slug}")
}

pub fn files_url(project: u64, page: usize) -> String {
    format!("{API}/{project}/files?pageIndex={page}&pageSize={PAGE_SIZE}")
}

pub fn file_url(project: u64, file: u64) -> String {
    format!("{API}/{project}/files/{file}")
}

pub fn dependencies_url(project: u64, file: u64) -> String {
    format!("{API}/{project}/files/{file}/dependencies")
}

pub fn changelog_url(project: u64, file: u64) -> String {
    format!("{API}/{project}/files/{file}/change-log")
}

/// The site's link to a file: a redirect to forgecdn.
pub fn download_url(project: u64, file: u64) -> String {
    format!("{API}/{project}/files/{file}/download")
}

fn text(value: &Value, key: &str, max: usize) -> String {
    clean(
        value.get(key).and_then(Value::as_str).unwrap_or_default(),
        max,
        false,
    )
}

fn time(value: &Value, key: &str) -> i64 {
    value.get(key).map_or(0, moment)
}

/// A moment: RFC 3339 text, or a number of seconds or milliseconds since
/// 1970 (between 2000 and 2100).
fn moment(value: &Value) -> i64 {
    let seconds = match value {
        Value::String(text) => text
            .strip_prefix("$D")
            .map_or(text.as_str(), |t| t)
            .parse::<chrono::DateTime<chrono::FixedOffset>>()
            .map(|t| t.timestamp())
            .unwrap_or(0),
        Value::Number(n) => {
            let n = n.as_i64().unwrap_or(0);
            if n > 100_000_000_000 { n / 1000 } else { n }
        }
        _ => 0,
    };
    if (946_684_800..4_102_444_800).contains(&seconds) {
        seconds
    } else {
        0
    }
}

fn number(value: &Value, key: &str) -> i64 {
    value.get(key).and_then(Value::as_i64).unwrap_or(0).max(0)
}

fn https(value: Option<&Value>) -> Option<String> {
    let url = value?.as_str()?;
    (url.starts_with("https://")
        && url.len() <= 2048
        && !url.chars().any(|c| c.is_control() || c.is_whitespace()))
    .then(|| url.to_owned())
}

/// A loader among the words of a file's `gameVersions`.
fn loader(word: &str) -> Option<&'static str> {
    Some(match word.to_ascii_lowercase().as_str() {
        "forge" => "forge",
        "neoforge" => "neoforge",
        "fabric" => "fabric",
        "quilt" => "quilt",
        "liteloader" => "liteloader",
        "iris" => "iris",
        "optifine" => "optifine",
        "canvas" => "canvas",
        _ => return None,
    })
}

/// A game version among those words: they start with a digit (`1.20.1`,
/// `24w14a`); `Client`, `Server` and `Java 17` do not.
fn game(text: &str) -> Option<String> {
    text.starts_with(|c: char| c.is_ascii_digit())
        .then(|| word(text))
        .flatten()
}

/// A file name we may write to disk.
fn file_name(text: &str) -> Option<String> {
    let name = clean(text, 255, false);
    (!name.is_empty() && !name.contains(['/', '\\']) && !name.starts_with('.')).then_some(name)
}

/// One file of the site's API, as a version of ours (CurseForge's files
/// are its versions). Files not released (status other than approved or
/// released) and early-access ones are left out.
pub fn file(project: u64, value: &Value) -> Option<SourceVersion> {
    let file_id = value.get("id")?.as_u64().filter(|n| *n > 0)?;
    if value.get("projectId").and_then(Value::as_u64) != Some(project)
        || !matches!(value.get("status").and_then(Value::as_u64), Some(4 | 10))
        || value
            .get("isEarlyAccessContent")
            .and_then(Value::as_bool)
            .unwrap_or(false)
    {
        return None;
    }
    let name = file_name(value.get("fileName")?.as_str()?)?;
    let words: Vec<&str> = value
        .get("gameVersions")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .take(500)
        .collect();
    let mut games: Vec<String> = words.iter().filter_map(|w| game(w)).collect();
    games.sort();
    games.dedup();
    let mut loaders: Vec<String> = words
        .iter()
        .filter_map(|w| loader(w))
        .map(str::to_owned)
        .collect();
    loaders.sort();
    loaders.dedup();
    let display = text(value, "displayName", 256);
    Some(SourceVersion {
        source_id: file_id.to_string(),
        number: if display.is_empty() {
            clean(&name, 128, false)
        } else {
            clean(&display, 128, false)
        },
        name: display,
        changelog: String::new(),
        channel: match value.get("releaseType").and_then(Value::as_u64) {
            Some(2) => "beta",
            Some(3) => "alpha",
            _ => "release",
        }
        .to_owned(),
        published: time(value, "dateCreated"),
        downloads: number(value, "totalDownloads"),
        games,
        loaders,
        files: vec![SourceFile {
            name,
            size: number(value, "fileLength"),
            // The site gives no hashes: the file is checked by its size,
            // and ours are made when it is kept.
            sha1: None,
            sha512: None,
            url: download_url(project, file_id),
            primary: true,
            role: "file".to_owned(),
        }],
        dependencies: Vec::new(),
        details: false,
    })
}

/// A page of the files list: the versions on it, how many entries it had
/// (some are left out), and how many the project has.
pub fn files(project: u64, value: &Value) -> (Vec<SourceVersion>, usize, usize) {
    let entries = value
        .get("data")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    let versions = entries
        .iter()
        .take(PAGE_SIZE)
        .filter_map(|f| file(project, f))
        .collect();
    let total = value
        .get("pagination")
        .and_then(|p| p.get("totalCount"))
        .and_then(Value::as_u64)
        .unwrap_or(0)
        .min(100_000) as usize;
    (versions, entries.len().min(PAGE_SIZE), total)
}

/// A dependency's project as the site names it: enough to fetch it later.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Needed {
    pub dependency: SourceDependency,
    pub class: &'static str,
    pub slug: String,
}

/// A file's dependencies (`…/files/{id}/dependencies`). Tools are left out:
/// nothing in the game needs them.
pub fn dependencies(value: &Value) -> Vec<Needed> {
    value
        .get("data")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|d| {
            let kind = match d.get("type").and_then(Value::as_str)? {
                "RequiredDependency" => "required",
                "OptionalDependency" => "optional",
                "Incompatible" => "incompatible",
                "EmbeddedLibrary" => "embedded",
                _ => return None,
            };
            let project = d.get("id")?.as_u64().filter(|n| *n > 0)?;
            let slug = d
                .get("slug")
                .and_then(Value::as_str)
                .filter(|s| is_slug(s))?
                .to_owned();
            let class = d
                .get("categoryClass")
                .and_then(|c| c.get("slug"))
                .and_then(Value::as_str)
                .and_then(|c| CLASSES.iter().find(|(name, _)| *name == c))
                .map(|(name, _)| *name)?;
            Some(Needed {
                dependency: SourceDependency {
                    kind: kind.to_owned(),
                    project: Some(project.to_string()),
                    version: None,
                },
                class,
                slug,
            })
        })
        .take(100)
        .collect()
}

/// A file's changelog (HTML, kept as text; the launcher draws only text,
/// line breaks and pictures of it).
pub fn changelog(value: &Value) -> String {
    clean(
        value
            .get("changelogBody")
            .and_then(Value::as_str)
            .unwrap_or_default(),
        32 * 1024,
        true,
    )
}

/// What a project's page tells of it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Card {
    pub id: Option<u64>,
    pub title: String,
    pub summary: String,
    /// The description (HTML from the site).
    pub body: String,
    pub icon: Option<String>,
    pub authors: Vec<String>,
    pub gallery: Vec<SourceImage>,
    pub categories: Vec<String>,
    pub downloads: i64,
    pub published: i64,
    pub updated: i64,
}

/// Pages are read up to this size.
pub const MAX_PAGE: usize = 8 * 1024 * 1024;

/// Reads a project's page: the project object in the App Router stream
/// (the one with this slug and a numeric ID), then JSON-LD and Open Graph
/// tags for what it lacks.
pub fn page(html: &str, slug: &str) -> Card {
    let html = &html[..floor(html, MAX_PAGE)];
    let stream = stream(html);
    let mut card = project_in(&stream, slug)
        .map(|p| card_of(&stream, &p))
        .unwrap_or_default();
    let ld = ld_json(html);
    let fill = |field: &mut String, value: Option<String>, max: usize| {
        if field.is_empty()
            && let Some(value) = value
        {
            *field = clean(&value, max, false);
        }
    };
    fill(
        &mut card.title,
        ld.as_ref()
            .and_then(|v| v.get("name")?.as_str().map(str::to_owned)),
        256,
    );
    fill(&mut card.title, meta(html, "og:title"), 256);
    fill(
        &mut card.summary,
        ld.as_ref()
            .and_then(|v| v.get("description")?.as_str().map(str::to_owned)),
        1000,
    );
    fill(&mut card.summary, meta(html, "og:description"), 1000);
    if card.icon.is_none() {
        card.icon = ld
            .as_ref()
            .and_then(|v| image_of(v.get("image")?))
            .or_else(|| meta(html, "og:image").filter(|u| u.starts_with("https://")));
    }
    if card.authors.is_empty()
        && let Some(author) = ld.as_ref().and_then(|v| v.get("author"))
    {
        card.authors = names(author);
    }
    if card.updated == 0 {
        card.updated = ld.as_ref().map_or(0, |v| time(v, "dateModified"));
    }
    if card.published == 0 {
        card.published = ld.as_ref().map_or(0, |v| time(v, "datePublished"));
    }
    card
}

/// The largest char boundary at or below `max`.
fn floor(text: &str, max: usize) -> usize {
    let mut end = text.len().min(max);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    end
}

/// The App Router stream: the strings of every `self.__next_f.push([1,"…"])`,
/// in order.
fn stream(html: &str) -> String {
    const PUSH: &str = "self.__next_f.push(";
    let mut out = String::new();
    let mut rest = html;
    while let Some(at) = rest.find(PUSH) {
        rest = &rest[at + PUSH.len()..];
        let mut values = serde_json::Deserializer::from_str(rest).into_iter::<Value>();
        if let Some(Ok(Value::Array(items))) = values.next()
            && items.first().and_then(Value::as_u64) == Some(1)
            && let Some(Value::String(chunk)) = items.get(1)
        {
            if out.len() + chunk.len() > MAX_PAGE {
                break;
            }
            out.push_str(chunk);
        }
    }
    out
}

/// The project object in the stream: an object with this slug, a numeric
/// `id`, and Minecraft's game (or a class) beside them.
fn project_in(text: &str, slug: &str) -> Option<Value> {
    let needle = format!("\"slug\":\"{slug}\"");
    let mut tries = 0;
    let mut from = 0;
    while let Some(found) = text[from..].find(&needle) {
        let at = from + found;
        from = at + needle.len();
        // Objects that start before it, nearest first.
        let start = floor(text, at.saturating_sub(64 * 1024));
        for (open, _) in text[start..at].rmatch_indices('{').take(64) {
            tries += 1;
            if tries > 512 {
                return None;
            }
            let mut values =
                serde_json::Deserializer::from_str(&text[start + open..]).into_iter::<Value>();
            if let Some(Ok(value)) = values.next()
                && let Some(project) = find_project(&value, slug, 0)
            {
                return Some(project.clone());
            }
        }
    }
    None
}

fn find_project<'a>(value: &'a Value, slug: &str, depth: usize) -> Option<&'a Value> {
    if depth > 32 {
        return None;
    }
    match value {
        Value::Object(map) => {
            let is_it = map.get("slug").and_then(Value::as_str) == Some(slug)
                && map.get("id").and_then(Value::as_u64).is_some_and(|n| n > 0)
                && (map.get("gameId").and_then(Value::as_u64) == Some(GAME)
                    || map.contains_key("classId")
                    || map.contains_key("class"));
            if is_it {
                return Some(value);
            }
            map.values().find_map(|v| find_project(v, slug, depth + 1))
        }
        Value::Array(items) => items
            .iter()
            .take(10_000)
            .find_map(|v| find_project(v, slug, depth + 1)),
        _ => None,
    }
}

/// A value of the stream: a reference (`"$1f"`) is replaced by the chunk it
/// names, a text chunk (`1f:T<hex length>,…`) as a string. Other values
/// are as they are.
fn resolved(text: &str, value: &Value) -> Value {
    let Some(id) = value
        .as_str()
        .and_then(|s| s.strip_prefix('$'))
        .map(|s| s.strip_prefix('L').unwrap_or(s))
        .filter(|id| !id.is_empty() && id.len() <= 8 && id.bytes().all(|b| b.is_ascii_hexdigit()))
    else {
        return value.clone();
    };
    chunk(text, id).unwrap_or(Value::Null)
}

/// The chunk with this ID in the stream.
fn chunk(text: &str, id: &str) -> Option<Value> {
    let label = format!("{id}:");
    let mut from = 0;
    let at = loop {
        let at = from + text[from..].find(&label)?;
        if at == 0 || text.as_bytes()[at - 1] == b'\n' {
            break at + label.len();
        }
        from = at + label.len();
    };
    let rest = &text[at..];
    if let Some(sized) = rest.strip_prefix('T') {
        let comma = sized.find(',').filter(|c| *c <= 8)?;
        let length = usize::from_str_radix(&sized[..comma], 16).ok()?;
        let body = &sized[comma + 1..];
        return Some(Value::String(body[..floor(body, length)].to_owned()));
    }
    serde_json::Deserializer::from_str(rest)
        .into_iter::<Value>()
        .next()?
        .ok()
}

fn card_of(stream: &str, project: &Value) -> Card {
    let field = |key: &str| project.get(key).map(|v| resolved(stream, v));
    let first_text = |keys: &[&str], max: usize| -> String {
        keys.iter()
            .map(|k| text(project, k, max))
            .find(|t| !t.is_empty())
            .unwrap_or_default()
    };
    let icon = ["logo", "avatar"]
        .iter()
        .filter_map(|k| project.get(*k))
        .find_map(|logo| {
            https(logo.get("url"))
                .or_else(|| https(logo.get("thumbnailUrl")))
                .or_else(|| https(Some(logo)))
        })
        .or_else(|| https(project.get("avatarUrl")))
        .or_else(|| https(project.get("logoUrl")));
    let authors = project
        .get("authors")
        .map(names)
        .filter(|a| !a.is_empty())
        .or_else(|| project.get("author").map(names))
        .unwrap_or_else(|| {
            let one = text(project, "authorName", 64);
            if one.is_empty() { vec![] } else { vec![one] }
        });
    let gallery = ["screenshots", "images", "gallery", "attachments"]
        .iter()
        .filter_map(|k| field(k))
        .find(|v| v.as_array().is_some_and(|a| !a.is_empty()))
        .unwrap_or(Value::Null);
    let gallery = gallery
        .as_array()
        .into_iter()
        .flatten()
        .take(200)
        .map(|s| resolved(stream, s))
        .filter_map(|s| {
            Some(SourceImage {
                url: https(s.get("url")).or_else(|| https(s.get("imageUrl")))?,
                title: text(&s, "title", 200),
                description: text(&s, "description", 1000),
            })
        })
        .take(40)
        .collect();
    let mut categories: Vec<String> = project
        .get("categories")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|c| c.get("slug").and_then(Value::as_str).and_then(word))
        .take(32)
        .collect();
    categories.sort();
    categories.dedup();
    Card {
        id: project.get("id").and_then(Value::as_u64),
        title: first_text(&["name", "title"], 256),
        summary: first_text(&["summary", "description"], 1000),
        body: ["description", "descriptionHtml", "about"]
            .iter()
            .filter_map(|k| field(k))
            .find_map(|d| {
                d.as_str()
                    .filter(|d| d.contains('<'))
                    .map(|d| clean(d, 64 * 1024, true))
            })
            .unwrap_or_default(),
        icon,
        authors,
        gallery,
        categories,
        downloads: ["downloadCount", "downloads", "totalDownloads"]
            .iter()
            .map(|k| number(project, k))
            .max()
            .unwrap_or(0),
        published: PUBLISHED
            .iter()
            .filter_map(|k| field(k))
            .map(|v| moment(&v))
            .find(|t| *t > 0)
            .unwrap_or(0),
        updated: UPDATED
            .iter()
            .filter_map(|k| field(k))
            .map(|v| moment(&v))
            .find(|t| *t > 0)
            .unwrap_or(0),
    }
}

/// The projects a search page lists, in the order the stream gives them:
/// objects with a numeric ID, a slug, a name and a class we keep. At most
/// five.
pub fn candidates(html: &str) -> Vec<crate::modrinth::Candidate> {
    let html = &html[..floor(html, MAX_PAGE)];
    let stream = stream(html);
    let mut found: Vec<crate::modrinth::Candidate> = Vec::new();
    let mut seen = 0;
    for start in chunk_starts(&stream) {
        let Some(Ok(value)) = serde_json::Deserializer::from_str(&stream[start..])
            .into_iter::<Value>()
            .next()
        else {
            continue;
        };
        collect(&value, 0, &mut seen, &mut found);
        if found.len() >= 5 {
            break;
        }
    }
    found
}

/// Where each JSON chunk of the stream starts (after `<hex id>:`).
fn chunk_starts(text: &str) -> impl Iterator<Item = usize> + '_ {
    std::iter::once(0)
        .chain(text.match_indices('\n').map(|(at, _)| at + 1))
        .filter_map(move |line| {
            let rest = &text[line..];
            let colon = rest.find(':').filter(|c| (1..=8).contains(c))?;
            rest[..colon]
                .bytes()
                .all(|b| b.is_ascii_hexdigit())
                .then_some(line + colon + 1)
                .filter(|at| matches!(text.as_bytes().get(*at), Some(b'[' | b'{')))
        })
        .take(10_000)
}

fn collect(
    value: &Value,
    depth: usize,
    seen: &mut usize,
    found: &mut Vec<crate::modrinth::Candidate>,
) {
    *seen += 1;
    if depth > 32 || *seen > 200_000 || found.len() >= 5 {
        return;
    }
    match value {
        Value::Object(map) => {
            if let Some(candidate) = candidate_of(value) {
                if !found.iter().any(|c| c.id == candidate.id) {
                    found.push(candidate);
                }
                return;
            }
            for v in map.values() {
                collect(v, depth + 1, seen, found);
            }
        }
        Value::Array(items) => {
            for v in items {
                collect(v, depth + 1, seen, found);
            }
        }
        _ => {}
    }
}

/// A search hit: the site's project object, if it is one we keep.
fn candidate_of(project: &Value) -> Option<crate::modrinth::Candidate> {
    let id = project
        .get("id")?
        .as_u64()
        .filter(|n| *n > 0 && *n < 10_000_000_000)?;
    let slug = project.get("slug")?.as_str().filter(|s| is_slug(s))?;
    let title = text(project, "name", 256);
    if title.is_empty() {
        return None;
    }
    let class = project
        .get("classId")
        .and_then(Value::as_u64)
        .and_then(|c| CLASS_IDS.iter().find(|(n, _)| *n == c))
        .map(|(_, name)| *name)
        .or_else(|| {
            ["class", "categoryClass"].iter().find_map(|k| {
                let word = project.get(*k)?.get("slug")?.as_str()?;
                CLASSES.iter().find(|(c, _)| *c == word).map(|(c, _)| *c)
            })
        })?;
    if project
        .get("gameId")
        .and_then(Value::as_u64)
        .is_some_and(|g| g != GAME)
    {
        return None;
    }
    let author = project
        .get("author")
        .or_else(|| project.get("authors"))
        .map(names)
        .and_then(|n| n.into_iter().next())
        .unwrap_or_else(|| text(project, "authorName", 64));
    Some(crate::modrinth::Candidate {
        source: SOURCE,
        id: id.to_string(),
        slug: slug.to_owned(),
        kind: kind_of_class(class)?,
        title,
        summary: text(project, "summary", 500),
        author,
        downloads: ["downloads", "downloadCount", "totalDownloads"]
            .iter()
            .map(|k| number(project, k))
            .max()
            .unwrap_or(0),
        reference: Some(
            Ref {
                id: Some(id),
                class: class.to_owned(),
                slug: slug.to_owned(),
            }
            .text(),
        ),
    })
}

/// Names a project's dates go by.
const PUBLISHED: &[&str] = &[
    "dateCreated",
    "dateReleased",
    "creationDate",
    "releaseDate",
    "createdAt",
];
const UPDATED: &[&str] = &[
    "dateModified",
    "dateUpdated",
    "updateDate",
    "lastUpdated",
    "updatedAt",
];

/// The shapes of the project object's fields the card could use, for the
/// probe: `<key>:<shape>` for each one present (our words only).
pub fn shapes(html: &str, slug: &str) -> Vec<String> {
    let html = &html[..floor(html, MAX_PAGE)];
    let stream = stream(html);
    let Some(project) = project_in(&stream, slug) else {
        return Vec::new();
    };
    let keys = [
        "description",
        "descriptionHtml",
        "about",
        "screenshots",
        "images",
        "gallery",
        "attachments",
    ]
    .iter()
    .chain(PUBLISHED)
    .chain(UPDATED);
    keys.filter_map(|key| {
        let value = project.get(*key)?;
        let found = resolved(&stream, value);
        let reference = found != *value;
        let shape = match found {
            Value::Null => "null".to_owned(),
            Value::Bool(_) => "bool".to_owned(),
            Value::Number(_) => "num".to_owned(),
            Value::String(s) => format!("str{}", s.len()),
            Value::Array(a) => format!("list{}", a.len()),
            Value::Object(_) => "obj".to_owned(),
        };
        Some(format!(
            "{key}:{}{shape}",
            if reference { "ref_" } else { "" }
        ))
    })
    .collect()
}

/// Names of authors: a string, `{name}`, `{username}`, or a list of those.
fn names(value: &Value) -> Vec<String> {
    let one = |v: &Value| -> Option<String> {
        let name = v
            .as_str()
            .or_else(|| v.get("name").and_then(Value::as_str))
            .or_else(|| v.get("username").and_then(Value::as_str))?;
        let name = clean(name, 64, false);
        (!name.is_empty()).then_some(name)
    };
    match value {
        Value::Array(items) => items.iter().filter_map(one).take(50).collect(),
        other => one(other).into_iter().collect(),
    }
}

/// An image's address: a string, `{url}`, or the first of a list.
fn image_of(value: &Value) -> Option<String> {
    match value {
        Value::Array(items) => items.iter().find_map(image_of),
        Value::Object(_) => https(value.get("url")),
        other => https(Some(other)),
    }
}

/// The first JSON-LD object of the page that has a name.
fn ld_json(html: &str) -> Option<Value> {
    const OPEN: &str = "application/ld+json";
    let mut rest = html;
    let mut tries = 0;
    while let Some(at) = rest.find(OPEN) {
        tries += 1;
        if tries > 16 {
            return None;
        }
        rest = &rest[at + OPEN.len()..];
        let body_start = rest.find('>')? + 1;
        let body_end = rest[body_start..].find("</script")? + body_start;
        let Ok(value) = serde_json::from_str::<Value>(&rest[body_start..body_end]) else {
            continue;
        };
        let candidates: Vec<&Value> = match &value {
            Value::Array(items) => items.iter().collect(),
            other => other
                .get("@graph")
                .and_then(Value::as_array)
                .map(|g| g.iter().collect())
                .unwrap_or_else(|| vec![other]),
        };
        if let Some(found) = candidates
            .into_iter()
            .find(|v| v.get("name").and_then(Value::as_str).is_some())
        {
            return Some(found.clone());
        }
    }
    None
}

/// The content of `<meta property="{name}" content="…">`, unescaped.
fn meta(html: &str, name: &str) -> Option<String> {
    let needle = format!("property=\"{name}\"");
    let at = html.find(&needle)?;
    let tag_start = html[..at].rfind('<')?;
    let tag_end = at + html[at..].find('>')?;
    let tag = &html[tag_start..tag_end];
    let content = tag.find("content=\"")? + "content=\"".len();
    let value = &tag[content..content + tag[content..].find('"')?];
    Some(unescape(value))
}

/// HTML's character references that tags use.
fn unescape(text: &str) -> String {
    text.replace("&quot;", "\"")
        .replace("&#x27;", "'")
        .replace("&#39;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

/// How a CurseForge project is named in the queue: `<class>/<slug>` from a
/// page's address, `<id>/<class>/<slug>` from a dependency.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ref {
    pub id: Option<u64>,
    pub class: String,
    pub slug: String,
}

impl Ref {
    pub fn parse(text: &str) -> Option<Self> {
        let parts: Vec<&str> = text.split('/').collect();
        let (id, class, slug) = match parts.as_slice() {
            [class, slug] => (None, *class, *slug),
            [id_text, class, slug] => (Some(id(id_text)?), *class, *slug),
            _ => return None,
        };
        (kind_of_class(class).is_some() && is_slug(slug)).then(|| Self {
            id,
            class: class.to_owned(),
            slug: slug.to_owned(),
        })
    }

    pub fn text(&self) -> String {
        match self.id {
            Some(id) => format!("{id}/{}/{}", self.class, self.slug),
            None => format!("{}/{}", self.class, self.slug),
        }
    }
}
