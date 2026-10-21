//! The skins API through its router, with tokens signed as the account
//! service signs them and a directory of nicks in memory.

use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::Body;
use axum::http::{HeaderMap, Request, StatusCode, header};
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64;
use ed25519_dalek::{Signer, SigningKey};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

use crate::api::{App, now};
use crate::auth::Tokens;
use crate::directory::{Accounts, Directory};
use crate::store::Store;

const ISSUER: &str = "https://oauth.example";
const PUBLIC: &str = "https://skins.example";

type Boxed<'a, T> = Pin<Box<dyn Future<Output = Result<T, String>> + Send + 'a>>;

/// Nicks (changeable), how often a nick was asked for, sessions that ended,
/// accounts deleted (with when).
struct People {
    nicks: Arc<Mutex<BTreeMap<String, String>>>,
    asked: Arc<AtomicUsize>,
    ended: Arc<Mutex<Vec<String>>>,
    deleted: Arc<Mutex<Vec<(String, i64)>>>,
}

impl Accounts for People {
    fn find<'a>(&'a self, nick: &'a str) -> Boxed<'a, Option<(String, String)>> {
        Box::pin(async move {
            self.asked.fetch_add(1, Ordering::SeqCst);
            Ok(self
                .nicks
                .lock()
                .unwrap()
                .iter()
                .find(|(_, n)| n.eq_ignore_ascii_case(nick))
                .map(|(id, n)| (id.clone(), n.clone())))
        })
    }

    fn ended<'a>(&'a self, sessions: &'a [String]) -> Boxed<'a, Vec<String>> {
        Box::pin(async move {
            let ended = self.ended.lock().unwrap();
            Ok(sessions
                .iter()
                .filter(|s| ended.contains(s))
                .cloned()
                .collect())
        })
    }

    fn deleted(&self, since: i64) -> Boxed<'_, Vec<(String, i64)>> {
        Box::pin(async move {
            Ok(self
                .deleted
                .lock()
                .unwrap()
                .iter()
                .filter(|(_, at)| *at > since)
                .cloned()
                .collect())
        })
    }
}

fn id(n: u8) -> String {
    format!("00000000-0000-4000-8000-0000000000{n:02x}")
}

const NICKS: [&str; 4] = ["", "Alice", "Bob", "Carol"];

struct T {
    _dir: tempfile::TempDir,
    nicks: Arc<Mutex<BTreeMap<String, String>>>,
    asked: Arc<AtomicUsize>,
    ended: Arc<Mutex<Vec<String>>>,
    deleted: Arc<Mutex<Vec<(String, i64)>>>,
    app: Arc<App>,
    router: Router,
    key: SigningKey,
    /// The next account that has uploaded nothing yet (refusals count
    /// against the hourly limit too).
    fresh: AtomicUsize,
}

fn setup() -> T {
    let dir = tempfile::tempdir().unwrap();
    let key = SigningKey::from_bytes(&[5; 32]);
    let tokens = Tokens::new(ISSUER);
    tokens.set_jwks(&json!({ "keys": [{
        "kty": "OKP", "crv": "Ed25519", "kid": "k1",
        "x": B64.encode(key.verifying_key().as_bytes()),
    }]}));
    let nicks = Arc::new(Mutex::new(
        (1..=3)
            .map(|n| (id(n), NICKS[n as usize].to_owned()))
            .collect(),
    ));
    let asked = Arc::new(AtomicUsize::new(0));
    let ended = Arc::new(Mutex::new(Vec::new()));
    let deleted = Arc::new(Mutex::new(Vec::new()));
    let people = People {
        nicks: Arc::clone(&nicks),
        asked: Arc::clone(&asked),
        ended: Arc::clone(&ended),
        deleted: Arc::clone(&deleted),
    };
    let app = Arc::new(App::new(
        Store::open(dir.path()).unwrap(),
        tokens,
        Directory::new(Box::new(people)),
        PUBLIC,
    ));
    T {
        _dir: dir,
        nicks,
        asked,
        ended,
        deleted,
        router: crate::api::router(Arc::clone(&app)),
        app,
        key,
        fresh: AtomicUsize::new(100),
    }
}

struct Answer {
    status: StatusCode,
    headers: HeaderMap,
    bytes: Vec<u8>,
}

impl Answer {
    fn json(&self) -> Value {
        serde_json::from_slice(&self.bytes).unwrap_or(Value::Null)
    }

    fn header(&self, name: header::HeaderName) -> &str {
        self.headers
            .get(name)
            .map_or("", |v| v.to_str().unwrap_or(""))
    }
}

impl T {
    fn token_with(&self, claims: Value) -> String {
        let header = json!({ "alg": "EdDSA", "typ": "JWT", "kid": "k1" });
        let input = format!(
            "{}.{}",
            B64.encode(header.to_string()),
            B64.encode(claims.to_string())
        );
        let signature = self.key.sign(input.as_bytes());
        format!("{input}.{}", B64.encode(signature.to_bytes()))
    }

    fn claims(who: u8) -> Value {
        json!({
            "iss": ISSUER, "sub": id(who), "aud": ["account", "friends", "skins"],
            "iat": now(), "exp": now() + 600, "sid": format!("s{who}"),
            "nick": NICKS.get(who as usize).copied().unwrap_or("Tester"), "ev": false,
        })
    }

    fn token(&self, who: u8) -> String {
        self.token_with(Self::claims(who))
    }

    async fn send(&self, request: Request<Body>) -> Answer {
        let response = self.router.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let headers = response.headers().clone();
        let bytes = response
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .to_vec();
        Answer {
            status,
            headers,
            bytes,
        }
    }

    async fn get(&self, uri: &str) -> Answer {
        self.send(Request::get(uri).body(Body::empty()).unwrap())
            .await
    }

    async fn call(&self, who: u8, method: &str, uri: &str) -> Answer {
        self.send(
            Request::builder()
                .method(method)
                .uri(uri)
                .header(header::AUTHORIZATION, format!("Bearer {}", self.token(who)))
                .body(Body::empty())
                .unwrap(),
        )
        .await
    }

    /// `PUT /v1/me/<kind>?<query>` with a PNG body.
    async fn upload(&self, who: u8, kind: &str, query: &str, png: Vec<u8>) -> Answer {
        self.upload_as(&self.token(who), kind, query, png).await
    }

    async fn upload_as(&self, token: &str, kind: &str, query: &str, png: Vec<u8>) -> Answer {
        let uri = if query.is_empty() {
            format!("/v1/me/{kind}")
        } else {
            format!("/v1/me/{kind}?{query}")
        };
        self.send(
            Request::put(uri)
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .header(header::CONTENT_TYPE, "image/png")
                .body(Body::from(png))
                .unwrap(),
        )
        .await
    }

    /// Uploads as a fresh account and expects a refusal with this code.
    async fn refused(&self, kind: &str, query: &str, png: Vec<u8>, code: &str) -> StatusCode {
        let who = u8::try_from(self.fresh.fetch_add(1, Ordering::SeqCst)).unwrap();
        let answer = self.upload_as(&self.token(who), kind, query, png).await;
        assert!(
            answer.status.is_client_error(),
            "{query}: {}",
            answer.status
        );
        assert_eq!(answer.json()["error"], code, "{query}");
        answer.status
    }

    fn textures(&self) -> i64 {
        self.app.store.counts().unwrap()["textures"]
            .as_i64()
            .unwrap()
    }
}

/// A PNG of `width`×`height` RGBA pixels, different for every `seed`.
fn image(width: u32, height: u32, seed: u8) -> Vec<u8> {
    let pixels: Vec<u8> = (0..width as usize * height as usize * 4)
        .map(|i| (i % 251) as u8 ^ seed)
        .collect();
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().unwrap();
    writer.write_image_data(&pixels).unwrap();
    writer.finish().unwrap();
    out
}

/// The chunk types of a PNG, in order.
fn chunks(png: &[u8]) -> Vec<String> {
    let mut at = 8;
    let mut names = Vec::new();
    while at + 8 <= png.len() {
        let length = u32::from_be_bytes(png[at..at + 4].try_into().unwrap()) as usize;
        names.push(String::from_utf8_lossy(&png[at + 4..at + 8]).into_owned());
        at += 12 + length;
    }
    names
}

#[tokio::test]
async fn skins_standard_old_hd_and_animated() {
    let t = setup();
    for (width, height, frames) in [
        (64, 64, 1),
        (64, 32, 1),
        (128, 128, 1),
        (256, 256, 1),
        (512, 256, 1),
    ] {
        let answer = t
            .upload(1, "skin", "model=slim", image(width, height, 1))
            .await;
        assert_eq!(answer.status, StatusCode::OK, "{width}×{height}");
        let skin = &answer.json()["skin"];
        assert_eq!(
            (&skin["width"], &skin["height"], &skin["frames"]),
            (&json!(width), &json!(height), &json!(frames))
        );
        assert_eq!(skin["model"], "slim");
        assert_eq!(skin["frame_ms"], json!([]));
    }
    let answer = t.upload(1, "skin", "", image(512, 512, 2)).await;
    assert_eq!(answer.status, StatusCode::OK);
    assert_eq!(answer.json()["skin"]["model"], "default");
    assert_eq!(answer.json()["nick"], "Alice");
    assert!(answer.json()["cape"].is_null());
    // Eight frames, each with its own duration.
    let answer = t
        .upload(
            1,
            "skin",
            "frames=8&frame_ms=50,100,150,200,250,300,350,10000",
            image(64, 64 * 8, 3),
        )
        .await;
    assert_eq!(answer.status, StatusCode::OK);
    let skin = answer.json()["skin"].clone();
    assert_eq!(skin["frames"], 8);
    assert_eq!(
        skin["frame_ms"],
        json!([50, 100, 150, 200, 250, 300, 350, 10000])
    );
    let hash = skin["hash"].as_str().unwrap();
    assert_eq!(
        skin["url"],
        format!("https://skins.example/v1/textures/{hash}")
    );
    // One duration for all, or the default; the frame count may be left out.
    let answer = t
        .upload(1, "skin", "frame_ms=250", image(64, 64 * 4, 4))
        .await;
    assert_eq!(answer.json()["skin"]["frame_ms"], json!(vec![250; 4]));
    let answer = t.upload(1, "skin", "", image(64, 64 * 32, 5)).await;
    assert_eq!(answer.json()["skin"]["frame_ms"], json!(vec![100; 32]));
    // Only the last one stays.
    assert_eq!(t.textures(), 1);
    // The public lookup shows the same, under the current nick.
    let found = t.get("/v1/profile/alice").await;
    assert_eq!(found.status, StatusCode::OK);
    assert_eq!(found.json(), answer.json());
    assert_eq!(found.header(header::CACHE_CONTROL), "public, max-age=60");
}

#[tokio::test]
async fn capes_plain_and_animated() {
    let t = setup();
    let answer = t.upload(2, "cape", "model=slim", image(64, 32, 1)).await;
    assert_eq!(answer.status, StatusCode::OK);
    let cape = answer.json()["cape"].clone();
    assert_eq!(
        (cape["width"].as_u64(), cape["height"].as_u64()),
        (Some(64), Some(32))
    );
    assert!(cape.get("model").is_none(), "capes have no model");
    assert!(answer.json()["skin"].is_null());
    let answer = t
        .upload(2, "cape", "frames=4&frame_ms=500", image(128, 64 * 4, 2))
        .await;
    assert_eq!(answer.status, StatusCode::OK);
    assert_eq!(answer.json()["cape"]["frames"], 4);
    assert_eq!(answer.json()["cape"]["frame_ms"], json!(vec![500; 4]));
    // A 64×64 cape is two frames; as a skin it is one.
    assert_eq!(
        t.upload(2, "cape", "", image(64, 64, 3)).await.json()["cape"]["frames"],
        2
    );
    // A skin's sizes are not a cape's.
    t.refused("cape", "", image(64, 48, 1), "bad_size").await;
    t.refused("cape", "", image(65, 32, 1), "bad_size").await;
    t.refused("cape", "", image(64, 32 * 33, 1), "bad_frames")
        .await;
}

#[tokio::test]
async fn broken_hostile_and_wrong_files_are_refused() {
    let t = setup();
    let good = image(64, 64, 1);
    t.refused("skin", "", b"GIF89a not a png".to_vec(), "bad_png")
        .await;
    t.refused("skin", "", Vec::new(), "bad_png").await;
    t.refused("skin", "", good[..good.len() / 2].to_vec(), "bad_png")
        .await;
    // Cut just before its end (IEND): the whole file must be sound.
    t.refused("skin", "", good[..good.len() - 12].to_vec(), "bad_png")
        .await;
    // A flipped byte fails its checksum.
    let mut flipped = good.clone();
    let middle = flipped.len() / 2;
    flipped[middle] ^= 0x55;
    t.refused("skin", "", flipped, "bad_png").await;
    // A PNG bomb: a small file whose header claims 30000×30000. Refused
    // from the header, before anything is allocated for it.
    let bomb = {
        let mut out = Vec::new();
        let mut encoder = png::Encoder::new(&mut out, 30_000, 30_000);
        encoder.set_color(png::ColorType::Rgba);
        let writer = encoder.write_header().unwrap();
        drop(writer);
        // Some image data after the header, as a real bomb would have.
        out.extend_from_slice(&image(64, 64, 9)[33..]);
        out
    };
    assert!(bomb.len() < 64 * 1024);
    t.refused("skin", "", bomb, "bad_size").await;
    for (width, height) in [
        (65, 65),
        (64, 48),
        (1024, 1024),
        (64, 100),
        (128, 32),
        (512, 768),
    ] {
        t.refused("skin", "", image(width, height, 1), "bad_size")
            .await;
    }
    t.refused("skin", "", image(64, 64 * 33, 1), "bad_frames")
        .await;
    // The old 64×32 layout is one frame only.
    t.refused("skin", "", image(64, 96, 1), "bad_size").await;
    // Too large: told by the length, or found while reading.
    let big = vec![0u8; 2 * 1024 * 1024 + 1];
    assert_eq!(
        t.refused("skin", "", big.clone(), "too_large").await,
        StatusCode::PAYLOAD_TOO_LARGE
    );
    let with_length = t
        .send(
            Request::put("/v1/me/skin")
                .header(header::AUTHORIZATION, format!("Bearer {}", t.token(1)))
                .header(header::CONTENT_TYPE, "image/png")
                .header(header::CONTENT_LENGTH, big.len())
                .body(Body::from(big))
                .unwrap(),
        )
        .await;
    assert_eq!(with_length.status, StatusCode::PAYLOAD_TOO_LARGE);
    t.refused("cape", "", vec![0u8; 1024 * 1024 + 1], "too_large")
        .await;
    // Not sent as a PNG.
    let as_json = t
        .send(
            Request::put("/v1/me/skin")
                .header(header::AUTHORIZATION, format!("Bearer {}", t.token(1)))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(image(64, 64, 1)))
                .unwrap(),
        )
        .await;
    assert_eq!(as_json.status, StatusCode::BAD_REQUEST);
    assert_eq!(as_json.json()["error"], "bad_png");
    // Nothing was kept.
    assert_eq!(t.textures(), 0);
    assert!(t.call(1, "GET", "/v1/me").await.json()["skin"].is_null());
    // Other kinds than skin and cape do not exist.
    assert_eq!(
        t.upload(1, "elytra", "", image(64, 32, 1)).await.status,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn models_frames_and_durations_follow_the_rules() {
    let t = setup();
    let strip = || image(64, 64 * 4, 1);
    for model in ["alex", "Slim", "", "slim%00", "default'--"] {
        t.refused("skin", &format!("model={model}"), strip(), "bad_model")
            .await;
    }
    t.refused("skin", "model=slim&model=default", strip(), "bad_model")
        .await;
    for frames in ["3", "5", "0", "33", "-4", "4.0", "%34%00"] {
        t.refused("skin", &format!("frames={frames}"), strip(), "bad_frames")
            .await;
    }
    for ms in [
        "49",
        "10001",
        "100,100",
        "100,100,100,100,100",
        "1%3BDROP",
        "1;DROP",
        "100%20OR%201=1",
        "",
        "1e2",
        "100,,100,100",
    ] {
        t.refused("skin", &format!("frame_ms={ms}"), strip(), "bad_frame_ms")
            .await;
    }
    // A single frame needs no durations; one given is checked and dropped.
    let one = t.upload(1, "skin", "frame_ms=100", image(64, 64, 2)).await;
    assert_eq!(one.json()["skin"]["frame_ms"], json!([]));
    t.refused("skin", "frame_ms=1", image(64, 64, 2), "bad_frame_ms")
        .await;
    let long = format!("frame_ms={}", vec!["100"; 200].join(","));
    t.refused("skin", &long, strip(), "bad_query").await;
}

#[tokio::test]
async fn nicks_that_cannot_be_never_reach_the_account_service() {
    let t = setup();
    t.upload(1, "skin", "", image(64, 64, 1)).await;
    for nick in [
        "..%2Fx",
        "a",
        "ab",
        "abcdefghijklmnopq",
        "%00",
        "Alice%00",
        "Al%C3%ADce",
        "%D0%90lice", // Cyrillic А
        "%EF%BC%A1lice",
        "Alice%20",
        "%20Alice",
        "'%20OR%201=1--",
        "%3Cscript%3E",
        "%FF%FE",
    ] {
        let answer = t.get(&format!("/v1/profile/{nick}")).await;
        assert_eq!(answer.status, StatusCode::NOT_FOUND, "{nick}");
    }
    assert_eq!(
        t.get("/v1/profile/../x").await.status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(t.asked.load(Ordering::SeqCst), 0);
    let batch = t
        .send(
            Request::post("/v1/profiles")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({ "nicks": ["../x", "a", "x".repeat(17), "Al\u{0}ce", "Аlice", "ALICE"] })
                        .to_string(),
                ))
                .unwrap(),
        )
        .await;
    assert_eq!(batch.status, StatusCode::OK);
    let profiles = &batch.json()["profiles"];
    assert_eq!(profiles["ALICE"]["nick"], "Alice");
    assert!(profiles["../x"].is_null() && profiles["Аlice"].is_null());
    assert_eq!(profiles.as_object().unwrap().len(), 6);
    assert_eq!(t.asked.load(Ordering::SeqCst), 1, "only ALICE was asked");
}

#[tokio::test]
async fn lookups_follow_the_nick_and_are_kept_a_minute() {
    let t = setup();
    // Nobody, someone who wears nothing, someone who wears a cape.
    assert_eq!(
        t.get("/v1/profile/Nobody").await.status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(t.get("/v1/profile/Bob").await.status, StatusCode::NOT_FOUND);
    t.upload(3, "cape", "", image(64, 32, 1)).await;
    let carol = t.get("/v1/profile/carol").await;
    assert_eq!(carol.status, StatusCode::OK);
    assert!(carol.json()["skin"].is_null());
    assert_eq!(carol.json()["nick"], "Carol");
    // Each nick reached the account service once; then it was kept, the
    // one naming nobody too.
    let asked = t.asked.load(Ordering::SeqCst);
    for _ in 0..5 {
        t.get("/v1/profile/Nobody").await;
        t.get("/v1/profile/CAROL").await;
    }
    assert_eq!(t.asked.load(Ordering::SeqCst), asked);
    // The 404 may be kept as long as an answer.
    let missing = t.get("/v1/profile/Nobody").await;
    assert_eq!(missing.header(header::CACHE_CONTROL), "public, max-age=60");
    // A batch: up to 50, answers under the nick as asked.
    let ask = |nicks: Vec<String>| {
        Request::post("/v1/profiles")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(json!({ "nicks": nicks }).to_string()))
            .unwrap()
    };
    let batch = t
        .send(ask(vec!["carol".into(), "Bob".into(), "Nobody".into()]))
        .await;
    assert_eq!(
        batch.json()["profiles"],
        json!({ "carol": carol.json(), "Bob": null, "Nobody": null })
    );
    let fifty: Vec<String> = (0..50).map(|i| format!("Player_{i}")).collect();
    assert_eq!(t.send(ask(fifty.clone())).await.status, StatusCode::OK);
    let mut many = fifty;
    many.push("Carol".into());
    let too_many = t.send(ask(many)).await;
    assert_eq!(too_many.status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(too_many.json()["error"], "too_many");
    for junk in ["", "{", "[]", r#"{"nicks":"Carol"}"#, r#"{"nicks":[1,2]}"#] {
        let answer = t
            .send(
                Request::post("/v1/profiles")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(junk))
                    .unwrap(),
            )
            .await;
        assert!(answer.status.is_client_error(), "{junk}");
        assert_eq!(answer.header(header::CACHE_CONTROL), "no-store");
    }
    let huge = format!(r#"{{"nicks":["{}"]}}"#, "x".repeat(20_000));
    let answer = t
        .send(
            Request::post("/v1/profiles")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(huge))
                .unwrap(),
        )
        .await;
    assert_eq!(answer.status, StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn bad_tokens_are_refused() {
    let t = setup();
    let claims = |change: &dyn Fn(&mut Value)| {
        let mut c = T::claims(1);
        change(&mut c);
        t.token_with(c)
    };
    let other = SigningKey::from_bytes(&[6; 32]);
    let foreign = {
        let header = B64.encode(json!({ "alg": "EdDSA", "kid": "k1" }).to_string());
        let payload = B64.encode(T::claims(1).to_string());
        let input = format!("{header}.{payload}");
        format!(
            "{input}.{}",
            B64.encode(other.sign(input.as_bytes()).to_bytes())
        )
    };
    let unsigned = format!(
        "{}.{}.",
        B64.encode(json!({ "alg": "none" }).to_string()),
        B64.encode(T::claims(1).to_string())
    );
    for token in [
        claims(&|c| c["exp"] = json!(now() - 1)),
        claims(&|c| c["aud"] = json!(["account", "friends"])),
        claims(&|c| c["aud"] = json!("skins")),
        claims(&|c| c["iss"] = json!("https://evil")),
        claims(&|c| c["sub"] = json!("../../etc/passwd")),
        claims(&|c| c["sub"] = json!("00000000000040008000000000000001")),
        claims(&|c| c["iat"] = json!(now() + 3600)),
        foreign,
        unsigned,
        "garbage".to_owned(),
        "a.b.c".to_owned(),
        "x".repeat(5000),
    ] {
        for (method, uri) in [
            ("GET", "/v1/me"),
            ("DELETE", "/v1/me/skin"),
            ("GET", "/v1/export"),
        ] {
            let answer = t
                .send(
                    Request::builder()
                        .method(method)
                        .uri(uri)
                        .header(header::AUTHORIZATION, format!("Bearer {token}"))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await;
            assert_eq!(answer.status, StatusCode::UNAUTHORIZED, "{uri}");
        }
        let answer = t
            .send(
                Request::put("/v1/me/skin")
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .header(header::CONTENT_TYPE, "image/png")
                    .body(Body::from(image(64, 64, 1)))
                    .unwrap(),
            )
            .await;
        assert_eq!(answer.status, StatusCode::UNAUTHORIZED);
    }
    assert_eq!(t.get("/v1/me").await.status, StatusCode::UNAUTHORIZED);
    assert_eq!(t.textures(), 0);
    // A session that ended: its still-signed token stops working.
    assert_eq!(t.call(2, "GET", "/v1/me").await.status, StatusCode::OK);
    t.ended.lock().unwrap().push("s2".to_owned());
    crate::api::check_sessions(&t.app).await;
    assert_eq!(
        t.call(2, "GET", "/v1/me").await.status,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(t.call(1, "GET", "/v1/me").await.status, StatusCode::OK);
}

#[tokio::test]
async fn ten_uploads_an_hour() {
    let t = setup();
    for i in 0..10 {
        assert_eq!(
            t.upload(1, "skin", "", image(64, 64, i)).await.status,
            StatusCode::OK
        );
    }
    let eleventh = t.upload(1, "cape", "", image(64, 32, 1)).await;
    assert_eq!(eleventh.status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(eleventh.json()["error"], "rate_limited");
    // Skins and capes together; other accounts are not affected.
    assert_eq!(
        t.upload(2, "skin", "", image(64, 64, 9)).await.status,
        StatusCode::OK
    );
    assert_eq!(t.textures(), 1, "the same image for both");
}

#[tokio::test]
async fn replaced_and_removed_textures_go_unless_someone_wears_them() {
    let t = setup();
    let first = t.upload(1, "skin", "", image(64, 64, 1)).await.json()["skin"]["hash"]
        .as_str()
        .unwrap()
        .to_owned();
    // Bob wears the same picture: one texture for both.
    let bobs = t.upload(2, "skin", "", image(64, 64, 1)).await.json()["skin"]["hash"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(first, bobs);
    assert_eq!(t.textures(), 1);
    // Alice changes hers: Bob still wears the first.
    let second = t.upload(1, "skin", "", image(64, 64, 2)).await.json()["skin"]["hash"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(t.textures(), 2);
    assert_eq!(
        t.get(&format!("/v1/textures/{first}")).await.status,
        StatusCode::OK
    );
    // Alice changes it again: her second goes.
    t.upload(1, "skin", "", image(64, 64, 3)).await;
    assert_eq!(t.textures(), 2);
    assert_eq!(
        t.get(&format!("/v1/textures/{second}")).await.status,
        StatusCode::NOT_FOUND
    );
    // Bob takes his off: the first goes; the profile is gone.
    let off = t.call(2, "DELETE", "/v1/me/skin").await;
    assert_eq!(off.status, StatusCode::OK);
    assert!(off.json()["skin"].is_null());
    assert_eq!(t.textures(), 1);
    assert_eq!(t.get("/v1/profile/Bob").await.status, StatusCode::NOT_FOUND);
    // Taking off what one does not wear changes nothing.
    assert_eq!(
        t.call(2, "DELETE", "/v1/me/cape").await.status,
        StatusCode::OK
    );
    // The owner's command, by account ID.
    t.upload(1, "cape", "", image(64, 32, 4)).await;
    assert!(crate::remove(&t.app.store, &[id(1), "cape".into()]).is_ok());
    assert!(t.call(1, "GET", "/v1/me").await.json()["cape"].is_null());
    assert!(!t.call(1, "GET", "/v1/me").await.json()["skin"].is_null());
    assert!(crate::remove(&t.app.store, &[id(1), "cape".into()]).is_err());
    assert!(crate::remove(&t.app.store, &["Alice".into()]).is_err());
    assert!(crate::remove(&t.app.store, &[id(1), "hat".into()]).is_err());
    assert!(crate::remove(&t.app.store, &[id(1)]).is_ok());
    assert_eq!(t.textures(), 0);
}

#[tokio::test]
async fn a_deleted_account_leaves_nothing_behind() {
    let t = setup();
    t.upload(1, "skin", "", image(64, 64, 1)).await;
    t.upload(1, "cape", "", image(64, 32, 1)).await;
    t.upload(2, "skin", "", image(64, 64, 2)).await;
    assert_eq!(t.get("/v1/profile/Alice").await.status, StatusCode::OK);
    // The account service says Alice deleted her account (and her nick is
    // free again).
    t.nicks.lock().unwrap().remove(&id(1));
    t.deleted.lock().unwrap().push((id(1), now()));
    crate::api::purge_deleted(&t.app).await;
    assert_eq!(t.textures(), 1);
    assert!(t.app.store.worn(&id(1)).unwrap().is_empty());
    assert_eq!(
        t.get("/v1/profile/Alice").await.status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(t.get("/v1/profile/Bob").await.status, StatusCode::OK);
    // Asked again, nothing older comes back; junk IDs are skipped.
    t.deleted
        .lock()
        .unwrap()
        .push(("' OR 1=1 --".to_owned(), now() + 1));
    crate::api::purge_deleted(&t.app).await;
    assert_eq!(t.textures(), 1);
    // Her token, still signed, wears nothing new after the cleaning.
    assert_eq!(
        t.upload(1, "skin", "", image(64, 64, 3)).await.status,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(t.textures(), 1);
}

#[tokio::test]
async fn textures_are_served_for_good_and_nothing_else() {
    let t = setup();
    let skin = t.upload(1, "skin", "", image(64, 64, 1)).await.json()["skin"].clone();
    let hash = skin["hash"].as_str().unwrap();
    let answer = t.get(&format!("/v1/textures/{hash}")).await;
    assert_eq!(answer.status, StatusCode::OK);
    assert_eq!(answer.header(header::CONTENT_TYPE), "image/png");
    assert_eq!(
        answer.header(header::CACHE_CONTROL),
        "public, max-age=31536000, immutable"
    );
    assert_eq!(answer.header(header::X_CONTENT_TYPE_OPTIONS), "nosniff");
    assert_eq!(answer.header(header::ETAG), format!("\"{hash}\""));
    // What is served is what the name says.
    use sha2::Digest as _;
    let digest: String = sha2::Sha256::digest(&answer.bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    assert_eq!(digest, hash);
    let again = t
        .send(
            Request::get(format!("/v1/textures/{hash}"))
                .header(header::IF_NONE_MATCH, format!("\"{hash}\""))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(again.status, StatusCode::NOT_MODIFIED);
    assert!(again.bytes.is_empty());
    for bad in [
        hash.to_uppercase(),
        hash[..63].to_owned(),
        format!("{hash}0"),
        "..%2F..%2Fskins.db".to_owned(),
        "%00".to_owned(),
        "0".repeat(64),
    ] {
        let answer = t.get(&format!("/v1/textures/{bad}")).await;
        assert_eq!(answer.status, StatusCode::NOT_FOUND, "{bad}");
        assert_eq!(answer.header(header::CACHE_CONTROL), "no-store");
    }
}

#[tokio::test]
async fn kept_files_are_our_own_with_no_extra_chunks() {
    let t = setup();
    // A grey, palette and 16-bit picture with a text chunk, a gamma and a
    // time: kept as plain RGBA, nothing else.
    let mut upload = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut upload, 64, 64);
        encoder.set_color(png::ColorType::Indexed);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_palette(vec![255, 0, 0, 0, 255, 0]);
        encoder.set_trns(vec![255, 0]);
        encoder.set_source_gamma(png::ScaledFloat::new(0.45));
        encoder
            .add_text_chunk("Comment".into(), "<script>alert(1)</script>".into())
            .unwrap();
        let mut writer = encoder.write_header().unwrap();
        writer
            .write_chunk(png::chunk::ChunkType(*b"tIME"), &[7, 234, 10, 19, 12, 0, 0])
            .unwrap();
        let pixels: Vec<u8> = (0..64 * 64).map(|i| (i % 2) as u8).collect();
        writer.write_image_data(&pixels).unwrap();
        writer.finish().unwrap();
    }
    assert!(chunks(&upload).contains(&"tEXt".to_owned()));
    let hash = t.upload(1, "skin", "", upload).await.json()["skin"]["hash"]
        .as_str()
        .unwrap()
        .to_owned();
    let kept = t.app.store.texture(&hash).unwrap().unwrap();
    assert_eq!(chunks(&kept), ["IHDR", "IDAT", "IEND"]);
    assert!(!kept.windows(6).any(|w| w == b"script"));
    let decoded = {
        let mut reader = png::Decoder::new(std::io::Cursor::new(&kept))
            .read_info()
            .unwrap();
        let mut buffer = vec![0; reader.output_buffer_size().unwrap()];
        reader.next_frame(&mut buffer).unwrap();
        assert_eq!(
            reader.output_color_type(),
            (png::ColorType::Rgba, png::BitDepth::Eight)
        );
        buffer
    };
    assert_eq!(&decoded[..8], &[255, 0, 0, 255, 0, 255, 0, 0]);
    // 16-bit grey with alpha is taken too.
    let mut grey = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut grey, 64, 32);
        encoder.set_color(png::ColorType::GrayscaleAlpha);
        encoder.set_depth(png::BitDepth::Sixteen);
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(&vec![0x80; 64 * 32 * 4]).unwrap();
        writer.finish().unwrap();
    }
    let answer = t.upload(1, "cape", "", grey).await;
    assert_eq!(answer.status, StatusCode::OK);
}

#[tokio::test]
async fn the_own_profile_and_export() {
    let t = setup();
    let empty = t.call(1, "GET", "/v1/me").await;
    assert_eq!(empty.status, StatusCode::OK);
    assert_eq!(
        empty.json(),
        json!({ "nick": "Alice", "skin": null, "cape": null })
    );
    assert_eq!(empty.header(header::CACHE_CONTROL), "no-store");
    t.upload(1, "skin", "model=slim", image(64, 64, 1)).await;
    t.upload(2, "cape", "", image(64, 32, 1)).await;
    let export = t.call(1, "GET", "/v1/export").await;
    assert_eq!(export.status, StatusCode::OK);
    let data = export.json();
    assert_eq!(data["account"], id(1));
    assert_eq!(data["skin"]["model"], "slim");
    assert!(data["skin"]["hash"].as_str().is_some());
    assert!(data["skin"]["updated"].as_i64().unwrap() > 0);
    assert!(data["cape"].is_null(), "only one's own");
    assert!(!export.bytes.windows(4).any(|w| w == b"\x89PNG"));
    for _ in 0..9 {
        t.call(1, "GET", "/v1/export").await;
    }
    assert_eq!(
        t.call(1, "GET", "/v1/export").await.status,
        StatusCode::TOO_MANY_REQUESTS
    );
}

#[tokio::test]
async fn each_address_has_its_limit() {
    let t = setup();
    let from = |ip: &str| {
        Request::get("/v1/textures/nothing")
            .header("x-real-ip", ip)
            .body(Body::empty())
            .unwrap()
    };
    for _ in 0..crate::api::READS_PER_MINUTE {
        assert_eq!(
            t.send(from("203.0.113.7")).await.status,
            StatusCode::NOT_FOUND
        );
    }
    let limited = t.send(from("203.0.113.7")).await;
    assert_eq!(limited.status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(limited.header(header::CACHE_CONTROL), "no-store");
    assert_eq!(
        t.send(from("203.0.113.8")).await.status,
        StatusCode::NOT_FOUND
    );
    // An IPv6 /64 is one address.
    for _ in 0..crate::api::READS_PER_MINUTE {
        t.send(from("2001:db8:1:2::1")).await;
    }
    assert_eq!(
        t.send(from("2001:db8:1:2:ffff::9")).await.status,
        StatusCode::TOO_MANY_REQUESTS
    );
    assert_eq!(
        t.send(from("2001:db8:1:3::1")).await.status,
        StatusCode::NOT_FOUND
    );
    // The health check is not counted.
    assert_eq!(t.get("/v1/health").await.json()["service"], "lapi-skins");
}

#[tokio::test]
async fn backups_are_whole_private_and_only_the_newest_stay() {
    let t = setup();
    t.upload(1, "skin", "", image(64, 64, 1)).await;
    let dir = t._dir.path().join("backups");
    let first = t.app.store.backup(&dir, 1).unwrap();
    let restored = tempfile::tempdir().unwrap();
    std::fs::copy(&first, restored.path().join("skins.db")).unwrap();
    let copy = Store::open(restored.path()).unwrap();
    assert_eq!(copy.counts().unwrap(), t.app.store.counts().unwrap());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&first).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    let second = t.app.store.backup(&dir, 1).unwrap();
    assert!(!first.exists() && second.exists(), "only the newest stays");
}
