//! `GET /<host>/<path>`: the origin's content from the mirror (D-062).
//!
//! nginx answers kept files from disk itself; this service sees the rest:
//! files not kept yet (fetched once, streamed while kept) and metadata
//! (fetched again after [`App::meta_ttl`], the last good copy served while the
//! origin fails).

use std::collections::HashMap;
use std::net::IpAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use bytes::Bytes;
use chrono::Utc;
use serde_json::json;
use tokio::io::AsyncWriteExt;
use tokio::sync::{Semaphore, mpsc, watch};

use crate::origin::{Class, Origin, parse};
use crate::store::{MetaInfo, Store};

/// Fetches from origins at once, over all clients.
const FETCHES_AT_ONCE: usize = 64;
/// Of those, metadata fetches (each held in memory, up to [`MAX_META`]).
const META_AT_ONCE: usize = 16;
/// Requests the service handles at once per client address (kept files never
/// reach it: nginx serves them).
const PER_CLIENT_AT_ONCE: u32 = 16;
/// Biggest file and metadata response the mirror passes on.
const MAX_FILE: u64 = 4 * 1024 * 1024 * 1024;
/// Real metadata is well under 1 MiB (Mojang's version list, about 280 KB, is
/// the largest).
const MAX_META: usize = 8 * 1024 * 1024;
/// How long a request waits for another request's fetch of the same file
/// before it fetches without keeping.
const FOLLOW_WAIT: Duration = Duration::from_secs(120);
const META_TIMEOUT: Duration = Duration::from_secs(20);

/// Gives the origin URL to fetch for a request; tests point it at a local
/// server.
pub type Upstream = Box<dyn Fn(&Origin) -> String + Send + Sync>;

pub struct App {
    pub store: Store,
    pub client: reqwest::Client,
    pub upstream: Upstream,
    pub meta_ttl: Duration,
    fetches: Arc<Semaphore>,
    meta_fetches: Semaphore,
    /// Files being fetched, by origin URL: later requests wait for that fetch.
    inflight: Mutex<HashMap<String, watch::Receiver<()>>>,
    meta_locks: Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
    clients: Mutex<HashMap<IpAddr, u32>>,
}

impl App {
    pub fn new(store: Store, client: reqwest::Client, upstream: Upstream) -> Self {
        Self {
            store,
            client,
            upstream,
            meta_ttl: Duration::from_secs(600),
            fetches: Arc::new(Semaphore::new(FETCHES_AT_ONCE)),
            meta_fetches: Semaphore::new(META_AT_ONCE),
            inflight: Mutex::new(HashMap::new()),
            meta_locks: Mutex::new(HashMap::new()),
            clients: Mutex::new(HashMap::new()),
        }
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn refuse(status: StatusCode, reason: &str) -> Response {
    (status, Json(json!({ "error": reason }))).into_response()
}

/// The client's address as nginx passes it, for the per-client limit only:
/// an IPv6 /64 counts as one, since one connection usually has a whole /64 to
/// pick addresses from.
pub fn client(headers: &HeaderMap) -> IpAddr {
    let ip = headers
        .get("x-real-ip")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().parse::<IpAddr>().ok())
        .unwrap_or(IpAddr::from([0, 0, 0, 0]))
        .to_canonical();
    match ip {
        IpAddr::V6(v6) => {
            let s = v6.segments();
            IpAddr::from([s[0], s[1], s[2], s[3], 0, 0, 0, 0])
        }
        v4 => v4,
    }
}

/// Counts a client's running request until dropped.
struct Slot<'a> {
    app: &'a App,
    ip: IpAddr,
}

impl Drop for Slot<'_> {
    fn drop(&mut self) {
        let mut clients = lock(&self.app.clients);
        if let Some(n) = clients.get_mut(&self.ip) {
            *n -= 1;
            if *n == 0 {
                clients.remove(&self.ip);
            }
        }
    }
}

fn slot(app: &App, ip: IpAddr) -> Option<Slot<'_>> {
    let mut clients = lock(&app.clients);
    let n = clients.entry(ip).or_insert(0);
    if *n >= PER_CLIENT_AT_ONCE {
        return None;
    }
    *n += 1;
    Some(Slot { app, ip })
}

async fn health() -> Json<serde_json::Value> {
    Json(json!({ "ok": true, "service": "lapi-mirror", "version": env!("CARGO_PKG_VERSION") }))
}

async fn mirror(State(app): State<Arc<App>>, request: Request) -> Response {
    if request.method() != Method::GET && request.method() != Method::HEAD {
        return refuse(StatusCode::METHOD_NOT_ALLOWED, "only GET");
    }
    let Some(origin) = parse(request.uri().path(), request.uri().query()) else {
        return refuse(StatusCode::NOT_FOUND, "not a mirrored address");
    };
    let Some(_slot) = slot(&app, client(request.headers())) else {
        return refuse(StatusCode::TOO_MANY_REQUESTS, "too many requests at once");
    };
    let head = request.method() == Method::HEAD;
    match origin.class.clone() {
        Class::File(relative) if head => head_of_file(&app, &origin, &relative).await,
        Class::File(relative) => file(&app, origin, relative).await,
        Class::Meta => meta(&app, &origin).await,
    }
}

fn content_type(path: &std::path::Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("json") => "application/json",
        Some("jar") => "application/java-archive",
        Some("xml" | "pom") => "application/xml",
        Some("zip") => "application/zip",
        Some("gz" | "tgz") => "application/gzip",
        Some("sha1" | "sha256" | "sha512" | "md5" | "txt") => "text/plain",
        _ => "application/octet-stream",
    }
}

/// An origin's content type as the mirror passes it on: data types as they
/// are, anything a browser would run or render (an HTML page, an image) as
/// plain bytes, so nothing the mirror relays acts as a page of its own.
fn safe_type(origin_type: &str) -> String {
    let essence = origin_type
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    let data = matches!(
        essence.as_str(),
        "application/json" | "application/xml" | "text/xml" | "text/plain"
    );
    if data {
        origin_type.to_owned()
    } else {
        "application/octet-stream".to_owned()
    }
}

const IMMUTABLE: &str = "public, max-age=31536000, immutable";

async fn from_disk(path: &std::path::Path, how: &'static str) -> Option<Response> {
    let file = tokio::fs::File::open(path).await.ok()?;
    let len = file.metadata().await.ok()?.len();
    Response::builder()
        .header(header::CONTENT_TYPE, content_type(path))
        .header(header::CONTENT_LENGTH, len)
        .header(header::CACHE_CONTROL, IMMUTABLE)
        .header("x-mirror-cache", how)
        .body(Body::from_stream(tokio_util::io::ReaderStream::new(file)))
        .ok()
}

/// A `HEAD` request: the kept file's headers, or the origin's. It fetches
/// nothing to keep (a full fetch for headers alone would cost the whole file).
async fn head_of_file(app: &Arc<App>, origin: &Origin, relative: &std::path::Path) -> Response {
    if let Some(response) = from_disk(&app.store.file(relative), "kept").await {
        return response;
    }
    let Ok(answer) = app.client.head((app.upstream)(origin)).send().await else {
        return refuse(StatusCode::BAD_GATEWAY, "origin unreachable");
    };
    let status = answer.status();
    if status == reqwest::StatusCode::NOT_FOUND || status == reqwest::StatusCode::GONE {
        return refuse(StatusCode::NOT_FOUND, "not at the origin");
    }
    if !status.is_success() {
        return refuse(StatusCode::BAD_GATEWAY, "origin failed");
    }
    let mut builder = Response::builder()
        .header(
            header::CONTENT_TYPE,
            content_type(std::path::Path::new(&origin.url)),
        )
        .header("x-mirror-cache", "origin");
    if let Some(length) = answer.content_length() {
        builder = builder.header(header::CONTENT_LENGTH, length);
    }
    builder
        .body(Body::empty())
        .unwrap_or_else(|_| refuse(StatusCode::INTERNAL_SERVER_ERROR, "response"))
}

/// The request that fetches and keeps a file. However it ends (done, failed,
/// or its client left before the fetch began), dropping this takes the file
/// off the in-flight list and then lets the waiting requests go.
struct Leading {
    app: Arc<App>,
    url: String,
    _done: watch::Sender<()>,
}

impl Drop for Leading {
    fn drop(&mut self) {
        lock(&self.app.inflight).remove(&self.url);
    }
}

enum Role {
    Leader(Leading),
    Follower(watch::Receiver<()>),
}

async fn file(app: &Arc<App>, origin: Origin, relative: PathBuf) -> Response {
    let path = app.store.file(&relative);
    if let Some(response) = from_disk(&path, "kept").await {
        return response;
    }
    let role = {
        let mut inflight = lock(&app.inflight);
        match inflight.get(&origin.url) {
            Some(rx) => Role::Follower(rx.clone()),
            None => {
                let (tx, rx) = watch::channel(());
                inflight.insert(origin.url.clone(), rx);
                Role::Leader(Leading {
                    app: Arc::clone(app),
                    url: origin.url.clone(),
                    _done: tx,
                })
            }
        }
    };
    match role {
        Role::Leader(leading) => stream(app, &origin, Some((path, leading))).await,
        Role::Follower(mut rx) => {
            // `changed` ends when the leader drops its sender.
            let _ = tokio::time::timeout(FOLLOW_WAIT, rx.changed()).await;
            match from_disk(&path, "kept").await {
                Some(response) => response,
                None => stream(app, &origin, None).await,
            }
        }
    }
}

/// Fetches a file from its origin and streams it to the client; with `keep`,
/// also writes it and puts it in place once complete (the client leaving does
/// not stop that), then lets waiting requests go.
async fn stream(app: &Arc<App>, origin: &Origin, keep: Option<(PathBuf, Leading)>) -> Response {
    let Ok(permit) = Arc::clone(&app.fetches).acquire_owned().await else {
        return refuse(StatusCode::SERVICE_UNAVAILABLE, "closing");
    };
    let response = match app.client.get((app.upstream)(origin)).send().await {
        Ok(r) => r,
        Err(e) => {
            eprintln!("fetching {} failed: {e}", origin.url);
            return refuse(StatusCode::BAD_GATEWAY, "origin unreachable");
        }
    };
    let status = response.status();
    if status == reqwest::StatusCode::NOT_FOUND || status == reqwest::StatusCode::GONE {
        return refuse(StatusCode::NOT_FOUND, "not at the origin");
    }
    if !status.is_success() {
        eprintln!("fetching {} answered {status}", origin.url);
        return refuse(StatusCode::BAD_GATEWAY, "origin failed");
    }
    let length = response.content_length();
    if length.is_some_and(|l| l > MAX_FILE) {
        return refuse(StatusCode::BAD_GATEWAY, "too large");
    }
    let keep = keep.filter(|_| app.store.may_keep(length));
    let (tx, rx) = mpsc::channel::<Result<Bytes, std::io::Error>>(16);
    let task_app = Arc::clone(app);
    let url = origin.url.clone();
    tokio::spawn(async move {
        let _permit = permit;
        let mut response = response;
        let part = task_app.store.part();
        let mut file = match &keep {
            Some(_) => tokio::fs::File::create(&part).await.ok(),
            None => None,
        };
        let (mut written, mut failed, mut listening) = (0u64, false, true);
        loop {
            match response.chunk().await {
                Ok(Some(chunk)) => {
                    written += chunk.len() as u64;
                    if written > MAX_FILE {
                        failed = true;
                        break;
                    }
                    if let Some(f) = file.as_mut()
                        && f.write_all(&chunk).await.is_err()
                    {
                        file = None;
                    }
                    if listening && tx.send(Ok(chunk)).await.is_err() {
                        listening = false;
                    }
                    if !listening && file.is_none() {
                        break;
                    }
                }
                Ok(None) => break,
                Err(e) => {
                    eprintln!("fetching {url} stopped: {e}");
                    failed = true;
                    if listening {
                        let _ = tx.send(Err(std::io::Error::other("origin stopped"))).await;
                    }
                    break;
                }
            }
        }
        drop(tx);
        if let Some((destination, leading)) = keep {
            let complete = !failed && length.is_none_or(|l| l == written);
            let placed = match file {
                Some(f) if complete && f.sync_all().await.is_ok() => {
                    drop(f);
                    let folder = match destination.parent() {
                        Some(dir) => tokio::fs::create_dir_all(dir).await.is_ok(),
                        None => false,
                    };
                    folder && tokio::fs::rename(&part, &destination).await.is_ok()
                }
                _ => false,
            };
            if placed {
                task_app.store.kept(written);
            } else {
                let _ = tokio::fs::remove_file(&part).await;
            }
            drop(leading);
        }
    });
    let mut builder = Response::builder()
        .header(
            header::CONTENT_TYPE,
            content_type(std::path::Path::new(&origin.url)),
        )
        .header(header::CACHE_CONTROL, IMMUTABLE)
        .header("x-mirror-cache", "fetched");
    if let Some(l) = length {
        builder = builder.header(header::CONTENT_LENGTH, l);
    }
    let body = futures_util::stream::unfold(rx, |mut rx| async move {
        rx.recv().await.map(|item| (item, rx))
    });
    builder
        .body(Body::from_stream(body))
        .unwrap_or_else(|_| refuse(StatusCode::INTERNAL_SERVER_ERROR, "response"))
}

enum MetaFailure {
    /// The origin says there is nothing there.
    Missing,
    Failed,
}

fn meta_response(info: &MetaInfo, body: Vec<u8>, how: &'static str) -> Response {
    let mut builder = Response::builder()
        .header(header::CACHE_CONTROL, "public, max-age=60")
        .header("x-mirror-cache", how);
    if let Some(t) = info
        .content_type
        .as_deref()
        .and_then(|t| HeaderValue::from_str(t).ok())
    {
        builder = builder.header(header::CONTENT_TYPE, t);
    }
    builder
        .body(Body::from(body))
        .unwrap_or_else(|_| refuse(StatusCode::INTERNAL_SERVER_ERROR, "response"))
}

async fn meta(app: &Arc<App>, origin: &Origin) -> Response {
    let fresh = |info: &MetaInfo| {
        Utc::now()
            .signed_duration_since(info.fetched)
            .to_std()
            .is_ok_and(|age| age < app.meta_ttl)
    };
    let read = || {
        let app = Arc::clone(app);
        let (url, host) = (origin.url.clone(), origin.host);
        async move {
            tokio::task::spawn_blocking(move || app.store.read_meta(&url, host))
                .await
                .ok()
                .flatten()
        }
    };
    if let Some((info, body)) = read().await
        && fresh(&info)
    {
        return meta_response(&info, body, "fresh");
    }
    // One refresh per URL at a time.
    let key_lock = Arc::clone(lock(&app.meta_locks).entry(origin.url.clone()).or_default());
    let _refreshing = key_lock.lock().await;
    let cached = read().await;
    if let Some((info, body)) = &cached
        && fresh(info)
    {
        return meta_response(info, body.clone(), "fresh");
    }
    let fetched = tokio::time::timeout(META_TIMEOUT, fetch_meta(app, origin))
        .await
        .unwrap_or(Err(MetaFailure::Failed));
    lock(&app.meta_locks).remove(&origin.url);
    match (fetched, cached) {
        (Ok((content_type, body)), _) => {
            let info = MetaInfo {
                url: origin.url.clone(),
                fetched: Utc::now(),
                content_type,
            };
            let store_app = Arc::clone(app);
            let (host, kept_info, kept_body) = (origin.host, info.clone(), body.clone());
            let _ = tokio::task::spawn_blocking(move || {
                // Like files: kept only within the quota and the free space.
                if !store_app.store.may_keep(Some(kept_body.len() as u64)) {
                    return;
                }
                if let Err(e) = store_app.store.write_meta(host, &kept_info, &kept_body) {
                    eprintln!("keeping {} failed: {e}", kept_info.url);
                }
            })
            .await;
            meta_response(&info, body, "fetched")
        }
        (Err(_), Some((info, body))) => meta_response(&info, body, "stale"),
        (Err(MetaFailure::Missing), None) => refuse(StatusCode::NOT_FOUND, "not at the origin"),
        (Err(MetaFailure::Failed), None) => refuse(StatusCode::BAD_GATEWAY, "origin failed"),
    }
}

async fn fetch_meta(app: &App, origin: &Origin) -> Result<(Option<String>, Vec<u8>), MetaFailure> {
    let _meta_permit = app
        .meta_fetches
        .acquire()
        .await
        .map_err(|_| MetaFailure::Failed)?;
    let _permit = app
        .fetches
        .acquire()
        .await
        .map_err(|_| MetaFailure::Failed)?;
    let mut response = app
        .client
        .get((app.upstream)(origin))
        .send()
        .await
        .map_err(|e| {
            eprintln!("fetching {} failed: {e}", origin.url);
            MetaFailure::Failed
        })?;
    let status = response.status();
    if status == reqwest::StatusCode::NOT_FOUND || status == reqwest::StatusCode::GONE {
        return Err(MetaFailure::Missing);
    }
    if !status.is_success() {
        eprintln!("fetching {} answered {status}", origin.url);
        return Err(MetaFailure::Failed);
    }
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(safe_type);
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| MetaFailure::Failed)? {
        body.extend_from_slice(&chunk);
        if body.len() > MAX_META {
            return Err(MetaFailure::Failed);
        }
    }
    Ok((content_type, body))
}

pub fn router(app: Arc<App>) -> Router {
    Router::new()
        .route("/v1/health", get(health))
        .fallback(mirror)
        .with_state(app)
}
