//! The API at stats.lapi.sh (D-071): two writes and a health check. Nothing
//! can be read from outside: the numbers are read on the server with the
//! command line. The client's address is used for the rate limits, in memory
//! only, and never stored or logged; the country comes from Cloudflare.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::extract::rejection::JsonRejection;
use axum::extract::{DefaultBodyLimit, Request, State};
use axum::http::{HeaderMap, Method, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, NaiveDate, Utc};
use serde_json::json;
use tower_http::decompression::RequestDecompressionLayer;

use crate::ads::Shown;
use crate::model::{self, Counts, Ping};
use crate::pow;
use crate::store::{Fresh, Store};
use crate::tally::Tally;

/// Per address (an IPv6 /64 counts as one) and hour. Generous: many players
/// can share one address behind a carrier's NAT, and each sends one ping a
/// day and a batch an hour. nginx limits the request rate too.
const PINGS_PER_HOUR: u32 = 300;
const COUNTS_PER_HOUR: u32 = 600;
/// Over all addresses and per hour: a flood from many addresses stops here.
const PINGS_PER_HOUR_ALL: u32 = 400_000;
const COUNTS_PER_HOUR_ALL: u32 = 800_000;
/// The biggest bodies, unpacked: a ping is under 1 KiB, a batch of 50 ads
/// under 32 KiB.
pub const PING_BODY: usize = 8 * 1024;
pub const COUNTS_BODY: usize = 64 * 1024;

/// Requests counted since the hour began, per address (with its prefix
/// length) and kind.
type Windows = HashMap<(IpAddr, u8, &'static str), (Instant, u32)>;

/// An IPv6 /48, a site's whole network where one person can take as many
/// /64s as they like, gets this many addresses' allowance.
const NETWORK_TIMES: u32 = 5;

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

/// The /48 of an IPv6 address; none for IPv4.
fn network(ip: IpAddr) -> Option<IpAddr> {
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
    /// Counts one request; false once the hour's allowance of the address,
    /// of its IPv6 /48, or of everyone together, is used.
    pub fn allow(&self, ip: IpAddr, bucket: &'static str, per_hour: u32, overall: u32) -> bool {
        let mut windows = self
            .windows
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let now = Instant::now();
        if windows.len() > 100_000 {
            windows.retain(|_, (start, _)| now.duration_since(*start) < Duration::from_secs(3600));
        }
        let mut count = |key: IpAddr, prefix: u8| {
            let entry = windows.entry((key, prefix, bucket)).or_insert((now, 0));
            if now.duration_since(entry.0) >= Duration::from_secs(3600) {
                *entry = (now, 0);
            }
            entry.1 = entry.1.saturating_add(1);
            entry.1
        };
        // An address over its own limit does not use up its network's, nor a
        // network over its limit everyone's.
        count(subject(ip), 64) <= per_hour
            && network(ip)
                .is_none_or(|net| count(net, 48) <= per_hour.saturating_mul(NETWORK_TIMES))
            && count(ALL, 0) <= overall
    }
}

pub struct App {
    pub store: Store,
    pub tally: Tally,
    pub shown: Shown,
    pub limiter: Limiter,
    /// Zero bits a request's work must give.
    pub bits: u32,
    /// The time now; tests set their own.
    pub clock: fn() -> DateTime<Utc>,
}

pub fn now() -> DateTime<Utc> {
    Utc::now()
}

impl App {
    pub fn new(store: Store, bits: u32, clock: fn() -> DateTime<Utc>) -> Result<Self, String> {
        let shown = Shown::load(&store, clock().date_naive())?;
        Ok(Self {
            store,
            tally: Tally::default(),
            shown,
            limiter: Limiter::default(),
            bits,
            clock,
        })
    }

    pub fn today(&self) -> NaiveDate {
        (self.clock)().date_naive()
    }

    /// Writes what was counted in memory; on a failure it stays there for
    /// the next time. Returns how many new rows went to `other`.
    pub fn flush(&self) -> Result<usize, String> {
        let counts = self.tally.take();
        if counts.is_empty() {
            return Ok(0);
        }
        self.store.write(&counts).inspect_err(|_| {
            self.tally.put_back(counts.clone());
        })
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

/// The client's country as nginx passes it (`X-Country`, from Cloudflare's
/// `CF-IPCountry`).
fn country(headers: &HeaderMap) -> String {
    model::country(
        headers
            .get("x-country")
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default(),
    )
}

async fn health() -> Json<serde_json::Value> {
    Json(json!({ "ok": true, "service": "lapi-stats", "version": env!("CARGO_PKG_VERSION") }))
}

async fn not_found() -> Response {
    refuse(StatusCode::NOT_FOUND, "not_found")
}

/// Rate limits, before the body is read or unpacked: a refused request
/// costs no memory.
async fn admit(State(app): State<Arc<App>>, request: Request, next: Next) -> Response {
    if request.method() != Method::POST {
        return next.run(request).await;
    }
    let (bucket, per_hour, overall) = match request.uri().path() {
        "/v1/ping" => ("ping", PINGS_PER_HOUR, PINGS_PER_HOUR_ALL),
        "/v1/counts" => ("counts", COUNTS_PER_HOUR, COUNTS_PER_HOUR_ALL),
        _ => return next.run(request).await,
    };
    if !app
        .limiter
        .allow(client(request.headers()), bucket, per_hour, overall)
    {
        return refuse(StatusCode::TOO_MANY_REQUESTS, "too many requests");
    }
    next.run(request).await
}

/// The work was not done: the answer says how much is asked for.
fn work(bits: u32) -> Response {
    (
        StatusCode::UNPROCESSABLE_ENTITY,
        Json(json!({ "error": "work", "bits": bits })),
    )
        .into_response()
}

/// Notes the batch as counted, off the async threads.
async fn first_time(app: &Arc<App>, batch: uuid::Uuid, today: NaiveDate) -> Option<Fresh> {
    let app = Arc::clone(app);
    match tokio::task::spawn_blocking(move || app.store.first_time(batch, today)).await {
        Ok(Ok(fresh)) => Some(fresh),
        Ok(Err(e)) => {
            eprintln!("noting a batch failed: {e}");
            None
        }
        Err(_) => None,
    }
}

async fn ping(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    body: Result<Json<Ping>, JsonRejection>,
) -> Response {
    let today = app.today();
    let Ok(Json(ping)) = body else {
        app.tally.dropped(today, "bad_ping");
        return refuse(StatusCode::UNPROCESSABLE_ENTITY, "json");
    };
    let read = match ping.read(&country(&headers), today) {
        Ok(read) => read,
        Err(problem) => {
            app.tally.dropped(today, "bad_ping");
            return refuse(StatusCode::UNPROCESSABLE_ENTITY, problem);
        }
    };
    if !pow::done(&ping.batch, &ping.day, &ping.nonce, app.bits) {
        app.tally.dropped(today, "work");
        return work(app.bits);
    }
    let Ok(batch) = ping.batch() else {
        return refuse(StatusCode::UNPROCESSABLE_ENTITY, "batch");
    };
    match first_time(&app, batch, today).await {
        Some(Fresh::New) => {
            app.tally.ping(&read);
            StatusCode::NO_CONTENT.into_response()
        }
        Some(Fresh::Seen) => {
            app.tally.dropped(today, "again");
            StatusCode::NO_CONTENT.into_response()
        }
        Some(Fresh::Full) => refuse(StatusCode::SERVICE_UNAVAILABLE, "full"),
        None => refuse(StatusCode::INTERNAL_SERVER_ERROR, "not counted"),
    }
}

async fn counts(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    body: Result<Json<Counts>, JsonRejection>,
) -> Response {
    let today = app.today();
    let Ok(Json(counts)) = body else {
        app.tally.dropped(today, "bad_counts");
        return refuse(StatusCode::UNPROCESSABLE_ENTITY, "json");
    };
    let read = match counts.read(today) {
        Ok(read) => read,
        Err(problem) => {
            app.tally.dropped(today, "bad_counts");
            return refuse(StatusCode::UNPROCESSABLE_ENTITY, problem);
        }
    };
    if !pow::done(&counts.batch, &counts.day, &counts.nonce, app.bits) {
        app.tally.dropped(today, "work");
        return work(app.bits);
    }
    // Real ads are told from made-up ones by the list of the ads service;
    // without a fresh one the launcher sends the batch again later.
    if !read.ads.is_empty() && !app.shown.fresh() {
        return refuse(StatusCode::SERVICE_UNAVAILABLE, "ads list");
    }
    let Ok(batch) = counts.batch() else {
        return refuse(StatusCode::UNPROCESSABLE_ENTITY, "batch");
    };
    match first_time(&app, batch, today).await {
        Some(Fresh::New) => {
            app.tally.counts(&read, &country(&headers), |ad| {
                app.shown.known(ad, read.day)
            });
            StatusCode::NO_CONTENT.into_response()
        }
        Some(Fresh::Seen) => {
            app.tally.dropped(today, "again");
            StatusCode::NO_CONTENT.into_response()
        }
        Some(Fresh::Full) => refuse(StatusCode::SERVICE_UNAVAILABLE, "full"),
        None => refuse(StatusCode::INTERNAL_SERVER_ERROR, "not counted"),
    }
}

pub fn router(app: Arc<App>) -> Router {
    Router::new()
        .route("/v1/health", get(health))
        .route(
            "/v1/ping",
            post(ping).layer(DefaultBodyLimit::max(PING_BODY)),
        )
        .route(
            "/v1/counts",
            post(counts).layer(DefaultBodyLimit::max(COUNTS_BODY)),
        )
        .fallback(not_found)
        .layer(DefaultBodyLimit::max(PING_BODY))
        .layer(RequestDecompressionLayer::new())
        .layer(middleware::from_fn_with_state(Arc::clone(&app), admit))
        .with_state(app)
}
