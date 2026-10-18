//! The HTTP API. Only writes are public; reading reports is done on the server
//! with the command line (D-061).

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::extract::{DefaultBodyLimit, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::Utc;
use serde_json::json;
use tower_http::decompression::RequestDecompressionLayer;

use crate::model::{Feedback, MAX_EVENTS, MAX_TEXT_BYTES, Report, Stats};
use crate::store::Store;

/// Per address and hour; nginx limits the request rate too.
const REPORTS_PER_HOUR: u32 = 20;
const OTHER_PER_HOUR: u32 = 120;
/// Per install and day.
const REPORTS_PER_INSTALL_DAY: i64 = 30;

/// Requests counted since the start of the hour, per address and kind.
type Windows = HashMap<(IpAddr, &'static str), (Instant, u32)>;

#[derive(Default)]
pub struct Limiter {
    windows: Mutex<Windows>,
}

impl Limiter {
    /// Counts one request; false once the hour's allowance is used.
    pub fn allow(&self, ip: IpAddr, bucket: &'static str, per_hour: u32) -> bool {
        let mut windows = self
            .windows
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let now = Instant::now();
        if windows.len() > 100_000 {
            windows.retain(|_, (start, _)| now.duration_since(*start) < Duration::from_secs(3600));
        }
        let entry = windows.entry((ip, bucket)).or_insert((now, 0));
        if now.duration_since(entry.0) >= Duration::from_secs(3600) {
            *entry = (now, 0);
        }
        entry.1 += 1;
        entry.1 <= per_hour
    }
}

pub struct App {
    pub store: Store,
    pub limiter: Limiter,
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

async fn reports(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Json(report): Json<Report>,
) -> Response {
    if !app
        .limiter
        .allow(client(&headers), "reports", REPORTS_PER_HOUR)
    {
        return refuse(StatusCode::TOO_MANY_REQUESTS, "too many reports");
    }
    if let Some(problem) = report.problem() {
        return refuse(StatusCode::UNPROCESSABLE_ENTITY, problem);
    }
    let result = tokio::task::spawn_blocking(move || {
        let now = Utc::now();
        if app.store.today_from(&report.install_id, now) >= REPORTS_PER_INSTALL_DAY {
            return Err(None);
        }
        app.store.add(&report, now).map_err(Some)
    })
    .await;
    match result {
        Ok(Ok(id)) => (StatusCode::CREATED, Json(json!({ "id": id }))).into_response(),
        Ok(Err(None)) => refuse(StatusCode::TOO_MANY_REQUESTS, "too many reports today"),
        Ok(Err(Some(error))) => {
            eprintln!("storing a report failed: {error}");
            refuse(StatusCode::INTERNAL_SERVER_ERROR, "not stored")
        }
        Err(_) => refuse(StatusCode::INTERNAL_SERVER_ERROR, "not stored"),
    }
}

async fn stats(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Json(stats): Json<Stats>,
) -> Response {
    if !app.limiter.allow(client(&headers), "other", OTHER_PER_HOUR) {
        return refuse(StatusCode::TOO_MANY_REQUESTS, "too many requests");
    }
    if stats.schema != 1 || stats.events.is_empty() || stats.events.len() > MAX_EVENTS {
        return refuse(StatusCode::UNPROCESSABLE_ENTITY, "events");
    }
    if let Some(problem) = stats.events.iter().find_map(|e| e.problem()) {
        return refuse(StatusCode::UNPROCESSABLE_ENTITY, problem);
    }
    let stored =
        tokio::task::spawn_blocking(move || app.store.count(&stats.events, Utc::now())).await;
    match stored {
        Ok(Ok(())) => StatusCode::NO_CONTENT.into_response(),
        _ => refuse(StatusCode::INTERNAL_SERVER_ERROR, "not stored"),
    }
}

async fn feedback(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Json(feedback): Json<Feedback>,
) -> Response {
    if !app.limiter.allow(client(&headers), "other", OTHER_PER_HOUR) {
        return refuse(StatusCode::TOO_MANY_REQUESTS, "too many requests");
    }
    if let Some(problem) = feedback.problem() {
        return refuse(StatusCode::UNPROCESSABLE_ENTITY, problem);
    }
    let stored =
        tokio::task::spawn_blocking(move || app.store.feedback(&feedback, Utc::now())).await;
    match stored {
        Ok(Ok(())) => StatusCode::NO_CONTENT.into_response(),
        _ => refuse(StatusCode::INTERNAL_SERVER_ERROR, "not stored"),
    }
}

pub fn router(app: Arc<App>) -> Router {
    Router::new()
        .route("/v1/health", get(health))
        .route("/v1/reports", post(reports))
        .route("/v1/stats", post(stats))
        .route("/v1/feedback", post(feedback))
        // The decompressed body: the files plus the JSON around them.
        .layer(DefaultBodyLimit::max(MAX_TEXT_BYTES + 256 * 1024))
        .layer(RequestDecompressionLayer::new())
        .with_state(app)
}
