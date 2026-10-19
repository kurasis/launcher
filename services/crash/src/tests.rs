use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

use crate::api::{App, Limiter, router};
use crate::store::Store;

fn app_with(max_stored: i64) -> (tempfile::TempDir, Arc<App>) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path()).unwrap();
    (dir, Arc::new(App::new(store, max_stored)))
}

fn app() -> (tempfile::TempDir, Arc<App>) {
    app_with(i64::MAX)
}

fn report() -> Value {
    json!({
        "schema": 1,
        "installId": "6f1c2a1e-6c7c-4d3e-9a55-0d1f5f0a2b11",
        "launcher": { "version": "0.1.0", "os": "windows", "osVersion": "10.0.22631", "arch": "x86_64", "locale": "ru" },
        "source": "game",
        "kind": "missing_dependency",
        "signature": "missing_dependency:geckolib>fabric-api",
        "analyzerVersion": 1,
        "game": { "version": "1.21.1", "loader": "fabric", "loaderVersion": "0.18.2", "java": "21.0.7", "mods": 3 },
        "summary": { "mod": "GeckoLib 4", "dependency": "fabric-api" },
        "files": [ { "name": "output.log", "text": "Mod 'GeckoLib 4' (geckolib) requires fabric-api" } ]
    })
}

fn gzip(bytes: &[u8]) -> Vec<u8> {
    use std::io::Write;
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    gz.write_all(bytes).unwrap();
    gz.finish().unwrap()
}

/// Posts raw bytes as nginx would pass them, from `ip`.
async fn post(
    app: &Arc<App>,
    path: &str,
    bytes: Vec<u8>,
    gzipped: bool,
    content_type: &str,
    ip: &str,
) -> (StatusCode, Value) {
    let mut request = Request::post(path)
        .header("content-type", content_type)
        .header("x-real-ip", ip);
    if gzipped {
        request = request.header("content-encoding", "gzip");
    }
    let response = router(Arc::clone(app))
        .oneshot(request.body(Body::from(bytes)).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&body).unwrap_or(Value::Null))
}

async fn send_from(app: &Arc<App>, path: &str, body: Value, ip: &str) -> StatusCode {
    let bytes = serde_json::to_vec(&body).unwrap();
    post(app, path, bytes, false, "application/json", ip)
        .await
        .0
}

async fn send(app: &Arc<App>, path: &str, body: Value, gzipped: bool) -> (StatusCode, Value) {
    let bytes = serde_json::to_vec(&body).unwrap();
    let bytes = if gzipped { gzip(&bytes) } else { bytes };
    post(app, path, bytes, gzipped, "application/json", "203.0.113.7").await
}

#[tokio::test]
async fn a_gzip_report_is_stored_grouped_and_readable() {
    let (_dir, app) = app();
    let (status, body) = send(&app, "/v1/reports", report(), true).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let id = body["id"].as_str().unwrap().to_owned();
    send(&app, "/v1/reports", report(), false).await;

    let groups = app.store.groups(30, None, 10);
    assert_eq!(groups.len(), 1);
    assert_eq!((groups[0].reports, groups[0].installs), (2, 1));
    assert_eq!(groups[0].kind, "missing_dependency");

    let stored = app.store.get(&id).unwrap().unwrap();
    assert_eq!(stored.files[0].name, "output.log");
    assert_eq!(stored.game.unwrap().loader, "fabric");
    assert_eq!(
        app.store.list(Some("missing_dependency"), None, 10).len(),
        2
    );
}

#[tokio::test]
async fn bad_reports_are_refused() {
    let (_dir, app) = app();
    for (field, value) in [
        ("installId", json!("not-a-uuid")),
        ("source", json!("server")),
        ("kind", json!("rm -rf /")),
        ("schema", json!(2)),
    ] {
        let mut bad = report();
        bad[field] = value;
        let (status, _) = send(&app, "/v1/reports", bad, false).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{field}");
    }
    let mut bad = report();
    bad["files"] = json!([{ "name": "../../etc/passwd", "text": "x" }]);
    assert_eq!(
        send(&app, "/v1/reports", bad, false).await.0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let mut big = report();
    big["files"] = json!([{ "name": "output.log", "text": "x".repeat(9 * 1024 * 1024) }]);
    let status = send(&app, "/v1/reports", big, true).await.0;
    assert!(
        status == StatusCode::UNPROCESSABLE_ENTITY || status == StatusCode::PAYLOAD_TOO_LARGE,
        "{status}"
    );
    assert!(app.store.groups(30, None, 10).is_empty());
}

#[tokio::test]
async fn one_address_is_limited() {
    let (_dir, app) = app();
    let mut last = StatusCode::OK;
    for _ in 0..21 {
        last = send(&app, "/v1/reports", report(), false).await.0;
    }
    assert_eq!(last, StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn statistics_and_feedback_are_counted() {
    let (_dir, app) = app();
    let event = json!({ "source": "game", "kind": "out_of_memory", "launcherVersion": "0.1.0", "os": "linux", "loader": "forge" });
    let (status, _) = send(
        &app,
        "/v1/stats",
        json!({ "schema": 1, "events": [event.clone(), event] }),
        false,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let helped = json!({ "schema": 1, "kind": "out_of_memory", "helpful": false, "launcherVersion": "0.1.0" });
    assert_eq!(
        send(&app, "/v1/feedback", helped, false).await.0,
        StatusCode::NO_CONTENT
    );
    let summary = app.store.summary(30);
    assert_eq!(app.store.overview(30)["statistics"][0]["count"], 2);
    assert_eq!(summary["causes"][0]["count"], 2);
    assert_eq!(summary["feedback"][0]["not_helped"], 1);
}

#[tokio::test]
async fn health_answers_and_old_reports_are_pruned() {
    let (_dir, app) = app();
    let response = router(Arc::clone(&app))
        .oneshot(Request::get("/v1/health").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    send(&app, "/v1/reports", report(), false).await;
    assert_eq!(app.store.prune(90, 400).unwrap(), 0);
    assert_eq!(
        app.store
            .forget("00000000-0000-4000-8000-000000000000")
            .unwrap(),
        0
    );
    assert_eq!(app.store.prune(-1, 400).unwrap(), 1);
    assert!(app.store.groups(30, None, 10).is_empty());
    send(&app, "/v1/reports", report(), false).await;
    assert_eq!(
        app.store
            .forget("6f1c2a1e-6c7c-4d3e-9a55-0d1f5f0a2b11")
            .unwrap(),
        1
    );
    assert!(app.store.groups(30, None, 10).is_empty());
}

// ------------------------------------------------------------ attacks (D-061)

#[tokio::test]
async fn a_gzip_bomb_is_cut_off_at_the_unpacked_limit() {
    use std::io::Write;
    let (_dir, app) = app();
    // 256 MiB of JSON that packs into about 256 KiB.
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
    let head = serde_json::to_string(&report()).unwrap();
    let head = head.replace(
        "\"text\":\"Mod 'GeckoLib 4' (geckolib) requires fabric-api\"",
        "\"text\":\"",
    );
    gz.write_all(head.split("\"text\":\"").next().unwrap().as_bytes())
        .unwrap();
    gz.write_all(b"\"text\":\"").unwrap();
    let zeros = vec![b'0'; 1024 * 1024];
    for _ in 0..256 {
        gz.write_all(&zeros).unwrap();
    }
    gz.write_all(b"\"}]}").unwrap();
    let bomb = gz.finish().unwrap();
    assert!(
        bomb.len() < 2 * 1024 * 1024,
        "{} bytes pass nginx",
        bomb.len()
    );
    let (status, _) = post(
        &app,
        "/v1/reports",
        bomb,
        true,
        "application/json",
        "203.0.113.7",
    )
    .await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    assert!(app.store.groups(30, None, 10).is_empty());
}

#[tokio::test]
async fn a_limited_address_is_refused_before_its_body_is_read() {
    let (_dir, app) = app();
    for _ in 0..20 {
        send_from(&app, "/v1/reports", json!({}), "198.51.100.1").await;
    }
    // Not 400 or 413: the body (here not even JSON) is never looked at.
    let (status, _) = post(
        &app,
        "/v1/reports",
        vec![b'{'; 64],
        false,
        "application/json",
        "198.51.100.1",
    )
    .await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    // Another address is not affected.
    assert_eq!(
        send_from(&app, "/v1/reports", report(), "198.51.100.2").await,
        StatusCode::CREATED
    );
}

#[tokio::test]
async fn addresses_of_one_ipv6_network_share_a_limit() {
    let (_dir, app) = app();
    for i in 0..20 {
        send_from(
            &app,
            "/v1/feedback",
            json!({}),
            &format!("2001:db8:1:2::{i:x}"),
        )
        .await;
    }
    let mut last = StatusCode::OK;
    for i in 0..101 {
        last = send_from(
            &app,
            "/v1/feedback",
            json!({}),
            &format!("2001:db8:1:2:{i:x}::1"),
        )
        .await;
    }
    assert_eq!(last, StatusCode::TOO_MANY_REQUESTS, "same /64");
    assert_ne!(
        send_from(&app, "/v1/feedback", json!({}), "2001:db8:1:3::1").await,
        StatusCode::TOO_MANY_REQUESTS,
        "another /64"
    );
}

#[test]
fn many_addresses_together_meet_the_overall_limit() {
    let limiter = Limiter::default();
    let allowed = (0..50u8)
        .filter(|i| limiter.allow(std::net::IpAddr::from([10, 0, 0, *i]), "reports", 5, 30))
        .count();
    assert_eq!(allowed, 30);
    // An address over its own limit does not use up everyone's allowance.
    let limiter = Limiter::default();
    let one = std::net::IpAddr::from([10, 0, 0, 1]);
    for _ in 0..100 {
        limiter.allow(one, "reports", 5, 30);
    }
    assert!(limiter.allow(std::net::IpAddr::from([10, 0, 0, 2]), "reports", 5, 30));
}

#[tokio::test]
async fn at_the_storage_limit_the_oldest_reports_make_room() {
    let (_dir, app) = app_with(1);
    let (status, first) = send(&app, "/v1/reports", report(), false).await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, second) = send(&app, "/v1/reports", report(), false).await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "a flood cannot stop new reports"
    );
    assert_ne!(first["id"], second["id"]);
    assert!(
        app.store
            .get(first["id"].as_str().unwrap())
            .unwrap()
            .is_none()
    );
    assert!(
        app.store
            .get(second["id"].as_str().unwrap())
            .unwrap()
            .is_some()
    );
}

#[tokio::test]
async fn statistics_stop_at_the_daily_row_limit() {
    let (_dir, app) = app();
    let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
    app.store.fill_feedback_for_test(&today, 20_000);
    let answer = json!({ "schema": 1, "kind": "out_of_memory", "helpful": true, "launcherVersion": "0.1.0" });
    assert_eq!(
        send(&app, "/v1/feedback", answer, false).await.0,
        StatusCode::INSUFFICIENT_STORAGE
    );
}

#[tokio::test]
async fn malformed_requests_are_refused_and_nothing_is_stored() {
    let (_dir, app) = app();
    let ip = "192.0.2.10";
    let json = serde_json::to_vec(&report()).unwrap();
    // A browser page can only send "simple" types without asking first.
    assert_eq!(
        post(&app, "/v1/reports", json.clone(), false, "text/plain", ip)
            .await
            .0,
        StatusCode::UNSUPPORTED_MEDIA_TYPE
    );
    assert_eq!(
        post(
            &app,
            "/v1/reports",
            b"{\"schema\":".to_vec(),
            false,
            "application/json",
            ip
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    // Claimed gzip that is not.
    assert!(
        post(&app, "/v1/reports", json, true, "application/json", ip)
            .await
            .0
            .is_client_error()
    );
    // Deep nesting stops at the parser's depth limit.
    let nested = format!(
        "{}\"summary\":{}{}}}",
        &serde_json::to_string(&report()).unwrap()[..1],
        "[".repeat(5000),
        "]".repeat(5000)
    );
    assert!(
        post(
            &app,
            "/v1/reports",
            nested.into_bytes(),
            false,
            "application/json",
            ip
        )
        .await
        .0
        .is_client_error()
    );
    for (field, value) in [
        ("signature", json!("ok\u{1b}[2Jcleared")),
        (
            "files",
            json!(
                (0..13)
                    .map(|i| json!({ "name": format!("f{i}.log"), "text": "" }))
                    .collect::<Vec<_>>()
            ),
        ),
        ("note", json!("x".repeat(2001))),
        ("summary", json!({ "big": "x".repeat(17 * 1024) })),
    ] {
        let mut bad = report();
        bad[field] = value;
        assert_eq!(
            send_from(&app, "/v1/reports", bad, ip).await,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{field}"
        );
    }
    let get = router(Arc::clone(&app))
        .oneshot(Request::get("/v1/reports").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(
        get.status(),
        StatusCode::METHOD_NOT_ALLOWED,
        "no reading over HTTP"
    );
    assert!(app.store.groups(30, None, 10).is_empty());
}

#[tokio::test]
async fn hostile_text_is_stored_as_text_and_unknown_fields_are_dropped() {
    let (dir, app) = app();
    let mut hostile = report();
    hostile["signature"] = json!("x'); DROP TABLE reports; --");
    hostile["note"] = json!("<script>alert(1)</script> \u{1b}]52;c;Y2xpcA==\u{7}");
    hostile["evil"] = json!("not kept");
    let (status, body) = send(&app, "/v1/reports", hostile, false).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let groups = app.store.groups(30, None, 10);
    assert_eq!(groups[0].signature, "x'); DROP TABLE reports; --");
    let stored = app
        .store
        .get(body["id"].as_str().unwrap())
        .unwrap()
        .unwrap();
    assert!(stored.note.starts_with("<script>"));
    let mut raw = Vec::new();
    for entry in walk(&dir.path().join("reports")) {
        std::io::Read::read_to_end(
            &mut flate2::read::GzDecoder::new(std::fs::File::open(entry).unwrap()),
            &mut raw,
        )
        .unwrap();
    }
    assert!(!String::from_utf8_lossy(&raw).contains("not kept"));
    // A report ID is a UUID the service made; any other text finds nothing.
    assert!(app.store.get("../../crash.db").unwrap().is_none());
    assert!(app.store.get("' OR 1=1 --").unwrap().is_none());
}

fn walk(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut found = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            found.extend(walk(&path));
        } else {
            found.push(path);
        }
    }
    found
}

#[tokio::test]
async fn the_public_overview_holds_counts_only() {
    let (_dir, app) = app();
    let mut report = report();
    report["note"] = json!("my secret server play.example.net");
    report["signature"] = json!("``` [click](https://evil.example)");
    send(&app, "/v1/reports", report, false).await;
    let event = json!({ "source": "game", "kind": "out_of_memory", "launcherVersion": "0.1.0", "os": "linux", "loader": "`evil`" });
    send(
        &app,
        "/v1/stats",
        json!({ "schema": 1, "events": [event] }),
        false,
    )
    .await;
    let text = app.store.overview(30).to_string();
    assert!(text.contains("missing_dependency") && text.contains("out_of_memory"));
    for leaked in ["secret", "evil", "`", "click"] {
        assert!(!text.contains(leaked), "{leaked} in {text}");
    }
}

#[tokio::test]
async fn stats_and_feedback_take_small_bodies_only() {
    let (_dir, app) = app();
    let event = json!({ "source": "game", "kind": "out_of_memory", "launcherVersion": "0.1.0", "os": "linux", "loader": "x".repeat(200_000) });
    for path in ["/v1/stats", "/v1/feedback"] {
        let (status, _) = send(&app, path, json!({ "schema": 1, "events": [event] }), true).await;
        assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE, "{path}");
    }
}

#[test]
fn every_cause_the_launcher_names_is_known_to_the_summary() {
    let catalog: Value =
        serde_json::from_str(include_str!("../../../src/locales/messages/en.json")).unwrap();
    let kinds = catalog["ui"]["crash"]["kinds"].as_object().unwrap();
    assert!(kinds.len() > 30);
    for kind in kinds.keys() {
        assert!(crate::store::KINDS.contains(&kind.as_str()), "{kind}");
    }
}
