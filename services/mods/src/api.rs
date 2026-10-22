//! The catalog's API at mods.lapi.sh. Reading is open to anyone; adding to
//! the catalog takes an account's access token (aud `mods`). Every value from
//! a query or a body is checked before it reaches the database.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::extract::{DefaultBodyLimit, Path, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::App;
use crate::auth::Claims;
use crate::import::{self, now};
use crate::model::{KINDS, word};
use crate::store::Search;

/// Requests an account sends to the catalog: an hour, a day, and the bytes
/// they may bring in a day.
pub const IMPORTS_PER_HOUR: u32 = 20;
pub const IMPORTS_PER_DAY: i64 = 100;
pub const IMPORT_BYTES_PER_DAY: i64 = 5 * 1024 * 1024 * 1024;
/// From one address, whatever the account.
pub const IMPORTS_PER_HOUR_PER_ADDRESS: u32 = 60;
/// Old versions' files asked for, per address an hour.
pub const FETCHES_PER_HOUR: u32 = 30;
const MAX_LOOKUP: usize = 200;
const MAX_OFFSET: i64 = 10_000;

/// Actions counted per key (an account or an address) in a window. Kept in
/// memory only.
#[derive(Default)]
pub struct Limiter {
    windows: Mutex<Windows>,
}

/// When each window started and what it counted, per key and kind.
type Windows = HashMap<(String, &'static str), (Instant, u32)>;

impl Limiter {
    pub fn allow(&self, key: &str, bucket: &'static str, limit: u32, window: Duration) -> bool {
        let mut windows = self
            .windows
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let now = Instant::now();
        if windows.len() > 200_000 {
            windows.retain(|_, (start, _)| now.duration_since(*start) < Duration::from_secs(3600));
        }
        let entry = windows.entry((key.to_owned(), bucket)).or_insert((now, 0));
        if now.duration_since(entry.0) >= window {
            *entry = (now, 0);
        }
        entry.1 = entry.1.saturating_add(1);
        entry.1 <= limit
    }
}

fn refuse(status: StatusCode, code: &str) -> Response {
    (status, Json(json!({ "error": code }))).into_response()
}

fn failed(e: &str) -> Response {
    eprintln!("lapi-mods: {e}");
    refuse(StatusCode::INTERNAL_SERVER_ERROR, "server_error")
}

/// An answer anyone may cache for a while (Cloudflare and the launcher).
fn cached(value: Value, seconds: u32) -> Response {
    let mut response = Json(value).into_response();
    if let Ok(v) = HeaderValue::from_str(&format!("public, max-age={seconds}")) {
        response.headers_mut().insert(header::CACHE_CONTROL, v);
    }
    response
}

fn private(value: Value, status: StatusCode) -> Response {
    let mut response = (status, Json(value)).into_response();
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

/// The client's address as nginx passes it, for the limits only: an IPv6
/// /64 counts as one, since one connection usually has a whole /64 to pick
/// addresses from.
fn client(headers: &HeaderMap) -> String {
    let ip = headers
        .get("x-real-ip")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().parse::<IpAddr>().ok())
        .unwrap_or(IpAddr::from([0, 0, 0, 0]))
        .to_canonical();
    match ip {
        IpAddr::V6(v6) => {
            let s = v6.segments();
            format!("{:x}:{:x}:{:x}:{:x}::/64", s[0], s[1], s[2], s[3])
        }
        IpAddr::V4(v4) => v4.to_string(),
    }
}

/// The account behind a request's bearer token.
fn who(app: &App, headers: &HeaderMap) -> Result<Claims, Box<Response>> {
    let unauthorized = || Box::new(refuse(StatusCode::UNAUTHORIZED, "unauthorized"));
    let token = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .ok_or_else(unauthorized)?;
    let claims = app
        .tokens
        .verify(token.trim(), now())
        .ok_or_else(unauthorized)?;
    // Signed out, switched off or deleted.
    if !app.sessions.admit(&claims.sid, &claims.sub) {
        return Err(unauthorized());
    }
    Ok(claims)
}

/// The account behind a request that publishes a pack: its email must be
/// confirmed when the rule asks for that, so publishing takes more than a
/// throwaway account.
fn publisher(app: &App, headers: &HeaderMap) -> Result<Claims, Box<Response>> {
    let me = who(app, headers)?;
    if app.email.required() && !me.ev {
        return Err(Box::new(refuse(
            StatusCode::FORBIDDEN,
            "email_not_confirmed",
        )));
    }
    Ok(me)
}

/// Our IDs are UUIDs; slugs are short, plain ASCII.
fn reference(text: &str) -> Option<&str> {
    (!text.is_empty()
        && text.len() <= 100
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'!')))
    .then_some(text)
}

fn our_id(text: &str) -> Option<&str> {
    uuid::Uuid::parse_str(text).ok().map(|_| text)
}

async fn health(State(app): State<Arc<App>>) -> Response {
    private(
        json!({
            "ok": true,
            "service": "lapi-mods",
            "version": env!("CARGO_PKG_VERSION"),
            "keys": app.tokens.has_keys(),
        }),
        StatusCode::OK,
    )
}

#[derive(Deserialize, Default)]
pub struct HarmfulQuery {
    since: Option<i64>,
    /// With `since`, the last SHA-256 of the page before.
    after: Option<String>,
}

/// At most this many harmful files a page.
const HARMFUL_PAGE: i64 = 500;

/// Files held back as harmful since a time (D-066, stage 6), so a launcher
/// can warn about one installed before it was held. `next` holds the
/// `since` and `after` of the next page while one is due.
async fn harmful(State(app): State<Arc<App>>, Query(q): Query<HarmfulQuery>) -> Response {
    let since = q.since.unwrap_or(0).clamp(0, i64::MAX / 2);
    let after = q
        .after
        .filter(|a| crate::blobs::is_sha256(a))
        .unwrap_or_default();
    match app.store.harmful_since(since, &after, HARMFUL_PAGE + 1) {
        Ok(mut found) => {
            let next = if found.len() as i64 > HARMFUL_PAGE {
                found.truncate(HARMFUL_PAGE as usize);
                found
                    .last()
                    .map(|(f, key)| json!({ "since": f["since"], "after": key }))
            } else {
                None
            };
            let files: Vec<Value> = found.into_iter().map(|(f, _)| f).collect();
            cached(json!({ "files": files, "next": next }), 300)
        }
        Err(e) => failed(&e),
    }
}

#[derive(Deserialize, Default)]
pub struct SearchQuery {
    q: Option<String>,
    kind: Option<String>,
    game: Option<String>,
    loader: Option<String>,
    /// Categories' IDs (`mod.optimization`) or the sites' own slugs, comma
    /// between; a project in any of them.
    category: Option<String>,
    /// Where projects came from (`own`: players' packs).
    source: Option<String>,
    /// Added in the last this many days.
    since: Option<i64>,
    /// An owner's collection, by its key.
    collection: Option<String>,
    sort: Option<String>,
    offset: Option<i64>,
    limit: Option<i64>,
}

/// Where a project in the catalog can come from.
const SOURCES: &[&str] = &["modrinth", "curseforge", "packwiz", "own", "external"];

/// Days a search for what is new looks back at most.
const MAX_SINCE_DAYS: i64 = 365;

impl SearchQuery {
    /// What it asks for, checked; `None` for anything malformed.
    fn checked(&self) -> Option<Search> {
        let (Ok(kind), Ok(game), Ok(loader), Ok(source), Ok(collection)) = (
            filter(&self.kind),
            filter(&self.game),
            filter(&self.loader),
            filter(&self.source),
            filter(&self.collection),
        ) else {
            return None;
        };
        if kind.as_deref().is_some_and(|k| !KINDS.contains(&k))
            || source.as_deref().is_some_and(|s| !SOURCES.contains(&s))
            || collection.as_deref().is_some_and(|c| !collection_key(c))
        {
            return None;
        }
        let categories = categories(self.category.as_deref().unwrap_or_default())?;
        let since = match self.since {
            None => None,
            Some(days @ 1..=MAX_SINCE_DAYS) => Some(now() - days * 86_400),
            Some(_) => return None,
        };
        let sort = match self.sort.as_deref() {
            // Relevance with words, the most downloaded without.
            None | Some("" | "relevance") => "relevance",
            Some("downloads") => "downloads",
            Some("updated") => "updated",
            Some("new") => "new",
            Some("title") => "title",
            Some("trending") => "trending",
            Some(_) => return None,
        };
        Some(Search {
            words: words(self.q.as_deref().unwrap_or_default()),
            kind,
            game,
            loader,
            categories,
            source,
            since,
            collection,
            sort,
            offset: self.offset.unwrap_or(0).clamp(0, MAX_OFFSET),
            limit: self.limit.unwrap_or(20).clamp(1, 50),
        })
    }
}

/// The categories asked for: at most 8, each a word.
fn categories(text: &str) -> Option<Vec<String>> {
    let mut out = Vec::new();
    for part in text.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        out.push(word(part)?);
    }
    (out.len() <= crate::categories::MAX_ASKED).then_some(out)
}

/// A collection's key: 1 to 32 of `a-z`, `0-9` and `-`.
pub fn collection_key(key: &str) -> bool {
    (1..=32).contains(&key.len())
        && key
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// The words of a search: letters and digits, at most 8 of 32 characters.
pub fn words(q: &str) -> Vec<String> {
    q.chars()
        .take(200)
        .collect::<String>()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(|w| w.chars().take(32).collect::<String>().to_lowercase())
        .take(8)
        .collect()
}

/// A filter of a closed vocabulary; an empty one is no filter, a bad one an
/// error.
fn filter(value: &Option<String>) -> Result<Option<String>, ()> {
    match value.as_deref().map(str::trim) {
        None | Some("") => Ok(None),
        Some(v) => word(v).map(Some).ok_or(()),
    }
}

async fn search(State(app): State<Arc<App>>, Query(q): Query<SearchQuery>) -> Response {
    let Some(search) = q.checked() else {
        return refuse(StatusCode::BAD_REQUEST, "bad_query");
    };
    match app.store.search(&search, &app.urls) {
        Ok((total, hits)) => cached(json!({ "total": total, "hits": hits }), 60),
        Err(e) => failed(&e),
    }
}

/// How many projects a search finds in each category of its kind (`mod`
/// when none is given), the categories it asks for aside.
async fn category_counts(State(app): State<Arc<App>>, Query(q): Query<SearchQuery>) -> Response {
    let Some(search) = q.checked() else {
        return refuse(StatusCode::BAD_REQUEST, "bad_query");
    };
    match app.store.category_counts(&search) {
        Ok(counts) => {
            let list: Vec<Value> = counts
                .into_iter()
                .map(|(id, count)| json!({ "id": id, "count": count }))
                .collect();
            cached(json!({ "categories": list }), 60)
        }
        Err(e) => failed(&e),
    }
}

#[derive(Deserialize, Default)]
pub struct SimilarQuery {
    game: Option<String>,
    loader: Option<String>,
}

/// Up to five projects like one, for a game version and a loader.
async fn similar(
    State(app): State<Arc<App>>,
    Path(r): Path<String>,
    Query(q): Query<SimilarQuery>,
) -> Response {
    let (Ok(game), Ok(loader)) = (filter(&q.game), filter(&q.loader)) else {
        return refuse(StatusCode::BAD_REQUEST, "bad_query");
    };
    let Some(r) = reference(&r) else {
        return refuse(StatusCode::NOT_FOUND, "not_found");
    };
    let id = match app.store.project_id(r) {
        Ok(Some(id)) => id,
        Ok(None) => return refuse(StatusCode::NOT_FOUND, "not_found"),
        Err(e) => return failed(&e),
    };
    match app
        .store
        .similar(&id, game.as_deref(), loader.as_deref(), &app.urls)
    {
        Ok(hits) => cached(json!({ "hits": hits }), 600),
        Err(e) => failed(&e),
    }
}

#[derive(Deserialize, Default)]
pub struct CollectionsQuery {
    kind: Option<String>,
    game: Option<String>,
    loader: Option<String>,
}

/// Projects of a collection shown on its card at most.
const COLLECTION_ICONS: i64 = 4;
/// Projects the week's most popular has.
const POPULAR: i64 = 20;
/// Days what is new looks back.
const NEW_DAYS: i64 = 7;

/// Collections for a kind (`mod` when none is given), a game version and a
/// loader: the week's most popular, what is new, players' packs (for
/// modpacks) and the owner's. Each says what to search for to list it
/// (`query`, the search's own parameters), how many projects it has under the
/// game version and loader, and the first ones' icons and titles for its
/// card. Empty ones are left out.
async fn collections(State(app): State<Arc<App>>, Query(q): Query<CollectionsQuery>) -> Response {
    let (Ok(kind), Ok(game), Ok(loader)) = (filter(&q.kind), filter(&q.game), filter(&q.loader))
    else {
        return refuse(StatusCode::BAD_REQUEST, "bad_query");
    };
    let kind = kind.unwrap_or_else(|| "mod".to_owned());
    if !KINDS.contains(&kind.as_str()) {
        return refuse(StatusCode::BAD_REQUEST, "bad_query");
    }
    // By the catalog's file too: tests run several catalogs in one process.
    let key = format!(
        "{} collections {kind} {} {}",
        app.store.path().display(),
        game.as_deref().unwrap_or("-"),
        loader.as_deref().unwrap_or("-")
    );
    if let Some(answer) = remembered(&key) {
        return cached(answer, 60);
    }
    let base = Search {
        kind: Some(kind.clone()),
        game: game.clone(),
        loader: loader.clone(),
        sort: "relevance",
        limit: COLLECTION_ICONS,
        ..Search::default()
    };
    let mut wanted: Vec<(String, Option<crate::store::CollectionRow>, Search, Value)> = vec![
        (
            "popular-week".to_owned(),
            None,
            Search {
                sort: "trending",
                ..base.clone()
            },
            json!({ "sort": "trending" }),
        ),
        (
            "new".to_owned(),
            None,
            Search {
                sort: "new",
                since: Some(now() - NEW_DAYS * 86_400),
                ..base.clone()
            },
            json!({ "sort": "new", "since": NEW_DAYS }),
        ),
    ];
    if kind == "modpack" {
        wanted.push((
            "players-packs".to_owned(),
            None,
            Search {
                source: Some("own".to_owned()),
                sort: "new",
                ..base.clone()
            },
            json!({ "source": "own", "sort": "new" }),
        ));
    }
    match app
        .store
        .collections(&kind, game.as_deref(), loader.as_deref())
    {
        Ok(rows) => {
            for row in rows {
                let search = Search {
                    collection: Some(row.key.clone()),
                    ..base.clone()
                };
                let query = json!({ "collection": row.key });
                wanted.push((row.key.clone(), Some(row), search, query));
            }
        }
        Err(e) => return failed(&e),
    }
    let mut list = Vec::new();
    for (id, row, search, query) in wanted {
        let (total, hits) = match app.store.search(&search, &app.urls) {
            Ok(found) => found,
            Err(e) => return failed(&e),
        };
        let count = if id == "popular-week" {
            total.min(POPULAR)
        } else {
            total
        };
        if count == 0 {
            continue;
        }
        let icons: Vec<&Value> = hits
            .iter()
            .filter_map(|h| h.get("iconThumb").filter(|i| !i.is_null()))
            .collect();
        let names: Vec<&Value> = hits.iter().filter_map(|h| h.get("title")).take(2).collect();
        list.push(json!({
            "id": id,
            "preset": row.is_none(),
            "titles": row.as_ref().map(|r| Value::Object(r.titles.clone())),
            "notes": row.as_ref().map(|r| Value::Object(r.notes.clone())),
            "game": row.as_ref().and_then(|r| r.game.clone()),
            "loader": row.as_ref().and_then(|r| r.loader.clone()),
            "query": query,
            "count": count,
            "icons": icons,
            "names": names,
        }));
    }
    let answer = json!({ "collections": list });
    remember(key, answer.clone());
    cached(answer, 60)
}

/// The catalog's own collections' keys: not the owner's to take.
pub const PRESETS: &[&str] = &["popular-week", "new", "players-packs"];

/// Answers that take long to make, for a minute: collections.
static REMEMBERED: Mutex<Option<HashMap<String, (Instant, Value)>>> = Mutex::new(None);
/// Answers remembered at most.
const MAX_REMEMBERED: usize = 500;

fn remembered(key: &str) -> Option<Value> {
    let mut map = REMEMBERED.lock().unwrap_or_else(|e| e.into_inner());
    let map = map.get_or_insert_with(HashMap::new);
    match map.get(key) {
        Some((at, value)) if at.elapsed() < Duration::from_secs(60) => Some(value.clone()),
        _ => None,
    }
}

fn remember(key: String, value: Value) {
    let mut map = REMEMBERED.lock().unwrap_or_else(|e| e.into_inner());
    let map = map.get_or_insert_with(HashMap::new);
    if map.len() >= MAX_REMEMBERED {
        map.retain(|_, (at, _)| at.elapsed() < Duration::from_secs(60));
        if map.len() >= MAX_REMEMBERED {
            map.clear();
        }
    }
    map.insert(key, (Instant::now(), value));
}

async fn project(State(app): State<Arc<App>>, Path(r): Path<String>) -> Response {
    let Some(r) = reference(&r) else {
        return refuse(StatusCode::NOT_FOUND, "not_found");
    };
    let id = match app.store.project_id(r) {
        Ok(Some(id)) => id,
        Ok(None) => return refuse(StatusCode::NOT_FOUND, "not_found"),
        Err(e) => return failed(&e),
    };
    import::wanted(&app, &id);
    match app.store.project_json(&id, &app.urls) {
        Ok(Some(p)) => cached(p, 60),
        Ok(None) => refuse(StatusCode::NOT_FOUND, "not_found"),
        Err(e) => failed(&e),
    }
}

#[derive(Deserialize, Default)]
pub struct VersionsQuery {
    game: Option<String>,
    loader: Option<String>,
    /// `release`, `beta` or `alpha`.
    channel: Option<String>,
    offset: Option<i64>,
    /// At most 100; without it, every version (up to 2000) as before.
    limit: Option<i64>,
}

async fn versions(
    State(app): State<Arc<App>>,
    Path(r): Path<String>,
    Query(q): Query<VersionsQuery>,
) -> Response {
    let (Ok(game), Ok(loader), Ok(channel)) =
        (filter(&q.game), filter(&q.loader), filter(&q.channel))
    else {
        return refuse(StatusCode::BAD_REQUEST, "bad_query");
    };
    if channel
        .as_deref()
        .is_some_and(|c| !matches!(c, "release" | "beta" | "alpha"))
    {
        return refuse(StatusCode::BAD_REQUEST, "bad_query");
    }
    let Some(r) = reference(&r) else {
        return refuse(StatusCode::NOT_FOUND, "not_found");
    };
    let id = match app.store.project_id(r) {
        Ok(Some(id)) => id,
        Ok(None) => return refuse(StatusCode::NOT_FOUND, "not_found"),
        Err(e) => return failed(&e),
    };
    let page = crate::store::VersionsPage {
        game,
        loader,
        channel,
        offset: q.offset.unwrap_or(0).clamp(0, MAX_OFFSET),
        limit: q
            .limit
            .map_or(crate::store::MAX_VERSIONS, |l| l.clamp(1, 100)),
    };
    match app.store.versions(&id, &page, &app.urls) {
        // "saved": "all" is kept here; anything else is fetched when asked.
        Ok(list) => cached(
            json!({
                "versions": list.versions,
                "total": list.total,
                "games": list.games,
                "loaders": list.loaders,
            }),
            60,
        ),
        Err(e) => failed(&e),
    }
}

async fn version(State(app): State<Arc<App>>, Path(id): Path<String>) -> Response {
    let Some(id) = our_id(&id) else {
        return refuse(StatusCode::NOT_FOUND, "not_found");
    };
    match app.store.version(id, &app.urls) {
        Ok(Some(v)) => cached(v, 30),
        Ok(None) => refuse(StatusCode::NOT_FOUND, "not_found"),
        Err(e) => failed(&e),
    }
}

/// The projects whose kept jars give a mod ID, as the jars name it: what a
/// mod's own descriptor needs, found where its page names nothing (D-066).
async fn provides(
    State(app): State<Arc<App>>,
    Path(id): Path<String>,
    Query(q): Query<VersionsQuery>,
) -> Response {
    let (Ok(game), Ok(loader)) = (filter(&q.game), filter(&q.loader)) else {
        return refuse(StatusCode::BAD_REQUEST, "bad_query");
    };
    let id = id.to_ascii_lowercase();
    let ok = (1..=64).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'));
    if !ok {
        return refuse(StatusCode::NOT_FOUND, "not_found");
    }
    match app
        .store
        .provided_by(&id, game.as_deref(), loader.as_deref())
    {
        Ok(list) => cached(json!({ "projects": list }), 60),
        Err(e) => failed(&e),
    }
}

/// Mods known not to start together though neither jar says so (D-066).
async fn clashes(State(app): State<Arc<App>>) -> Response {
    match app.store.clashes() {
        Ok(list) => cached(json!({ "clashes": list }), 600),
        Err(e) => failed(&e),
    }
}

/// Asks for an older version's files to be kept; open to anyone, counted per
/// address.
async fn fetch_version(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    let Some(id) = our_id(&id) else {
        return refuse(StatusCode::NOT_FOUND, "not_found");
    };
    let state = match app.store.version_state(id) {
        Ok(Some((_, state))) => state,
        Ok(None) => return refuse(StatusCode::NOT_FOUND, "not_found"),
        Err(e) => return failed(&e),
    };
    if state == "all" {
        return private(json!({ "saved": "all", "queued": false }), StatusCode::OK);
    }
    if !app.limiter.allow(
        &client(&headers),
        "fetch",
        FETCHES_PER_HOUR,
        Duration::from_secs(3600),
    ) {
        return refuse(StatusCode::TOO_MANY_REQUESTS, "too_many");
    }
    match import::queue_files(&app, id, None) {
        Ok(queued) => private(
            json!({ "saved": state, "queued": queued }),
            StatusCode::ACCEPTED,
        ),
        Err(e) => failed(&e),
    }
}

/// A modpack version as the launcher installs it: its game version, loader
/// and files at our addresses, and how far the catalog is with keeping them
/// (`none`, `working`, `ready` or `failed` with an `error`). `None` when the
/// version is not a Modrinth modpack.
fn pack_answer(app: &App, id: &str) -> Result<Option<(Value, bool)>, String> {
    let Some(archive) = import::pack_archive(app, id)? else {
        return Ok(None);
    };
    let read = app.store.pack_json(id, &app.urls)?;
    let ready = read
        .as_ref()
        .is_some_and(|p| p["kept"] == p["total"] && archive.sha256.is_some());
    // A pack with a file held back is not installed (D-066, stage 6).
    let held = read.as_ref().is_some_and(|p| {
        p["files"]
            .as_array()
            .is_some_and(|files| files.iter().any(|f| f["held"] == true))
    }) || match &archive.sha256 {
        Some(sha256) => app.store.blob_state(sha256)?.is_some_and(|s| s != "stored"),
        None => false,
    };
    let ready = ready && !held;
    let mut answer = read.unwrap_or_else(|| json!({ "files": [] }));
    let (state, error) = match app.store.job_state(&import::pack_key(id))? {
        _ if held => ("failed", Some("held".to_owned())),
        _ if ready => ("ready", None),
        None => ("none", None),
        Some((state, error)) if state == "failed" => ("failed", error),
        Some((state, _)) if state == "done" => ("none", None),
        Some(_) => ("working", None),
    };
    answer["state"] = state.into();
    answer["error"] = error.into();
    answer["archive"] = json!({
        "name": archive.name,
        "size": archive.size,
        "sha256": archive.sha256,
        "url": match archive.sha256.as_deref().filter(|_| !held) {
            Some(s) => Some(app.urls.served(
                s,
                &archive.name,
                app.store.copy_of(s)?.as_deref() == Some("public"),
            )),
            None => None,
        },
    });
    Ok(Some((answer, ready)))
}

async fn get_pack(State(app): State<Arc<App>>, Path(id): Path<String>) -> Response {
    let Some(id) = our_id(&id) else {
        return refuse(StatusCode::NOT_FOUND, "not_found");
    };
    match pack_answer(&app, id) {
        Ok(Some((answer, true))) => cached(answer, 300),
        Ok(Some((answer, false))) => private(answer, StatusCode::OK),
        Ok(None) => refuse(StatusCode::NOT_FOUND, "not_a_pack"),
        Err(e) => failed(&e),
    }
}

/// Asks for a modpack version's files to be kept, before an install; open to
/// anyone, counted per address with the other fetches.
async fn fetch_pack(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    let Some(id) = our_id(&id) else {
        return refuse(StatusCode::NOT_FOUND, "not_found");
    };
    match pack_answer(&app, id) {
        Ok(Some((answer, true))) => return private(answer, StatusCode::OK),
        Ok(Some(_)) => {}
        Ok(None) => return refuse(StatusCode::NOT_FOUND, "not_a_pack"),
        Err(e) => return failed(&e),
    }
    if !app.limiter.allow(
        &client(&headers),
        "fetch",
        FETCHES_PER_HOUR,
        Duration::from_secs(3600),
    ) {
        return refuse(StatusCode::TOO_MANY_REQUESTS, "too_many");
    }
    if let Err(e) = import::queue_pack(&app, id) {
        return failed(&e);
    }
    match pack_answer(&app, id) {
        Ok(Some((answer, _))) => private(answer, StatusCode::ACCEPTED),
        Ok(None) => refuse(StatusCode::NOT_FOUND, "not_a_pack"),
        Err(e) => failed(&e),
    }
}

#[derive(Deserialize)]
pub struct LookupBody {
    hashes: Vec<String>,
}

/// Which of these files (by SHA-1 or SHA-256) the catalog knows.
async fn lookup(State(app): State<Arc<App>>, Json(body): Json<LookupBody>) -> Response {
    if body.hashes.len() > MAX_LOOKUP
        || !body.hashes.iter().all(|h| {
            (h.len() == 40 || h.len() == 64)
                && h.bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        })
    {
        return refuse(StatusCode::BAD_REQUEST, "bad_hashes");
    }
    match app.store.lookup(&body.hashes) {
        Ok(found) => {
            // Installed: checked for new versions more often.
            let mut seen = std::collections::HashSet::new();
            for project in found.iter().filter_map(|f| f["projectId"].as_str()) {
                if seen.insert(project) {
                    import::wanted(&app, project);
                }
            }
            private(json!({ "found": found }), StatusCode::OK)
        }
        Err(e) => failed(&e),
    }
}

#[derive(Deserialize)]
pub struct ImportBody {
    input: String,
}

/// "Add to the catalog": an address of Modrinth or CurseForge, or a name.
async fn create_import(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Json(body): Json<ImportBody>,
) -> Response {
    let me = match who(&app, &headers) {
        Ok(me) => me,
        Err(response) => return *response,
    };
    let text = body.input.trim();
    if crate::input::parse(text).is_none() {
        return refuse(StatusCode::BAD_REQUEST, "bad_input");
    }
    if !app.limiter.allow(
        &client(&headers),
        "import-address",
        IMPORTS_PER_HOUR_PER_ADDRESS,
        Duration::from_secs(3600),
    ) || !app.limiter.allow(
        &me.sub,
        "import",
        IMPORTS_PER_HOUR,
        Duration::from_secs(3600),
    ) {
        return refuse(StatusCode::TOO_MANY_REQUESTS, "too_many");
    }
    match app.store.account_usage(&me.sub, now() - 86_400) {
        Ok((count, _)) if count >= IMPORTS_PER_DAY => {
            return refuse(StatusCode::TOO_MANY_REQUESTS, "too_many");
        }
        Ok((_, bytes)) if bytes >= IMPORT_BYTES_PER_DAY => {
            return refuse(StatusCode::TOO_MANY_REQUESTS, "too_much_today");
        }
        Ok(_) => {}
        Err(e) => return failed(&e),
    }
    let id = uuid::Uuid::now_v7().to_string();
    let created = app
        .store
        .create_request(&id, &me.sub, text, now())
        .and_then(|()| {
            app.store.enqueue(
                &format!("resolve:{id}"),
                "resolve",
                &json!({ "request": id }),
                Some(&id),
                None,
                now(),
                None,
            )
        })
        .and_then(|_| app.store.request(&id));
    match created {
        Ok(Some((_, request))) => private(json!(request), StatusCode::ACCEPTED),
        Ok(None) => failed("a request just made is not there"),
        Err(e) => failed(&e),
    }
}

/// A request of the asking account; another's is not found.
fn own_request(app: &App, me: &Claims, id: &str) -> Result<crate::store::Request, Box<Response>> {
    let not_found = || Box::new(refuse(StatusCode::NOT_FOUND, "not_found"));
    let id = our_id(id).ok_or_else(not_found)?;
    match app.store.request(id) {
        Ok(Some((account, request))) if account == me.sub => Ok(request),
        Ok(_) => Err(not_found()),
        Err(e) => Err(Box::new(failed(&e))),
    }
}

async fn get_import(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    let me = match who(&app, &headers) {
        Ok(me) => me,
        Err(response) => return *response,
    };
    match own_request(&app, &me, &id) {
        Ok(request) => private(json!(request), StatusCode::OK),
        Err(response) => *response,
    }
}

async fn my_imports(State(app): State<Arc<App>>, headers: HeaderMap) -> Response {
    let me = match who(&app, &headers) {
        Ok(me) => me,
        Err(response) => return *response,
    };
    match app.store.requests_of(&me.sub, 50) {
        Ok(list) => private(json!({ "requests": list }), StatusCode::OK),
        Err(e) => failed(&e),
    }
}

#[derive(Deserialize)]
pub struct ChooseBody {
    source: String,
    id: String,
}

/// The player picks one of the projects a name found.
async fn choose(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<ChooseBody>,
) -> Response {
    let me = match who(&app, &headers) {
        Ok(me) => me,
        Err(response) => return *response,
    };
    let request = match own_request(&app, &me, &id) {
        Ok(request) => request,
        Err(response) => return *response,
    };
    if request.state != "choosing" {
        return refuse(StatusCode::CONFLICT, "not_choosing");
    }
    // Only one of those offered, named as the import names it.
    let offered = request
        .candidates
        .as_ref()
        .and_then(Value::as_array)
        .and_then(|list| {
            list.iter()
                .find(|c| c["source"] == body.source && c["id"] == body.id)
        })
        .map(|c| c["ref"].as_str().unwrap_or(&body.id).to_owned());
    let Some(reference) = offered else {
        return refuse(StatusCode::BAD_REQUEST, "not_offered");
    };
    if let Err(e) = import::start(&app, &request.id, &body.source, &reference, None) {
        return failed(&e.code);
    }
    match app.store.request(&request.id) {
        Ok(Some((_, request))) => private(json!(request), StatusCode::ACCEPTED),
        Ok(None) => refuse(StatusCode::NOT_FOUND, "not_found"),
        Err(e) => failed(&e),
    }
}

/// Own packs published, per account and per address an hour (own.rs has
/// the limits a day); reports likewise.
pub const PUBLISHES_PER_HOUR: u32 = 10;
pub const PUBLISHES_PER_HOUR_PER_ADDRESS: u32 = 20;
pub const REPORTS_PER_HOUR: u32 = 10;
pub const REPORTS_PER_HOUR_PER_ADDRESS: u32 = 30;

#[derive(Deserialize, Default)]
pub struct PublishQuery {
    project: Option<String>,
    /// A room pack (W12): never shown in the catalog.
    #[serde(default)]
    room: bool,
}

/// Writes a request's body to a part file, at most `max` bytes; `None` when
/// it is larger or breaks off.
async fn body_to_part(app: &App, body: axum::body::Body, max: u64) -> Option<std::path::PathBuf> {
    use http_body_util::BodyExt as _;
    use tokio::io::AsyncWriteExt as _;
    let part = app.blobs.part();
    let written = async {
        let mut file = tokio::fs::File::create(&part).await.ok()?;
        let mut body = body;
        let mut total = 0u64;
        while let Some(frame) = body.frame().await {
            let Ok(data) = frame.ok()?.into_data() else {
                continue;
            };
            total += data.len() as u64;
            if total > max {
                return None;
            }
            file.write_all(&data).await.ok()?;
        }
        file.flush().await.ok()?;
        file.sync_all().await.ok()?;
        (total > 0).then_some(())
    }
    .await;
    if written.is_none() {
        let _ = tokio::fs::remove_file(&part).await;
        return None;
    }
    Some(part)
}

/// A player publishes their own pack (own.rs): the archive is the body, and
/// `?project=` makes it a new version of one of their packs.
async fn publish_pack(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Query(q): Query<PublishQuery>,
    body: axum::body::Body,
) -> Response {
    let me = match publisher(&app, &headers) {
        Ok(me) => me,
        Err(response) => return *response,
    };
    let project = match q.project.as_deref().map(our_id) {
        None => None,
        Some(Some(id)) => Some(id.to_owned()),
        Some(None) => return refuse(StatusCode::NOT_FOUND, "not_found"),
    };
    if !app.limiter.allow(
        &client(&headers),
        "publish-address",
        PUBLISHES_PER_HOUR_PER_ADDRESS,
        Duration::from_secs(3600),
    ) || !app.limiter.allow(
        &me.sub,
        "publish",
        PUBLISHES_PER_HOUR,
        Duration::from_secs(3600),
    ) {
        return refuse(StatusCode::TOO_MANY_REQUESTS, "too_many");
    }
    if declared_length(&headers).is_some_and(|n| n > crate::own::MAX_SINGLE) {
        return refuse(StatusCode::PAYLOAD_TOO_LARGE, "too_big");
    }
    let Some(part) = body_to_part(&app, body, crate::own::MAX_SINGLE).await else {
        return refuse(StatusCode::PAYLOAD_TOO_LARGE, "too_big");
    };
    publish_answer(&app, &me, project.as_deref(), q.room, part).await
}

fn declared_length(headers: &HeaderMap) -> Option<u64> {
    headers
        .get(header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok())
}

/// Publishes an archive put together (`part`) and answers with the pack.
async fn publish_answer(
    app: &Arc<App>,
    me: &Claims,
    project: Option<&str>,
    room: bool,
    part: std::path::PathBuf,
) -> Response {
    let author = crate::own::Author {
        account: &me.sub,
        nick: &me.nick,
    };
    match crate::own::publish(app, &author, project, room, part).await {
        Ok((id, version)) => match app.store.project_json(&id, &app.urls) {
            Ok(Some(p)) => private(
                json!({ "project": p, "version": version }),
                StatusCode::CREATED,
            ),
            Ok(None) => failed("a pack just published is not there"),
            Err(e) => failed(&e),
        },
        Err(refusal) => {
            if let crate::own::Refusal::Server(e) = &refusal {
                eprintln!("lapi-mods: publishing a pack: {e}");
            }
            let (status, answer) = crate::own::refusal_json(&refusal);
            private(
                answer,
                StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
            )
        }
    }
}

fn refusal(refusal: &crate::own::Refusal) -> Response {
    if let crate::own::Refusal::Server(e) = refusal {
        eprintln!("lapi-mods: sending a pack in parts: {e}");
    }
    let (status, answer) = crate::own::refusal_json(refusal);
    private(
        answer,
        StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
    )
}

/// Parts of an archive sent, per account an hour (a 512 MiB archive is 8).
pub const PARTS_PER_HOUR: u32 = 100;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UploadBody {
    size: u64,
    sha256: String,
}

/// Starts an archive sent in parts (`?project=` for a new version): the
/// player sends its size and SHA-256 and gets where to send the parts.
async fn start_upload(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Query(q): Query<PublishQuery>,
    Json(body): Json<UploadBody>,
) -> Response {
    let me = match publisher(&app, &headers) {
        Ok(me) => me,
        Err(response) => return *response,
    };
    let project = match q.project.as_deref().map(our_id) {
        None => None,
        Some(Some(id)) => Some(id.to_owned()),
        Some(None) => return refuse(StatusCode::NOT_FOUND, "not_found"),
    };
    if !app.limiter.allow(
        &client(&headers),
        "publish-address",
        PUBLISHES_PER_HOUR_PER_ADDRESS,
        Duration::from_secs(3600),
    ) || !app.limiter.allow(
        &me.sub,
        "publish",
        PUBLISHES_PER_HOUR,
        Duration::from_secs(3600),
    ) {
        return refuse(StatusCode::TOO_MANY_REQUESTS, "too_many");
    }
    match app.uploads.start(
        &app,
        &me.sub,
        &client(&headers),
        project.as_deref(),
        q.room,
        body.size,
        &body.sha256,
    ) {
        Ok((id, parts)) => private(
            json!({ "upload": id, "parts": parts, "partSize": crate::own::CHUNK }),
            StatusCode::CREATED,
        ),
        Err(e) => refusal(&e),
    }
}

/// One part of an archive sent in parts; `n` counts from 0.
async fn put_part(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Path((id, n)): Path<(String, usize)>,
    body: axum::body::Body,
) -> Response {
    let me = match publisher(&app, &headers) {
        Ok(me) => me,
        Err(response) => return *response,
    };
    let Some(id) = our_id(&id) else {
        return refuse(StatusCode::NOT_FOUND, "not_found");
    };
    if !app
        .limiter
        .allow(&me.sub, "part", PARTS_PER_HOUR, Duration::from_secs(3600))
    {
        return refuse(StatusCode::TOO_MANY_REQUESTS, "too_many");
    }
    let expected = match app.uploads.expected(&me.sub, id, n) {
        Ok(len) => len,
        Err(e) => return refusal(&e),
    };
    if declared_length(&headers).is_some_and(|len| len != expected) {
        return refuse(StatusCode::BAD_REQUEST, "bad_part");
    }
    let Some(file) = body_to_part(&app, body, expected).await else {
        return refuse(StatusCode::BAD_REQUEST, "bad_part");
    };
    match app.uploads.put(&me.sub, id, n, file) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => refusal(&e),
    }
}

/// Puts an archive sent in parts together and publishes it.
async fn finish_upload(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    let me = match publisher(&app, &headers) {
        Ok(me) => me,
        Err(response) => return *response,
    };
    let Some(id) = our_id(&id).map(str::to_owned) else {
        return refuse(StatusCode::NOT_FOUND, "not_found");
    };
    let assembled = {
        let (app, account) = (Arc::clone(&app), me.sub.clone());
        tokio::task::spawn_blocking(move || app.uploads.assemble(&app, &account, &id)).await
    };
    match assembled {
        Ok(Ok(done)) => {
            publish_answer(&app, &me, done.project.as_deref(), done.room, done.part).await
        }
        Ok(Err(e)) => refusal(&e),
        Err(_) => failed("putting an upload together stopped"),
    }
}

/// The asking account's own packs.
async fn my_packs(State(app): State<Arc<App>>, headers: HeaderMap) -> Response {
    let me = match who(&app, &headers) {
        Ok(me) => me,
        Err(response) => return *response,
    };
    match app.store.own_packs(&me.sub) {
        Ok(packs) => private(json!({ "packs": packs }), StatusCode::OK),
        Err(e) => failed(&e),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HiddenBody {
    hidden: bool,
}

/// The author hides their pack from the catalog, or shows it again.
async fn set_hidden(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<HiddenBody>,
) -> Response {
    let me = match who(&app, &headers) {
        Ok(me) => me,
        Err(response) => return *response,
    };
    let Some(id) = our_id(&id) else {
        return refuse(StatusCode::NOT_FOUND, "not_found");
    };
    match app.store.own_account(id) {
        Ok(Some(account)) if account == me.sub => {}
        Ok(_) => return refuse(StatusCode::NOT_FOUND, "not_found"),
        Err(e) => return failed(&e),
    }
    match app.store.set_own_hidden(id, body.hidden) {
        Ok(_) => match app.store.project_status(id) {
            Ok(status) => private(json!({ "status": status }), StatusCode::OK),
            Err(e) => failed(&e),
        },
        Err(e) => failed(&e),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReportBody {
    reason: String,
    #[serde(default)]
    text: String,
}

/// A player reports a player's own pack to the owner.
async fn report(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<ReportBody>,
) -> Response {
    let me = match who(&app, &headers) {
        Ok(me) => me,
        Err(response) => return *response,
    };
    let Some(id) = our_id(&id) else {
        return refuse(StatusCode::NOT_FOUND, "not_found");
    };
    if !crate::own::REASONS.contains(&body.reason.as_str())
        || body.text.chars().count() > crate::own::MAX_REPORT
    {
        return refuse(StatusCode::BAD_REQUEST, "bad_report");
    }
    if !app.limiter.allow(
        &client(&headers),
        "report-address",
        REPORTS_PER_HOUR_PER_ADDRESS,
        Duration::from_secs(3600),
    ) || !app.limiter.allow(
        &me.sub,
        "report",
        REPORTS_PER_HOUR,
        Duration::from_secs(3600),
    ) {
        return refuse(StatusCode::TOO_MANY_REQUESTS, "too_many");
    }
    let own = app
        .store
        .own_account(id)
        .and_then(|a| Ok((a, app.store.project_status(id)?)));
    match own {
        Ok((Some(_), Some(status))) if status != "removed" => {}
        Ok(_) => return refuse(StatusCode::NOT_FOUND, "not_found"),
        Err(e) => return failed(&e),
    }
    let text = crate::model::clean(&body.text, crate::own::MAX_REPORT, false);
    match app
        .store
        .add_report(id, &me.sub, &body.reason, text.trim(), now())
    {
        Ok(_) => private(json!({ "reported": true }), StatusCode::ACCEPTED),
        Err(e) => failed(&e),
    }
}

/// Asks the account service whether it sends letters, for the email rule.
pub async fn check_mail(app: &App, http: &reqwest::Client, accounts: &str) {
    if app.email.mode != crate::auth::EmailMode::Auto {
        return;
    }
    match crate::auth::mail_sent(http, accounts).await {
        Ok(sent) => app.email.set_mail_sent(sent),
        Err(e) => eprintln!("lapi-mods: asking whether letters are sent failed: {e}"),
    }
}

/// Exports an account asks for an hour.
const EXPORTS_PER_HOUR: u32 = 10;

/// Everything the catalog keeps about the asking account (D-064): its
/// requests to add to the catalog, its packs with their versions, and its
/// reports.
async fn export(State(app): State<Arc<App>>, headers: HeaderMap) -> Response {
    let me = match who(&app, &headers) {
        Ok(me) => me,
        Err(response) => return *response,
    };
    if !app.limiter.allow(
        &me.sub,
        "export",
        EXPORTS_PER_HOUR,
        Duration::from_secs(3600),
    ) {
        return refuse(StatusCode::TOO_MANY_REQUESTS, "too_many");
    }
    match app.store.export_account(&me.sub) {
        Ok(data) => private(data, StatusCode::OK),
        Err(e) => failed(&e),
    }
}

/// Asks the account service which recently seen sessions ended and refuses
/// their tokens from now on. When it cannot be asked, tokens hold until they
/// run out.
pub async fn check_sessions(app: &App, http: &reqwest::Client, accounts: &str) {
    let sessions = app.sessions.recent();
    for chunk in sessions.chunks(500) {
        match crate::auth::ended(http, accounts, chunk).await {
            Ok(ended) => app.sessions.end(&ended),
            Err(e) => {
                eprintln!("lapi-mods: asking which sessions ended failed: {e}");
                return;
            }
        }
    }
}

/// Cleans out accounts the account service says were deleted after
/// `since`; returns the time to ask from next.
pub async fn purge_deleted(
    app: &Arc<App>,
    http: &reqwest::Client,
    accounts: &str,
    since: i64,
) -> i64 {
    let deleted = match crate::auth::deleted(http, accounts, since).await {
        Ok(deleted) => deleted,
        Err(e) => {
            eprintln!("lapi-mods: asking which accounts were deleted failed: {e}");
            return since;
        }
    };
    let app = Arc::clone(app);
    tokio::task::spawn_blocking(move || purge(&app, &deleted, since))
        .await
        .unwrap_or(since)
}

/// Refuses the tokens of deleted accounts, then cleans them out (D-064):
/// their requests and reports go, and their own packs stay without the
/// author's name or account (blocking).
pub fn purge(app: &App, deleted: &[(String, i64)], since: i64) -> i64 {
    let now = now();
    let lately: Vec<String> = deleted
        .iter()
        .filter(|(_, at)| now - at < crate::auth::REMEMBER.as_secs().cast_signed())
        .map(|(account, _)| account.clone())
        .collect();
    app.sessions.forget(&lately);
    let mut newest = since;
    for (account, at) in deleted {
        if let Err(e) = app.store.purge_account(account) {
            eprintln!("lapi-mods: cleaning out a deleted account failed: {e}");
            return newest;
        }
        newest = newest.max(*at);
    }
    newest
}

pub fn router(app: Arc<App>) -> Router {
    Router::new()
        .route("/v1/health", get(health))
        .route("/v1/search", get(search))
        .route("/v1/projects/{project}", get(project))
        .route("/v1/projects/{project}/versions", get(versions))
        .route("/v1/projects/{project}/similar", get(similar))
        .route("/v1/categories", get(category_counts))
        .route("/v1/collections", get(collections))
        .route("/v1/versions/{version}", get(version))
        .route("/v1/versions/{version}/fetch", post(fetch_version))
        .route("/v1/provides/{mod}", get(provides))
        .route("/v1/clashes", get(clashes))
        .route(
            "/v1/versions/{version}/pack",
            get(get_pack).post(fetch_pack),
        )
        .route("/v1/lookup", post(lookup))
        .route("/v1/harmful", get(harmful))
        .route("/v1/imports", get(my_imports).post(create_import))
        .route("/v1/imports/{request}", get(get_import))
        .route("/v1/imports/{request}/choose", post(choose))
        .route("/v1/packs", post(publish_pack))
        .route("/v1/packs/uploads", post(start_upload))
        .route("/v1/packs/uploads/{upload}/{part}", put(put_part))
        .route("/v1/packs/uploads/{upload}/finish", post(finish_upload))
        .route("/v1/packs/mine", get(my_packs))
        .route("/v1/packs/{project}/hidden", post(set_hidden))
        .route("/v1/projects/{project}/report", post(report))
        .route("/v1/export", get(export))
        .layer(DefaultBodyLimit::max(32 * 1024))
        .with_state(app)
}
