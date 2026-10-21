//! Players' own modpacks (D-066, stage 6). A player with a launcher account
//! publishes an installation as a pack of the catalog: an `.mrpack` whose
//! index names mods, resource packs and shaders the catalog keeps already
//! (found by their hashes; where the index says they download from is not
//! used), plus their own settings in `overrides/`. Next to the index the
//! archive carries `catalog.json` (title, summary, description, version
//! number and changes) and may carry `icon.png`, `icon.jpg` or `icon.webp`.
//!
//! Everything is checked before anything is kept: the archive's size, the
//! texts, the icon (decoded with limits), every file of the index (in the
//! catalog, not held, not blocked), every override (a path a pack may write,
//! never under `mods/`, no code or program, bounded in size), and the
//! scanner over the archive. The pack is then a project of kind `modpack`
//! with the source `own`, shown at once, and installed like any other pack.
//! The account that published it adds versions, hides it or shows it again;
//! players report it to the owner (`lapi-mods review`, kind `report`).
//!
//! A room pack (W12) is published the same way and checked the same, with
//! `room` asked for: the mods of a world with friends that its guests
//! install. It is never shown in the catalog, searched or listed among the
//! account's packs; its guests find a version by its ID only, which the
//! world's session names.

use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::Deserialize;
use serde_json::{Value, json};

use crate::App;
use crate::blobs::Hashed;
use crate::import::now;
use crate::model::{SourceFile, SourceProject, SourceVersion, clean, word};
use crate::pack;

/// The archive, whole. Cloudflare takes requests up to 100 MB, so a larger
/// one comes in parts ([`Uploads`]); one request carries at most
/// [`MAX_SINGLE`].
pub const MAX_ARCHIVE: u64 = 512 * 1024 * 1024;
pub const MAX_SINGLE: u64 = 90 * 1024 * 1024;
/// One part of an archive sent in parts; the last one may be shorter.
pub const CHUNK: u64 = if cfg!(test) {
    256 * 1024
} else {
    64 * 1024 * 1024
};
/// Archives being sent in parts at once, on the whole server, and how long
/// one waits for its next part.
const MAX_UPLOADS: usize = 8;
const UPLOAD_IDLE: std::time::Duration = std::time::Duration::from_secs(30 * 60);
/// One override, and all of them unpacked.
pub const MAX_OVERRIDE: u64 = 64 * 1024 * 1024;
pub const MAX_OVERRIDES: u64 = 512 * 1024 * 1024;
const MAX_ENTRIES: usize = 20_000;
const MAX_INDEX: u64 = 16 * 1024 * 1024;
pub const META: &str = "catalog.json";
const MAX_META: u64 = 256 * 1024;
pub const ICONS: &[&str] = &["icon.png", "icon.jpg", "icon.webp"];
pub const MAX_ICON: u64 = 512 * 1024;
const MAX_ICON_SIDE: u32 = 2048;
/// Per account: new packs and versions a day, and the bytes of all its
/// versions together.
pub const PACKS_PER_DAY: i64 = 5;
pub const VERSIONS_PER_DAY: i64 = 20;
pub const BYTES_PER_ACCOUNT: i64 = 2 * 1024 * 1024 * 1024;
/// Paths named in a refusal.
const MAX_NAMED: usize = 50;
/// Texts: a title on one line, a summary, a description and changes in
/// Markdown (characters).
const MAX_TITLE: usize = 80;
const MAX_SUMMARY: usize = 300;
pub const MAX_BODY: usize = 65_536;
const MAX_CHANGES: usize = 16_384;
/// What a player may report a pack for.
pub const REASONS: &[&str] = &["malware", "stolen", "inappropriate", "broken", "other"];
pub const MAX_REPORT: usize = 500;

/// Files of the game a pack never brings in its overrides, beyond the
/// programs [`pack::PROGRAMS`] names: code the game would load.
const CODE: &[&str] = &["jar", "class", "jnilib", "litemod", "zip-mod"];

/// Why a pack is refused: a short code, and the paths it is about.
#[derive(Debug, PartialEq, Eq)]
pub enum Refusal {
    /// Something wrong with what was sent (400).
    Bad(&'static str),
    /// Some of its files (422), named by their paths.
    Files(&'static str, Vec<String>),
    /// Not the asker's pack, or no such pack (404).
    NotFound,
    /// A version with that number is there (409); the pack was removed
    /// by the owner (410, `removed`).
    Conflict(&'static str),
    Gone,
    /// A limit of the account (429).
    Limit(&'static str),
    /// Our fault (500), logged with fixed words only.
    Server(String),
}

impl From<String> for Refusal {
    fn from(e: String) -> Self {
        Self::Server(e)
    }
}

/// What `catalog.json` says, checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Meta {
    pub title: String,
    pub summary: String,
    pub body: String,
    pub number: String,
    pub changes: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawMeta {
    title: String,
    #[serde(default)]
    summary: String,
    #[serde(default)]
    body: String,
    version: String,
    #[serde(default)]
    changes: String,
}

/// Markdown with no HTML in it: every `<` is written as text.
fn markdown(text: &str, max: usize) -> String {
    clean(text, max, true).replace('<', "&lt;")
}

fn one_line(text: &str, max: usize) -> String {
    clean(text, max, false).trim().to_owned()
}

/// Reads `catalog.json`.
pub fn meta(text: &[u8]) -> Result<Meta, &'static str> {
    let raw: RawMeta = serde_json::from_slice(text).map_err(|_| "bad_meta")?;
    let too_long = raw.title.chars().count() > MAX_TITLE
        || raw.summary.chars().count() > MAX_SUMMARY
        || raw.body.chars().count() > MAX_BODY
        || raw.changes.chars().count() > MAX_CHANGES;
    if too_long {
        return Err("too_long");
    }
    let title = one_line(&raw.title, MAX_TITLE);
    if title.chars().filter(|c| c.is_alphanumeric()).count() < 2 {
        return Err("bad_title");
    }
    Ok(Meta {
        title,
        summary: one_line(&raw.summary, MAX_SUMMARY),
        body: markdown(&raw.body, MAX_BODY).trim().to_owned(),
        number: word(raw.version.trim()).ok_or("bad_version")?,
        changes: markdown(&raw.changes, MAX_CHANGES).trim().to_owned(),
    })
}

/// A path an own pack's overrides may bring: one a pack may write, never in
/// `mods/` (mods come from the catalog), and no code or program under any of
/// its extensions (`x.jar.disabled` too).
pub fn overridable(path: &str) -> bool {
    if !pack::pack_path(path) || path.split('/').next() == Some("mods") {
        return false;
    }
    let name = path
        .rsplit('/')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    !name
        .split('.')
        .skip(1)
        .any(|e| CODE.contains(&e) || pack::PROGRAMS.contains(&e))
}

/// An icon: a PNG, JPEG or WebP picture that decodes, not too large.
pub fn icon(bytes: &[u8]) -> Result<&'static str, &'static str> {
    let media_type = crate::thumb::sniff(bytes).ok_or("bad_icon")?;
    if media_type == "image/gif" || bytes.len() as u64 > MAX_ICON {
        return Err("bad_icon");
    }
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|_| "bad_icon")?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_ICON_SIDE);
    limits.max_image_height = Some(MAX_ICON_SIDE);
    limits.max_alloc = Some(64 * 1024 * 1024);
    reader.limits(limits);
    reader.decode().map_err(|_| "bad_icon")?;
    Ok(media_type)
}

/// What an archive holds, checked.
#[derive(Debug)]
pub struct Archive {
    pub index: pack::Index,
    pub meta: Meta,
    pub icon: Option<(Vec<u8>, &'static str)>,
}

fn named(mut paths: Vec<String>) -> Vec<String> {
    paths.truncate(MAX_NAMED);
    paths.into_iter().map(|p| one_line(&p, 400)).collect()
}

fn entry_bytes<R: std::io::Read>(entry: R, max: u64) -> Result<Vec<u8>, Refusal> {
    let mut bytes = Vec::new();
    entry
        .take(max + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| Refusal::Bad("bad_pack"))?;
    if bytes.len() as u64 > max {
        return Err(Refusal::Bad("too_big"));
    }
    Ok(bytes)
}

/// Reads and checks an uploaded archive (blocking).
pub fn read(archive: &Path) -> Result<Archive, Refusal> {
    let file =
        std::fs::File::open(archive).map_err(|_| Refusal::Server("reading an upload".into()))?;
    let mut zip = zip::ZipArchive::new(file).map_err(|_| Refusal::Bad("bad_pack"))?;
    if zip.len() > MAX_ENTRIES {
        return Err(Refusal::Bad("too_big"));
    }
    let index = {
        let entry = zip
            .by_name(pack::INDEX)
            .map_err(|_| Refusal::Bad("bad_pack"))?;
        pack::own_index(&entry_bytes(entry, MAX_INDEX)?).map_err(Refusal::Bad)?
    };
    let meta = {
        let entry = zip.by_name(META).map_err(|_| Refusal::Bad("bad_meta"))?;
        meta(&entry_bytes(entry, MAX_META)?).map_err(Refusal::Bad)?
    };
    let mut icon_found = None;
    let mut refused = Vec::new();
    let mut total = 0u64;
    let mut seen = std::collections::HashSet::new();
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(|_| Refusal::Bad("bad_pack"))?;
        let name = entry.name().to_owned();
        if !seen.insert(name.to_ascii_lowercase()) {
            return Err(Refusal::Bad("bad_pack"));
        }
        if name == pack::INDEX || name == META {
            continue;
        }
        if ICONS.contains(&name.as_str()) && !entry.is_symlink() {
            if icon_found.is_some() {
                return Err(Refusal::Bad("bad_icon"));
            }
            let bytes = entry_bytes(&mut entry, MAX_ICON).map_err(|_| Refusal::Bad("bad_icon"))?;
            let media_type = icon(&bytes).map_err(Refusal::Bad)?;
            icon_found = Some((bytes, media_type));
            continue;
        }
        if entry.is_dir() && (name == "overrides/" || name.starts_with("overrides/")) {
            continue;
        }
        let Some(path) = name.strip_prefix("overrides/") else {
            refused.push(name);
            continue;
        };
        if entry.is_dir() || entry.is_symlink() || !overridable(path) {
            refused.push(path.to_owned());
            continue;
        }
        let read = std::io::copy(
            &mut (&mut entry).take(MAX_OVERRIDE + 1),
            &mut std::io::sink(),
        )
        .map_err(|_| Refusal::Bad("bad_pack"))?;
        if read > MAX_OVERRIDE {
            return Err(Refusal::Files("too_big", named(vec![path.to_owned()])));
        }
        total += read;
        if total > MAX_OVERRIDES {
            return Err(Refusal::Bad("too_big"));
        }
    }
    if !refused.is_empty() {
        return Err(Refusal::Files("not_allowed", named(refused)));
    }
    Ok(Archive {
        index,
        meta,
        icon: icon_found,
    })
}

/// A slug from a title: plain lowercase ASCII words joined by `-`.
pub fn slug(title: &str) -> String {
    let mut out = String::new();
    for c in title.chars().flat_map(char::to_lowercase) {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
        if out.len() >= 40 {
            break;
        }
    }
    let out = out.trim_matches('-').to_owned();
    if out.len() < 3 {
        "pack".to_owned()
    } else {
        out
    }
}

/// The pack's files as the catalog keeps them: each found by its hashes,
/// served (not held, not blocked) and of the size the index says.
fn files_here(app: &App, index: &mut pack::Index) -> Result<Vec<String>, Refusal> {
    let (mut missing, mut held) = (Vec::new(), Vec::new());
    let mut found = Vec::with_capacity(index.files.len());
    for file in &mut index.files {
        let info = match app
            .store
            .blob_by_hashes(Some(&file.sha1), Some(&file.sha512))?
        {
            Some(sha256) => app.store.blob_info(&sha256)?,
            None => None,
        };
        match info {
            Some(b) if b.sha1 != file.sha1 || u64::try_from(b.size).ok() != Some(file.size) => {
                missing.push(file.path.clone());
            }
            Some(b) if b.state != "stored" || app.store.is_blocked(&b.sha256, &b.sha1)? => {
                held.push(file.path.clone());
            }
            Some(b) if app.blobs.has(&b.sha256) => {
                let name = file.path.rsplit('/').next().unwrap_or("file").to_owned();
                file.url = app.urls.file(&b.sha256, &name);
                found.push(b.sha256);
            }
            _ => missing.push(file.path.clone()),
        }
    }
    if !held.is_empty() {
        return Err(Refusal::Files("held", named(held)));
    }
    if !missing.is_empty() {
        return Err(Refusal::Files("not_in_catalog", named(missing)));
    }
    Ok(found)
}

/// Who publishes: the account and its nick.
pub struct Author<'a> {
    pub account: &'a str,
    pub nick: &'a str,
}

/// Publishes an uploaded archive (`part`, removed in every case) as a new
/// pack, or as a new version of `project`. Returns the project's ID.
/// The pack's ID and its new version's.
pub async fn publish(
    app: &Arc<App>,
    author: &Author<'_>,
    project: Option<&str>,
    room: bool,
    part: PathBuf,
) -> Result<(String, String), Refusal> {
    let result = publish_part(app, author, project, room, &part).await;
    let _ = std::fs::remove_file(&part);
    result
}

/// Whose pack `project` is, and whether the account may publish an archive
/// of `size` bytes now. A room pack takes room versions only, and a pack of
/// the catalog none.
pub fn may_publish(
    app: &App,
    account: &str,
    project: Option<&str>,
    room: bool,
    size: u64,
) -> Result<(), Refusal> {
    if let Some(project) = project {
        if app.store.own_account(project)?.as_deref() != Some(account) {
            return Err(Refusal::NotFound);
        }
        let status = app.store.project_status(project)?;
        if status.as_deref() == Some("removed") {
            return Err(Refusal::Gone);
        }
        if (status.as_deref() == Some(ROOM)) != room {
            return Err(Refusal::NotFound);
        }
    }
    let (packs, versions, bytes) = app.store.own_usage(account, now() - 86_400)?;
    if project.is_none() && packs >= PACKS_PER_DAY {
        return Err(Refusal::Limit("too_many_packs"));
    }
    if versions >= VERSIONS_PER_DAY {
        return Err(Refusal::Limit("too_many_versions"));
    }
    if bytes.saturating_add(i64::try_from(size).unwrap_or(i64::MAX)) > BYTES_PER_ACCOUNT {
        return Err(Refusal::Limit("too_much"));
    }
    if !app.blobs.may_keep(size) {
        return Err(Refusal::Server("no space".into()));
    }
    Ok(())
}

async fn publish_part(
    app: &Arc<App>,
    author: &Author<'_>,
    project: Option<&str>,
    room: bool,
    part: &Path,
) -> Result<(String, String), Refusal> {
    let size = std::fs::metadata(part)
        .map_err(|_| Refusal::Server("an upload".into()))?
        .len();
    if size > MAX_ARCHIVE {
        return Err(Refusal::Bad("too_big"));
    }
    may_publish(app, author.account, project, room, size)?;
    // The archive, then its files, then the scanner over all of it.
    let path = part.to_path_buf();
    let (archive, hashed, verdict) = tokio::task::spawn_blocking(move || {
        let archive = read(&path)?;
        let hashed =
            crate::blobs::hash_file(&path).map_err(|_| Refusal::Server("hashing".into()))?;
        let verdict =
            crate::scan::scan_file(&path).map_err(|_| Refusal::Server("scanning".into()))?;
        Ok::<_, Refusal>((archive, hashed, verdict))
    })
    .await
    .map_err(|_| Refusal::Server("checking stopped".into()))??;
    let Archive {
        mut index,
        meta,
        icon,
    } = archive;
    if let Some(project) = project
        && app.store.has_number(project, &meta.number)?
    {
        return Err(Refusal::Conflict("version_exists"));
    }
    let found = files_here(app, &mut index)?;
    if verdict.level.held() || app.store.is_blocked(&hashed.sha256, &hashed.sha1)? {
        eprintln!(
            "lapi-mods: refused an own pack ({})",
            verdict.level.as_str()
        );
        return Err(Refusal::Bad("refused_by_scanner"));
    }
    keep_archive(app, part, &hashed, &verdict)?;
    let draft = Draft {
        meta: &meta,
        index: &index,
        hashed: &hashed,
        found: &found,
    };
    let (id, version) = record(app, author, project, room, &draft).await?;
    if let Some((bytes, media_type)) = icon {
        keep_icon(app, &id, &bytes, media_type).await;
    }
    Ok((id, version))
}

/// A room pack's status: kept and installed like a shown pack, never shown.
pub const ROOM: &str = "room";

/// A checked archive to record.
struct Draft<'a> {
    meta: &'a Meta,
    index: &'a pack::Index,
    hashed: &'a Hashed,
    found: &'a [String],
}

fn keep_archive(
    app: &App,
    part: &Path,
    hashed: &Hashed,
    verdict: &crate::scan::Verdict,
) -> Result<(), Refusal> {
    if !app.blobs.has(&hashed.sha256) {
        app.blobs
            .keep(part, hashed)
            .map_err(|_| Refusal::Server("keeping an upload".into()))?;
    }
    app.store
        .add_blob(hashed, "application/zip", Some(verdict), now())?;
    Ok(())
}

async fn record(
    app: &App,
    author: &Author<'_>,
    project: Option<&str>,
    room: bool,
    draft: &Draft<'_>,
) -> Result<(String, String), Refusal> {
    let Draft {
        meta,
        index,
        hashed,
        found,
    } = *draft;
    let at = now();
    let source_id = match project {
        Some(project) => app
            .store
            .source_id(project, "own")?
            .ok_or(Refusal::NotFound)?,
        None => uuid::Uuid::new_v4().to_string(),
    };
    let slug = slug(&meta.title);
    let (id, _) = app.store.upsert_project(
        &SourceProject {
            source: "own",
            source_id,
            slug: slug.clone(),
            kind: "modpack",
            title: meta.title.clone(),
            summary: meta.summary.clone(),
            body: meta.body.clone(),
            license: String::new(),
            icon_url: None,
            client_side: "required".to_owned(),
            server_side: "unknown".to_owned(),
            downloads: 0,
            followers: 0,
            categories: Vec::new(),
            authors: vec![(one_line(author.nick, 32), "owner".to_owned())],
            gallery: Vec::new(),
            url: String::new(),
            published: at,
            updated: at,
        },
        at,
    )?;
    let name = format!("{slug}-{}.mrpack", meta.number);
    let version_source = uuid::Uuid::new_v4().to_string();
    let ids = app.store.upsert_versions(
        &id,
        "own",
        &[SourceVersion {
            source_id: version_source,
            number: meta.number.clone(),
            name: meta.number.clone(),
            changelog: meta.changes.clone(),
            channel: "release".to_owned(),
            published: at,
            downloads: 0,
            games: vec![index.game.clone()],
            loaders: index.loader.iter().map(|l| l.0.clone()).collect(),
            files: vec![SourceFile {
                name: name.clone(),
                size: i64::try_from(hashed.size).unwrap_or(i64::MAX),
                sha1: Some(hashed.sha1.clone()),
                sha512: Some(hashed.sha512.clone()),
                url: app.urls.file(&hashed.sha256, &name),
                primary: true,
                role: "file".to_owned(),
            }],
            dependencies: Vec::new(),
            details: true,
        }],
    )?;
    let version = ids.first().ok_or(Refusal::Server("no version".into()))?;
    app.store.file_kept(version, 0, &hashed.sha256)?;
    app.store.set_pack(version, &hashed.sha256, index, at)?;
    for (ord, sha256) in found.iter().enumerate() {
        app.store
            .pack_file_kept(version, i64::try_from(ord).unwrap_or(i64::MAX), sha256)?;
    }
    app.store
        .add_own(&id, version, author.account, hashed.size, at)?;
    // Shown at once; a pack its author hid stays hidden, and a room pack
    // is never shown.
    if app.store.project_status(&id)?.as_deref() == Some("importing") {
        app.store
            .set_project_status(&id, if room { ROOM } else { "ready" })?;
    }
    Ok((id, version.clone()))
}

async fn keep_icon(app: &App, project: &str, bytes: &[u8], media_type: &'static str) {
    let part = app.blobs.part();
    if std::fs::write(&part, bytes).is_err() {
        let _ = std::fs::remove_file(&part);
        return;
    }
    let Ok(hashed) = crate::blobs::hash_file(&part) else {
        let _ = std::fs::remove_file(&part);
        return;
    };
    if crate::import::keep(app, &part, &hashed, media_type)
        .await
        .is_err()
    {
        return;
    }
    let thumb = crate::import::small_copy(app, &hashed.sha256, 256, "webp").await;
    if let Err(e) = app
        .store
        .set_icon(project, &hashed.sha256, media_type, thumb.as_deref())
    {
        eprintln!("lapi-mods: an icon: {e}");
    }
}

/// The JSON a refusal answers with.
/// An archive being sent in parts.
struct Upload {
    account: String,
    project: Option<String>,
    room: bool,
    size: u64,
    sha256: String,
    /// Each part's file in `tmp/` once it came.
    parts: Vec<Option<PathBuf>>,
    touched: std::time::Instant,
}

impl Upload {
    /// How long part `n` is.
    fn part_len(&self, n: usize) -> u64 {
        let start = CHUNK.saturating_mul(n as u64);
        self.size.saturating_sub(start).min(CHUNK)
    }

    fn drop_files(&self) {
        for path in self.parts.iter().flatten() {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// Archives larger than one request takes, sent in parts of [`CHUNK`] and
/// put together here. Kept in memory: a restart clears `tmp/` and the
/// launcher starts over.
#[derive(Default)]
pub struct Uploads(std::sync::Mutex<std::collections::HashMap<String, Upload>>);

/// An upload put together, to be published.
pub struct Assembled {
    pub project: Option<String>,
    pub room: bool,
    pub part: PathBuf,
}

impl Uploads {
    fn map(&self) -> std::sync::MutexGuard<'_, std::collections::HashMap<String, Upload>> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn sweep(map: &mut std::collections::HashMap<String, Upload>) {
        map.retain(|_, u| {
            let live = u.touched.elapsed() < UPLOAD_IDLE;
            if !live {
                u.drop_files();
            }
            live
        });
    }

    /// Starts an upload of `size` bytes whose SHA-256 is `sha256`; an
    /// account has one at a time. Returns its ID and the number of parts.
    pub fn start(
        &self,
        app: &App,
        account: &str,
        project: Option<&str>,
        room: bool,
        size: u64,
        sha256: &str,
    ) -> Result<(String, usize), Refusal> {
        if size == 0 || size > MAX_ARCHIVE {
            return Err(Refusal::Bad("too_big"));
        }
        if !crate::blobs::is_sha256(sha256) {
            return Err(Refusal::Bad("bad_upload"));
        }
        may_publish(app, account, project, room, size)?;
        let mut map = self.map();
        Self::sweep(&mut map);
        map.retain(|_, u| {
            let other = u.account != account;
            if !other {
                u.drop_files();
            }
            other
        });
        if map.len() >= MAX_UPLOADS {
            return Err(Refusal::Limit("busy"));
        }
        let waiting: u64 = map.values().map(|u| u.size).sum();
        if !app
            .blobs
            .may_keep(waiting.saturating_add(size).saturating_mul(2))
        {
            return Err(Refusal::Server("no space for uploads".into()));
        }
        let count = usize::try_from(size.div_ceil(CHUNK)).unwrap_or(usize::MAX);
        let id = uuid::Uuid::new_v4().to_string();
        map.insert(
            id.clone(),
            Upload {
                account: account.to_owned(),
                project: project.map(str::to_owned),
                room,
                size,
                sha256: sha256.to_owned(),
                parts: vec![None; count],
                touched: std::time::Instant::now(),
            },
        );
        Ok((id, count))
    }

    /// How long part `n` of the account's upload `id` must be.
    pub fn expected(&self, account: &str, id: &str, n: usize) -> Result<u64, Refusal> {
        let map = self.map();
        let upload = map
            .get(id)
            .filter(|u| u.account == account)
            .ok_or(Refusal::NotFound)?;
        if n >= upload.parts.len() {
            return Err(Refusal::Bad("bad_part"));
        }
        Ok(upload.part_len(n))
    }

    /// Takes part `n` (a file in `tmp/`, removed when it does not fit); a
    /// part sent again replaces the earlier one.
    pub fn put(&self, account: &str, id: &str, n: usize, file: PathBuf) -> Result<(), Refusal> {
        let mut map = self.map();
        let fits = map
            .get(id)
            .filter(|u| u.account == account && n < u.parts.len())
            .is_some_and(|u| std::fs::metadata(&file).is_ok_and(|m| m.len() == u.part_len(n)));
        let Some(upload) = map.get_mut(id).filter(|_| fits) else {
            let _ = std::fs::remove_file(&file);
            return Err(if map.get(id).is_some_and(|u| u.account == account) {
                Refusal::Bad("bad_part")
            } else {
                Refusal::NotFound
            });
        };
        if let Some(old) = upload.parts[n].replace(file) {
            let _ = std::fs::remove_file(old);
        }
        upload.touched = std::time::Instant::now();
        Ok(())
    }

    /// Puts the account's upload `id` together in a new part file once all
    /// its parts came and their SHA-256 is the one announced (blocking).
    pub fn assemble(&self, app: &App, account: &str, id: &str) -> Result<Assembled, Refusal> {
        let upload = {
            let mut map = self.map();
            match map.get(id) {
                Some(u) if u.account == account => {}
                _ => return Err(Refusal::NotFound),
            }
            if map[id].parts.iter().any(Option::is_none) {
                return Err(Refusal::Bad("not_complete"));
            }
            map.remove(id).ok_or(Refusal::NotFound)?
        };
        let part = app.blobs.part();
        let joined = (|| {
            let mut out = std::fs::File::create(&part)?;
            for path in upload.parts.iter().flatten() {
                std::io::copy(&mut std::fs::File::open(path)?, &mut out)?;
            }
            out.sync_all()
        })();
        upload.drop_files();
        let hashed = joined.and_then(|()| crate::blobs::hash_file(&part));
        match hashed {
            Ok(h) if h.size == upload.size && h.sha256 == upload.sha256 => Ok(Assembled {
                project: upload.project,
                room: upload.room,
                part,
            }),
            Ok(_) => {
                let _ = std::fs::remove_file(&part);
                Err(Refusal::Bad("bad_upload"))
            }
            Err(_) => {
                let _ = std::fs::remove_file(&part);
                Err(Refusal::Server("putting an upload together".into()))
            }
        }
    }
}

pub fn refusal_json(refusal: &Refusal) -> (u16, Value) {
    match refusal {
        Refusal::Bad(code) => (400, json!({ "error": code })),
        Refusal::Files(code, paths) => (422, json!({ "error": code, "paths": paths })),
        Refusal::NotFound => (404, json!({ "error": "not_found" })),
        Refusal::Conflict(code) => (409, json!({ "error": code })),
        Refusal::Gone => (410, json!({ "error": "removed" })),
        Refusal::Limit(code) => (429, json!({ "error": code })),
        Refusal::Server(_) => (500, json!({ "error": "server_error" })),
    }
}
