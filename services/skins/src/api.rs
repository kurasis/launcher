//! The skins API (D-065). Lookups by nick and the textures themselves are
//! public; changes carry the account service's access token, and the account
//! is its `sub`, never anything the request says. Nothing about a player (a
//! nick, an address) is ever written to the log.

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
use axum::routing::{get, post, put};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::auth::{Claims, Sessions, Tokens};
use crate::directory::{self, Directory};
use crate::store::{Store, Worn};
use crate::texture::{self, Kind};

/// Nicks in one `POST /v1/profiles`.
pub const MAX_NICKS: usize = 50;
/// Uploads per account an hour.
pub const UPLOADS_PER_HOUR: u32 = 10;
/// Requests per address a minute (an IPv6 /64 counts as one), besides
/// nginx's own limit.
pub const READS_PER_MINUTE: u32 = 600;
/// Uploads decoded at once: each may hold up to 32 MiB of pixels.
pub const DECODING_AT_ONCE: usize = 2;
/// Longest query of an upload (32 frame durations fit easily).
const QUERY_MAX: usize = 512;
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
    // Signed out, or the account was switched off.
    if app.sessions.ended(&claims.sid) {
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

/// The account's own profile, also when it wears nothing.
fn mine(app: &App, me: &Claims) -> Response {
    match profile(app, &me.sub, &me.nick, true) {
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
    // What was not read is read to its end and dropped (nginx lets 3 MiB
    // through at most), so a refusal reaches the launcher instead of a
    // connection closed under nginx.
    if let Some(rest) = body {
        let _ = axum::body::to_bytes(rest, READ_MAX).await;
    }
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
    // Limits before work: nothing is decoded past an exhausted limit.
    if !app.limiter.allow(
        &me.sub,
        "upload",
        UPLOADS_PER_HOUR,
        Duration::from_secs(3600),
    ) {
        return refuse(StatusCode::TOO_MANY_REQUESTS, "rate_limited");
    }
    if uri.query().map_or(0, str::len) > QUERY_MAX {
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
    let texture = match processed {
        Ok(Ok(texture)) => texture,
        Ok(Err(refusal)) => {
            let status = if refusal == texture::Refusal::TooLarge {
                StatusCode::PAYLOAD_TOO_LARGE
            } else {
                StatusCode::BAD_REQUEST
            };
            return refuse(status, refusal.code());
        }
        Err(_) => return failed("decoding an upload stopped"),
    };
    if let Err(e) = app.store.wear(&me.sub, &texture, now()) {
        return failed(&e);
    }
    mine(app, &me)
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
/// their hashes, not the files.
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
    Json(json!({
        "account": me.sub,
        "skin": of(Kind::Skin),
        "cape": of(Kind::Cape),
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
        .route("/v1/me/{kind}", put(upload).delete(take_off))
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
