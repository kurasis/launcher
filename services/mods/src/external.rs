//! Projects from sites that are not catalogs (D-066, external sources): added
//! by the owner's commands only (`lapi-mods external …`), never by a player.
//! Their files are kept here like any other (the authors allowed it), and
//! each project is checked again every few days, as the owner set for it.
//!
//! - `optifine`: OptiFine, read from optifine.net's downloads page. A file
//!   comes through the page each download link leads to (`adloadx`), whose
//!   link (`downloadx`) carries a token; what comes back must be a jar.
//! - `manual:<slug>`: a project the owner describes, its versions one file
//!   each, added by a command with the file's address on any public site.

use crate::model::{KINDS, SourceFile, SourceProject, SourceVersion, clean, word};

pub const SOURCE: &str = "external";
pub const OPTIFINE: &str = "optifine";
const MANUAL: &str = "manual:";

const OPTIFINE_HOST: &str = "optifine.net";
pub const OPTIFINE_PAGE: &str = "https://optifine.net/home";
pub const OPTIFINE_DOWNLOADS: &str = "https://optifine.net/downloads";
/// The owner's choice for OptiFine (Ai, 2026-10-21).
pub const OPTIFINE_DAYS: i64 = 3;
/// For a project the owner adds without saying.
pub const DEFAULT_DAYS: i64 = 7;
pub const MAX_DAYS: i64 = 365;

/// The downloads page is about 200 KB.
pub const MAX_PAGE: u64 = 4 * 1024 * 1024;
pub const MAX_CHANGELOG: u64 = 256 * 1024;
const MAX_ROWS: usize = 2000;
/// Changelogs read in one check (only those of new versions are read).
pub const MAX_CHANGELOGS: usize = 120;
/// Smaller than this is no mod (optifine.net answers a bad token with a
/// 200 and "Request not found.").
pub const MIN_ARCHIVE: u64 = 1024;

/// A project of this source, as the queue names it.
pub fn is_ref(source_id: &str) -> bool {
    source_id == OPTIFINE || manual_slug(source_id).is_some()
}

fn manual_slug(source_id: &str) -> Option<&str> {
    source_id.strip_prefix(MANUAL).filter(|s| is_slug(s))
}

/// A slug the owner gives a project: 2 to 48 of `a-z`, digits and `-`.
pub fn is_slug(slug: &str) -> bool {
    (2..=48).contains(&slug.len())
        && slug
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && !slug.starts_with('-')
        && !slug.ends_with('-')
}

pub fn manual_id(slug: &str) -> Option<String> {
    is_slug(slug).then(|| format!("{MANUAL}{slug}"))
}

/// What the owner wrote about a project (the `external_project` table).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Card {
    pub source_id: String,
    pub kind: String,
    pub slug: String,
    pub title: String,
    pub summary: String,
    pub page: String,
    pub icon: Option<String>,
    pub days: i64,
}

impl Card {
    pub fn optifine(days: i64) -> Self {
        Self {
            source_id: OPTIFINE.to_owned(),
            kind: "mod".to_owned(),
            slug: "optifine".to_owned(),
            title: "OptiFine".to_owned(),
            summary: "A Minecraft optimization mod: Minecraft runs faster and looks better, \
                      with HD textures, shaders and many settings."
                .to_owned(),
            page: OPTIFINE_PAGE.to_owned(),
            icon: None,
            days,
        }
    }

    /// The card as the catalog stores a project.
    pub fn project(&self, now: i64) -> SourceProject {
        let kind = KINDS
            .iter()
            .copied()
            .find(|k| *k == self.kind)
            .unwrap_or("mod");
        let body = if self.source_id == OPTIFINE {
            "OptiFine is a Minecraft optimization mod. It allows Minecraft to run faster and \
             look better with full support for HD textures and many configuration options.\n\n\
             Each version names the Forge it works with; versions without one are not for \
             Forge."
                .to_owned()
        } else {
            self.summary.clone()
        };
        SourceProject {
            source: SOURCE,
            source_id: self.source_id.clone(),
            slug: self.slug.clone(),
            kind,
            title: self.title.clone(),
            summary: self.summary.clone(),
            body,
            license: String::new(),
            icon_url: self.icon.clone(),
            client_side: "required".to_owned(),
            server_side: if self.source_id == OPTIFINE {
                "unsupported"
            } else {
                "unknown"
            }
            .to_owned(),
            downloads: 0,
            followers: 0,
            categories: Vec::new(),
            authors: Vec::new(),
            gallery: Vec::new(),
            url: self.page.clone(),
            published: now,
            updated: now,
        }
    }
}

/// A title or summary from the owner: one line, bounded, not empty.
pub fn text(value: &str, max: usize) -> Option<String> {
    let cleaned = clean(value, max, false);
    (!cleaned.is_empty()).then_some(cleaned)
}

/// A page or file address from the owner: plain HTTPS on a public name.
pub fn address(url: &str) -> Option<String> {
    crate::fetch::external_site(url).map(|_| url.to_owned())
}

/// Days between checks: 0 (never) to [`MAX_DAYS`].
pub fn days(text: &str) -> Option<i64> {
    text.parse::<i64>()
        .ok()
        .filter(|d| (0..=MAX_DAYS).contains(d))
}

// ----- OptiFine -----

/// A file name on optifine.net: `[preview_]OptiFine_<game>_HD_U_<edition>.jar`.
pub fn optifine_name(name: &str) -> bool {
    let Some(rest) = name
        .strip_prefix("preview_")
        .unwrap_or(name)
        .strip_prefix("OptiFine_")
    else {
        return false;
    };
    let Some(rest) = rest.strip_suffix(".jar") else {
        return false;
    };
    name.len() <= 96
        && rest
            .split_once('_')
            .is_some_and(|(game, edition)| optifine_game(game).is_some() && !edition.is_empty())
        && rest
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_'))
}

/// A game version as optifine.net writes it (`1.8.0`, `26.1.2`), as the
/// catalog does (`1.8`).
fn optifine_game(text: &str) -> Option<String> {
    let parts: Vec<&str> = text.split('.').collect();
    let ok = (2..=3).contains(&parts.len())
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.len() <= 3 && p.bytes().all(|b| b.is_ascii_digit()));
    if !ok {
        return None;
    }
    match parts.as_slice() {
        [major, minor, "0"] => Some(format!("{major}.{minor}")),
        _ => word(text),
    }
}

/// The address a version's file is fetched from: the page its link leads to.
pub fn optifine_file_url(name: &str) -> String {
    format!("https://{OPTIFINE_HOST}/adloadx?f={name}")
}

pub fn optifine_changelog_url(name: &str) -> String {
    format!("https://{OPTIFINE_HOST}/changelog?f={name}")
}

/// The file an address of [`optifine_file_url`] names.
pub fn optifine_file(url: &str) -> Option<String> {
    let name = url.strip_prefix(&format!("https://{OPTIFINE_HOST}/adloadx?f="))?;
    optifine_name(name).then(|| name.to_owned())
}

/// The download link on a file's page: `downloadx?f=<the same file>&x=<32
/// hex>`, on optifine.net.
pub fn optifine_link(page: &str, name: &str) -> Option<String> {
    let wanted = format!("downloadx?f={name}&x=");
    let mut from = 0;
    while let Some(at) = page[from..].find(&wanted) {
        let start = from + at + wanted.len();
        let token = page.get(start..start + 32)?;
        let ends = page[start + 32..]
            .chars()
            .next()
            .is_none_or(|c| matches!(c, '\'' | '"' | '&' | ' ' | '>'));
        if ends
            && token
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Some(format!("https://{OPTIFINE_HOST}/{wanted}{token}"));
        }
        from = start;
    }
    None
}

/// A row of the downloads page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub file: String,
    /// `OptiFine HD U J9`.
    pub title: String,
    pub game: String,
    /// The Forge it works with, as the page writes it (`61.0.8`, `#2847`).
    pub forge: Option<String>,
    pub preview: bool,
    pub published: i64,
}

/// The text of the first `<td class='<class>'>…</td>` in `row`.
fn cell<'a>(row: &'a str, class: &str) -> Option<&'a str> {
    let open = row
        .find(&format!("class='{class}'"))
        .or_else(|| row.find(&format!("class=\"{class}\"")))?;
    let after = &row[open..];
    let start = after.find('>')? + 1;
    let end = after[start..].find("</td>")?;
    Some(&after[start..start + end])
}

fn inner_text(html: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            c if !in_tag => out.push(c),
            _ => {}
        }
    }
    clean(&out, 120, false)
}

/// `DD.MM.YYYY` at midnight UTC.
fn date(text: &str) -> Option<i64> {
    let mut parts = text.trim().split('.');
    let day: u32 = parts.next()?.parse().ok()?;
    let month: u32 = parts.next()?.parse().ok()?;
    let year: i32 = parts.next()?.parse().ok()?;
    if parts.next().is_some() || !(2000..=2200).contains(&year) {
        return None;
    }
    chrono::NaiveDate::from_ymd_opt(year, month, day)?
        .and_hms_opt(0, 0, 0)
        .map(|t| t.and_utc().timestamp())
}

/// The rows of optifine.net's downloads page, each under its `<h2>Minecraft
/// …</h2>`; a row whose file names another game version is left out.
pub fn optifine_rows(html: &str) -> Vec<Row> {
    let mut rows = Vec::new();
    let sections: Vec<&str> = html.split("<h2>").skip(1).collect();
    for section in sections {
        let Some(heading) = section.split("</h2>").next() else {
            continue;
        };
        let Some(written) = heading.trim().strip_prefix("Minecraft ") else {
            continue;
        };
        let Some(game) = optifine_game(written.trim()) else {
            continue;
        };
        for row in section.split("<tr").skip(1) {
            if rows.len() >= MAX_ROWS {
                return rows;
            }
            let row = row.split("</tr>").next().unwrap_or(row);
            if !row.contains("downloadLine") {
                continue;
            }
            let Some(file) = cell(row, "colMirror").and_then(|c| {
                let at = c.find("adloadx?f=")? + "adloadx?f=".len();
                let rest = &c[at..];
                let end = rest.find(['"', '\'', '&']).unwrap_or(rest.len());
                Some(rest[..end].to_owned())
            }) else {
                continue;
            };
            let in_section = file
                .strip_prefix("preview_")
                .unwrap_or(&file)
                .starts_with(&format!("OptiFine_{}_", written.trim()));
            if !optifine_name(&file) || !in_section {
                continue;
            }
            let title = cell(row, "colFile").map(inner_text).unwrap_or_default();
            if title.is_empty() {
                continue;
            }
            let forge = cell(row, "colForge")
                .map(inner_text)
                .and_then(|f| f.strip_prefix("Forge ").map(str::to_owned))
                .filter(|f| f != "N/A" && !f.is_empty() && f.len() <= 24)
                .filter(|f| {
                    f.bytes()
                        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'#' | b'-'))
                });
            let Some(published) = cell(row, "colDate").and_then(date) else {
                continue;
            };
            rows.push(Row {
                preview: row.contains("downloadLinePreview") || file.starts_with("preview_"),
                file,
                title,
                game: game.clone(),
                forge,
                published,
            });
        }
    }
    rows
}

/// A version's ID at this source: the file's name.
pub fn optifine_version_id(file: &str) -> String {
    format!("{OPTIFINE}:{file}")
}

/// A row as a version; `changelog` when it was read.
pub fn optifine_version(row: &Row, changelog: Option<&str>) -> SourceVersion {
    let needs = match &row.forge {
        Some(forge) => format!("For Forge {forge}."),
        None => "Not for Forge.".to_owned(),
    };
    let details = changelog.is_some();
    let changelog = match changelog {
        Some(text) => format!("{needs}\n\n{}", clean(text, 60_000, true)),
        None => needs,
    };
    let edition = row.title.strip_prefix("OptiFine ").unwrap_or(&row.title);
    SourceVersion {
        source_id: optifine_version_id(&row.file),
        number: clean(&format!("{} {edition}", row.game), 64, false),
        name: row.title.clone(),
        changelog,
        channel: if row.preview { "beta" } else { "release" }.to_owned(),
        published: row.published,
        downloads: 0,
        games: vec![row.game.clone()],
        loaders: if row.forge.is_some() {
            vec!["forge".to_owned()]
        } else {
            Vec::new()
        },
        files: vec![SourceFile {
            name: row.file.clone(),
            // optifine.net does not say; our hashes are made when it is kept.
            size: 0,
            sha1: None,
            sha512: None,
            url: optifine_file_url(&row.file),
            primary: true,
            role: "file".to_owned(),
        }],
        dependencies: Vec::new(),
        details,
    }
}

// ----- the owner's versions -----

/// A version the owner added (the `external_version` table).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Added {
    pub file_url: String,
    pub file_name: String,
    pub number: String,
    pub channel: String,
    pub games: Vec<String>,
    pub loaders: Vec<String>,
    pub added: i64,
}

/// A file's name from its address: the last part of the path, plain.
pub fn file_name(url: &str) -> Option<String> {
    let parsed = reqwest::Url::parse(url).ok()?;
    let last = parsed.path_segments()?.next_back()?;
    let decoded = percent_encoding::percent_decode_str(last)
        .decode_utf8()
        .ok()?;
    let name: String = decoded
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | '+'))
        .take(128)
        .collect();
    let lower = name.to_ascii_lowercase();
    (lower.ends_with(".jar") || lower.ends_with(".zip") || lower.ends_with(".mrpack"))
        .then_some(name)
}

/// A comma-separated list of words (game versions, loaders): 1 to 32.
pub fn words(text: &str) -> Option<Vec<String>> {
    let list: Option<Vec<String>> = text.split(',').map(|w| word(w.trim())).collect();
    list.filter(|l| (1..=32).contains(&l.len()))
}

pub fn channel(text: &str) -> Option<String> {
    matches!(text, "release" | "beta" | "alpha").then(|| text.to_owned())
}

pub fn added_version_id(project: &str, file_url: &str) -> String {
    use sha2::Digest;
    let hash = hex::encode(sha2::Sha256::digest(file_url.as_bytes()));
    format!("{project}:{}", &hash[..16])
}

pub fn added_version(project: &Card, added: &Added) -> SourceVersion {
    SourceVersion {
        source_id: added_version_id(&project.source_id, &added.file_url),
        number: added.number.clone(),
        name: format!("{} {}", project.title, added.number),
        changelog: String::new(),
        channel: added.channel.clone(),
        published: added.added,
        downloads: 0,
        games: added.games.clone(),
        loaders: added.loaders.clone(),
        files: vec![SourceFile {
            name: added.file_name.clone(),
            size: 0,
            sha1: None,
            sha512: None,
            url: added.file_url.clone(),
            primary: true,
            role: "file".to_owned(),
        }],
        dependencies: Vec::new(),
        details: true,
    }
}

/// Whether a downloaded file is an archive (a jar, a zip, a `.mrpack`):
/// every kind of project is one, and a site's error page is not.
pub fn is_archive(path: &std::path::Path) -> bool {
    use std::io::Read;
    let Ok(mut file) = std::fs::File::open(path) else {
        return false;
    };
    let mut magic = [0u8; 4];
    let size = file.metadata().map(|m| m.len()).unwrap_or(0);
    size >= MIN_ARCHIVE && file.read_exact(&mut magic).is_ok() && magic == *b"PK\x03\x04"
}

// ----- the owner's commands -----

const USAGE: &str = "usage: lapi-mods external add optifine [--days N] | \
manual <slug> --title T --kind K --page URL [--summary S] [--icon URL] [--days N] | \
version <project> --file URL --game G[,G] [--loader L[,L]] [--number N] [--channel C] | \
days <project> <N|default> | check <project> | list";

/// The arguments of a command as the workflow sends them: a JSON array of
/// at most 32 strings, in at most 64 KB.
pub fn stdin_args(text: &str) -> Result<Vec<String>, String> {
    let bad = || "the arguments are a JSON array of at most 32 strings".to_owned();
    if text.len() > 64 * 1024 {
        return Err(bad());
    }
    let list: Vec<String> = serde_json::from_str(text).map_err(|_| bad())?;
    if list.len() > 32 || list.iter().any(|a| a.len() > 2048) {
        return Err(bad());
    }
    Ok(list)
}

/// `--name value` pairs, each of the allowed names at most once.
fn flags<'a>(
    args: &'a [String],
    allowed: &[&str],
) -> Result<std::collections::BTreeMap<&'a str, &'a str>, String> {
    let mut out = std::collections::BTreeMap::new();
    let mut rest = args.iter();
    while let Some(name) = rest.next() {
        let key = name
            .strip_prefix("--")
            .filter(|k| allowed.contains(k))
            .ok_or_else(|| format!("unknown option (allowed: {})", allowed.join(", ")))?;
        let value = rest
            .next()
            .ok_or_else(|| format!("--{key} needs a value"))?;
        if value.len() > 2048 || out.insert(key, value.as_str()).is_some() {
            return Err(format!("--{key} is given twice or too long"));
        }
    }
    Ok(out)
}

fn days_flag(flags: &std::collections::BTreeMap<&str, &str>, default: i64) -> Result<i64, String> {
    flags
        .get("days")
        .map_or(Some(default), |d| days(d))
        .ok_or_else(|| format!("--days is 0 (never) to {MAX_DAYS}"))
}

/// The ID at this source of a project the owner names: `optifine`,
/// `manual:<slug>` or the slug alone.
fn named(store: &crate::store::Store, name: &str) -> Result<Option<String>, String> {
    let name = name.trim().to_ascii_lowercase();
    for candidate in [Some(name.clone()), manual_id(&name)].into_iter().flatten() {
        if is_ref(&candidate) && store.external(&candidate)?.is_some() {
            return Ok(Some(candidate));
        }
    }
    Ok(None)
}

/// `lapi-mods external …`: what it prints is fixed words, numbers and the
/// owner's own names of projects, fit for the workflow's public log.
pub fn command(
    store: &crate::store::Store,
    args: &[String],
    now: i64,
) -> Result<Vec<String>, String> {
    let queue = |source_id: &str| -> Result<String, String> {
        let queued = crate::import::queue_external(store, source_id)?;
        Ok(if queued {
            format!("{source_id}: queued for a check")
        } else {
            format!("{source_id}: a check is queued already")
        })
    };
    match args {
        [add, site, rest @ ..] if add == "add" => {
            let optifine = site == OPTIFINE
                || crate::fetch::site(site) == Some(crate::fetch::Site::External)
                    && reqwest::Url::parse(site).is_ok_and(|u| u.host_str() == Some(OPTIFINE_HOST));
            if !optifine {
                return Err(
                    "only optifine has its own reader; add others with `external manual`".into(),
                );
            }
            let flags = flags(rest, &["days"])?;
            let kept = store.external(OPTIFINE)?.map(|c| c.days);
            let days = days_flag(&flags, kept.unwrap_or(OPTIFINE_DAYS))?;
            store.set_external(&Card::optifine(days), now)?;
            set_days(store, OPTIFINE, days)?;
            Ok(vec![
                format!("{OPTIFINE}: every {days} days"),
                queue(OPTIFINE)?,
            ])
        }
        [manual, slug, rest @ ..] if manual == "manual" => {
            let source_id = manual_id(slug).ok_or("a slug is 2 to 48 of a-z, 0-9 and -")?;
            let flags = flags(rest, &["title", "kind", "page", "summary", "icon", "days"])?;
            let kind = flags
                .get("kind")
                .copied()
                .filter(|k| KINDS.contains(k))
                .ok_or_else(|| format!("--kind is one of {}", KINDS.join(", ")))?;
            let title = flags
                .get("title")
                .and_then(|t| text(t, 128))
                .ok_or("--title is needed")?;
            let page = flags
                .get("page")
                .and_then(|p| address(p))
                .ok_or("--page is an https address on a public site")?;
            let summary = match flags.get("summary") {
                Some(s) => text(s, 512).ok_or("--summary is empty")?,
                None => String::new(),
            };
            let icon = match flags.get("icon") {
                Some(i) => Some(
                    crate::fetch::picture_site(i)
                        .map(|_| (*i).to_owned())
                        .ok_or("--icon is an https address on a public site")?,
                ),
                None => None,
            };
            let kept = store.external(&source_id)?.map(|c| c.days);
            let days = days_flag(&flags, kept.unwrap_or(DEFAULT_DAYS))?;
            store.set_external(
                &Card {
                    source_id: source_id.clone(),
                    kind: kind.to_owned(),
                    slug: slug.clone(),
                    title,
                    summary,
                    page,
                    icon,
                    days,
                },
                now,
            )?;
            set_days(store, &source_id, days)?;
            Ok(vec![
                format!("{source_id}: every {days} days"),
                queue(&source_id)?,
            ])
        }
        [version, project, rest @ ..] if version == "version" => {
            let source_id = named(store, project)?.ok_or("no such project of the owner's")?;
            if source_id == OPTIFINE {
                return Err("optifine's versions come from its site".into());
            }
            let flags = flags(rest, &["file", "game", "loader", "number", "channel"])?;
            let file_url = flags
                .get("file")
                .and_then(|f| address(f))
                .ok_or("--file is an https address on a public site")?;
            let file_name =
                file_name(&file_url).ok_or("the file's name ends in .jar, .zip or .mrpack")?;
            let games = flags
                .get("game")
                .and_then(|g| words(g))
                .ok_or("--game is a list of game versions, such as 1.21.1,1.21")?;
            let loaders = match flags.get("loader") {
                Some(l) => {
                    words(l).ok_or("--loader is a list of loaders, such as forge,neoforge")?
                }
                None => Vec::new(),
            };
            let number = match flags.get("number") {
                Some(n) => text(n, 64).ok_or("--number is empty")?,
                None => clean(
                    file_name
                        .rsplit_once('.')
                        .map_or(file_name.as_str(), |(stem, _)| stem),
                    64,
                    false,
                ),
            };
            let channel = match flags.get("channel") {
                Some(c) => channel(c).ok_or("--channel is release, beta or alpha")?,
                None => "release".to_owned(),
            };
            store.add_external_version(
                &source_id,
                &Added {
                    file_url,
                    file_name,
                    number,
                    channel,
                    games,
                    loaders,
                    added: now,
                },
            )?;
            Ok(vec![
                format!(
                    "{source_id}: {} versions added by hand",
                    store.external_versions(&source_id)?.len()
                ),
                queue(&source_id)?,
            ])
        }
        [d, project, value] if d == "days" => {
            let days = if value == "default" {
                None
            } else {
                Some(
                    days(value)
                        .ok_or_else(|| format!("days are 0 (never) to {MAX_DAYS}, or default"))?,
                )
            };
            if let Some(source_id) = named(store, project)? {
                let days = days.ok_or("a project of the owner's has its days; 0 is never")?;
                let mut card = store.external(&source_id)?.ok_or("no such project")?;
                card.days = days;
                store.set_external(&card, now)?;
                set_days(store, &source_id, days)?;
                return Ok(vec![format!("{source_id}: every {days} days")]);
            }
            let id = store.find_project(project)?.ok_or("no such project")?;
            store.set_recheck_days(&id, days)?;
            Ok(vec![match days {
                Some(0) => format!("{id}: never checked"),
                Some(days) => format!("{id}: checked at least every {days} days"),
                None => format!("{id}: checked as usual"),
            }])
        }
        [check, project] if check == "check" => {
            let source_id = named(store, project)?.ok_or("no such project of the owner's")?;
            Ok(vec![queue(&source_id)?])
        }
        [list] if list == "list" => {
            let rows = store.externals()?;
            let mut out = vec![format!("{} projects", rows.len())];
            for row in rows {
                let fetched = row.fetched.map_or_else(
                    || "never".to_owned(),
                    |t| {
                        chrono::DateTime::from_timestamp(t, 0)
                            .map_or_else(String::new, |d| d.format("%Y-%m-%d %H:%M").to_string())
                    },
                );
                out.push(format!(
                    "{} {} every {} days, {} versions, {} kept, read {fetched}",
                    row.source_id, row.kind, row.days, row.versions, row.kept
                ));
            }
            Ok(out)
        }
        _ => Err(USAGE.to_owned()),
    }
}

/// The days of an imported project, as its card says.
fn set_days(store: &crate::store::Store, source_id: &str, days: i64) -> Result<(), String> {
    if let Some(id) = store.project_by_source(SOURCE, source_id)? {
        store.set_recheck_days(&id, Some(days))?;
    }
    Ok(())
}
