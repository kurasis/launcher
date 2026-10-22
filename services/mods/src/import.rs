//! The work behind "Add to the catalog", done by a few workers from the queue
//! in the database, so it survives a restart:
//!
//! - `resolve` reads what the player typed: an address queues the project
//!   (and the version it names); a name is searched for, and the player picks;
//! - `project` fetches a project's page, members and all its versions'
//!   metadata, then queues the files of the newest version for each pair of
//!   game version and loader, its pictures, and its required dependencies;
//! - `version` fetches one version (a dependency, or the one an address
//!   names) and queues its files;
//! - `files` downloads a version's files, checks them against the source's
//!   hashes and keeps them;
//! - `image` keeps an icon or a gallery picture with a small copy, `body`
//!   the pictures in a project's description, pointing it at our copies;
//! - `pack` reads a modpack version's index out of its kept `.mrpack` and
//!   keeps every file it lists (asked for by the launcher before it installs
//!   the pack, never at once for every version).
//!
//! Older versions keep their metadata only, until someone asks for their
//! files (`POST /v1/versions/{id}/fetch`); then they are kept for good too.
//!
//! CurseForge (stage 4) has no key here: a project's card comes from its
//! page, its files (CurseForge's versions) a page at a time from the site's
//! API, newest first, and the dependencies of the files fetched at once one
//! by one. The site gives no hashes, so a file must have the size the site
//! names; it comes over TLS from forgecdn, and our hashes are made when it is
//! kept.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};

use crate::App;
use crate::blobs::{Hashed, hash_file};
use crate::fetch::FetchError;
use crate::input::{self, Input};
use crate::model::{SourceProject, SourceVersion, newest_per_pair};
use crate::{curseforge, external, modrinth, pack, packwiz};

pub fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

/// Required dependencies are followed to the end of the chain (each project
/// once a day, so a loop ends), at most this many from one project.
const MAX_DEPENDENCIES: usize = 200;
/// Pages of a CurseForge project's files read at once (newest first, 50 a
/// page); older files are not listed until the project has fewer.
const MAX_CURSEFORGE_PAGES: usize = 20;
/// The largest file kept (a modpack's server files can be big).
pub const MAX_FILE: u64 = 2 * 1024 * 1024 * 1024;
const MAX_IMAGE: u64 = 20 * 1024 * 1024;
const MAX_BODY_IMAGES: usize = 30;
const MAX_ATTEMPTS: i64 = 12;
/// A project asked for again is fetched anew after this long.
const PROJECT_FRESH: i64 = 6 * 3600;
/// A project players wanted lately is checked for new versions once a day,
/// any other project in the catalog once a week (Ai, 2026-10-20).
const WANTED_FRESH: i64 = 24 * 3600;
const IDLE_FRESH: i64 = 7 * 24 * 3600;
/// A project checked every few days, as the owner set: not queued again
/// sooner than this.
const OWNER_FRESH: i64 = 3600;
/// Looked at or installed this recently: wanted.
const WANTED_FOR: i64 = 30 * 24 * 3600;
/// Projects queued for a check at a time.
const REFRESH_BATCH: usize = 100;
const DEPENDENCY_FRESH: i64 = 24 * 3600;
/// A request that has not finished in a day has failed.
const REQUEST_DAY: i64 = 86_400;

/// Why a job failed: a short code (for a public log and the player) and
/// whether to try again later.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobError {
    pub code: String,
    pub retry: bool,
}

impl JobError {
    fn new(code: &str, retry: bool) -> Self {
        Self {
            code: code.to_owned(),
            retry,
        }
    }
}

impl From<FetchError> for JobError {
    fn from(e: FetchError) -> Self {
        Self {
            code: e.code(),
            retry: e.retryable(),
        }
    }
}

impl From<String> for JobError {
    fn from(e: String) -> Self {
        eprintln!("lapi-mods: {e}");
        Self::new("server_error", true)
    }
}

/// How small copies of pictures are made: by a process of our own on the
/// server, in this process in tests.
pub enum Thumbs {
    Process(PathBuf),
    Inline,
}

/// Seconds to wait before the next attempt.
pub fn backoff(attempts: i64) -> i64 {
    (30i64 << attempts.clamp(0, 10)).min(6 * 3600)
}

pub async fn work(app: Arc<App>, mut stop: tokio::sync::watch::Receiver<bool>) {
    loop {
        if *stop.borrow() {
            return;
        }
        let job = match app.store.claim(now()) {
            Ok(Some(job)) => job,
            Ok(None) => {
                tokio::select! {
                    _ = tokio::time::sleep(Duration::from_secs(2)) => {}
                    _ = stop.changed() => return,
                }
                continue;
            }
            Err(e) => {
                eprintln!("lapi-mods: {e}");
                tokio::time::sleep(Duration::from_secs(10)).await;
                continue;
            }
        };
        let result = tokio::select! {
            result = run(&app, &job) => result,
            // The job stays running; the next start queues it again.
            _ = stop.changed() => return,
        };
        let done = match result {
            Ok(()) => app.store.job_done(job.id, now()),
            Err(e) => {
                let again =
                    (e.retry && job.attempts < MAX_ATTEMPTS).then(|| now() + backoff(job.attempts));
                app.store.job_failed(job.id, &e.code, again, now())
            }
        };
        if let Err(e) = done {
            eprintln!("lapi-mods: {e}");
        }
    }
}

/// Runs one job.
pub async fn run(app: &App, job: &crate::store::Job) -> Result<(), JobError> {
    let p = &job.payload;
    let text = |key: &str| {
        p.get(key)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    let depth = p.get("depth").and_then(Value::as_i64).unwrap_or(0);
    let request = job.request_id.as_deref();
    match job.kind.as_str() {
        "resolve" => resolve(app, &text("request")).await,
        "project" => project(app, &text("source"), &text("ref"), depth, request).await,
        "version" => version(app, &text("source"), &text("id"), depth, request).await,
        "files" => files(app, &text("version"), request).await,
        "image" => image(app, &text("project"), &text("kind"), &text("url")).await,
        "body" => body(app, &text("project")).await,
        "pack" => pack_files(app, &text("version")).await,
        _ => Err(JobError::new("unknown_job", false)),
    }
}

// ----- queueing -----

fn project_key(source: &str, reference: &str) -> String {
    format!("project:{source}:{}", reference.to_ascii_lowercase())
}

/// How a project due for a check is named to its source.
fn due_reference(due: &crate::store::Due) -> Option<String> {
    match due.source.as_str() {
        s if s == modrinth::SOURCE => {
            modrinth::is_ref(&due.source_id).then(|| due.source_id.clone())
        }
        s if s == packwiz::SOURCE => {
            let url = reqwest::Url::parse(&due.source_id).ok()?;
            packwiz::pack_url(&url)
        }
        s if s == external::SOURCE => {
            external::is_ref(&due.source_id).then(|| due.source_id.clone())
        }
        s if s == curseforge::SOURCE => {
            let class = curseforge::CLASSES
                .iter()
                .find(|(_, kind)| *kind == due.kind)?
                .0;
            Some(
                curseforge::Ref {
                    id: Some(curseforge::id(&due.source_id)?),
                    class: class.to_owned(),
                    slug: due.source_slug.clone(),
                }
                .text(),
            )
        }
        _ => None,
    }
}

/// Queues the checks of projects due for one (or of `project` alone, when it
/// is due); returns how many were queued.
pub fn refresh(app: &App, project: Option<&str>) -> Result<usize, String> {
    let now = now();
    let due = app.store.due_projects(
        now,
        now - WANTED_FOR,
        WANTED_FRESH,
        IDLE_FRESH,
        project,
        REFRESH_BATCH,
    )?;
    let mut queued = 0;
    for due in &due {
        let Some(reference) = due_reference(due) else {
            continue;
        };
        // Days the owner set are counted by the check above alone.
        let fresh = if due.days.is_some_and(|d| d > 0) {
            OWNER_FRESH
        } else if due.wanted {
            PROJECT_FRESH
        } else {
            IDLE_FRESH
        };
        if queue_project(app, &due.source, &reference, 0, None, fresh)? {
            queued += 1;
        }
    }
    Ok(queued)
}

/// Players want a project: noted, and checked for new versions now if its
/// last check is older than [`WANTED_FRESH`].
pub fn wanted(app: &App, project: &str) {
    let noted = app.store.note_wanted(project, now());
    if noted.is_ok() {
        let _ = refresh(app, Some(project));
    }
}

/// Queues a project of a source.
pub fn queue_project(
    app: &App,
    source: &str,
    reference: &str,
    depth: i64,
    request: Option<&str>,
    fresh: i64,
) -> Result<bool, String> {
    app.store.enqueue(
        &project_key(source, reference),
        "project",
        &json!({ "source": source, "ref": reference, "depth": depth }),
        request,
        None,
        now(),
        Some(fresh),
    )
}

// ----- the most downloaded -----

/// Seconds between looks at Modrinth's most downloaded projects.
pub const POPULAR_EVERY: i64 = 24 * 3600;
/// Most downloaded projects looked for in each kind, unless
/// `LAPI_MODS_POPULAR` says otherwise (0 turns it off).
pub const POPULAR: usize = 100;

/// Projects queued at a time from the most downloaded, and only while fewer
/// jobs than `SEED_WHEN_BELOW` are due: what players ask for never waits
/// behind them for long.
pub const SEED_AT_ONCE: usize = 2;
pub const SEED_WHEN_BELOW: i64 = 20;

/// Each kind's `limit` most downloaded projects on Modrinth that the catalog
/// does not have yet (D-066), so the popular ones are there before anyone
/// asks; those it has are checked as any other. CurseForge's search answers
/// the server with 403, and Modrinth has no worlds. `seed` queues them.
pub async fn popular(app: &App, limit: usize) -> Result<Vec<String>, String> {
    let mut missing = Vec::new();
    for project_type in modrinth::POPULAR_TYPES {
        let ids = app
            .client
            .json(&modrinth::popular_url(project_type, limit))
            .await
            .map_err(|e| format!("asking Modrinth for the most downloaded: {}", e.code()))?
            .map(|v| modrinth::project_ids(&v))
            .unwrap_or_default();
        for id in ids.into_iter().take(limit) {
            // One the owner removed stays removed: it is known too.
            if app
                .store
                .project_by_source(modrinth::SOURCE, &id)?
                .is_none()
            {
                missing.push(id);
            }
        }
    }
    app.store.set_upkeep("popular_seeded", now())?;
    Ok(missing)
}

/// Queues up to `SEED_AT_ONCE` of the popular projects still missing, when
/// the queue is nearly empty. Returns how many were queued.
pub fn seed(app: &App, missing: &mut Vec<String>) -> Result<usize, String> {
    if app.store.jobs_due(now())? >= SEED_WHEN_BELOW {
        return Ok(0);
    }
    let mut queued = 0;
    while queued < SEED_AT_ONCE {
        let Some(id) = missing.pop() else { break };
        if app
            .store
            .project_by_source(modrinth::SOURCE, &id)?
            .is_some()
        {
            continue;
        }
        if queue_project(app, modrinth::SOURCE, &id, 0, None, IDLE_FRESH)? {
            queued += 1;
        }
    }
    Ok(queued)
}

/// Queues a check of a project the owner added (external.rs) at once; run
/// by the owner's commands, which have the store alone.
pub fn queue_external(store: &crate::store::Store, source_id: &str) -> Result<bool, String> {
    store.enqueue(
        &project_key(external::SOURCE, source_id),
        "project",
        &json!({ "source": external::SOURCE, "ref": source_id, "depth": 0 }),
        None,
        None,
        now(),
        Some(0),
    )
}

pub fn queue_version(
    app: &App,
    source: &str,
    id: &str,
    depth: i64,
    request: Option<&str>,
) -> Result<bool, String> {
    app.store.enqueue(
        &format!("version:{source}:{id}"),
        "version",
        &json!({ "source": source, "id": id, "depth": depth }),
        request,
        None,
        now(),
        Some(DEPENDENCY_FRESH),
    )
}

/// Queues the download of a version's files, unless all are kept. A failed
/// download is tried again when asked an hour later.
pub fn queue_files(app: &App, version: &str, request: Option<&str>) -> Result<bool, String> {
    match app.store.version_state(version)? {
        Some((_, state)) if state == "all" => Ok(false),
        Some((project, _)) => app.store.enqueue(
            &format!("files:{version}"),
            "files",
            &json!({ "version": version }),
            request,
            Some(&project),
            now(),
            Some(3600),
        ),
        None => Ok(false),
    }
}

/// The archive of a modpack version, the main one first: its `.mrpack`, or
/// the `.zip` of a modpack (CurseForge's).
pub fn pack_archive(app: &App, version: &str) -> Result<Option<crate::store::FileRow>, String> {
    let modpack = app.store.version_kind(version)?.as_deref() == Some("modpack");
    Ok(app
        .store
        .files(version)?
        .into_iter()
        .filter(|f| {
            let name = f.name.to_ascii_lowercase();
            name.ends_with(".mrpack") || (modpack && name.ends_with(".zip"))
        })
        .min_by_key(|f| (!f.primary, f.ord)))
}

/// Queues reading a modpack version and keeping its files. A failed one is
/// tried again when asked an hour later.
pub fn queue_pack(app: &App, version: &str) -> Result<bool, String> {
    match app.store.version_state(version)? {
        Some((project, _)) => app.store.enqueue(
            &pack_key(version),
            "pack",
            &json!({ "version": version }),
            None,
            Some(&project),
            now(),
            Some(3600),
        ),
        None => Ok(false),
    }
}

pub fn pack_key(version: &str) -> String {
    format!("pack:{version}")
}

/// A request is ready once its project is in and nothing of it waits;
/// failed when its project could not be had, or after a day.
pub fn check_requests(app: &App) {
    let result = (|| -> Result<(), String> {
        for id in app.store.open_requests()? {
            let Some((_, request)) = app.store.request(&id)? else {
                continue;
            };
            let (pending, failed) = app.store.request_jobs(&id)?;
            if pending > 0 {
                continue;
            }
            if let Some(project) = &request.project_id {
                // Its project came through (some files or dependencies may
                // not have; the catalog shows which).
                match app.store.project_status(project)?.as_deref() {
                    Some("ready") => {
                        app.store
                            .set_request(&id, "ready", None, None, None, now())?;
                        continue;
                    }
                    // Taken out at its author's request (lapi-mods remove).
                    Some("removed") => {
                        app.store
                            .set_request(&id, "failed", Some("removed"), None, None, now())?;
                        continue;
                    }
                    _ => {}
                }
            }
            if let Some(error) = failed {
                app.store
                    .set_request(&id, "failed", Some(&error), None, None, now())?;
            } else if now() - request.created > REQUEST_DAY {
                app.store
                    .set_request(&id, "failed", Some("timeout"), None, None, now())?;
            } else {
                // Its project is being fetched for someone else: look again.
                app.store.enqueue(
                    &format!("resolve:{id}"),
                    "resolve",
                    &json!({ "request": id }),
                    Some(&id),
                    None,
                    now() + 5,
                    Some(0),
                )?;
            }
        }
        Ok(())
    })();
    if let Err(e) = result {
        eprintln!("lapi-mods: {e}");
    }
}

// ----- jobs -----

async fn resolve(app: &App, id: &str) -> Result<(), JobError> {
    let Some((_, request)) = app.store.request(id)? else {
        return Ok(());
    };
    if !matches!(request.state.as_str(), "queued" | "importing") {
        return Ok(());
    }
    let fail = |code: &str| -> Result<(), JobError> {
        app.store
            .set_request(id, "failed", Some(code), None, None, now())?;
        Ok(())
    };
    match input::parse(&request.input) {
        None => fail("bad_input"),
        Some(Input::CurseForgeFile) => fail("curseforge_file_link"),
        Some(Input::CurseForge { class, slug, file }) => {
            let reference = curseforge::Ref {
                id: None,
                class,
                slug,
            }
            .text();
            let version = file.map(|f| format!("{f}@{reference}"));
            start(app, id, curseforge::SOURCE, &reference, version.as_deref())
        }
        Some(Input::Name(words)) => {
            // Only Modrinth: CurseForge's search answers the server with 403
            // whatever is asked, and its search page carries no results, so
            // CurseForge projects come by their page address (D-066).
            let mut found = app
                .client
                .json(&modrinth::search_url(&words))
                .await?
                .map(|v| modrinth::candidates(&v))
                .unwrap_or_default();
            if found.is_empty() {
                return fail("nothing_found");
            }
            // Their pictures, kept here; one that cannot be had is left out.
            for candidate in &mut found {
                candidate.icon =
                    candidate_picture(app, candidate.icon_url.as_deref(), 256, "webp").await;
                candidate.banner =
                    candidate_picture(app, candidate.banner_url.as_deref(), 512, "jpeg").await;
            }
            let mut list = serde_json::to_value(&found).unwrap_or_default();
            // Those in the catalog already say so.
            if let Some(items) = list.as_array_mut() {
                for item in items {
                    let source = item["source"].as_str().unwrap_or_default().to_owned();
                    let source_id = item["id"].as_str().unwrap_or_default().to_owned();
                    if let Some(project) = app.store.project_by_source(&source, &source_id)? {
                        item["projectId"] = project.into();
                    }
                }
            }
            app.store
                .set_request(id, "choosing", None, None, Some(&list), now())?;
            Ok(())
        }
        Some(Input::Modrinth { project, version }) => {
            start(app, id, modrinth::SOURCE, &project, version.as_deref())
        }
        Some(Input::ModrinthFile { project, version }) => {
            start(app, id, modrinth::SOURCE, &project, Some(&version))
        }
        Some(Input::Packwiz(url)) => start(app, id, packwiz::SOURCE, &url, None),
    }
}

/// A picture of a search hit at our address: its small copy, or itself when
/// none can be made.
async fn candidate_picture(
    app: &App,
    url: Option<&str>,
    size: u32,
    format: &str,
) -> Option<String> {
    let (sha256, media_type) = keep_image(app, url?).await.ok()?;
    Some(match small_copy(app, &sha256, size, format).await {
        Some(small) if format == "webp" => app.urls.image(&small, "image/webp"),
        Some(small) => app.urls.image(&small, "image/jpeg"),
        None => app.urls.image(&sha256, &media_type),
    })
}

/// Queues a request's project (and the version it names).
pub fn start(
    app: &App,
    request: &str,
    source: &str,
    project: &str,
    version: Option<&str>,
) -> Result<(), JobError> {
    // CurseForge's projects are known here by their slug.
    let lookup = if source == curseforge::SOURCE {
        curseforge::Ref::parse(project).map_or_else(|| project.to_owned(), |r| r.slug)
    } else {
        project.to_owned()
    };
    let known = app.store.project_by_source_ref(source, &lookup)?;
    app.store
        .set_request(request, "importing", None, known.as_deref(), None, now())?;
    if !queue_project(app, source, project, 0, Some(request), PROJECT_FRESH)?
        && known.is_none()
        && let Some((state, error)) = app.store.job_state(&project_key(source, project))?
        && state == "failed"
    {
        // Tried lately for someone else, and it could not be had.
        app.store.set_request(
            request,
            "failed",
            Some(error.as_deref().unwrap_or("not_found")),
            None,
            None,
            now(),
        )?;
        return Ok(());
    }
    if let Some(version) = version {
        // A CurseForge file is queued as `<file>@<project>`.
        let theirs = version.split_once('@').map_or(version, |(v, _)| v);
        match app.store.version_by_source(source, theirs)? {
            Some(ours) => {
                queue_files(app, &ours, Some(request))?;
            }
            None => {
                queue_version(app, source, version, 0, Some(request))?;
            }
        }
    }
    Ok(())
}

async fn project(
    app: &App,
    source: &str,
    reference: &str,
    depth: i64,
    request: Option<&str>,
) -> Result<(), JobError> {
    if source == curseforge::SOURCE {
        return project_curseforge(app, reference, depth, request).await;
    }
    if source == packwiz::SOURCE {
        return project_packwiz(app, reference, request).await;
    }
    if source == external::SOURCE {
        return project_external(app, reference, request).await;
    }
    if source != modrinth::SOURCE || !modrinth::is_ref(reference) {
        return Err(JobError::new("unsupported_source", false));
    }
    let Some(page) = app.client.json(&modrinth::project_url(reference)).await? else {
        return Err(JobError::new("not_found", false));
    };
    let source_id = page
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    if !modrinth::is_ref(&source_id) {
        return Err(JobError::new("bad_answer", true));
    }
    // Members are good to have, not needed.
    let members = app
        .client
        .json(&modrinth::members_url(&source_id))
        .await
        .ok()
        .flatten();
    let Some(found) = modrinth::project(&page, members.as_ref()) else {
        return Err(JobError::new("unsupported_kind", false));
    };
    let (id, card) = app.store.upsert_project(&found, now())?;
    not_removed(app, &id)?;
    if let Some(request) = request
        && depth == 0
    {
        app.store
            .set_request(request, "importing", None, Some(&id), None, now())?;
    }
    let list = app
        .client
        .json(&modrinth::versions_url(&source_id))
        .await?
        .ok_or_else(|| JobError::new("not_found", false))?;
    let versions: Vec<_> = list
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(modrinth::version)
        .take(5000)
        .collect();
    let ids = app.store.upsert_versions(&id, source, &versions)?;
    // Versions the source no longer lists stay here, marked.
    for (ours, theirs) in app.store.version_sources(&id, source)? {
        if !versions.iter().any(|v| v.source_id == theirs) {
            app.store.mark_version_gone(&ours, now())?;
        }
    }
    let chosen = newest_per_pair(&versions);
    for &i in &chosen {
        queue_files(app, &ids[i], request)?;
    }
    let mut queued = 0;
    for &i in &chosen {
        for dependency in &versions[i].dependencies {
            if dependency.kind != "required" || queued >= MAX_DEPENDENCIES {
                continue;
            }
            let added = match (&dependency.version, &dependency.project) {
                (Some(v), _) => queue_version(app, source, v, depth + 1, request)?,
                (None, Some(p)) => {
                    queue_project(app, source, p, depth + 1, request, DEPENDENCY_FRESH)?
                }
                (None, None) => false,
            };
            if added {
                queued += 1;
            }
        }
    }
    if card {
        pictures(app, &id, &found)?;
    }
    app.store.set_project_status(&id, "ready")?;
    Ok(())
}

/// A file of a pack fetched whole into memory (a toml); `None` when the
/// address has none.
async fn fetch_small(app: &App, url: &str, max: u64) -> Result<Option<Vec<u8>>, JobError> {
    let part = app.blobs.part();
    let result = app.client.get(url, &part, max, false).await;
    let bytes = std::fs::read(&part);
    let _ = std::fs::remove_file(&part);
    match result {
        Ok(_) => {
            let bytes = bytes.map_err(|_| JobError::new("disk_error", true))?;
            if bytes.len() as u64 > max {
                return Err(JobError::new("too_big", false));
            }
            Ok(Some(bytes))
        }
        Err(FetchError::NotFound) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// A packwiz pack (stage 5): its `pack.toml`, index and every file, kept
/// here and written out as a `.mrpack` version of a modpack project. A pack
/// read again with another index is a new version; the old ones stay.
async fn project_packwiz(app: &App, url: &str, request: Option<&str>) -> Result<(), JobError> {
    let source_id = reqwest::Url::parse(url)
        .ok()
        .and_then(|u| packwiz::pack_url(&u))
        .filter(|u| u == url)
        .ok_or_else(|| JobError::new("bad_input", false))?;
    let text = fetch_small(app, url, packwiz::MAX_TOML)
        .await?
        .ok_or_else(|| JobError::new("not_found", false))?;
    let found =
        packwiz::pack(&String::from_utf8_lossy(&text)).map_err(|c| JobError::new(c, false))?;
    let index_url =
        packwiz::relative(url, &found.index).ok_or_else(|| JobError::new("bad_pack", false))?;
    let index_text = fetch_small(app, &index_url, packwiz::MAX_TOML)
        .await?
        .ok_or_else(|| JobError::new("not_found", false))?;
    if found.index_hash.matches_bytes(&index_text) != Some(true) {
        return Err(JobError::new("hash_mismatch", true));
    }
    let (listed, _skipped) = packwiz::index(&String::from_utf8_lossy(&index_text))
        .map_err(|c| JobError::new(c, false))?;

    // The mods kept here, and the other files fetched for the archive.
    let mut mods: Vec<(String, Hashed, String, bool)> = Vec::new();
    let mut overrides: Vec<(String, PathBuf)> = Vec::new();
    let mut override_bytes = 0u64;
    let mut paths = std::collections::HashSet::new();
    let result: Result<(), JobError> = async {
        for file in &listed {
            let at = packwiz::relative(&index_url, &file.file)
                .ok_or_else(|| JobError::new("bad_pack", false))?;
            if !file.metafile {
                if !paths.insert(file.path.to_ascii_lowercase()) {
                    continue;
                }
                let (part, hashed) = download(app, &at, packwiz::MAX_OVERRIDE, false).await?;
                overrides.push((file.path.clone(), part));
                override_bytes += hashed.size;
                if override_bytes > packwiz::MAX_OVERRIDES {
                    return Err(JobError::new("too_big", false));
                }
                if file.hash.matches(&hashed) != Some(true) {
                    return Err(JobError::new("hash_mismatch", true));
                }
                continue;
            }
            let meta_text = fetch_small(app, &at, packwiz::MAX_META)
                .await?
                .ok_or_else(|| JobError::new("not_found", false))?;
            if file.hash.matches_bytes(&meta_text) != Some(true) {
                return Err(JobError::new("hash_mismatch", true));
            }
            let Some(meta) = packwiz::meta(&String::from_utf8_lossy(&meta_text), &file.path)
                .map_err(|c| JobError::new(c, false))?
            else {
                continue;
            };
            if !paths.insert(meta.path.to_ascii_lowercase()) {
                continue;
            }
            let hashed = match app.store.kept_by_origin(&meta.url)? {
                Some(kept) if here(app, &kept.sha256) => kept,
                _ => {
                    if !app.blobs.may_keep(0) {
                        return Err(JobError::new("no_space", true));
                    }
                    let (part, hashed) = download(app, &meta.url, MAX_FILE, false).await?;
                    if meta.hash.matches(&hashed) == Some(false) {
                        let _ = std::fs::remove_file(&part);
                        return Err(JobError::new("hash_mismatch", true));
                    }
                    keep(app, &part, &hashed, file_type(&meta.path)).await?;
                    hashed
                }
            };
            if meta.hash.matches(&hashed) == Some(false) {
                return Err(JobError::new("hash_mismatch", true));
            }
            mods.push((meta.path, hashed, meta.url, meta.optional));
        }
        Ok(())
    }
    .await;
    let written = match result {
        Ok(()) => {
            let part = app.blobs.part();
            let to = part.clone();
            let pack = found.clone();
            let list: Vec<(String, Hashed, String, bool)> = mods.clone();
            let files = overrides.clone();
            let wrote = tokio::task::spawn_blocking(move || {
                let written: Vec<packwiz::Written<'_>> = list
                    .iter()
                    .map(|(path, hashed, url, optional)| packwiz::Written {
                        path,
                        hashed,
                        url,
                        optional: *optional,
                    })
                    .collect();
                packwiz::write_mrpack(&to, &pack, &written, &files).and_then(|()| hash_file(&to))
            })
            .await;
            match wrote {
                Ok(Ok(hashed)) => Ok((part, hashed)),
                _ => {
                    let _ = std::fs::remove_file(&part);
                    Err(JobError::new("disk_error", true))
                }
            }
        }
        Err(e) => Err(e),
    };
    for (_, part) in &overrides {
        let _ = std::fs::remove_file(part);
    }
    let (part, archive) = written?;
    keep(app, &part, &archive, "application/zip").await?;

    let slug = packwiz::slug(&found.name);
    let title = if found.name.is_empty() {
        slug.clone()
    } else {
        found.name.clone()
    };
    let project_found = SourceProject {
        source: packwiz::SOURCE,
        source_id: source_id.clone(),
        slug: slug.clone(),
        kind: "modpack",
        title: title.clone(),
        summary: String::new(),
        body: String::new(),
        license: String::new(),
        icon_url: None,
        client_side: "required".to_owned(),
        server_side: "unknown".to_owned(),
        downloads: 0,
        followers: 0,
        categories: Vec::new(),
        authors: if found.author.is_empty() {
            Vec::new()
        } else {
            vec![(found.author.clone(), "author".to_owned())]
        },
        gallery: Vec::new(),
        url: source_id.clone(),
        published: now(),
        updated: now(),
    };
    let (id, _) = app.store.upsert_project(&project_found, now())?;
    not_removed(app, &id)?;
    if let Some(request) = request {
        app.store
            .set_request(request, "importing", None, Some(&id), None, now())?;
    }
    let number = if found.version.is_empty() {
        archive.sha256[..8].to_owned()
    } else {
        found.version.clone()
    };
    let version = SourceVersion {
        // The same pack read again is the same version.
        source_id: archive.sha256.clone(),
        number: number.clone(),
        name: format!("{title} {number}"),
        changelog: String::new(),
        channel: "release".to_owned(),
        published: now(),
        downloads: 0,
        games: vec![found.game.clone()],
        loaders: found.loader.iter().map(|(k, _)| k.clone()).collect(),
        files: vec![crate::model::SourceFile {
            name: format!("{slug}-{number}.mrpack")
                .chars()
                .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
                .collect(),
            size: i64::try_from(archive.size).unwrap_or(i64::MAX),
            sha1: Some(archive.sha1.clone()),
            sha512: Some(archive.sha512.clone()),
            url: source_id.clone(),
            primary: true,
            role: "file".to_owned(),
        }],
        dependencies: Vec::new(),
        details: true,
    };
    let ids = app
        .store
        .upsert_versions(&id, packwiz::SOURCE, &[version])?;
    // The archive is kept already: this only links it.
    for ours in &ids {
        files(app, ours, request).await?;
    }
    app.store.set_project_status(&id, "ready")?;
    Ok(())
}

/// A project the owner added from a site that is not a catalog
/// (external.rs): its card as the owner wrote it, its versions from the
/// site's page (OptiFine) or as the owner added them.
async fn project_external(
    app: &App,
    source_id: &str,
    request: Option<&str>,
) -> Result<(), JobError> {
    if !external::is_ref(source_id) {
        return Err(JobError::new("unsupported_source", false));
    }
    let card = app
        .store
        .external(source_id)?
        .ok_or_else(|| JobError::new("not_found", false))?;
    let versions = if source_id == external::OPTIFINE {
        optifine_versions(app).await?
    } else {
        app.store
            .external_versions(source_id)?
            .iter()
            .map(|added| external::added_version(&card, added))
            .collect()
    };
    let found = card.project(now());
    let (id, owns_card) = app.store.upsert_project(&found, now())?;
    not_removed(app, &id)?;
    app.store.default_recheck_days(&id, card.days)?;
    if let Some(request) = request {
        app.store
            .set_request(request, "importing", None, Some(&id), None, now())?;
    }
    // A page that lost half of what it listed is read wrong, not emptied:
    // nothing is marked gone, and it is read again later.
    let known = app.store.version_sources(&id, external::SOURCE)?.len();
    if source_id == external::OPTIFINE && versions.len() * 2 < known {
        return Err(JobError::new("bad_answer", true));
    }
    let ids = app
        .store
        .upsert_versions(&id, external::SOURCE, &versions)?;
    for (ours, theirs) in app.store.version_sources(&id, external::SOURCE)? {
        if !versions.iter().any(|v| v.source_id == theirs) {
            app.store.mark_version_gone(&ours, now())?;
        }
    }
    // The owner's versions are all wanted; of a site's, the newest for each
    // game version and loader, as on Modrinth.
    let chosen: Vec<usize> = if source_id == external::OPTIFINE {
        newest_per_pair(&versions)
    } else {
        (0..versions.len()).collect()
    };
    for &i in &chosen {
        queue_files(app, &ids[i], request)?;
    }
    if owns_card {
        pictures(app, &id, &found)?;
    }
    app.store.set_project_status(&id, "ready")?;
    Ok(())
}

/// OptiFine's versions from its downloads page; the changelogs of those not
/// here yet that are fetched at once.
async fn optifine_versions(app: &App) -> Result<Vec<SourceVersion>, JobError> {
    let html = app
        .client
        .page(external::OPTIFINE_DOWNLOADS, external::MAX_PAGE)
        .await?
        .ok_or_else(|| JobError::new("not_found", true))?;
    let rows = external::optifine_rows(&html);
    if rows.is_empty() {
        // The page changed, or this is not it: nothing is marked gone.
        return Err(JobError::new("bad_answer", true));
    }
    let mut versions: Vec<SourceVersion> = rows
        .iter()
        .map(|row| external::optifine_version(row, None))
        .collect();
    let mut read = 0;
    for i in newest_per_pair(&versions) {
        let known = app
            .store
            .version_by_source(external::SOURCE, &versions[i].source_id)?
            .is_some();
        if known || read >= external::MAX_CHANGELOGS {
            continue;
        }
        read += 1;
        // A changelog is good to have, not needed.
        if let Ok(Some(text)) = app
            .client
            .page(
                &external::optifine_changelog_url(&rows[i].file),
                external::MAX_CHANGELOG,
            )
            .await
        {
            versions[i] = external::optifine_version(&rows[i], Some(&text));
        }
    }
    Ok(versions)
}

/// A file of a project the owner added: from OptiFine through the page its
/// link leads to, else from the address the owner gave. What comes back
/// must be an archive.
async fn download_external(app: &App, url: &str, max: u64) -> Result<(PathBuf, Hashed), JobError> {
    let from = match external::optifine_file(url) {
        Some(name) => {
            let page = app
                .client
                .page(url, external::MAX_PAGE)
                .await?
                .ok_or_else(|| JobError::new("not_found", false))?;
            external::optifine_link(&page, &name).ok_or_else(|| JobError::new("no_link", true))?
        }
        None => url.to_owned(),
    };
    let part = app.blobs.part();
    if let Err(e) = app.client.external(&from, &part, max).await {
        let _ = std::fs::remove_file(&part);
        return Err(e.into());
    }
    if !external::is_archive(&part) {
        let _ = std::fs::remove_file(&part);
        return Err(JobError::new("not_an_archive", true));
    }
    let hashing = part.clone();
    match tokio::task::spawn_blocking(move || hash_file(&hashing)).await {
        Ok(Ok(hashed)) if hashed.size <= max => Ok((part, hashed)),
        Ok(Ok(_)) => {
            let _ = std::fs::remove_file(&part);
            Err(JobError::new("too_big", false))
        }
        _ => {
            let _ = std::fs::remove_file(&part);
            Err(JobError::new("disk_error", true))
        }
    }
}

/// Queues a project's icon, gallery and the pictures in its description.
fn pictures(app: &App, id: &str, found: &SourceProject) -> Result<(), JobError> {
    let picture = |kind: &str, url: &str| -> Result<bool, String> {
        app.store.enqueue(
            &format!("image:{id}:{kind}:{url}"),
            "image",
            &json!({ "project": id, "kind": kind, "url": url }),
            None,
            Some(id),
            now(),
            None,
        )
    };
    if let Some(url) = &found.icon_url {
        picture("icon", url)?;
    }
    for image in &found.gallery {
        picture("gallery", &image.url)?;
    }
    app.store.enqueue(
        &format!("body:{id}"),
        "body",
        &json!({ "project": id }),
        None,
        Some(id),
        now(),
        Some(0),
    )?;
    Ok(())
}

/// A CurseForge project: its card from its page, its files from the site's
/// API, the dependencies of those fetched at once.
async fn project_curseforge(
    app: &App,
    reference: &str,
    depth: i64,
    request: Option<&str>,
) -> Result<(), JobError> {
    let Some(r) = curseforge::Ref::parse(reference) else {
        return Err(JobError::new("unsupported_source", false));
    };
    let kind = curseforge::kind_of_class(&r.class)
        .ok_or_else(|| JobError::new("unsupported_kind", false))?;
    let page_url = curseforge::page_url(&r.class, &r.slug);
    // The page gives the card, and the ID when the reference has none; a
    // dependency (which has its ID) goes on without it.
    let card = match app
        .client
        .page(&page_url, curseforge::MAX_PAGE as u64)
        .await
    {
        Ok(Some(html)) => Some(curseforge::page(&html, &r.slug)),
        Ok(None) if r.id.is_none() => return Err(JobError::new("not_found", false)),
        Err(e) if r.id.is_none() => return Err(e.into()),
        _ => None,
    };
    let source_id =
        r.id.or_else(|| card.as_ref().and_then(|c| c.id))
            .ok_or_else(|| JobError::new("bad_answer", true))?;
    let known = app
        .store
        .project_by_source(curseforge::SOURCE, &source_id.to_string())?;
    let (id, found) = match (card, known) {
        // Without its page, a project already here keeps its card.
        (None, Some(known)) => (known, None),
        (card, _) => {
            let card = card.unwrap_or_default();
            let allowed = |url: &String| crate::fetch::site(url).is_some();
            let found = SourceProject {
                source: curseforge::SOURCE,
                source_id: source_id.to_string(),
                slug: r.slug.clone(),
                kind,
                title: if card.title.is_empty() {
                    r.slug.clone()
                } else {
                    card.title
                },
                summary: card.summary,
                body: card.body,
                license: String::new(),
                icon_url: card.icon.filter(allowed),
                client_side: "unknown".to_owned(),
                server_side: "unknown".to_owned(),
                downloads: card.downloads,
                followers: 0,
                categories: card.categories,
                authors: card
                    .authors
                    .into_iter()
                    .map(|name| (name, String::new()))
                    .collect(),
                gallery: card
                    .gallery
                    .into_iter()
                    .filter(|g| allowed(&g.url))
                    .collect(),
                url: page_url.clone(),
                published: card.published,
                updated: card.updated,
            };
            let (id, card) = app.store.upsert_project(&found, now())?;
            not_removed(app, &id)?;
            (id, card.then_some(found))
        }
    };
    if let Some(request) = request
        && depth == 0
    {
        app.store
            .set_request(request, "importing", None, Some(&id), None, now())?;
    }
    let mut versions = Vec::new();
    let mut complete = false;
    for index in 0..MAX_CURSEFORGE_PAGES {
        let Some(answer) = app
            .client
            .json(&curseforge::files_url(source_id, index))
            .await?
        else {
            return Err(JobError::new("not_found", false));
        };
        let (mut list, entries, total) = curseforge::files(source_id, &answer);
        versions.append(&mut list);
        if entries < curseforge::PAGE_SIZE || (index + 1) * curseforge::PAGE_SIZE >= total {
            complete = true;
            break;
        }
    }
    let chosen = newest_per_pair(&versions);
    let mut needed = Vec::new();
    for &i in chosen.iter().take(200) {
        let Ok(file) = versions[i].source_id.parse::<u64>() else {
            continue;
        };
        // A file's dependencies are good to have; without them it still installs.
        if let Ok(Some(answer)) = app
            .client
            .json(&curseforge::dependencies_url(source_id, file))
            .await
        {
            let found = curseforge::dependencies(&answer);
            versions[i].dependencies = found.iter().map(|n| n.dependency.clone()).collect();
            needed.extend(found);
        }
        if let Ok(Some(answer)) = app
            .client
            .json(&curseforge::changelog_url(source_id, file))
            .await
        {
            versions[i].changelog = curseforge::changelog(&answer);
        }
        versions[i].details = true;
    }
    let ids = app
        .store
        .upsert_versions(&id, curseforge::SOURCE, &versions)?;
    if complete {
        for (ours, theirs) in app.store.version_sources(&id, curseforge::SOURCE)? {
            if !versions.iter().any(|v| v.source_id == theirs) {
                app.store.mark_version_gone(&ours, now())?;
            }
        }
    }
    for &i in &chosen {
        queue_files(app, &ids[i], request)?;
    }
    queue_curseforge_dependencies(app, &needed, depth, request)?;
    if let Some(found) = &found {
        pictures(app, &id, found)?;
    }
    app.store.set_project_status(&id, "ready")?;
    Ok(())
}

/// Queues the projects a CurseForge file requires (each once).
fn queue_curseforge_dependencies(
    app: &App,
    needed: &[curseforge::Needed],
    depth: i64,
    request: Option<&str>,
) -> Result<(), JobError> {
    let mut queued: Vec<&str> = Vec::new();
    for need in needed {
        if need.dependency.kind != "required" || queued.len() >= MAX_DEPENDENCIES {
            continue;
        }
        let Some(project) = need.dependency.project.as_deref() else {
            continue;
        };
        if queued.contains(&project) {
            continue;
        }
        queued.push(project);
        let reference = curseforge::Ref {
            id: curseforge::id(project),
            class: need.class.to_owned(),
            slug: need.slug.clone(),
        };
        queue_project(
            app,
            curseforge::SOURCE,
            &reference.text(),
            depth + 1,
            request,
            DEPENDENCY_FRESH,
        )?;
    }
    Ok(())
}

/// One CurseForge file a page's address names (`<file>@<project>`).
async fn version_curseforge(
    app: &App,
    text: &str,
    depth: i64,
    request: Option<&str>,
) -> Result<(), JobError> {
    let bad = || JobError::new("unsupported_source", false);
    let (file, reference) = text.split_once('@').ok_or_else(bad)?;
    let file = curseforge::id(file).ok_or_else(bad)?;
    let r = curseforge::Ref::parse(reference).ok_or_else(bad)?;
    let Some(project) = app
        .store
        .project_by_source_ref(curseforge::SOURCE, &r.slug)?
    else {
        // Its project first; this job waits for it.
        queue_project(
            app,
            curseforge::SOURCE,
            reference,
            depth,
            request,
            DEPENDENCY_FRESH,
        )?;
        return Err(JobError::new("waiting_for_project", true));
    };
    let source_id = app
        .store
        .source_id(&project, curseforge::SOURCE)?
        .as_deref()
        .and_then(curseforge::id)
        .ok_or_else(|| JobError::new("bad_answer", false))?;
    let Some(answer) = app
        .client
        .json(&curseforge::file_url(source_id, file))
        .await?
    else {
        return Err(JobError::new("not_found", false));
    };
    let mut found = answer
        .get("data")
        .and_then(|data| curseforge::file(source_id, data))
        .ok_or_else(|| JobError::new("not_found", false))?;
    let mut needed = Vec::new();
    if let Ok(Some(answer)) = app
        .client
        .json(&curseforge::dependencies_url(source_id, file))
        .await
    {
        needed = curseforge::dependencies(&answer);
        found.dependencies = needed.iter().map(|n| n.dependency.clone()).collect();
    }
    if let Ok(Some(answer)) = app
        .client
        .json(&curseforge::changelog_url(source_id, file))
        .await
    {
        found.changelog = curseforge::changelog(&answer);
    }
    found.details = true;
    let ids =
        app.store
            .upsert_versions(&project, curseforge::SOURCE, std::slice::from_ref(&found))?;
    queue_files(app, &ids[0], request)?;
    queue_curseforge_dependencies(app, &needed, depth, request)
}

async fn version(
    app: &App,
    source: &str,
    id: &str,
    depth: i64,
    request: Option<&str>,
) -> Result<(), JobError> {
    if source == curseforge::SOURCE {
        return version_curseforge(app, id, depth, request).await;
    }
    if source != modrinth::SOURCE || !modrinth::is_ref(id) {
        return Err(JobError::new("unsupported_source", false));
    }
    let Some(answer) = app.client.json(&modrinth::version_url(id)).await? else {
        return Err(JobError::new("not_found", false));
    };
    let Some(found) = modrinth::version(&answer) else {
        return Err(JobError::new("bad_answer", false));
    };
    let project_ref = answer
        .get("project_id")
        .and_then(Value::as_str)
        .filter(|p| modrinth::is_ref(p))
        .ok_or_else(|| JobError::new("bad_answer", false))?;
    let Some(project) = app.store.project_by_source(source, project_ref)? else {
        // Its project first; this job waits for it.
        queue_project(app, source, project_ref, depth, request, DEPENDENCY_FRESH)?;
        return Err(JobError::new("waiting_for_project", true));
    };
    let ids = app
        .store
        .upsert_versions(&project, source, std::slice::from_ref(&found))?;
    queue_files(app, &ids[0], request)?;
    for dependency in found
        .dependencies
        .iter()
        .filter(|d| d.kind == "required")
        .take(MAX_DEPENDENCIES)
    {
        match (&dependency.version, &dependency.project) {
            (Some(v), _) => queue_version(app, source, v, depth + 1, request)?,
            (None, Some(p)) => queue_project(app, source, p, depth + 1, request, DEPENDENCY_FRESH)?,
            (None, None) => false,
        };
    }
    Ok(())
}

/// Downloads `url` into a new part file and hashes it; a `picture` may come
/// from any public host ([`crate::fetch::picture_site`]).
async fn download(
    app: &App,
    url: &str,
    max: u64,
    picture: bool,
) -> Result<(PathBuf, Hashed), JobError> {
    let part = app.blobs.part();
    let result = if picture {
        app.client.picture(url, &part, max).await
    } else {
        app.client.get(url, &part, max, false).await
    };
    if let Err(e) = result {
        let _ = std::fs::remove_file(&part);
        return Err(e.into());
    }
    let hashing = part.clone();
    match tokio::task::spawn_blocking(move || hash_file(&hashing)).await {
        Ok(Ok(hashed)) if hashed.size <= max => Ok((part, hashed)),
        Ok(Ok(_)) => {
            let _ = std::fs::remove_file(&part);
            Err(JobError::new("too_big", false))
        }
        _ => {
            let _ = std::fs::remove_file(&part);
            Err(JobError::new("disk_error", true))
        }
    }
}

/// Scans up to `limit` kept archives last scanned under older rules (or
/// never): files kept before the scanner, and all of them again whenever the
/// rules change. One the scanner holds back now is taken out of service.
pub async fn rescan(app: &App, limit: i64) -> Result<usize, String> {
    let due = app.store.to_rescan(limit)?;
    let count = due.len();
    for (sha256, _) in due {
        let path = app.blobs.path(&sha256);
        let verdict = if path.is_file() {
            match tokio::task::spawn_blocking(move || crate::scan::scan_file(&path)).await {
                Ok(Ok(verdict)) => verdict,
                // Unreadable now: tried again under the next rules.
                _ => crate::scan::Verdict::clean(),
            }
        } else {
            crate::scan::Verdict::clean()
        };
        let held = verdict.level.held();
        if held && let Err(e) = app.blobs.withdraw(&sha256) {
            eprintln!("lapi-mods: holding back a file: {e}");
            continue;
        }
        app.store.set_scanned(&sha256, &verdict, held, now())?;
        if held
            && let Err(e) = crate::upkeep::clear(
                &app.store,
                &app.urls,
                app.purge.as_deref(),
                std::slice::from_ref(&sha256),
            )
        {
            eprintln!(
                "lapi-mods: clearing the cache: {}",
                crate::model::clean(&e, 200, false)
            );
        }
        if held {
            eprintln!(
                "lapi-mods: held back a kept file ({})",
                verdict.level.as_str()
            );
            if verdict.level == crate::scan::Level::Suspicious {
                app.store.add_review("scan", &sha256, now())?;
                trusted(app, &sha256)?;
            }
        }
    }
    Ok(count)
}

/// Whether a file need not be fetched: here, served or held, or deleted for
/// good by the owner.
fn here(app: &App, sha256: &str) -> bool {
    app.blobs.holds(sha256)
        || app
            .store
            .blob_state(sha256)
            .is_ok_and(|state| state.as_deref() == Some("forgotten"))
}

/// A project the owner removed is not brought back by an import.
fn not_removed(app: &App, project: &str) -> Result<(), JobError> {
    if app.store.project_status(project)?.as_deref() == Some("removed") {
        return Err(JobError::new("removed", false));
    }
    Ok(())
}

/// Moves a hashed part into the kept files and records it. An archive is
/// scanned first (D-066, stage 6): one the scanner holds back goes into
/// quarantine instead, is recorded as held and waits for the owner.
pub(crate) async fn keep(
    app: &App,
    part: &Path,
    hashed: &Hashed,
    media_type: &str,
) -> Result<(), JobError> {
    if here(app, &hashed.sha256) && app.store.blob_state(&hashed.sha256)?.is_some() {
        // Here already, served or held, and scanned then; or deleted for
        // good by the owner.
        let _ = std::fs::remove_file(part);
        return Ok(());
    }
    if !app.blobs.may_keep(hashed.size) {
        let _ = std::fs::remove_file(part);
        return Err(JobError::new("no_space", true));
    }
    let blocked = app.store.is_blocked(&hashed.sha256, &hashed.sha1)?;
    let verdict = if blocked {
        // The owner blocked it: no scan needed.
        app.store.complete_block(&hashed.sha256, &hashed.sha1)?;
        Some(crate::scan::Verdict::blocked())
    } else if crate::scan::scanned_type(media_type) {
        let path = part.to_path_buf();
        match tokio::task::spawn_blocking(move || crate::scan::scan_file(&path)).await {
            Ok(Ok(verdict)) => Some(verdict),
            _ => {
                let _ = std::fs::remove_file(part);
                return Err(JobError::new("disk_error", true));
            }
        }
    } else {
        None
    };
    let held = verdict.as_ref().is_some_and(|v| v.level.held());
    let placed = if held {
        app.blobs.hold(part, hashed)
    } else {
        app.blobs.keep(part, hashed)
    };
    if let Err(e) = placed {
        let _ = std::fs::remove_file(part);
        eprintln!("lapi-mods: keeping a file: {e}");
        return Err(JobError::new("disk_error", true));
    }
    app.store
        .add_blob(hashed, media_type, verdict.as_ref(), now())?;
    if let Some(verdict) = verdict.filter(|v| v.level.held()) {
        eprintln!("lapi-mods: held back a file ({})", verdict.level.as_str());
        if verdict.level == crate::scan::Level::Suspicious {
            app.store.add_review("scan", &hashed.sha256, now())?;
        }
    }
    Ok(())
}

fn file_type(name: &str) -> &'static str {
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(".jar") {
        "application/java-archive"
    } else if lower.ends_with(".zip") || lower.ends_with(".mrpack") {
        "application/zip"
    } else {
        "application/octet-stream"
    }
}

/// Joins a version's project with its twin on the other source, known by a
/// file both have (D-066, stage 4).
fn join_twins(app: &App, version: &str, sha256: &str, sha1: &str) -> Result<(), JobError> {
    if let Some((stays, joined)) = app.store.join_twins(version, sha256, sha1, now())? {
        eprintln!("lapi-mods: joined project {joined} into {stays}");
    }
    Ok(())
}

/// A file of a project the owner trusts is not held for what the scanner
/// only suspects (external.rs `trust`).
fn trusted(app: &App, sha256: &str) -> Result<(), String> {
    if crate::owner::release_trusted(&app.store, &app.blobs, None, Some(sha256), now())? > 0 {
        eprintln!("lapi-mods: let a held file of a trusted project go");
    }
    Ok(())
}

async fn files(app: &App, version: &str, request: Option<&str>) -> Result<(), JobError> {
    let mut rows = app.store.files(version)?;
    let outside = app.store.version_source(version)?.as_deref() == Some(external::SOURCE);
    // The main file first.
    rows.sort_by_key(|f| (!f.primary, f.ord));
    let mut failed: Option<JobError> = None;
    for file in rows.into_iter().filter(|f| f.sha256.is_none()) {
        if let Some(kept) = app
            .store
            .blob_by_hashes(file.sha1.as_deref(), file.sha512.as_deref())?
            && here(app, &kept)
        {
            app.store.file_kept(version, file.ord, &kept)?;
            join_twins(
                app,
                version,
                &kept,
                file.sha1.as_deref().unwrap_or_default(),
            )?;
            if outside {
                trusted(app, &kept)?;
            }
            continue;
        }
        let declared = u64::try_from(file.size).unwrap_or(0);
        if declared > MAX_FILE {
            failed = Some(JobError::new("too_big", false));
            continue;
        }
        if declared > 0 && !app.blobs.may_keep(declared) {
            return Err(JobError::new("no_space", true));
        }
        let max = if declared > 0 { declared } else { MAX_FILE };
        let fetched = if outside {
            download_external(app, &file.origin_url, max).await
        } else {
            download(app, &file.origin_url, max, false).await
        };
        let (part, hashed) = match fetched {
            Ok(got) => got,
            Err(e) => {
                failed = Some(e);
                continue;
            }
        };
        let matches = file.sha512.as_ref().is_none_or(|h| *h == hashed.sha512)
            && file.sha1.as_ref().is_none_or(|h| *h == hashed.sha1)
            && (declared == 0 || declared == hashed.size);
        if !matches {
            let _ = std::fs::remove_file(&part);
            failed = Some(JobError::new("hash_mismatch", true));
            continue;
        }
        keep(app, &part, &hashed, file_type(&file.name)).await?;
        app.store.file_kept(version, file.ord, &hashed.sha256)?;
        join_twins(app, version, &hashed.sha256, &hashed.sha1)?;
        if outside {
            trusted(app, &hashed.sha256)?;
        }
        if let Some(request) = request {
            app.store.add_request_bytes(request, hashed.size)?;
        }
    }
    failed.map_or(Ok(()), Err)
}

async fn pack_files(app: &App, version: &str) -> Result<(), JobError> {
    let Some(archive) = pack_archive(app, version)? else {
        return Err(JobError::new("not_a_pack", false));
    };
    if let Some(sha256) = &archive.sha256
        && app.store.blob_state(sha256)?.is_some_and(|s| s != "stored")
    {
        // A held (or forgotten) pack is not opened.
        return Err(JobError::new("held", false));
    }
    let Some(sha256) = archive.sha256.filter(|s| app.blobs.has(s)) else {
        // The archive first.
        queue_files(app, version, None)?;
        return Err(JobError::new("waiting_for_files", true));
    };
    if !app.store.has_pack(version)? {
        let path = app.blobs.path(&sha256);
        let index = match tokio::task::spawn_blocking(move || pack::read(&path)).await {
            Ok(Ok(index)) => index,
            Ok(Err("disk_error")) | Err(_) => return Err(JobError::new("disk_error", true)),
            Ok(Err(code)) => return Err(JobError::new(code, false)),
        };
        app.store.set_pack(version, &sha256, &index, now())?;
    }
    let mut failed: Option<JobError> = None;
    let rows = app.store.pack_files(version)?;
    let mut paths: std::collections::HashSet<String> = rows
        .iter()
        .filter(|f| !f.path.is_empty())
        .map(|f| f.path.to_ascii_lowercase())
        .collect();
    for file in rows.into_iter().filter(|f| f.sha256.is_none()) {
        if file.path.is_empty() {
            if let Err(e) = curseforge_pack_file(app, version, &file, &mut paths).await {
                if e.code == "no_space" {
                    return Err(e);
                }
                failed = Some(e);
            }
            continue;
        }
        if let Some(kept) = app
            .store
            .blob_by_hashes(Some(&file.sha1), Some(&file.sha512))?
            && here(app, &kept)
        {
            app.store.pack_file_kept(version, file.ord, &kept)?;
            continue;
        }
        let declared = u64::try_from(file.size).unwrap_or(0);
        if !app.blobs.may_keep(declared) {
            return Err(JobError::new("no_space", true));
        }
        let (part, hashed) = match download(app, &file.origin_url, declared, false).await {
            Ok(got) => got,
            Err(e) => {
                failed = Some(e);
                continue;
            }
        };
        if hashed.sha512 != file.sha512 || hashed.sha1 != file.sha1 || hashed.size != declared {
            let _ = std::fs::remove_file(&part);
            failed = Some(JobError::new("hash_mismatch", true));
            continue;
        }
        keep(app, &part, &hashed, file_type(&file.origin_url)).await?;
        app.store
            .pack_file_kept(version, file.ord, &hashed.sha256)?;
    }
    failed.map_or(Ok(()), Err)
}

/// Finds, keeps and places one file of a CurseForge pack: its name from the
/// site, the file from forgecdn (or kept already from the same address), its
/// folder from what it is. A file that is nothing a pack may bring, or whose
/// path another file has, is left out.
async fn curseforge_pack_file(
    app: &App,
    version: &str,
    file: &crate::store::PackFileRow,
    paths: &mut std::collections::HashSet<String>,
) -> Result<(), JobError> {
    let (project, file_id) = curseforge::download_ids(&file.origin_url)
        .ok_or_else(|| JobError::new("bad_pack", false))?;
    let Some(answer) = app
        .client
        .json(&curseforge::file_url(project, file_id))
        .await?
    else {
        // Gone from the site: the pack goes without it, as counted.
        app.store.pack_file_left_out(version, file.ord)?;
        return Ok(());
    };
    let (name, declared) = answer
        .get("data")
        .and_then(|data| curseforge::pack_file(project, file_id, data))
        .ok_or_else(|| JobError::new("not_found", false))?;
    let Some(name) = name else {
        app.store.pack_file_left_out(version, file.ord)?;
        return Ok(());
    };
    if declared > MAX_FILE {
        return Err(JobError::new("too_big", false));
    }
    let (part, hashed) = match app.store.kept_by_origin(&file.origin_url)? {
        Some(kept) if here(app, &kept.sha256) => (None, kept),
        _ => {
            if !app.blobs.may_keep(declared) {
                return Err(JobError::new("no_space", true));
            }
            let max = if declared > 0 { declared } else { MAX_FILE };
            let (part, hashed) = download(app, &file.origin_url, max, false).await?;
            if declared > 0 && hashed.size != declared {
                let _ = std::fs::remove_file(&part);
                return Err(JobError::new("hash_mismatch", true));
            }
            (Some(part), hashed)
        }
    };
    let look = part
        .clone()
        .or_else(|| app.blobs.find(&hashed.sha256))
        .unwrap_or_else(|| app.blobs.path(&hashed.sha256));
    let looked = name.clone();
    let folder = tokio::task::spawn_blocking(move || pack::folder_of(&look, &looked))
        .await
        .ok()
        .flatten();
    let path = folder.map(|f| format!("{f}/{name}"));
    let Some(path) = path.filter(|p| pack::pack_path(p) && paths.insert(p.to_ascii_lowercase()))
    else {
        if let Some(part) = part {
            let _ = std::fs::remove_file(part);
        }
        app.store.pack_file_left_out(version, file.ord)?;
        return Ok(());
    };
    if let Some(part) = part {
        keep(app, &part, &hashed, file_type(&name)).await?;
    }
    app.store
        .pack_file_found(version, file.ord, &path, &hashed)?;
    Ok(())
}

/// Keeps a picture from `url` (once per address); its SHA-256 and type.
async fn keep_image(app: &App, url: &str) -> Result<(String, String), JobError> {
    if let Some((sha256, media_type)) = app.store.image_by_origin(url)?
        && app.blobs.has(&sha256)
    {
        return Ok((sha256, media_type));
    }
    let (part, hashed) = download(app, url, MAX_IMAGE, true).await?;
    let mut head = [0u8; 16];
    let read = {
        use std::io::Read as _;
        std::fs::File::open(&part).and_then(|mut f| f.read(&mut head))
    };
    let Some(media_type) = read.ok().and_then(|n| crate::thumb::sniff(&head[..n])) else {
        let _ = std::fs::remove_file(&part);
        return Err(JobError::new("not_a_picture", false));
    };
    keep(app, &part, &hashed, media_type).await?;
    app.store
        .set_image_origin(url, &hashed.sha256, media_type)?;
    Ok((hashed.sha256, media_type.to_owned()))
}

/// A small copy of a kept picture, kept too; `None` when it cannot be made.
pub(crate) async fn small_copy(app: &App, sha256: &str, size: u32, format: &str) -> Option<String> {
    let input = app.blobs.path(sha256);
    let output = app.blobs.part();
    let made = match &app.thumbs {
        Thumbs::Process(program) => {
            let run = tokio::process::Command::new(program)
                .arg("thumb")
                .arg(&input)
                .arg(&output)
                .arg(size.to_string())
                .arg(format)
                .env_clear()
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .kill_on_drop(true)
                .status();
            matches!(tokio::time::timeout(Duration::from_secs(60), run).await, Ok(Ok(s)) if s.success())
        }
        Thumbs::Inline => {
            let (input, output, format) = (input.clone(), output.clone(), format.to_owned());
            tokio::task::spawn_blocking(move || {
                crate::thumb::make(&input, &output, size, &format).is_ok()
            })
            .await
            .unwrap_or(false)
        }
    };
    if !made {
        let _ = std::fs::remove_file(&output);
        return None;
    }
    let hashing = output.clone();
    let hashed = tokio::task::spawn_blocking(move || hash_file(&hashing))
        .await
        .ok()?
        .ok()?;
    let media_type = if format == "webp" {
        "image/webp"
    } else {
        "image/jpeg"
    };
    keep(app, &output, &hashed, media_type).await.ok()?;
    Some(hashed.sha256)
}

async fn image(app: &App, project: &str, kind: &str, url: &str) -> Result<(), JobError> {
    let (sha256, media_type) = keep_image(app, url).await?;
    match kind {
        "icon" => {
            let thumb = small_copy(app, &sha256, 256, "webp").await;
            app.store
                .set_icon(project, &sha256, &media_type, thumb.as_deref())?;
        }
        "gallery" => {
            let thumb = small_copy(app, &sha256, 512, "jpeg").await;
            app.store
                .set_gallery_image(project, url, &sha256, &media_type, thumb.as_deref())?;
        }
        _ => return Err(JobError::new("unknown_job", false)),
    }
    Ok(())
}

/// The pictures a description shows that we may fetch: each address as
/// written and as fetched (a plain `http:` one over HTTPS, as older
/// descriptions on CurseForge have them). A picture is an address with a
/// picture's ending, or any address a description shows as a picture
/// (`![…](…)`, `<img src=…>`); what it really is, the download tells.
pub fn body_images(body: &str) -> Vec<(String, String)> {
    let mut found: Vec<(String, String)> = Vec::new();
    let mut from = 0;
    loop {
        let rest = &body[from..];
        let at = match (rest.find("https://"), rest.find("http://")) {
            (Some(a), Some(b)) => a.min(b),
            (Some(a), None) | (None, Some(a)) => a,
            (None, None) => break,
        };
        let start = from + at;
        let tail = &body[start..];
        let end = tail
            .find(|c: char| {
                c.is_whitespace() || matches!(c, ')' | '"' | '\'' | '<' | '>' | ']' | '`')
            })
            .unwrap_or(tail.len());
        let written = &tail[..end];
        let url = match written.strip_prefix("http://") {
            Some(plain) => format!("https://{plain}"),
            None => written.to_owned(),
        };
        let lower = url.to_ascii_lowercase();
        let path = lower.split(['?', '#']).next().unwrap_or_default();
        let picture = [".png", ".jpg", ".jpeg", ".gif", ".webp"]
            .iter()
            .any(|e| path.ends_with(e))
            || shown_as_picture(&body[..start]);
        if picture
            && crate::fetch::picture_site(&url).is_some()
            && !found.iter().any(|(w, _)| w == written)
        {
            found.push((written.to_owned(), url));
            if found.len() >= MAX_BODY_IMAGES {
                break;
            }
        }
        from = start + end.max(1);
    }
    found
}

/// Whether the address that follows `before` is the source of a picture:
/// `![alt](` in Markdown or `<img … src=` in HTML.
fn shown_as_picture(before: &str) -> bool {
    if let Some(inner) = before.strip_suffix("](") {
        // The `[` that this `]` closes, within a sane length of alt text.
        let mut depth = 0usize;
        for (at, c) in inner.char_indices().rev().take(512) {
            match c {
                ']' => depth += 1,
                '[' if depth == 0 => return inner[..at].ends_with('!'),
                '[' => depth -= 1,
                _ => {}
            }
        }
        return false;
    }
    let attribute = before
        .strip_suffix('"')
        .or_else(|| before.strip_suffix('\''))
        .unwrap_or(before)
        .trim_end();
    let Some(attribute) = attribute.strip_suffix('=') else {
        return false;
    };
    let attribute = attribute.trim_end();
    let lower = attribute
        .get(attribute.len().saturating_sub(4)..)
        .unwrap_or_default()
        .to_ascii_lowercase();
    if lower != " src" && lower != "\nsrc" && lower != "\tsrc" {
        return false;
    }
    // The tag the attribute is in: the last `<` before it.
    attribute.rfind('<').is_some_and(|open| {
        let tag = &attribute[open + 1..];
        tag.get(..3)
            .is_some_and(|name| name.eq_ignore_ascii_case("img"))
            && tag
                .get(3..)
                .is_some_and(|t| t.starts_with(char::is_whitespace))
            && !tag.contains('>')
    })
}

async fn body(app: &App, project: &str) -> Result<(), JobError> {
    let text = app.store.body(project)?;
    let mut ours = Vec::new();
    for (written, url) in body_images(&text) {
        // A picture that cannot be had stays as it was; the launcher does not
        // load pictures from other sites.
        if let Ok((sha256, media_type)) = keep_image(app, &url).await {
            ours.push((written, app.urls.image(&sha256, &media_type)));
        }
    }
    if ours.is_empty() {
        return Ok(());
    }
    // Read again: the project may have been fetched anew meanwhile.
    let mut text = app.store.body(project)?;
    for (theirs, kept) in &ours {
        text = text.replace(theirs.as_str(), kept);
    }
    app.store.set_body(project, &text)?;
    Ok(())
}
