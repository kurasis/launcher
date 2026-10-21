use std::io::Cursor;
use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use base64::Engine as _;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

use crate::api::{self, App};
use crate::auth::Tokens;
use crate::model::{self, Incoming};
use crate::pow::{self, Challenges};
use crate::store::{Filter, Sender, Store};

const INSTALL: &str = "0f1e2d3c-4b5a-4978-8796-a5b4c3d2e1f0";

fn app() -> (tempfile::TempDir, Arc<App>) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path()).unwrap();
    // Easy work in tests: a challenge is solved in a few tries.
    store.set_setting("pow_bits", "4").unwrap();
    let app = Arc::new(App::new(
        store,
        Tokens::new("https://oauth.example"),
        64 * 1024 * 1024,
    ));
    (dir, app)
}

fn png(width: u32, height: u32) -> Vec<u8> {
    let image = image::RgbaImage::from_pixel(width, height, image::Rgba([10, 200, 30, 255]));
    let mut out = Cursor::new(Vec::new());
    image.write_to(&mut out, image::ImageFormat::Png).unwrap();
    out.into_inner()
}

fn review(extra: Value) -> Value {
    let mut body = json!({
        "schema": 1,
        "challenge": "",
        "nonce": "0",
        "kind": "bug",
        "category": "launch",
        "subcategory": "crashes",
        "message": "The game closes when I open a world with Sodium.",
        "launcherVersion": "0.9.1",
        "os": "Windows 11 x86_64",
        "language": "ru",
        "installId": INSTALL,
        "files": [],
    });
    for (key, value) in extra.as_object().unwrap() {
        body[key] = value.clone();
    }
    body
}

async fn call(app: &Arc<App>, request: Request<Body>) -> (StatusCode, Value) {
    let response = api::router(Arc::clone(app)).oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

fn from(ip: &str) -> axum::http::request::Builder {
    Request::builder().header("x-real-ip", ip)
}

/// A solved challenge put into the body.
async fn solved(app: &Arc<App>, mut body: Value) -> Value {
    let (status, answer) = call(
        app,
        from("198.51.100.1")
            .uri("/v1/challenge")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let challenge = answer["challenge"].as_str().unwrap().to_owned();
    let bits = answer["bits"].as_u64().unwrap() as u8;
    body["nonce"] = json!(pow::solve(&challenge, bits));
    body["challenge"] = json!(challenge);
    body
}

async fn send(app: &Arc<App>, ip: &str, body: Value) -> (StatusCode, Value) {
    let body = solved(app, body).await;
    call(
        app,
        from(ip)
            .method("POST")
            .uri("/v1/reviews")
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap(),
    )
    .await
}

#[tokio::test]
async fn a_review_is_stored_and_answered() {
    let (_dir, app) = app();
    let picture = base64::engine::general_purpose::STANDARD.encode(png(3000, 1500));
    let (status, sent) = send(
        &app,
        "198.51.100.7",
        review(json!({
            "files": [
                { "name": "C:\\Users\\steve\\AppData\\logs\\latest.log", "kind": "text", "text": "[12:00:01] [main/INFO]: Loading\n" },
                { "name": "latest.log", "kind": "text", "text": "second\n" },
                { "name": "shot.png", "kind": "image", "data": picture },
            ]
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{sent}");
    let id = sent["id"].as_i64().unwrap();
    let key = sent["key"].as_str().unwrap().to_owned();
    assert_eq!(key.len(), 64);

    let stored = app.store.get(id).unwrap().unwrap();
    assert_eq!(stored.status, "new");
    assert_eq!(stored.install_id.as_deref(), Some(INSTALL));
    let names: Vec<&str> = stored.files.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(names, ["screenshot-1.png", "latest.log", "2-latest.log"]);
    let (_, text) = app.store.file(id, "latest.log").unwrap().unwrap();
    assert_eq!(text, b"[12:00:01] [main/INFO]: Loading\n");
    // The picture is ours: scaled to 2560 pixels at most.
    let (_, bytes) = app.store.file(id, "screenshot-1.png").unwrap().unwrap();
    let decoded = image::load_from_memory(&bytes).unwrap();
    assert_eq!((decoded.width(), decoded.height()), (2560, 1280));

    let ask = |key: &str| {
        from("198.51.100.7")
            .method("POST")
            .uri("/v1/status")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({ "reviews": [{ "id": id, "key": key }] }).to_string(),
            ))
            .unwrap()
    };
    let (_, answer) = call(&app, ask(&key)).await;
    assert_eq!(answer["reviews"][0]["status"], "sent");
    assert!(answer["reviews"][0].get("reply").is_none());

    app.store
        .reply(id, "Fixed in 0.9.2, thank you!", 1)
        .unwrap();
    let (_, answer) = call(&app, ask(&key)).await;
    assert_eq!(answer["reviews"][0]["status"], "answered");
    assert_eq!(answer["reviews"][0]["reply"], "Fixed in 0.9.2, thank you!");

    // Another key learns nothing.
    let (_, answer) = call(&app, ask(&"a".repeat(64))).await;
    assert_eq!(answer["reviews"][0]["status"], "gone");
    assert!(answer["reviews"][0].get("reply").is_none());

    let delete = |key: &str| {
        from("198.51.100.7")
            .method("DELETE")
            .uri(format!("/v1/reviews/{id}"))
            .header("x-review-key", key)
            .body(Body::empty())
            .unwrap()
    };
    assert_eq!(
        call(&app, delete(&"b".repeat(64))).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(call(&app, delete(&key)).await.0, StatusCode::NO_CONTENT);
    assert!(app.store.get(id).unwrap().is_none());
    assert!(app.store.file(id, "latest.log").unwrap().is_none());
}

#[tokio::test]
async fn a_challenge_is_needed_and_taken_once() {
    let (_dir, app) = app();
    let body = solved(&app, review(json!({}))).await;
    let post = |body: &Value| {
        from("198.51.100.8")
            .method("POST")
            .uri("/v1/reviews")
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap()
    };
    assert_eq!(call(&app, post(&body)).await.0, StatusCode::CREATED);
    assert_eq!(
        call(&app, post(&body)).await.0,
        StatusCode::FORBIDDEN,
        "reused"
    );
    let mut forged = body.clone();
    forged["challenge"] = json!("0.9999999999.00.00");
    assert_eq!(call(&app, post(&forged)).await.0, StatusCode::FORBIDDEN);

    let challenges = Challenges::new([1; 32]);
    let challenge = challenges.issue(8, 100);
    let nonce = pow::solve(&challenge, 8);
    assert!(
        !challenges.take(&challenge, &nonce, 100 + pow::LIFETIME),
        "expired"
    );
    assert!(
        !Challenges::new([2; 32]).take(&challenge, &nonce, 100),
        "other secret"
    );
    assert!(!challenges.take(&challenge, "12a", 100));
    assert!(challenges.take(&challenge, &nonce, 100));
}

#[tokio::test]
async fn junk_is_refused() {
    let (_dir, app) = app();
    let cases = [
        json!({ "message": "short" }),
        json!({ "message": "x".repeat(5001) }),
        json!({ "kind": "rant" }),
        json!({ "rating": 5 }),
        json!({ "launcherVersion": "1.0; rm -rf /" }),
        json!({ "installId": "not-an-id" }),
        json!({ "files": [{ "name": "evil.exe", "kind": "text", "text": "MZ" }] }),
        json!({ "files": [{ "name": "a.log", "kind": "text", "text": "PK\u{3}\u{4}\u{0}\u{0}" }] }),
        json!({ "files": [{ "name": "a.log", "kind": "text", "text": "x".repeat(512 * 1024 + 1) }] }),
        json!({ "files": [{ "name": "a.png", "kind": "image", "data": "not base64!" }] }),
        json!({ "files": [{ "name": "a.png", "kind": "image", "data": base64::engine::general_purpose::STANDARD.encode(b"GIF89a junk") }] }),
        json!({ "files": [{ "name": "a.png", "kind": "image", "data": base64::engine::general_purpose::STANDARD.encode(&png(4, 4)[..40]) }] }),
        json!({ "unknown": 1 }),
    ];
    for (n, extra) in cases.into_iter().enumerate() {
        let (status, body) = send(&app, &format!("203.0.113.{n}"), review(extra)).await;
        assert!(
            status == StatusCode::UNPROCESSABLE_ENTITY || status == StatusCode::BAD_REQUEST,
            "case {n}: {status} {body}"
        );
    }
    assert!(
        app.store
            .list(&Filter {
                suspect: true,
                limit: 10,
                ..Default::default()
            })
            .unwrap()
            .is_empty()
    );
}

#[test]
fn a_huge_picture_is_refused_before_decoding() {
    // A PNG header that claims 60000 × 60000 pixels.
    let mut bytes = png(1, 1);
    bytes[16..20].copy_from_slice(&60000u32.to_be_bytes());
    bytes[20..24].copy_from_slice(&60000u32.to_be_bytes());
    assert!(crate::files::picture(&bytes).is_err());
    assert!(crate::files::picture(&png(7000, 7000)).is_err(), "49 Mpx");
}

#[test]
fn checks_keep_what_is_safe() {
    assert_eq!(
        model::place("idea", "mods", Some("suggest")),
        Some(("mods".into(), "suggest".into()))
    );
    assert_eq!(
        model::place("idea", "mods", Some("newer")),
        Some(("mods".into(), "other".into()))
    );
    assert_eq!(
        model::place("idea", "space", None),
        Some(("other".into(), "other".into()))
    );
    assert_eq!(model::place("rant", "mods", None), None);
    assert_eq!(
        model::text_name("../../etc/passwd.log").as_deref(),
        Some("passwd.log")
    );
    assert_eq!(
        model::text_name("моды конфиг.toml").as_deref(),
        Some("___________.toml")
    );
    assert_eq!(model::text_name(".bashrc"), None);
    assert_eq!(model::text_name("run.sh"), None);
    assert_eq!(
        model::clean("a\u{1b}]52;c;x\u{7}b\u{202E}c\r\n\td"),
        "a\u{FFFD}]52;c;x\u{FFFD}b\u{FFFD}c\n\td"
    );
    assert!(model::looks_like_spam(&"https://x.example ".repeat(6)));
    assert!(model::looks_like_spam(
        "!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!"
    ));
    assert!(!model::looks_like_spam(
        "Лаунчер не видит Java 21 после обновления"
    ));
    let incoming: Incoming =
        serde_json::from_value(review(json!({ "kind": "impression", "rating": 4 }))).unwrap();
    assert_eq!(incoming.check().unwrap().rating, Some(4));
}

#[tokio::test]
async fn limits_and_bans() {
    let (_dir, app) = app();
    for n in 0..5 {
        let message = format!("Review number {n} about the launcher interface.");
        let (status, _) = send(&app, "2001:db8::1", review(json!({ "message": message }))).await;
        assert_eq!(status, StatusCode::CREATED);
    }
    // The same /64 is one sender.
    let (status, _) = send(&app, "2001:db8::ffff", review(json!({}))).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);

    // Per install and day, from any address.
    for n in 0..5 {
        let message = format!("Another review number {n} about the interface.");
        let (status, _) = send(
            &app,
            &format!("192.0.2.{n}"),
            review(json!({ "message": message })),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
    }
    let (status, _) = send(&app, "192.0.2.50", review(json!({}))).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);

    // A banned network is told "stored", and nothing is.
    let other = json!({ "installId": "11111111-2222-4333-8444-555555555555" });
    let (_, first) = send(&app, "198.51.100.20", review(other.clone())).await;
    let first = first["id"].as_i64().unwrap();
    app.store
        .ban("net", first, None, Some("spam"), api::now())
        .unwrap();
    let before = app.store.all_since(0).unwrap();
    let (status, fake) = send(
        &app,
        "198.51.100.20",
        review(json!({ "installId": null, "message": "Buy cheap gold at the best shop ever!" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert!(fake["id"].as_i64().unwrap() > first);
    assert_eq!(app.store.all_since(0).unwrap(), before);

    // A repeated text from one network is kept but marked.
    let (_, again) = send(&app, "198.51.100.30", review(json!({ "installId": null }))).await;
    let (_, repeat) = send(&app, "198.51.100.30", review(json!({ "installId": null }))).await;
    assert!(
        !app.store
            .get(again["id"].as_i64().unwrap())
            .unwrap()
            .unwrap()
            .suspect
    );
    assert!(
        app.store
            .get(repeat["id"].as_i64().unwrap())
            .unwrap()
            .unwrap()
            .suspect
    );
}

#[tokio::test]
async fn paused_and_account_tokens() {
    let (_dir, app) = app();
    app.store.set_setting("paused", "1").unwrap();
    let (status, body) = call(
        &app,
        from("198.51.100.1")
            .uri("/v1/challenge")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(
        (status, body["error"].as_str()),
        (StatusCode::SERVICE_UNAVAILABLE, Some("paused"))
    );
    app.store.set_setting("paused", "0").unwrap();

    // Without the account service's keys a token cannot be checked.
    let body = solved(&app, review(json!({}))).await;
    let request = from("198.51.100.2")
        .method("POST")
        .uri("/v1/reviews")
        .header("content-type", "application/json")
        .header("authorization", "Bearer x.y.z")
        .body(Body::from(body.to_string()))
        .unwrap();
    assert_eq!(call(&app, request).await.0, StatusCode::SERVICE_UNAVAILABLE);
}

#[test]
fn tokens_name_reviews() {
    use ed25519_dalek::{Signer, SigningKey};
    let b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD;
    let key = SigningKey::from_bytes(&[9; 32]);
    let tokens = Tokens::new("https://oauth.example/");
    tokens.set_jwks(
        &json!({ "keys": [{ "kty": "OKP", "crv": "Ed25519", "kid": "k1",
        "x": b64.encode(key.verifying_key().to_bytes()) }] }),
    );
    let make = |aud: &[&str], exp: i64| {
        let header = b64.encode(json!({ "alg": "EdDSA", "kid": "k1" }).to_string());
        let payload = b64.encode(
            json!({ "iss": "https://oauth.example", "sub": INSTALL, "aud": aud, "iat": 100, "exp": exp })
                .to_string(),
        );
        let signature = key.sign(format!("{header}.{payload}").as_bytes());
        format!("{header}.{payload}.{}", b64.encode(signature.to_bytes()))
    };
    assert_eq!(
        tokens
            .verify(&make(&["account", "reviews"], 700), 200)
            .as_deref(),
        Some(INSTALL)
    );
    assert!(
        tokens
            .verify(&make(&["account", "skins"], 700), 200)
            .is_none()
    );
    assert!(tokens.verify(&make(&["reviews"], 700), 701).is_none());
}

#[test]
fn prune_forget_and_overview() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path()).unwrap();
    let incoming: Incoming = serde_json::from_value(review(json!({
        "files": [{ "name": "latest.log", "kind": "text", "text": "secret line" }]
    })))
    .unwrap();
    let checked = incoming.check().unwrap();
    let sender = Sender {
        install_id: Some(INSTALL.into()),
        account: None,
        net: store.net("192.0.2.1"),
    };
    let upload = || {
        vec![crate::store::Upload {
            name: "latest.log".into(),
            kind: "text",
            bytes: b"secret line".to_vec(),
        }]
    };
    let day = 86_400;
    let old = store
        .add(&checked, &sender, &"a".repeat(64), upload(), 0)
        .unwrap();
    let mid = store
        .add(&checked, &sender, &"b".repeat(64), upload(), 99 * day)
        .unwrap();
    let new = store
        .add(&checked, &sender, &"c".repeat(64), upload(), 190 * day)
        .unwrap();
    assert_ne!(sender.net, "192.0.2.1");

    let overview = store.overview(365, 190 * day).unwrap().to_string();
    assert!(!overview.contains("Sodium") && !overview.contains(INSTALL));

    assert_eq!(store.prune(190 * day).unwrap(), (1, 1));
    assert!(store.get(old).unwrap().is_none());
    let kept = store.get(mid).unwrap().unwrap();
    assert!(kept.files_gone && kept.files.is_empty());
    assert!(!dir.path().join("files").join(mid.to_string()).exists());
    assert_eq!(store.get(new).unwrap().unwrap().files.len(), 1);

    assert_eq!(store.forget(INSTALL).unwrap(), 2);
    assert!(store.get(new).unwrap().is_none());
    let backup = store.backup(&dir.path().join("backups"), 2).unwrap();
    assert!(backup.exists());
}
