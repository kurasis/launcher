//! The HTTP API (D-070). Players send reviews and read the answers to their
//! own; nothing else is readable over HTTP. Limits run before a body is read.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::extract::{DefaultBodyLimit, Path, Request, State};
use axum::http::{HeaderMap, Method, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;
use tokio::sync::Semaphore;

use crate::auth::{Sessions, Tokens};
use crate::files;
use crate::model::{Incoming, MAX_BODY};
use crate::pow::{self, Challenges};
use crate::store::{Sender, Store, Upload};

const HOUR: Duration = Duration::from_secs(3600);
const DAY: Duration = Duration::from_secs(86_400);
/// Reviews per address (an IPv6 /64 counts as one).
const REVIEWS_PER_HOUR: u32 = 5;
const REVIEWS_PER_DAY: u32 = 15;
/// Reviews per install or account and day.
const REVIEWS_PER_SENDER_DAY: i64 = 10;
/// Over everyone: a flood from many addresses stops here.
const REVIEWS_PER_DAY_ALL: i64 = 1000;
const REVIEWS_PER_HOUR_ALL: u32 = 300;
const OTHER_PER_HOUR: u32 = 120;
const OTHER_PER_HOUR_ALL: u32 = 30_000;
/// Reviews read and decoded at once; pictures can take 200 MiB each.
pub const UPLOADS_AT_ONCE: usize = 2;
/// Reviews a player asks about in one request.
const MAX_STATUS_ITEMS: usize = 50;

/// An IPv6 /48, a site's whole network where one person can take as many
/// /64s as they like, gets this many players' allowance.
const NETWORK_TIMES: u32 = 5;

/// Requests counted since the window began, per address (with its prefix
/// length), kind and window.
type Windows = HashMap<(IpAddr, u8, &'static str, u64), (Instant, u32)>;

#[derive(Default)]
pub struct Limiter {
    windows: Mutex<Windows>,
}

/// Whom a limit counts: the address, or its /64 for IPv6.
pub fn subject(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V4(_) => ip,
        IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => IpAddr::V4(v4),
            None => {
                let mut segments = v6.segments();
                segments[4..].fill(0);
                IpAddr::from(segments)
            }
        },
    }
}

/// The /48 of an IPv6 address; none for IPv4.
pub fn network(ip: IpAddr) -> Option<IpAddr> {
    match subject(ip) {
        IpAddr::V4(_) => None,
        IpAddr::V6(v6) => {
            let mut segments = v6.segments();
            segments[3..].fill(0);
            Some(IpAddr::from(segments))
        }
    }
}

/// The address that stands for "everyone" in the overall limits.
const ALL: IpAddr = IpAddr::V6(std::net::Ipv6Addr::UNSPECIFIED);

impl Limiter {
    /// Counts one request; false once the window's allowance of the address,
    /// of its IPv6 /48, or of everyone together (`overall`, when given), is
    /// used.
    pub fn allow(
        &self,
        ip: IpAddr,
        bucket: &'static str,
        window: Duration,
        each: u32,
        overall: Option<u32>,
    ) -> bool {
        let mut windows = self
            .windows
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let now = Instant::now();
        if windows.len() > 100_000 {
            windows.retain(|(_, _, _, secs), (start, _)| {
                now.duration_since(*start) < Duration::from_secs(*secs)
            });
        }
        let mut count = |key: IpAddr, prefix: u8| {
            let entry = windows
                .entry((key, prefix, bucket, window.as_secs()))
                .or_insert((now, 0));
            if now.duration_since(entry.0) >= window {
                *entry = (now, 0);
            }
            entry.1 = entry.1.saturating_add(1);
            entry.1
        };
        // An address over its own limit does not use up its network's, nor a
        // network over its limit everyone's.
        count(subject(ip), 64) <= each
            && network(ip).is_none_or(|net| count(net, 48) <= each.saturating_mul(NETWORK_TIMES))
            && overall.is_none_or(|all| count(ALL, 0) <= all)
    }
}

pub struct App {
    pub store: Store,
    pub limiter: Limiter,
    pub challenges: Challenges,
    pub tokens: Tokens,
    /// Sign-outs and deleted accounts, from the account service.
    pub sessions: Sessions,
    pub uploads: Semaphore,
    /// Stored files, in bytes, beyond which reviews are refused.
    pub max_stored: i64,
}

impl App {
    pub fn new(store: Store, tokens: Tokens, max_stored: i64) -> Self {
        Self {
            challenges: Challenges::new(store.secret()),
            store,
            limiter: Limiter::default(),
            tokens,
            sessions: Sessions::default(),
            uploads: Semaphore::new(UPLOADS_AT_ONCE),
            max_stored,
        }
    }

    /// Whether reviews are taken now (`lapi-reviews pause` stops them).
    fn paused(&self) -> bool {
        self.store
            .setting("paused")
            .ok()
            .flatten()
            .is_some_and(|v| v == "1")
    }

    fn bits(&self) -> u8 {
        self.store
            .setting("pow_bits")
            .ok()
            .flatten()
            .and_then(|v| v.parse().ok())
            .unwrap_or(pow::DEFAULT_BITS)
            .min(pow::MAX_BITS)
    }
}

pub fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

/// A refusal before it becomes a response.
type Refusal = (StatusCode, &'static str);

fn refuse(status: StatusCode, reason: &str) -> Response {
    (status, Json(json!({ "error": reason }))).into_response()
}

/// The client's address as nginx passes it (`X-Real-IP`, from Cloudflare's
/// `CF-Connecting-IP`). Used for limits and, hashed, for bans; never stored.
fn client(headers: &HeaderMap) -> IpAddr {
    headers
        .get("x-real-ip")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(IpAddr::from([0, 0, 0, 0]))
}

/// The account behind `Authorization: Bearer …`: None without the header,
/// an error when the token is not good.
fn account(app: &App, headers: &HeaderMap) -> Result<Option<String>, Refusal> {
    let Some(value) = headers.get("authorization") else {
        return Ok(None);
    };
    if !app.tokens.has_keys() {
        return Err((StatusCode::SERVICE_UNAVAILABLE, "accounts"));
    }
    let (account, session) = value
        .to_str()
        .ok()
        .and_then(|v| v.strip_prefix("Bearer "))
        .and_then(|token| app.tokens.verify(token.trim(), now()))
        .ok_or((StatusCode::UNAUTHORIZED, "token"))?;
    // Signed out, switched off or deleted.
    if !app.sessions.admit(&session, &account) {
        return Err((StatusCode::UNAUTHORIZED, "token"));
    }
    Ok(Some(account))
}

async fn health() -> Json<serde_json::Value> {
    Json(json!({ "ok": true, "service": "lapi-reviews", "version": env!("CARGO_PKG_VERSION") }))
}

/// Rate limits and the upload slots, before the body is read.
async fn admit(State(app): State<Arc<App>>, request: Request, next: Next) -> Response {
    let ip = client(request.headers());
    let path = request.uri().path();
    if request.method() == Method::POST && path == "/v1/reviews" {
        let allowed = app.limiter.allow(
            ip,
            "reviews",
            HOUR,
            REVIEWS_PER_HOUR,
            Some(REVIEWS_PER_HOUR_ALL),
        ) && app.limiter.allow(ip, "reviews", DAY, REVIEWS_PER_DAY, None);
        if !allowed {
            return refuse(StatusCode::TOO_MANY_REQUESTS, "too many reviews");
        }
        return match app.uploads.acquire().await {
            Ok(_slot) => next.run(request).await,
            Err(_) => refuse(StatusCode::SERVICE_UNAVAILABLE, "closing"),
        };
    }
    if path == "/v1/health" {
        return next.run(request).await;
    }
    if !app
        .limiter
        .allow(ip, "other", HOUR, OTHER_PER_HOUR, Some(OTHER_PER_HOUR_ALL))
    {
        return refuse(StatusCode::TOO_MANY_REQUESTS, "too many requests");
    }
    next.run(request).await
}

async fn challenge(State(app): State<Arc<App>>) -> Response {
    if app.paused() {
        return refuse(StatusCode::SERVICE_UNAVAILABLE, "paused");
    }
    let bits = app.bits();
    Json(json!({ "challenge": app.challenges.issue(bits, now()), "bits": bits })).into_response()
}

async fn reviews(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Json(incoming): Json<Incoming>,
) -> Response {
    if app.paused() {
        return refuse(StatusCode::SERVICE_UNAVAILABLE, "paused");
    }
    if !app
        .challenges
        .take(&incoming.challenge, &incoming.nonce, now())
    {
        return refuse(StatusCode::FORBIDDEN, "challenge");
    }
    let account = match account(&app, &headers) {
        Ok(account) => account,
        Err((status, reason)) => return refuse(status, reason),
    };
    let review = match incoming.check() {
        Ok(review) => review,
        Err(problem) => return refuse(StatusCode::UNPROCESSABLE_ENTITY, problem),
    };
    let net = subject(client(&headers)).to_string();
    let result = tokio::task::spawn_blocking(move || -> Result<Response, String> {
        let now = now();
        let sender = Sender {
            install_id: review.install_id.clone(),
            account,
            net: app.store.net(&net),
        };
        let mut key = [0u8; 32];
        getrandom::fill(&mut key).map_err(|e| e.to_string())?;
        let key = hex::encode(key);
        // A banned sender is told what anyone is told, and nothing is kept.
        if app.store.banned(&sender, now)? {
            let id = app.store.next_id()?;
            return Ok((StatusCode::CREATED, Json(json!({ "id": id, "key": key }))).into_response());
        }
        let since = now - 86_400;
        for (column, value) in [
            ("install", &sender.install_id),
            ("account", &sender.account),
        ] {
            if let Some(value) = value
                && app.store.sent_since(column, value, since)? >= REVIEWS_PER_SENDER_DAY
            {
                return Ok(refuse(
                    StatusCode::TOO_MANY_REQUESTS,
                    "too many reviews today",
                ));
            }
        }
        if app.store.all_since(since)? >= REVIEWS_PER_DAY_ALL {
            return Ok(refuse(StatusCode::TOO_MANY_REQUESTS, "busy"));
        }
        let mut uploads = Vec::new();
        for (n, bytes) in review.images.iter().enumerate() {
            match files::picture(bytes) {
                Ok(picture) => uploads.push(Upload {
                    name: format!("screenshot-{}.{}", n + 1, picture.extension),
                    kind: "image",
                    bytes: picture.bytes,
                }),
                Err(problem) => return Ok(refuse(StatusCode::UNPROCESSABLE_ENTITY, problem)),
            }
        }
        for (name, text) in &review.texts {
            uploads.push(Upload {
                name: name.clone(),
                kind: "text",
                bytes: text.as_bytes().to_vec(),
            });
        }
        let adding: i64 = uploads.iter().map(|u| u.bytes.len() as i64).sum();
        if app.store.stored()? + adding > app.max_stored {
            eprintln!("refused a review: the storage limit is reached");
            return Ok(refuse(StatusCode::INSUFFICIENT_STORAGE, "full"));
        }
        let id = app.store.add(&review, &sender, &key, uploads, now)?;
        Ok((StatusCode::CREATED, Json(json!({ "id": id, "key": key }))).into_response())
    })
    .await;
    match result {
        Ok(Ok(response)) => response,
        Ok(Err(error)) => {
            eprintln!("storing a review failed: {error}");
            refuse(StatusCode::INTERNAL_SERVER_ERROR, "not stored")
        }
        Err(_) => refuse(StatusCode::INTERNAL_SERVER_ERROR, "not stored"),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StatusRequest {
    #[serde(default)]
    reviews: Vec<Owned>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Owned {
    id: i64,
    key: String,
}

fn key_shape(key: &str) -> bool {
    key.len() == 64 && key.bytes().all(|b| b.is_ascii_hexdigit())
}

/// The player's reviews and the answers to them. Keys travel in the body,
/// never in an address that a log could keep.
async fn status(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Json(request): Json<StatusRequest>,
) -> Response {
    if request.reviews.len() > MAX_STATUS_ITEMS
        || request
            .reviews
            .iter()
            .any(|r| r.id < 1 || !key_shape(&r.key))
    {
        return refuse(StatusCode::UNPROCESSABLE_ENTITY, "reviews");
    }
    let account = match account(&app, &headers) {
        Ok(account) => account,
        Err((status, reason)) => return refuse(status, reason),
    };
    let keys: Vec<(i64, String)> = request.reviews.into_iter().map(|r| (r.id, r.key)).collect();
    let found =
        tokio::task::spawn_blocking(move || app.store.statuses(&keys, account.as_deref())).await;
    match found {
        Ok(Ok(statuses)) => Json(json!({ "reviews": statuses })).into_response(),
        _ => refuse(StatusCode::INTERNAL_SERVER_ERROR, "not read"),
    }
}

/// The player deletes their review: its key in `X-Review-Key`, or their
/// account's token.
async fn remove(State(app): State<Arc<App>>, Path(id): Path<i64>, headers: HeaderMap) -> Response {
    let key = headers
        .get("x-review-key")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    if key.as_deref().is_some_and(|k| !key_shape(k)) {
        return refuse(StatusCode::UNPROCESSABLE_ENTITY, "key");
    }
    let account = match account(&app, &headers) {
        Ok(account) => account,
        Err((status, reason)) => return refuse(status, reason),
    };
    let removed = tokio::task::spawn_blocking(move || {
        app.store.delete_own(id, key.as_deref(), account.as_deref())
    })
    .await;
    match removed {
        Ok(Ok(true)) => StatusCode::NO_CONTENT.into_response(),
        Ok(Ok(false)) => refuse(StatusCode::NOT_FOUND, "no such review"),
        _ => refuse(StatusCode::INTERNAL_SERVER_ERROR, "not deleted"),
    }
}

/// Everything kept about the signed-in player's reviews (D-064).
async fn export(State(app): State<Arc<App>>, headers: HeaderMap) -> Response {
    let account = match account(&app, &headers) {
        Ok(Some(account)) => account,
        Ok(None) => return refuse(StatusCode::UNAUTHORIZED, "token"),
        Err((status, reason)) => return refuse(status, reason),
    };
    match tokio::task::spawn_blocking(move || app.store.export(&account)).await {
        Ok(Ok(reviews)) => Json(json!({ "reviews": reviews })).into_response(),
        _ => refuse(StatusCode::INTERNAL_SERVER_ERROR, "not read"),
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
                eprintln!("asking which sessions ended failed: {e}");
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
            eprintln!("asking which accounts were deleted failed: {e}");
            return since;
        }
    };
    let app = Arc::clone(app);
    tokio::task::spawn_blocking(move || purge(&app, &deleted, since))
        .await
        .unwrap_or(since)
}

/// Refuses the tokens of deleted accounts, then cleans them out.
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
        if let Err(e) = app.store.purge(account) {
            eprintln!("cleaning out a deleted account failed: {e}");
            return newest;
        }
        newest = newest.max(*at);
    }
    newest
}

/// Biggest body of a status request (50 short pairs).
const SMALL_BODY: usize = 16 * 1024;

pub fn router(app: Arc<App>) -> Router {
    Router::new()
        .route("/v1/health", get(health))
        .route("/v1/challenge", get(challenge))
        .route(
            "/v1/reviews",
            post(reviews).layer(DefaultBodyLimit::max(MAX_BODY)),
        )
        .route(
            "/v1/status",
            post(status).layer(DefaultBodyLimit::max(SMALL_BODY)),
        )
        .route("/v1/reviews/{id}", delete(remove))
        .route("/v1/export", get(export))
        .layer(middleware::from_fn_with_state(Arc::clone(&app), admit))
        .with_state(app)
}
