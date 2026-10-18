use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

use crate::api::{App, Limiter, router};
use crate::store::Store;

fn app() -> (tempfile::TempDir, Arc<App>) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path()).unwrap();
    (
        dir,
        Arc::new(App {
            store,
            limiter: Limiter::default(),
        }),
    )
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

async fn send(app: &Arc<App>, path: &str, body: Value, gzip: bool) -> (StatusCode, Value) {
    let bytes = serde_json::to_vec(&body).unwrap();
    let mut request = Request::post(path)
        .header("content-type", "application/json")
        .header("x-real-ip", "203.0.113.7");
    let bytes = if gzip {
        use std::io::Write;
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        gz.write_all(&bytes).unwrap();
        request = request.header("content-encoding", "gzip");
        gz.finish().unwrap()
    } else {
        bytes
    };
    let response = router(Arc::clone(app))
        .oneshot(request.body(Body::from(bytes)).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&body).unwrap_or(Value::Null))
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
    assert_eq!(app.store.prune(-1, 400).unwrap(), 1);
    assert!(app.store.groups(30, None, 10).is_empty());
}
