//! The skins API (D-065). Lookups by nick and the textures themselves are
//! public; changes carry the account service's access token, and the account
//! is its `sub`, never anything the request says. Each account has a
//! wardrobe of skins, capes and sets to wear again. Nothing about a player (a
//! nick, an address, a name in a wardrobe) is ever written to the log.

use std::collections::{BTreeMap, HashMap};
use std::net::IpAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::extract::rejection::{JsonRejection, PathRejection};
use axum::extract::{DefaultBodyLimit, Path, Query, Request, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, Uri, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, patch, post, put};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::auth::{Claims, Sessions, Tokens};
use crate::directory::{self, Directory};
use crate::store::{self, Change, Item, Kept, Refused, Set, Store, Worn};
use crate::texture::{self, Kind};

/// Nicks in one `POST /v1/profiles`.
pub const MAX_NICKS: usize = 50;
/// Uploads per account an hour.
pub const UPLOADS_PER_HOUR: u32 = 10;
/// Wardrobe items and sets put on per account an hour (no upload counted).
pub const WEARS_PER_HOUR: u32 = 120;
/// Other changes to a wardrobe per account an hour.
pub const CHANGES_PER_HOUR: u32 = 600;
/// The longest name of a wardrobe item or set, in characters.
pub const NAME_CHARS: usize = 32;
/// Requests per address a minute (an IPv6 /64 counts as one), besides
/// nginx's own limit.
pub const READS_PER_MINUTE: u32 = 600;
/// Uploads decoded at once: each may hold up to 32 MiB of pixels.
pub const DECODING_AT_ONCE: usize = 2;
/// Longest query of an upload (32 frame durations fit easily), and of one
/// to the wardrobe (with a name of 32 characters, percent-encoded).
const QUERY_MAX: usize = 512;
const ADD_QUERY_MAX: usize = 1024;
/// Most of an upload's body ever read: more than nginx lets through (3 MiB),
/// less than would matter.
const READ_MAX: usize = 4 * 1024 * 1024;

const CACHE_PROFILE: &str = "public, max-age=60";
const CACHE_TEXTURE: &str = "public, max-age=31536000, immutable";

/// Actions counted since the start of their window, per key and kind.
type Windows = HashMap<(String, &'static str), (Instant, u32)>;

/// Actions counted per account (or address) in a window.
#[derive(Default)]
pub struct Limiter {
    windows: Mutex<Windows>,
}

impl Limiter {
    pub fn allow(&self, key: &str, bucket: &'static str, limit: u32, window: Duration) -> bool {
        self.allow_n(key, bucket, 1, limit, window)
    }

    /// Counts `n` actions at once.
    pub fn allow_n(
        &self,
        key: &str,
        bucket: &'static str,
        n: u32,
        limit: u32,
        window: Duration,
    ) -> bool {
        let mut windows = self
            .windows
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let now = Instant::now();
        if windows.len() > 200_000 {
            windows.retain(|_, (start, _)| now.duration_since(*start) < Duration::from_secs(3600));
            // Very many addresses within the hour: start over rather than
            // grow without end.
            if windows.len() > 1_000_000 {
                windows.clear();
            }
        }
        let entry = windows.entry((key.to_owned(), bucket)).or_insert((now, 0));
        if now.duration_since(entry.0) >= window {
            *entry = (now, 0);
        }
        entry.1 = entry.1.saturating_add(n);
        entry.1 <= limit
    }

    /// What is left of a limit, without counting anything: how many, and the
    /// seconds until the window starts over (0 when nothing was counted).
    pub fn left(
        &self,
        key: &str,
        bucket: &'static str,
        limit: u32,
        window: Duration,
    ) -> (u32, u64) {
        let windows = self
            .windows
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match windows.get(&(key.to_owned(), bucket)) {
            Some((start, n)) if start.elapsed() < window => (
                limit.saturating_sub(*n),
                (window - start.elapsed()).as_secs() + 1,
            ),
            _ => (limit, 0),
        }
    }
}

pub struct App {
    pub store: Store,
    pub tokens: Tokens,
    pub directory: Directory,
    pub sessions: Sessions,
    /// Accounts deleted up to this time were cleaned out already.
    pub deleted_since: Mutex<i64>,
    pub limiter: Limiter,
    /// Where textures are fetched from (`https://skins.lapi.sh`).
    pub public: String,
    /// Uploads being decoded.
    pub decoding: Arc<tokio::sync::Semaphore>,
}

impl App {
    pub fn new(store: Store, tokens: Tokens, directory: Directory, public: &str) -> Self {
        Self {
            store,
            tokens,
            directory,
            sessions: Sessions::default(),
            deleted_since: Mutex::new(0),
            limiter: Limiter::default(),
            public: public.trim_end_matches('/').to_owned(),
            decoding: Arc::new(tokio::sync::Semaphore::new(DECODING_AT_ONCE)),
        }
    }
}

pub fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

fn refuse(status: StatusCode, code: &str) -> Response {
    (status, Json(json!({ "error": code }))).into_response()
}

fn failed(e: &str) -> Response {
    eprintln!("skins: {e}");
    refuse(StatusCode::INTERNAL_SERVER_ERROR, "server_error")
}

fn cached(mut response: Response, policy: &'static str) -> Response {
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static(policy));
    response
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
    // Signed out, or the account was switched off or deleted.
    if app.sessions.ended(&claims.sid) || app.sessions.gone(&claims.sub) {
        return Err(unauthorized());
    }
    app.sessions.seen(&claims.sid);
    Ok(claims)
}

macro_rules! me {
    ($app:expr, $headers:expr) => {
        match who(&$app, &$headers) {
            Ok(claims) => claims,
            Err(response) => return *response,
        }
    };
}

/// One worn texture as the mod and the launcher read it.
fn texture_json(app: &App, worn: &Worn) -> Value {
    let mut out = json!({
        "hash": worn.hash,
        "width": worn.width,
        "height": worn.height,
        "frames": worn.frames,
        "frame_ms": worn.frame_ms,
        "url": format!("{}/v1/textures/{}", app.public, worn.hash),
    });
    if let Some(model) = &worn.model {
        out["model"] = json!(model);
    }
    out
}

/// `{"nick", "skin", "cape"}` of an account; none when it wears nothing.
fn profile(app: &App, account: &str, nick: &str, always: bool) -> Result<Option<Value>, String> {
    let worn = app.store.worn(account)?;
    if worn.is_empty() && !always {
        return Ok(None);
    }
    let of = |kind: Kind| {
        worn.iter()
            .find(|w| w.kind == kind)
            .map_or(Value::Null, |w| texture_json(app, w))
    };
    Ok(Some(json!({
        "nick": nick,
        "skin": of(Kind::Skin),
        "cape": of(Kind::Cape),
    })))
}

/// The profile behind a nick, if the nick is one and its account wears
/// something. A nick that cannot be one never reaches the account service.
async fn lookup(app: &App, nick: &str) -> Result<Option<Value>, String> {
    if !directory::nick(nick) {
        return Ok(None);
    }
    let Some((account, current)) = app.directory.find(nick).await? else {
        return Ok(None);
    };
    profile(app, &account, &current, false)
}

async fn get_profile(
    State(app): State<Arc<App>>,
    nick: Result<Path<String>, PathRejection>,
) -> Response {
    let Ok(Path(nick)) = nick else {
        return cached(refuse(StatusCode::NOT_FOUND, "not_found"), CACHE_PROFILE);
    };
    match lookup(&app, &nick).await {
        Ok(Some(found)) => cached(Json(found).into_response(), CACHE_PROFILE),
        Ok(None) => cached(refuse(StatusCode::NOT_FOUND, "not_found"), CACHE_PROFILE),
        Err(e) => unavailable(&e),
    }
}

/// The account service did not answer: try again soon, keep nothing.
fn unavailable(e: &str) -> Response {
    eprintln!("asking the account service failed: {e}");
    refuse(StatusCode::SERVICE_UNAVAILABLE, "unavailable")
}

#[derive(Deserialize)]
struct ProfilesBody {
    nicks: Vec<String>,
}

async fn post_profiles(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    body: Result<Json<ProfilesBody>, JsonRejection>,
) -> Response {
    let body = match body {
        Ok(Json(body)) => body,
        Err(rejection) => return refuse(rejection.status(), "bad_request"),
    };
    if body.nicks.len() > MAX_NICKS {
        return refuse(StatusCode::PAYLOAD_TOO_LARGE, "too_many");
    }
    // A long list weighs more than one request (counted once already).
    let extra = u32::try_from(body.nicks.len() / 10).unwrap_or(u32::MAX);
    if !app.limiter.allow_n(
        &address(&headers),
        "read",
        extra,
        READS_PER_MINUTE,
        Duration::from_secs(60),
    ) {
        return refuse(StatusCode::TOO_MANY_REQUESTS, "rate_limited");
    }
    let mut profiles = BTreeMap::new();
    for nick in body.nicks {
        if profiles.contains_key(&nick) {
            continue;
        }
        match lookup(&app, &nick).await {
            Ok(found) => {
                profiles.insert(nick, found.unwrap_or(Value::Null));
            }
            Err(e) => return unavailable(&e),
        }
    }
    Json(json!({ "profiles": profiles })).into_response()
}

async fn get_texture(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    hash: Result<Path<String>, PathRejection>,
) -> Response {
    let not_found = || refuse(StatusCode::NOT_FOUND, "not_found");
    let Ok(Path(hash)) = hash else {
        return not_found();
    };
    if !texture::is_hash(&hash) {
        return not_found();
    }
    let png = match app.store.texture(&hash) {
        Ok(Some(png)) => png,
        Ok(None) => return not_found(),
        Err(e) => return failed(&e),
    };
    let etag = format!("\"{hash}\"");
    let matches = headers
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.split(',').any(|t| t.trim() == etag || t.trim() == "*"));
    let mut response = if matches {
        StatusCode::NOT_MODIFIED.into_response()
    } else {
        (
            [(header::CONTENT_TYPE, HeaderValue::from_static("image/png"))],
            png,
        )
            .into_response()
    };
    let out = response.headers_mut();
    out.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static(CACHE_TEXTURE),
    );
    out.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    if let Ok(value) = HeaderValue::from_str(&etag) {
        out.insert(header::ETAG, value);
    }
    response
}

/// The account's own profile, also when it wears nothing, with the uploads
/// it has left this hour.
fn mine_json(app: &App, me: &Claims) -> Result<Value, String> {
    let mut out = profile(app, &me.sub, &me.nick, true)?.unwrap_or_default();
    let (left, reset) = app.limiter.left(
        &me.sub,
        "upload",
        UPLOADS_PER_HOUR,
        Duration::from_secs(3600),
    );
    out["uploads"] = json!({ "left": left, "reset": reset });
    Ok(out)
}

fn mine(app: &App, me: &Claims) -> Response {
    match mine_json(app, me) {
        Ok(found) => Json(found).into_response(),
        Err(e) => failed(&e),
    }
}

/// An item of the wardrobe: its texture as a worn one reads, and its own
/// fields.
fn item_json(app: &App, item: &Item) -> Value {
    let mut out = texture_json(app, &item.texture);
    out["id"] = json!(item.id);
    out["kind"] = json!(item.texture.kind.as_str());
    out["name"] = json!(item.name);
    out["favourite"] = json!(item.favourite);
    out["created"] = json!(item.created);
    out["worn"] = json!(item.worn);
    out
}

fn set_json(app: &App, set: &Set) -> Value {
    let look = |w: &Option<Worn>| w.as_ref().map_or(Value::Null, |w| texture_json(app, w));
    json!({
        "id": set.id,
        "name": set.name,
        "favourite": set.favourite,
        "created": set.created,
        "worn": set.worn,
        "skin": look(&set.skin),
        "cape": look(&set.cape),
    })
}

/// The account's profile with its wardrobe: items, sets and their limits.
fn wardrobe(app: &App, me: &Claims) -> Response {
    let document = || -> Result<Value, String> {
        let mut out = mine_json(app, me)?;
        let items: Vec<Value> = app
            .store
            .items(&me.sub)?
            .iter()
            .map(|i| item_json(app, i))
            .collect();
        let sets: Vec<Value> = app
            .store
            .sets(&me.sub)?
            .iter()
            .map(|s| set_json(app, s))
            .collect();
        out["items"] = json!(items);
        out["sets"] = json!(sets);
        out["limits"] = json!({
            "skins": store::MAX_SKINS,
            "capes": store::MAX_CAPES,
            "sets": store::MAX_SETS,
        });
        Ok(out)
    };
    match document() {
        Ok(found) => Json(found).into_response(),
        Err(e) => failed(&e),
    }
}

async fn get_me(State(app): State<Arc<App>>, headers: HeaderMap) -> Response {
    let me = me!(app, headers);
    mine(&app, &me)
}

fn kind_of(kind: Result<Path<String>, PathRejection>) -> Option<Kind> {
    kind.ok().and_then(|Path(kind)| Kind::parse(&kind))
}

/// What was not read of an upload is read to its end and dropped (nginx
/// lets 3 MiB through at most), so a refusal reaches the launcher instead of
/// a connection closed under nginx.
async fn drain(body: Option<Body>) {
    if let Some(rest) = body {
        let _ = axum::body::to_bytes(rest, READ_MAX).await;
    }
}

/// `PUT /v1/me/skin|cape`: the PNG is the body (`Content-Type: image/png`),
/// the rest is in the query.
async fn upload(
    State(app): State<Arc<App>>,
    kind: Result<Path<String>, PathRejection>,
    uri: Uri,
    headers: HeaderMap,
    body: Body,
) -> Response {
    let mut body = Some(body);
    let response = wear(&app, kind, &uri, &headers, &mut body).await;
    drain(body).await;
    response
}

async fn wear(
    app: &App,
    kind: Result<Path<String>, PathRejection>,
    uri: &Uri,
    headers: &HeaderMap,
    body: &mut Option<Body>,
) -> Response {
    let Some(kind) = kind_of(kind) else {
        return refuse(StatusCode::NOT_FOUND, "not_found");
    };
    let me = me!(app, headers);
    let texture = match received(app, &me, kind, uri, headers, body, QUERY_MAX).await {
        Ok(texture) => texture,
        Err(response) => return *response,
    };
    if let Err(e) = app.store.wear(&me.sub, &texture, now()) {
        return failed(&e);
    }
    mine(app, &me)
}

/// `POST /v1/me/skin|cape`: an upload kept in the wardrobe without being
/// worn; its name (optional) is in the query with the rest. It shares the
/// address of `PUT`, so nginx gives both the same body limit.
async fn add_upload(
    State(app): State<Arc<App>>,
    kind: Result<Path<String>, PathRejection>,
    uri: Uri,
    headers: HeaderMap,
    body: Body,
) -> Response {
    let mut body = Some(body);
    let response = add(&app, kind, &uri, &headers, &mut body).await;
    drain(body).await;
    response
}

async fn add(
    app: &App,
    kind: Result<Path<String>, PathRejection>,
    uri: &Uri,
    headers: &HeaderMap,
    body: &mut Option<Body>,
) -> Response {
    let Some(kind) = kind_of(kind) else {
        return refuse(StatusCode::NOT_FOUND, "not_found");
    };
    let me = me!(app, headers);
    if uri.query().map_or(0, str::len) > ADD_QUERY_MAX {
        return refuse(StatusCode::BAD_REQUEST, "bad_query");
    }
    let Ok(Query(pairs)) = Query::<Vec<(String, String)>>::try_from_uri(uri) else {
        return refuse(StatusCode::BAD_REQUEST, "bad_query");
    };
    let mut names = pairs.iter().filter(|(k, _)| k == "name").map(|(_, v)| v);
    let name = match (names.next(), names.next()) {
        (None, _) => String::new(),
        (Some(name), None) => match wardrobe_name(name) {
            Some(name) => name,
            None => return refuse(StatusCode::BAD_REQUEST, "bad_name"),
        },
        (Some(_), Some(_)) => return refuse(StatusCode::BAD_REQUEST, "bad_name"),
    };
    if !app.limiter.allow(
        &me.sub,
        "wardrobe",
        CHANGES_PER_HOUR,
        Duration::from_secs(3600),
    ) {
        return refuse(StatusCode::TOO_MANY_REQUESTS, "rate_limited");
    }
    // A full wardrobe takes nothing, and costs no upload.
    let max = match kind {
        Kind::Skin => store::MAX_SKINS,
        Kind::Cape => store::MAX_CAPES,
    };
    match app.store.items_of(&me.sub, kind) {
        Ok(count) if count >= max => return refuse(StatusCode::CONFLICT, "wardrobe_full"),
        Ok(_) => {}
        Err(e) => return failed(&e),
    }
    let texture = match received(app, &me, kind, uri, headers, body, ADD_QUERY_MAX).await {
        Ok(texture) => texture,
        Err(response) => return *response,
    };
    match app.store.add_item(&me.sub, &texture, &name, now()) {
        Ok(Kept::Yes(_)) => wardrobe(app, &me),
        Ok(Kept::No(refused)) => refused_by(refused),
        Err(e) => failed(&e),
    }
}

fn refused_by(refused: Refused) -> Response {
    match refused {
        Refused::Full => refuse(StatusCode::CONFLICT, "wardrobe_full"),
        Refused::NothingWorn => refuse(StatusCode::CONFLICT, "nothing_worn"),
    }
}

/// Reads an upload's body and decodes it into one of our own PNGs. The
/// account's hourly uploads count it first: nothing is decoded past an
/// exhausted limit.
async fn received(
    app: &App,
    me: &Claims,
    kind: Kind,
    uri: &Uri,
    headers: &HeaderMap,
    body: &mut Option<Body>,
    query_max: usize,
) -> Result<texture::Texture, Box<Response>> {
    let refuse = |status, code| Err(Box::new(refuse(status, code)));
    let failed = |e| Err(Box::new(failed(e)));
    if !app.limiter.allow(
        &me.sub,
        "upload",
        UPLOADS_PER_HOUR,
        Duration::from_secs(3600),
    ) {
        return refuse(StatusCode::TOO_MANY_REQUESTS, "rate_limited");
    }
    if uri.query().map_or(0, str::len) > query_max {
        return refuse(StatusCode::BAD_REQUEST, "bad_query");
    }
    let options = match Query::<Vec<(String, String)>>::try_from_uri(uri) {
        Ok(Query(pairs)) => match texture::options(&pairs) {
            Ok(options) => options,
            Err(refusal) => return refuse(StatusCode::BAD_REQUEST, refusal.code()),
        },
        Err(_) => return refuse(StatusCode::BAD_REQUEST, "bad_query"),
    };
    let png_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.trim().eq_ignore_ascii_case("image/png"));
    if !png_type {
        return refuse(StatusCode::BAD_REQUEST, texture::Refusal::BadPng.code());
    }
    let max = kind.max_bytes();
    let too_large = || refuse(StatusCode::PAYLOAD_TOO_LARGE, "too_large");
    let declared = headers
        .get(header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok());
    if declared.is_some_and(|n| n > max as u64) {
        return too_large();
    }
    let Some(body) = body.take() else {
        return failed("the body was read already");
    };
    let bytes = match axum::body::to_bytes(body, READ_MAX).await {
        Ok(bytes) if bytes.len() <= max => bytes,
        _ => return too_large(),
    };
    let Ok(permit) = Arc::clone(&app.decoding).acquire_owned().await else {
        return failed("the decoding queue closed");
    };
    // The permit goes with the work, so it holds also if the launcher hangs
    // up meanwhile.
    let processed = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        texture::process(kind, &bytes, &options)
    })
    .await;
    match processed {
        Ok(Ok(texture)) => Ok(texture),
        Ok(Err(refusal)) => {
            let status = if refusal == texture::Refusal::TooLarge {
                StatusCode::PAYLOAD_TOO_LARGE
            } else {
                StatusCode::BAD_REQUEST
            };
            refuse(status, refusal.code())
        }
        Err(_) => failed("decoding an upload stopped"),
    }
}

/// A name for a wardrobe item or set: at most 32 characters with the spaces
/// at its ends dropped, no control or bidi formatting characters (which
/// could make it read as something else). Empty is a name too: the launcher
/// shows its own.
fn wardrobe_name(text: &str) -> Option<String> {
    let name = text.trim();
    let plain = name.chars().all(|c| {
        !c.is_control()
            && !matches!(
                c,
                '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{FEFF}'
            )
    });
    (plain && name.chars().count() <= NAME_CHARS).then(|| name.to_owned())
}

/// An item's or set's ID: 32 lowercase hex digits.
fn wardrobe_id(id: Result<Path<String>, PathRejection>) -> Option<String> {
    let Path(id) = id.ok()?;
    (id.len() == 32 && id.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))).then_some(id)
}

/// Counts a change against the account's hourly limit of its bucket.
fn allowed(app: &App, me: &Claims, bucket: &'static str, limit: u32) -> bool {
    app.limiter
        .allow(&me.sub, bucket, limit, Duration::from_secs(3600))
}

/// `GET /v1/me/wardrobe`.
async fn get_wardrobe(State(app): State<Arc<App>>, headers: HeaderMap) -> Response {
    let me = me!(app, headers);
    wardrobe(&app, &me)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SaveBody {
    kind: Option<String>,
    #[serde(default)]
    name: String,
}

/// `POST /v1/me/wardrobe/save` `{"kind", "name"}`: keeps what is worn of the
/// kind in the wardrobe (no upload counted).
async fn save_worn(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    body: Result<Json<SaveBody>, JsonRejection>,
) -> Response {
    let me = me!(app, headers);
    let body = match body {
        Ok(Json(body)) => body,
        Err(rejection) => return refuse(rejection.status(), "bad_request"),
    };
    let Some(kind) = body.kind.as_deref().and_then(Kind::parse) else {
        return refuse(StatusCode::BAD_REQUEST, "bad_request");
    };
    let Some(name) = wardrobe_name(&body.name) else {
        return refuse(StatusCode::BAD_REQUEST, "bad_name");
    };
    if !allowed(&app, &me, "wardrobe", CHANGES_PER_HOUR) {
        return refuse(StatusCode::TOO_MANY_REQUESTS, "rate_limited");
    }
    match app.store.save_worn(&me.sub, kind, &name, now()) {
        Ok(Kept::Yes(_)) => wardrobe(&app, &me),
        Ok(Kept::No(refused)) => refused_by(refused),
        Err(e) => failed(&e),
    }
}

/// `POST /v1/me/wardrobe/sets` `{"name"}`: keeps what is worn, skin and
/// cape, as a set.
async fn save_set(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    body: Result<Json<SaveBody>, JsonRejection>,
) -> Response {
    let me = me!(app, headers);
    let body = match body {
        Ok(Json(body)) => body,
        Err(rejection) => return refuse(rejection.status(), "bad_request"),
    };
    if body.kind.is_some() {
        return refuse(StatusCode::BAD_REQUEST, "bad_request");
    }
    let Some(name) = wardrobe_name(&body.name) else {
        return refuse(StatusCode::BAD_REQUEST, "bad_name");
    };
    if !allowed(&app, &me, "wardrobe", CHANGES_PER_HOUR) {
        return refuse(StatusCode::TOO_MANY_REQUESTS, "rate_limited");
    }
    match app.store.save_set(&me.sub, &name, now()) {
        Ok(Kept::Yes(_)) => wardrobe(&app, &me),
        Ok(Kept::No(refused)) => refused_by(refused),
        Err(e) => failed(&e),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ChangeBody {
    name: Option<String>,
    favourite: Option<bool>,
}

/// Which of the account's items or sets a request names.
#[derive(Clone, Copy)]
enum Of {
    Items,
    Sets,
}

/// `PATCH /v1/me/wardrobe/items|sets/{id}` `{"name"?, "favourite"?}`.
async fn change(
    app: &App,
    of: Of,
    id: Result<Path<String>, PathRejection>,
    headers: &HeaderMap,
    body: Result<Json<ChangeBody>, JsonRejection>,
) -> Response {
    let me = me!(app, headers);
    let Some(id) = wardrobe_id(id) else {
        return refuse(StatusCode::NOT_FOUND, "not_found");
    };
    let body = match body {
        Ok(Json(body)) => body,
        Err(rejection) => return refuse(rejection.status(), "bad_request"),
    };
    let name = match body.name.as_deref().map(wardrobe_name) {
        None => None,
        Some(Some(name)) => Some(name),
        Some(None) => return refuse(StatusCode::BAD_REQUEST, "bad_name"),
    };
    if !allowed(app, &me, "wardrobe", CHANGES_PER_HOUR) {
        return refuse(StatusCode::TOO_MANY_REQUESTS, "rate_limited");
    }
    let change = Change {
        name: name.as_deref(),
        favourite: body.favourite,
    };
    let changed = match of {
        Of::Items => app.store.change_item(&me.sub, &id, &change),
        Of::Sets => app.store.change_set(&me.sub, &id, &change),
    };
    match changed {
        Ok(true) => wardrobe(app, &me),
        Ok(false) => refuse(StatusCode::NOT_FOUND, "not_found"),
        Err(e) => failed(&e),
    }
}

/// `DELETE /v1/me/wardrobe/items|sets/{id}`; what is worn stays on.
async fn delete(
    app: &App,
    of: Of,
    id: Result<Path<String>, PathRejection>,
    headers: &HeaderMap,
) -> Response {
    let me = me!(app, headers);
    let Some(id) = wardrobe_id(id) else {
        return refuse(StatusCode::NOT_FOUND, "not_found");
    };
    if !allowed(app, &me, "wardrobe", CHANGES_PER_HOUR) {
        return refuse(StatusCode::TOO_MANY_REQUESTS, "rate_limited");
    }
    let deleted = match of {
        Of::Items => app.store.delete_item(&me.sub, &id),
        Of::Sets => app.store.delete_set(&me.sub, &id),
    };
    match deleted {
        Ok(true) => wardrobe(app, &me),
        Ok(false) => refuse(StatusCode::NOT_FOUND, "not_found"),
        Err(e) => failed(&e),
    }
}

/// `POST /v1/me/wardrobe/items|sets/{id}/wear`: no upload counted.
async fn put_on(
    app: &App,
    of: Of,
    id: Result<Path<String>, PathRejection>,
    headers: &HeaderMap,
) -> Response {
    let me = me!(app, headers);
    let Some(id) = wardrobe_id(id) else {
        return refuse(StatusCode::NOT_FOUND, "not_found");
    };
    if !allowed(app, &me, "wear", WEARS_PER_HOUR) {
        return refuse(StatusCode::TOO_MANY_REQUESTS, "rate_limited");
    }
    let worn = match of {
        Of::Items => app.store.wear_item(&me.sub, &id, now()),
        Of::Sets => app.store.wear_set(&me.sub, &id, now()),
    };
    match worn {
        Ok(true) => wardrobe(app, &me),
        Ok(false) => refuse(StatusCode::NOT_FOUND, "not_found"),
        Err(e) => failed(&e),
    }
}

async fn change_item(
    State(app): State<Arc<App>>,
    id: Result<Path<String>, PathRejection>,
    headers: HeaderMap,
    body: Result<Json<ChangeBody>, JsonRejection>,
) -> Response {
    change(&app, Of::Items, id, &headers, body).await
}

async fn change_set(
    State(app): State<Arc<App>>,
    id: Result<Path<String>, PathRejection>,
    headers: HeaderMap,
    body: Result<Json<ChangeBody>, JsonRejection>,
) -> Response {
    change(&app, Of::Sets, id, &headers, body).await
}

async fn delete_item(
    State(app): State<Arc<App>>,
    id: Result<Path<String>, PathRejection>,
    headers: HeaderMap,
) -> Response {
    delete(&app, Of::Items, id, &headers).await
}

async fn delete_set(
    State(app): State<Arc<App>>,
    id: Result<Path<String>, PathRejection>,
    headers: HeaderMap,
) -> Response {
    delete(&app, Of::Sets, id, &headers).await
}

async fn wear_item(
    State(app): State<Arc<App>>,
    id: Result<Path<String>, PathRejection>,
    headers: HeaderMap,
) -> Response {
    put_on(&app, Of::Items, id, &headers).await
}

async fn wear_set(
    State(app): State<Arc<App>>,
    id: Result<Path<String>, PathRejection>,
    headers: HeaderMap,
) -> Response {
    put_on(&app, Of::Sets, id, &headers).await
}

async fn take_off(
    State(app): State<Arc<App>>,
    kind: Result<Path<String>, PathRejection>,
    headers: HeaderMap,
) -> Response {
    let Some(kind) = kind_of(kind) else {
        return refuse(StatusCode::NOT_FOUND, "not_found");
    };
    let me = me!(app, headers);
    if !app
        .limiter
        .allow(&me.sub, "take_off", 60, Duration::from_secs(3600))
    {
        return refuse(StatusCode::TOO_MANY_REQUESTS, "rate_limited");
    }
    if let Err(e) = app.store.take_off(&me.sub, Some(kind)) {
        return failed(&e);
    }
    mine(&app, &me)
}

/// What is kept about the account, for "Download my data": the textures by
/// their hashes, not the files, worn and in the wardrobe.
async fn export(State(app): State<Arc<App>>, headers: HeaderMap) -> Response {
    let me = me!(app, headers);
    if !app
        .limiter
        .allow(&me.sub, "export", 10, Duration::from_secs(3600))
    {
        return refuse(StatusCode::TOO_MANY_REQUESTS, "rate_limited");
    }
    let worn = match app.store.worn(&me.sub) {
        Ok(worn) => worn,
        Err(e) => return failed(&e),
    };
    let of = |kind: Kind| {
        worn.iter()
            .find(|w| w.kind == kind)
            .map_or(Value::Null, |w| {
                let mut out = texture_json(&app, w);
                out["updated"] = json!(w.updated);
                out
            })
    };
    let (items, sets) = match (app.store.items(&me.sub), app.store.sets(&me.sub)) {
        (Ok(items), Ok(sets)) => (items, sets),
        (Err(e), _) | (_, Err(e)) => return failed(&e),
    };
    let items: Vec<Value> = items.iter().map(|i| item_json(&app, i)).collect();
    let sets: Vec<Value> = sets.iter().map(|s| set_json(&app, s)).collect();
    Json(json!({
        "account": me.sub,
        "skin": of(Kind::Skin),
        "cape": of(Kind::Cape),
        "items": items,
        "sets": sets,
    }))
    .into_response()
}

/// Cleans out accounts the account service says were deleted.
pub async fn purge_deleted(app: &App) {
    let since = *app
        .deleted_since
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let deleted = match app.directory.deleted(since).await {
        Ok(deleted) => deleted,
        Err(e) => {
            eprintln!("asking which accounts were deleted failed: {e}");
            return;
        }
    };
    let accounts: Vec<String> = deleted.iter().map(|(a, _)| a.clone()).collect();
    app.sessions.forget(&accounts);
    let mut newest = since;
    for (account, at) in deleted {
        if !directory::account_id(&account) {
            continue;
        }
        if let Err(e) = app.store.purge(&account) {
            eprintln!("cleaning out a deleted account failed: {e}");
            return;
        }
        app.directory.forget(&account);
        newest = newest.max(at);
    }
    *app.deleted_since
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = newest;
}

/// Asks the account service which recently seen sessions ended and refuses
/// their tokens from now on. When it cannot be asked, tokens hold until they
/// run out, as before.
pub async fn check_sessions(app: &App) {
    let sessions = app.sessions.recent();
    for chunk in sessions.chunks(500) {
        match app.directory.ended(chunk).await {
            Ok(ended) if !ended.is_empty() => app.sessions.end(&ended),
            Ok(_) => {}
            Err(e) => {
                eprintln!("asking which sessions ended failed: {e}");
                return;
            }
        }
    }
}

/// Who sent a request, for its limit: the address nginx gives
/// (`X-Real-IP`, from Cloudflare's header), an IPv6 /64 as one. Never
/// stored or logged.
fn address(headers: &HeaderMap) -> String {
    match headers
        .get("x-real-ip")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().parse::<IpAddr>().ok())
    {
        Some(IpAddr::V4(ip)) => ip.to_string(),
        Some(IpAddr::V6(ip)) => {
            let s = ip.segments();
            format!("{:x}:{:x}:{:x}:{:x}::/64", s[0], s[1], s[2], s[3])
        }
        None => "-".to_owned(),
    }
}

/// Every request counts against its address's limit. An answer that does
/// not say how long it may be kept is kept nowhere.
async fn per_address(State(app): State<Arc<App>>, request: Request, next: Next) -> Response {
    let mut response = if app.limiter.allow(
        &address(request.headers()),
        "read",
        READS_PER_MINUTE,
        Duration::from_secs(60),
    ) {
        next.run(request).await
    } else {
        refuse(StatusCode::TOO_MANY_REQUESTS, "rate_limited")
    };
    if !response.headers().contains_key(header::CACHE_CONTROL) {
        response
            .headers_mut()
            .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    }
    response
}

async fn health(State(app): State<Arc<App>>) -> Json<Value> {
    Json(json!({
        "ok": true,
        "service": "lapi-skins",
        "version": env!("CARGO_PKG_VERSION"),
        "keys": app.tokens.has_keys(),
    }))
}

pub fn router(app: Arc<App>) -> Router {
    Router::new()
        .route("/v1/profile/{nick}", get(get_profile))
        .route("/v1/profiles", post(post_profiles))
        .route("/v1/textures/{hash}", get(get_texture))
        .route("/v1/me", get(get_me))
        .route(
            "/v1/me/{kind}",
            put(upload).post(add_upload).delete(take_off),
        )
        .route("/v1/me/wardrobe", get(get_wardrobe))
        .route("/v1/me/wardrobe/save", post(save_worn))
        .route("/v1/me/wardrobe/sets", post(save_set))
        .route(
            "/v1/me/wardrobe/items/{id}",
            patch(change_item).delete(delete_item),
        )
        .route("/v1/me/wardrobe/items/{id}/wear", post(wear_item))
        .route(
            "/v1/me/wardrobe/sets/{id}",
            patch(change_set).delete(delete_set),
        )
        .route("/v1/me/wardrobe/sets/{id}/wear", post(wear_set))
        .route("/v1/export", get(export))
        .layer(axum::middleware::from_fn_with_state(
            Arc::clone(&app),
            per_address,
        ))
        .route("/v1/health", get(health))
        // JSON bodies; an upload reads its own body, up to its kind's limit.
        .layer(DefaultBodyLimit::max(16 * 1024))
        .with_state(app)
}
