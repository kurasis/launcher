use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use http_body_util::BodyExt;
use tower::ServiceExt;

use crate::api::{App, router};
use crate::origin::Origin;
use crate::store::{GB, Store};

/// A stand-in for all origins on a local port.
#[derive(Default)]
struct Fake {
    hits: Mutex<HashMap<String, u32>>,
    down: AtomicBool,
    version: AtomicU32,
}

impl Fake {
    fn hits(&self, path: &str) -> u32 {
        self.hits.lock().unwrap().get(path).copied().unwrap_or(0)
    }
}

async fn origin_handler(State(fake): State<Arc<Fake>>, request: Request) -> Response {
    let path = request.uri().path().to_owned();
    *fake.hits.lock().unwrap().entry(path.clone()).or_default() += 1;
    if fake.down.load(Ordering::SeqCst) {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    match path.as_str() {
        "/libraries.minecraft.net/org/lib/1.0/lib-1.0.jar" => vec![7u8; 100_000].into_response(),
        "/libraries.minecraft.net/org/slow/1.0/slow-1.0.jar" => {
            tokio::time::sleep(Duration::from_millis(300)).await;
            vec![1u8; 50_000].into_response()
        }
        "/libraries.minecraft.net/org/cut/1.0/cut-1.0.jar" => {
            // Claims more than it sends.
            let mut headers = HeaderMap::new();
            headers.insert(header::CONTENT_LENGTH, "100000".parse().unwrap());
            (headers, Body::from(vec![1u8; 10])).into_response()
        }
        "/meta.fabricmc.net/v2/page" => (
            [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
            "<script>alert(1)</script>",
        )
            .into_response(),
        "/piston-meta.mojang.com/mc/game/version_manifest_v2.json" => (
            [(header::CONTENT_TYPE, "application/json")],
            format!("{{\"v\":{}}}", fake.version.load(Ordering::SeqCst)),
        )
            .into_response(),
        _ => StatusCode::NOT_FOUND.into_response(),
    }
}

struct Setup {
    _dir: tempfile::TempDir,
    data: std::path::PathBuf,
    app: Arc<App>,
    fake: Arc<Fake>,
}

async fn setup(quota_gb: u64, meta_ttl: Duration) -> Setup {
    let fake = Arc::new(Fake::default());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let origin = axum::Router::new()
        .fallback(origin_handler)
        .with_state(Arc::clone(&fake));
    tokio::spawn(async move { axum::serve(listener, origin).await.unwrap() });
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("data");
    let store = Store::open(&data, quota_gb * GB).unwrap().with_min_free(0);
    store.count();
    let mut app = App::new(
        store,
        reqwest::Client::new(),
        Box::new(move |o: &Origin| {
            o.url
                .replace("https://", &format!("http://127.0.0.1:{port}/"))
        }),
    );
    app.meta_ttl = meta_ttl;
    Setup {
        _dir: dir,
        data,
        app: Arc::new(app),
        fake,
    }
}

async fn get(app: &Arc<App>, path: &str) -> (StatusCode, String, Vec<u8>) {
    let response = router(Arc::clone(app))
        .oneshot(
            Request::get(path)
                .header("x-real-ip", "203.0.113.5")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let how = response
        .headers()
        .get("x-mirror-cache")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_owned();
    let body = response
        .into_body()
        .collect()
        .await
        .map(|b| b.to_bytes().to_vec())
        .unwrap_or_default();
    (status, how, body)
}

/// Waits for the background part of a fetch to put the file in place.
async fn settled(path: &std::path::Path) -> bool {
    for _ in 0..100 {
        if path.exists() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    false
}

const LIB: &str = "/libraries.minecraft.net/org/lib/1.0/lib-1.0.jar";
const MANIFEST: &str = "/piston-meta.mojang.com/mc/game/version_manifest_v2.json";

#[tokio::test]
async fn a_file_is_fetched_once_and_kept_after_the_origin_loses_it() {
    let s = setup(1000, Duration::from_secs(600)).await;
    let (status, how, body) = get(&s.app, LIB).await;
    assert_eq!(
        (status, how.as_str(), body.len()),
        (StatusCode::OK, "fetched", 100_000)
    );
    let kept = s
        .data
        .join("files/libraries.minecraft.net/org/lib/1.0/lib-1.0.jar");
    assert!(settled(&kept).await, "kept where nginx looks");
    s.fake.down.store(true, Ordering::SeqCst);
    let (status, how, body) = get(&s.app, LIB).await;
    assert_eq!((status, how.as_str()), (StatusCode::OK, "kept"));
    assert_eq!(body, vec![7u8; 100_000]);
    assert_eq!(s.fake.hits(LIB), 1);
    assert_eq!(s.app.store.usage(), Some(100_000));
}

#[tokio::test]
async fn concurrent_requests_for_one_file_share_one_fetch() {
    let s = setup(1000, Duration::from_secs(600)).await;
    let path = "/libraries.minecraft.net/org/slow/1.0/slow-1.0.jar";
    let requests: Vec<_> = (0..10)
        .map(|_| {
            let app = Arc::clone(&s.app);
            tokio::spawn(async move { get(&app, path).await })
        })
        .collect();
    for request in requests {
        let (status, _, body) = request.await.unwrap();
        assert_eq!((status, body.len()), (StatusCode::OK, 50_000));
    }
    assert_eq!(s.fake.hits(path), 1);
}

#[tokio::test]
async fn a_cut_off_file_is_not_kept_and_missing_files_are_404() {
    let s = setup(1000, Duration::from_secs(600)).await;
    let cut = "/libraries.minecraft.net/org/cut/1.0/cut-1.0.jar";
    let _ = get(&s.app, cut).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(
        !s.data
            .join("files/libraries.minecraft.net/org/cut/1.0/cut-1.0.jar")
            .exists()
    );
    assert_eq!(
        std::fs::read_dir(s.data.join("tmp")).unwrap().count(),
        0,
        "no parts left"
    );
    let (status, ..) = get(&s.app, "/libraries.minecraft.net/org/none/1.0/none-1.0.jar").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    s.fake.down.store(true, Ordering::SeqCst);
    let (status, ..) = get(&s.app, "/libraries.minecraft.net/org/x/1.0/x-1.0.jar").await;
    assert_eq!(status, StatusCode::BAD_GATEWAY);
}

#[tokio::test]
async fn over_the_quota_files_pass_through_without_being_kept() {
    let s = setup(0, Duration::from_secs(600)).await;
    let (status, _, body) = get(&s.app, LIB).await;
    assert_eq!((status, body.len()), (StatusCode::OK, 100_000));
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(
        !s.data
            .join("files/libraries.minecraft.net/org/lib/1.0/lib-1.0.jar")
            .exists()
    );
    // The quota comes from the deploy's settings file.
    let settings = s.data.join("settings.json");
    std::fs::write(&settings, r#"{"maxGb": 7}"#).unwrap();
    s.app.store.reload_quota(&settings);
    assert_eq!(s.app.store.quota(), 7 * GB);
}

#[tokio::test]
async fn metadata_is_refreshed_and_served_stale_while_the_origin_fails() {
    let s = setup(1000, Duration::from_secs(600)).await;
    let (status, how, body) = get(&s.app, MANIFEST).await;
    assert_eq!(
        (status, how.as_str(), body.as_slice()),
        (StatusCode::OK, "fetched", &b"{\"v\":0}"[..])
    );
    s.fake.version.store(1, Ordering::SeqCst);
    let (_, how, body) = get(&s.app, MANIFEST).await;
    assert_eq!(
        (how.as_str(), body.as_slice()),
        ("fresh", &b"{\"v\":0}"[..]),
        "within its time"
    );
    assert_eq!(s.fake.hits(MANIFEST), 1);

    let s = setup(1000, Duration::ZERO).await;
    get(&s.app, MANIFEST).await;
    s.fake.version.store(2, Ordering::SeqCst);
    let (_, how, body) = get(&s.app, MANIFEST).await;
    assert_eq!(
        (how.as_str(), body.as_slice()),
        ("fetched", &b"{\"v\":2}"[..])
    );
    s.fake.down.store(true, Ordering::SeqCst);
    let (status, how, body) = get(&s.app, MANIFEST).await;
    assert_eq!(
        (status, how.as_str(), body.as_slice()),
        (StatusCode::OK, "stale", &b"{\"v\":2}"[..])
    );
    // Never fetched and the origin down: nothing to give.
    let (status, ..) = get(&s.app, "/meta.fabricmc.net/v2/versions/game").await;
    assert_eq!(status, StatusCode::BAD_GATEWAY);
    s.fake.down.store(false, Ordering::SeqCst);
    let (status, ..) = get(&s.app, "/meta.fabricmc.net/v2/versions/game").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn only_mirrored_addresses_and_get_are_answered() {
    let s = setup(1000, Duration::from_secs(600)).await;
    for path in [
        "/evil.example/x.jar",
        "/127.0.0.1/x",
        "/libraries.minecraft.net/a/%2e%2e/%2e%2e/etc/passwd",
        "/objects.githubusercontent.com/x",
    ] {
        assert_eq!(get(&s.app, path).await.0, StatusCode::NOT_FOUND, "{path}");
    }
    assert_eq!(s.fake.hits.lock().unwrap().len(), 0, "nothing was fetched");
    let post = router(Arc::clone(&s.app))
        .oneshot(Request::post(LIB).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(post.status(), StatusCode::METHOD_NOT_ALLOWED);
    let health = get(&s.app, "/v1/health").await;
    assert_eq!(health.0, StatusCode::OK);
}

#[tokio::test]
async fn a_client_leaving_before_the_fetch_does_not_stop_keeping() {
    let s = setup(1000, Duration::from_secs(600)).await;
    let path = "/libraries.minecraft.net/org/slow/1.0/slow-1.0.jar";
    // The first request leaves while its fetch waits for the origin.
    let first = {
        let app = Arc::clone(&s.app);
        tokio::spawn(async move { get(&app, path).await })
    };
    tokio::time::sleep(Duration::from_millis(50)).await;
    first.abort();
    let _ = first.await;
    let (status, _, body) = get(&s.app, path).await;
    assert_eq!((status, body.len()), (StatusCode::OK, 50_000));
    assert!(
        settled(
            &s.data
                .join("files/libraries.minecraft.net/org/slow/1.0/slow-1.0.jar")
        )
        .await,
        "the next request fetches and keeps it"
    );
}

#[tokio::test]
async fn metadata_never_goes_out_as_a_page_and_is_kept_only_with_room() {
    let s = setup(1000, Duration::from_secs(600)).await;
    let response = router(Arc::clone(&s.app))
        .oneshot(
            Request::get("/meta.fabricmc.net/v2/page")
                .header("x-real-ip", "203.0.113.5")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        response.headers().get(header::CONTENT_TYPE).unwrap(),
        "application/octet-stream"
    );
    let (_, how, _) = get(&s.app, MANIFEST).await;
    assert_eq!(how, "fetched");

    // No room: metadata passes through without being kept.
    let s = setup(0, Duration::from_secs(600)).await;
    let (status, how, _) = get(&s.app, MANIFEST).await;
    assert_eq!((status, how.as_str()), (StatusCode::OK, "fetched"));
    let kept = std::fs::read_dir(s.data.join("meta"))
        .unwrap()
        .flatten()
        .flat_map(|d| std::fs::read_dir(d.path()).into_iter().flatten())
        .count();
    assert_eq!(kept, 0);
}

#[tokio::test]
async fn head_of_a_file_not_kept_fetches_nothing_to_keep() {
    let s = setup(1000, Duration::from_secs(600)).await;
    let response = router(Arc::clone(&s.app))
        .oneshot(
            Request::head(LIB)
                .header("x-real-ip", "203.0.113.5")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(
        !s.data
            .join("files/libraries.minecraft.net/org/lib/1.0/lib-1.0.jar")
            .exists(),
        "nothing kept for a HEAD"
    );
}
