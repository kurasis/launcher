//! The HTTP API. Only writes are public; reading reports is done on the server
//! with the command line (D-061).

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::extract::{DefaultBodyLimit, Request, State};
use axum::http::{HeaderMap, Method, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::Utc;
use serde_json::json;
use tokio::sync::Semaphore;
use tower_http::decompression::RequestDecompressionLayer;

use crate::model::{Feedback, MAX_EVENTS, MAX_TEXT_BYTES, Report, Stats};
use crate::store::{Added, Store};

/// Per address (an IPv6 /64 counts as one) and hour; nginx limits the request
/// rate too.
const REPORTS_PER_HOUR: u32 = 20;
const OTHER_PER_HOUR: u32 = 120;
/// Over all addresses and per hour: a flood from many addresses stops here.
const REPORTS_PER_HOUR_ALL: u32 = 600;
const OTHER_PER_HOUR_ALL: u32 = 30_000;
/// Per install and day.
const REPORTS_PER_INSTALL_DAY: i64 = 30;
/// Reports read and stored at the same time; each can take tens of MiB.
pub const UPLOADS_AT_ONCE: usize = 4;

/// Requests counted since the start of the hour, per address and kind.
type Windows = HashMap<(IpAddr, &'static str), (Instant, u32)>;

#[derive(Default)]
pub struct Limiter {
    windows: Mutex<Windows>,
}

/// Whom a limit counts: the address, or its /64 for IPv6, where one
/// connection usually has a whole /64 to pick addresses from.
fn subject(ip: IpAddr) -> IpAddr {
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

/// The address that stands for "everyone" in the overall limits.
const ALL: IpAddr = IpAddr::V6(std::net::Ipv6Addr::UNSPECIFIED);

impl Limiter {
    /// Counts one request; false once the hour's allowance of the address,
    /// or of everyone together, is used.
    pub fn allow(&self, ip: IpAddr, bucket: &'static str, per_hour: u32, overall: u32) -> bool {
        let mut windows = self
            .windows
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let now = Instant::now();
        if windows.len() > 100_000 {
            windows.retain(|_, (start, _)| now.duration_since(*start) < Duration::from_secs(3600));
        }
        let mut count = |key: IpAddr| {
            let entry = windows.entry((key, bucket)).or_insert((now, 0));
            if now.duration_since(entry.0) >= Duration::from_secs(3600) {
                *entry = (now, 0);
            }
            entry.1 = entry.1.saturating_add(1);
            entry.1
        };
        // An address over its own limit does not use up everyone's.
        count(subject(ip)) <= per_hour && count(ALL) <= overall
    }
}

pub struct App {
    pub store: Store,
    pub limiter: Limiter,
    pub uploads: Semaphore,
    /// Stored reports, in bytes, beyond which new ones are refused.
    pub max_stored: i64,
}

impl App {
    pub fn new(store: Store, max_stored: i64) -> Self {
        Self {
            store,
            limiter: Limiter::default(),
            uploads: Semaphore::new(UPLOADS_AT_ONCE),
            max_stored,
        }
    }
}

fn refuse(status: StatusCode, reason: &str) -> Response {
    (status, Json(json!({ "error": reason }))).into_response()
}

/// The client's address as nginx passes it (`X-Real-IP`, from Cloudflare's
/// `CF-Connecting-IP`). Used for rate limits only, never stored.
fn client(headers: &HeaderMap) -> IpAddr {
    headers
        .get("x-real-ip")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(IpAddr::from([0, 0, 0, 0]))
}

async fn health() -> Json<serde_json::Value> {
    Json(json!({ "ok": true, "service": "lapi-crash", "version": env!("CARGO_PKG_VERSION") }))
}

/// Rate limits and the upload slots, before the body is read or unpacked:
/// a refused request costs no memory.
async fn admit(State(app): State<Arc<App>>, request: Request, next: Next) -> Response {
    if request.method() != Method::POST {
        return next.run(request).await;
    }
    let (bucket, per_hour, overall) = match request.uri().path() {
        "/v1/reports" => ("reports", REPORTS_PER_HOUR, REPORTS_PER_HOUR_ALL),
        "/v1/stats" | "/v1/feedback" => ("other", OTHER_PER_HOUR, OTHER_PER_HOUR_ALL),
        _ => return next.run(request).await,
    };
    if !app
        .limiter
        .allow(client(request.headers()), bucket, per_hour, overall)
    {
        return refuse(StatusCode::TOO_MANY_REQUESTS, "too many requests");
    }
    if bucket != "reports" {
        return next.run(request).await;
    }
    match app.uploads.acquire().await {
        Ok(_slot) => next.run(request).await,
        Err(_) => refuse(StatusCode::SERVICE_UNAVAILABLE, "closing"),
    }
}

async fn reports(State(app): State<Arc<App>>, Json(report): Json<Report>) -> Response {
    if let Some(problem) = report.problem() {
        return refuse(StatusCode::UNPROCESSABLE_ENTITY, problem);
    }
    let result = tokio::task::spawn_blocking(move || {
        let now = Utc::now();
        if app.store.today_from(&report.install_id, now) >= REPORTS_PER_INSTALL_DAY {
            return Ok(Added::Limited);
        }
        app.store.add(&report, now, app.max_stored)
    })
    .await;
    match result {
        Ok(Ok(Added::Stored(id))) => {
            (StatusCode::CREATED, Json(json!({ "id": id }))).into_response()
        }
        Ok(Ok(Added::Limited)) => refuse(StatusCode::TOO_MANY_REQUESTS, "too many reports today"),
        Ok(Ok(Added::Full)) => {
            eprintln!("refused a report: the storage limit is reached");
            refuse(StatusCode::INSUFFICIENT_STORAGE, "full")
        }
        Ok(Err(error)) => {
            eprintln!("storing a report failed: {error}");
            refuse(StatusCode::INTERNAL_SERVER_ERROR, "not stored")
        }
        Err(_) => refuse(StatusCode::INTERNAL_SERVER_ERROR, "not stored"),
    }
}

async fn stats(State(app): State<Arc<App>>, Json(stats): Json<Stats>) -> Response {
    if stats.schema != 1 || stats.events.is_empty() || stats.events.len() > MAX_EVENTS {
        return refuse(StatusCode::UNPROCESSABLE_ENTITY, "events");
    }
    if let Some(problem) = stats.events.iter().find_map(|e| e.problem()) {
        return refuse(StatusCode::UNPROCESSABLE_ENTITY, problem);
    }
    let stored =
        tokio::task::spawn_blocking(move || app.store.count(&stats.events, Utc::now())).await;
    match stored {
        Ok(Ok(true)) => StatusCode::NO_CONTENT.into_response(),
        Ok(Ok(false)) => refuse(StatusCode::INSUFFICIENT_STORAGE, "full"),
        _ => refuse(StatusCode::INTERNAL_SERVER_ERROR, "not stored"),
    }
}

async fn feedback(State(app): State<Arc<App>>, Json(feedback): Json<Feedback>) -> Response {
    if let Some(problem) = feedback.problem() {
        return refuse(StatusCode::UNPROCESSABLE_ENTITY, problem);
    }
    let stored =
        tokio::task::spawn_blocking(move || app.store.feedback(&feedback, Utc::now())).await;
    match stored {
        Ok(Ok(true)) => StatusCode::NO_CONTENT.into_response(),
        Ok(Ok(false)) => refuse(StatusCode::INSUFFICIENT_STORAGE, "full"),
        _ => refuse(StatusCode::INTERNAL_SERVER_ERROR, "not stored"),
    }
}

/// Biggest decompressed body of a stats or feedback request.
const SMALL_BODY: usize = 64 * 1024;

pub fn router(app: Arc<App>) -> Router {
    Router::new()
        .route("/v1/health", get(health))
        .route("/v1/reports", post(reports))
        // Counts and a yes/no: a few kilobytes (50 short events at most).
        .route(
            "/v1/stats",
            post(stats).layer(DefaultBodyLimit::max(SMALL_BODY)),
        )
        .route(
            "/v1/feedback",
            post(feedback).layer(DefaultBodyLimit::max(SMALL_BODY)),
        )
        // The decompressed body: the files plus the JSON around them.
        .layer(DefaultBodyLimit::max(MAX_TEXT_BYTES + 256 * 1024))
        .layer(RequestDecompressionLayer::new())
        .layer(middleware::from_fn_with_state(Arc::clone(&app), admit))
        .with_state(app)
}
