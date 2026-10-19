//! The catalog end to end with a fake source: Modrinth's answers come from a
//! table, curl never runs, names resolve without DNS. Tokens are signed as
//! the account service signs them.

use std::collections::HashMap;
use std::net::IpAddr;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64;
use ed25519_dalek::{Signer, SigningKey};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use sha1::Digest as _;
use tower::ServiceExt;

use crate::fetch::{self, Client, Curl, FetchError, Reply, Transport};
use crate::import::{self, now};
use crate::input::{self, Input};
use crate::{App, Urls, model, modrinth};

const ISSUER: &str = "https://oauth.example";
const FILES: &str = "https://files.example";

#[derive(Clone, Default)]
struct Page {
    status: u16,
    body: Vec<u8>,
    location: Option<String>,
    challenge: bool,
}

fn ok(body: impl Into<Vec<u8>>) -> Page {
    Page {
        status: 200,
        body: body.into(),
        ..Page::default()
    }
}

/// The sites, as a table of addresses; what was asked, in order.
#[derive(Default)]
struct Sites {
    pages: Mutex<HashMap<String, Page>>,
    asked: Mutex<Vec<fetch::Request>>,
}

impl Sites {
    fn put(&self, url: &str, page: Page) {
        self.pages.lock().unwrap().insert(url.to_owned(), page);
    }

    fn json(&self, url: &str, value: Value) {
        self.put(url, ok(value.to_string()));
    }

    fn asked(&self, url: &str) -> usize {
        self.asked
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r.url == url)
            .count()
    }
}

struct Shared(Arc<Sites>);

impl Transport for Shared {
    fn run(
        &self,
        request: fetch::Request,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Reply, FetchError>> + Send + '_>>
    {
        Box::pin(async move {
            self.0.asked.lock().unwrap().push(request.clone());
            let page = self
                .0
                .pages
                .lock()
                .unwrap()
                .get(&request.url)
                .cloned()
                .unwrap_or(Page {
                    status: 404,
                    ..Page::default()
                });
            if page.body.len() as u64 > request.max_bytes {
                return Err(FetchError::TooBig);
            }
            std::fs::write(&request.to, &page.body).unwrap();
            Ok(Reply {
                status: page.status,
                size: page.body.len() as u64,
                location: page.location,
                challenge: page.challenge,
            })
        })
    }
}

fn resolver(ip: [u8; 4]) -> fetch::Resolver {
    Box::new(move |_host| Box::pin(async move { vec![IpAddr::from(ip)] }))
}

struct T {
    _dir: tempfile::TempDir,
    sites: Arc<Sites>,
    app: Arc<App>,
    router: Router,
    key: SigningKey,
}

fn setup() -> T {
    setup_with(Vec::new())
}

fn setup_with(proxies: Vec<String>) -> T {
    let dir = tempfile::tempdir().unwrap();
    let key = SigningKey::from_bytes(&[5; 32]);
    let tokens = crate::auth::Tokens::new(ISSUER);
    tokens.set_jwks(&json!({ "keys": [{
        "kty": "OKP", "crv": "Ed25519", "kid": "k1",
        "x": B64.encode(key.verifying_key().as_bytes()),
    }]}));
    let blobs = crate::blobs::Blobs::open(dir.path(), 100 * crate::blobs::GB, 0).unwrap();
    blobs.count();
    let sites = Arc::new(Sites::default());
    let client = Client::new(
        Box::new(Shared(Arc::clone(&sites))),
        proxies,
        false,
        blobs.tmp(),
    )
    .with_resolver(resolver([93, 184, 216, 34]));
    let app = Arc::new(App {
        store: crate::store::Store::open(dir.path()).unwrap(),
        blobs,
        client,
        tokens,
        limiter: crate::api::Limiter::default(),
        urls: Urls::new(FILES),
        thumbs: import::Thumbs::Inline,
    });
    T {
        _dir: dir,
        sites,
        router: crate::api::router(Arc::clone(&app)),
        app,
        key,
    }
}

fn account(n: u8) -> String {
    format!("0190a000-0000-7000-8000-0000000000{n:02}")
}

impl T {
    fn token_with(&self, aud: &[&str], who: u8) -> String {
        let header = json!({ "alg": "EdDSA", "typ": "JWT", "kid": "k1" });
        let claims = json!({
            "iss": ISSUER, "sub": account(who), "aud": aud,
            "iat": now(), "exp": now() + 600, "sid": format!("s{who}"), "nick": "Player", "ev": true,
        });
        let input = format!(
            "{}.{}",
            B64.encode(header.to_string()),
            B64.encode(claims.to_string())
        );
        let signature = self.key.sign(input.as_bytes());
        format!("{input}.{}", B64.encode(signature.to_bytes()))
    }

    async fn call(
        &self,
        token: Option<&str>,
        method: &str,
        uri: &str,
        body: Value,
    ) -> (StatusCode, Value) {
        let mut request = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::CONTENT_TYPE, "application/json")
            .header("x-real-ip", "203.0.113.9");
        if let Some(token) = token {
            request = request.header(header::AUTHORIZATION, format!("Bearer {token}"));
        }
        let request = request
            .body(if body.is_null() {
                Body::empty()
            } else {
                Body::from(body.to_string())
            })
            .unwrap();
        let response = self.router.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    async fn get(&self, uri: &str) -> (StatusCode, Value) {
        self.call(None, "GET", uri, Value::Null).await
    }

    async fn player(&self, who: u8, method: &str, uri: &str, body: Value) -> (StatusCode, Value) {
        let token = self.token_with(&["account", "mods"], who);
        self.call(Some(&token), method, uri, body).await
    }

    /// Runs every job, retries included, as the workers would (time skips
    /// ahead), then settles the requests.
    async fn drain(&self) {
        for _ in 0..2000 {
            let Some(job) = self.app.store.claim(now() + 30 * 86_400).unwrap() else {
                import::check_requests(&self.app);
                if self.app.store.claim(now() + 30 * 86_400).unwrap().is_none() {
                    return;
                }
                self.app.store.requeue_running().unwrap();
                continue;
            };
            match import::run(&self.app, &job).await {
                Ok(()) => self.app.store.job_done(job.id, now()).unwrap(),
                Err(e) => {
                    let again = (e.retry && job.attempts < 12)
                        .then(|| now() + import::backoff(job.attempts));
                    self.app
                        .store
                        .job_failed(job.id, &e.code, again, now())
                        .unwrap();
                }
            }
        }
        panic!("the queue did not empty");
    }
}

fn png(width: u32, height: u32) -> Vec<u8> {
    let picture = image::RgbImage::from_fn(width, height, |x, y| {
        image::Rgb([(x % 256) as u8, (y % 256) as u8, 90])
    });
    let mut out = std::io::Cursor::new(Vec::new());
    picture.write_to(&mut out, image::ImageFormat::Png).unwrap();
    out.into_inner()
}

fn hashes(bytes: &[u8]) -> (String, String, String) {
    (
        hex::encode(sha1::Sha1::digest(bytes)),
        hex::encode(sha2::Sha512::digest(bytes)),
        hex::encode(sha2::Sha256::digest(bytes)),
    )
}

const SODIUM: &str = "AANobbMI";
const FABRIC_API: &str = "P7dR8mSH";

fn file_json(project: &str, version: &str, name: &str, bytes: &[u8]) -> Value {
    let (sha1, sha512, _) = hashes(bytes);
    json!({
        "filename": name, "size": bytes.len(), "primary": true,
        "hashes": { "sha1": sha1, "sha512": sha512 },
        "url": format!("https://cdn.modrinth.com/data/{project}/versions/{version}/{name}"),
    })
}

/// Sodium with three versions for one game version and loader (the newest
/// release is kept at once), a dependency, an icon, a gallery picture and
/// pictures in its description.
fn modrinth_site(sites: &Sites) -> HashMap<&'static str, Vec<u8>> {
    let mut files = HashMap::new();
    for (version, name) in [
        ("v1aaaaaa", "sodium-0.5.jar"),
        ("v2bbbbbb", "sodium-0.6.jar"),
        ("v3cccccc", "sodium-0.7-beta.jar"),
    ] {
        let bytes = format!("jar of {name}").into_bytes();
        sites.put(
            &format!("https://cdn.modrinth.com/data/{SODIUM}/versions/{version}/{name}"),
            ok(bytes.clone()),
        );
        files.insert(version, bytes);
    }
    let project = json!({
        "id": SODIUM, "slug": "sodium", "project_type": "mod",
        "title": "Sodium\u{202E}", "description": "Rendering engine",
        "body": "Fast.\n![shot](https://i.imgur.com/body1.png)\n<img src=\"https://evil.example/x.png\">",
        "license": { "id": "PolyForm-Shield-1.0.0" },
        "icon_url": format!("https://cdn.modrinth.com/data/{SODIUM}/icon.png"),
        "client_side": "required", "server_side": "unsupported",
        "downloads": 1000, "followers": 10,
        "categories": ["optimization", "BAD CATEGORY!"],
        "gallery": [{ "url": "https://i.imgur.com/gallery1.png", "title": "Before", "ordering": 0 }],
        "published": "2020-01-01T00:00:00Z", "updated": "2024-01-01T00:00:00Z",
    });
    sites.json(&modrinth::project_url("sodium"), project.clone());
    sites.json(&modrinth::project_url(SODIUM), project);
    sites.json(
        &modrinth::members_url(SODIUM),
        json!([{ "user": { "username": "jellysquid3" }, "role": "Owner", "ordering": 0 }]),
    );
    let version = |id: &str, number: &str, channel: &str, date: &str, name: &str| {
        json!({
            "id": id, "version_number": number, "name": number, "version_type": channel,
            "date_published": date, "game_versions": ["1.21.1"], "loaders": ["fabric"],
            "files": [file_json(SODIUM, id, name, &files[id])],
            "dependencies": [{ "project_id": FABRIC_API, "dependency_type": "required" }],
        })
    };
    sites.json(
        &modrinth::versions_url(SODIUM),
        json!([
            version(
                "v3cccccc",
                "0.7-beta",
                "beta",
                "2024-03-01T00:00:00Z",
                "sodium-0.7-beta.jar"
            ),
            version(
                "v2bbbbbb",
                "0.6",
                "release",
                "2024-02-01T00:00:00Z",
                "sodium-0.6.jar"
            ),
            version(
                "v1aaaaaa",
                "0.5",
                "release",
                "2024-01-01T00:00:00Z",
                "sodium-0.5.jar"
            ),
        ]),
    );
    sites.put(
        &format!("https://cdn.modrinth.com/data/{SODIUM}/icon.png"),
        ok(png(400, 400)),
    );
    sites.put("https://i.imgur.com/gallery1.png", ok(png(1200, 600)));
    sites.put("https://i.imgur.com/body1.png", ok(png(20, 20)));
    // Fabric API: one version, no pictures.
    let api_jar = b"jar of fabric-api".to_vec();
    sites.put(
        &format!("https://cdn.modrinth.com/data/{FABRIC_API}/versions/faaaaaaa/fabric-api.jar"),
        ok(api_jar.clone()),
    );
    sites.json(
        &modrinth::project_url(FABRIC_API),
        json!({ "id": FABRIC_API, "slug": "fabric-api", "project_type": "mod", "title": "Fabric API" }),
    );
    sites.json(&modrinth::members_url(FABRIC_API), json!([]));
    sites.json(
        &modrinth::versions_url(FABRIC_API),
        json!([{
            "id": "faaaaaaa", "version_number": "0.100", "version_type": "release",
            "date_published": "2024-01-01T00:00:00Z", "game_versions": ["1.21.1"], "loaders": ["fabric"],
            "files": [file_json(FABRIC_API, "faaaaaaa", "fabric-api.jar", &api_jar)],
        }]),
    );
    files.insert("faaaaaaa", api_jar);
    files
}

#[tokio::test]
async fn an_address_brings_the_project_its_newest_files_pictures_and_dependencies() {
    let t = setup();
    let jars = modrinth_site(&t.sites);
    let (status, request) = t
        .player(
            1,
            "POST",
            "/v1/imports",
            json!({ "input": "https://modrinth.com/mod/sodium" }),
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{request}");
    assert_eq!(request["state"], "queued");
    t.drain().await;

    let id = request["id"].as_str().unwrap();
    let (_, request) = t
        .player(1, "GET", &format!("/v1/imports/{id}"), Value::Null)
        .await;
    assert_eq!(request["state"], "ready", "{request}");
    let project_id = request["projectId"].as_str().unwrap().to_owned();

    let (status, project) = t.get("/v1/projects/sodium").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(project["id"], project_id);
    // Direction characters do not survive.
    assert_eq!(project["title"], "Sodium");
    assert_eq!(project["categories"], json!(["optimization"]));
    assert_eq!(project["authors"][0]["name"], "jellysquid3");
    assert!(
        project["icon"]
            .as_str()
            .unwrap()
            .starts_with("https://files.example/i/")
    );
    assert!(project["iconThumb"].as_str().unwrap().ends_with(".webp"));
    assert_eq!(project["gallery"].as_array().unwrap().len(), 1);
    assert!(
        project["gallery"][0]["thumb"]
            .as_str()
            .unwrap()
            .ends_with(".jpg")
    );
    // The description points at our copy; a site we do not fetch stays as is.
    let body = project["body"].as_str().unwrap();
    assert!(!body.contains("i.imgur.com"), "{body}");
    assert!(body.contains("https://files.example/i/"), "{body}");
    assert!(body.contains("https://evil.example/x.png"));

    let (_, versions) = t
        .get("/v1/projects/sodium/versions?game=1.21.1&loader=fabric")
        .await;
    let versions = versions["versions"].as_array().unwrap().clone();
    assert_eq!(versions.len(), 3);
    let saved: Vec<(&str, &str)> = versions
        .iter()
        .map(|v| (v["number"].as_str().unwrap(), v["saved"].as_str().unwrap()))
        .collect();
    assert_eq!(
        saved,
        [("0.7-beta", "none"), ("0.6", "all"), ("0.5", "none")]
    );
    let kept = &versions[1]["files"][0];
    let (_, _, sha256) = hashes(&jars["v2bbbbbb"]);
    assert_eq!(kept["sha256"], sha256.as_str());
    assert_eq!(
        kept["url"],
        format!("https://files.example/f/{sha256}/sodium%2D0%2E6%2Ejar")
    );
    assert_eq!(
        std::fs::read(t.app.blobs.path(&sha256)).unwrap(),
        jars["v2bbbbbb"]
    );
    // The older version's files were not fetched.
    assert_eq!(
        t.sites.asked(&format!(
            "https://cdn.modrinth.com/data/{SODIUM}/versions/v1aaaaaa/sodium-0.5.jar"
        )),
        0
    );
    // The dependency came along, with its file.
    assert_eq!(versions[1]["dependencies"][0]["kind"], "required");
    let dependency = versions[1]["dependencies"][0]["projectId"]
        .as_str()
        .unwrap();
    let (_, api_versions) = t.get(&format!("/v1/projects/{dependency}/versions")).await;
    assert_eq!(api_versions["versions"][0]["saved"], "all");

    // Searching finds both.
    let (_, found) = t.get("/v1/search?q=sodi").await;
    assert_eq!(found["total"], 1);
    assert_eq!(found["hits"][0]["slug"], "sodium");
    let (_, all) = t
        .get("/v1/search?kind=mod&game=1.21.1&loader=fabric&sort=title")
        .await;
    assert_eq!(all["total"], 2);
    assert_eq!(all["hits"][0]["title"], "Fabric API");

    // An older version on request, kept for good.
    let older = versions[2]["id"].as_str().unwrap();
    let (status, answer) = t
        .call(
            None,
            "POST",
            &format!("/v1/versions/{older}/fetch"),
            Value::Null,
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{answer}");
    assert_eq!(answer["queued"], true);
    t.drain().await;
    let (_, older) = t.get(&format!("/v1/versions/{older}")).await;
    assert_eq!(older["saved"], "all");

    // The launcher finds its files by hash.
    let (sha1, _, _) = hashes(&jars["v2bbbbbb"]);
    let (status, found) = t
        .call(
            None,
            "POST",
            "/v1/lookup",
            json!({ "hashes": [sha1, "0".repeat(64)] }),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(found["found"].as_array().unwrap().len(), 1);
    assert_eq!(found["found"][0]["projectId"], project_id);

    // Asked again: nothing is downloaded twice.
    let before = t.sites.asked.lock().unwrap().len();
    let (_, again) = t
        .player(
            2,
            "POST",
            "/v1/imports",
            json!({ "input": "modrinth.com/mod/sodium" }),
        )
        .await;
    t.drain().await;
    let (_, again) = t
        .player(
            2,
            "GET",
            &format!("/v1/imports/{}", again["id"].as_str().unwrap()),
            Value::Null,
        )
        .await;
    assert_eq!(again["state"], "ready");
    assert_eq!(again["projectId"], project_id);
    assert_eq!(t.sites.asked.lock().unwrap().len(), before);
}

#[tokio::test]
async fn a_file_that_does_not_match_its_hashes_is_not_kept() {
    let t = setup();
    modrinth_site(&t.sites);
    // The CDN now gives something else for the newest release.
    t.sites.put(
        &format!("https://cdn.modrinth.com/data/{SODIUM}/versions/v2bbbbbb/sodium-0.6.jar"),
        ok(b"jar of sodium-0.6.jaX".to_vec()),
    );
    t.player(
        1,
        "POST",
        "/v1/imports",
        json!({ "input": "https://modrinth.com/mod/sodium" }),
    )
    .await;
    t.drain().await;
    let (_, versions) = t.get("/v1/projects/sodium/versions").await;
    assert_eq!(versions["versions"][1]["saved"], "none");
    assert!(versions["versions"][1]["files"][0]["url"].is_null());
    assert_eq!(t.app.store.counts().unwrap()["jobsFailed"], 1);
}

#[tokio::test]
async fn a_name_offers_projects_to_choose_from() {
    let t = setup();
    modrinth_site(&t.sites);
    t.sites.json(
        &modrinth::search_url("sodium mod"),
        json!({ "hits": [
            { "project_id": SODIUM, "slug": "sodium", "project_type": "mod", "title": "Sodium",
              "author": "jellysquid3", "downloads": 5, "icon_url": "https://cdn.modrinth.com/x.png" },
            { "project_id": "plugin01", "slug": "a-plugin", "project_type": "plugin", "title": "Not ours" },
        ]}),
    );
    let (_, request) = t
        .player(1, "POST", "/v1/imports", json!({ "input": "sodium mod" }))
        .await;
    let id = request["id"].as_str().unwrap().to_owned();
    t.drain().await;
    let (_, request) = t
        .player(1, "GET", &format!("/v1/imports/{id}"), Value::Null)
        .await;
    assert_eq!(request["state"], "choosing");
    let candidates = request["candidates"].as_array().unwrap();
    assert_eq!(candidates.len(), 1);
    // No pictures from other sites reach the launcher.
    assert!(candidates[0].get("iconUrl").is_none());

    // Only what was offered, and only by whoever asked.
    let uri = format!("/v1/imports/{id}/choose");
    let (status, _) = t
        .player(
            1,
            "POST",
            &uri,
            json!({ "source": "modrinth", "id": FABRIC_API }),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = t
        .player(
            2,
            "POST",
            &uri,
            json!({ "source": "modrinth", "id": SODIUM }),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, chosen) = t
        .player(
            1,
            "POST",
            &uri,
            json!({ "source": "modrinth", "id": SODIUM }),
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{chosen}");
    t.drain().await;
    let (_, request) = t
        .player(1, "GET", &format!("/v1/imports/{id}"), Value::Null)
        .await;
    assert_eq!(request["state"], "ready");
    let (_, mine) = t.player(1, "GET", "/v1/imports", Value::Null).await;
    assert_eq!(mine["requests"].as_array().unwrap().len(), 1);
    let (_, theirs) = t.player(2, "GET", "/v1/imports", Value::Null).await;
    assert_eq!(theirs["requests"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn requests_that_cannot_be_had_fail_with_a_reason() {
    let t = setup();
    let (_, missing) = t
        .player(
            1,
            "POST",
            "/v1/imports",
            json!({ "input": "https://modrinth.com/mod/nothing-here" }),
        )
        .await;
    let (_, curse) = t
        .player(
            1,
            "POST",
            "/v1/imports",
            json!({ "input": "https://www.curseforge.com/minecraft/mc-mods/jei" }),
        )
        .await;
    t.sites.json(
        &modrinth::project_url("a-plugin"),
        json!({ "id": "plugin01", "slug": "a-plugin", "project_type": "plugin", "title": "P" }),
    );
    let (_, plugin) = t
        .player(
            1,
            "POST",
            "/v1/imports",
            json!({ "input": "https://modrinth.com/plugin/a-plugin" }),
        )
        .await;
    t.drain().await;
    for (request, error) in [
        (missing, "not_found"),
        (curse, "curseforge_later"),
        (plugin, "unsupported_kind"),
    ] {
        let (_, request) = t
            .player(
                1,
                "GET",
                &format!("/v1/imports/{}", request["id"].as_str().unwrap()),
                Value::Null,
            )
            .await;
        assert_eq!(request["state"], "failed");
        assert_eq!(request["error"], error);
    }
}

#[tokio::test]
async fn adding_needs_a_token_for_the_catalog_and_sane_input() {
    let t = setup();
    let body = json!({ "input": "https://modrinth.com/mod/sodium" });
    let (status, _) = t.call(None, "POST", "/v1/imports", body.clone()).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let friends_only = t.token_with(&["account", "friends"], 1);
    let (status, _) = t
        .call(Some(&friends_only), "POST", "/v1/imports", body.clone())
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) = t.call(Some("a.b.c"), "POST", "/v1/imports", body).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    for junk in [
        String::new(),
        "   ".to_owned(),
        "x".repeat(600),
        "sodium\u{0}".to_owned(),
        "https://evil.example/mod/sodium".to_owned(),
        "https://modrinth.com/mod/../../etc".to_owned(),
        "https://modrinth.com:8443/mod/sodium".to_owned(),
        "https://user@modrinth.com/mod/sodium".to_owned(),
        "file:///etc/passwd".to_owned(),
    ] {
        let (status, _) = t
            .player(1, "POST", "/v1/imports", json!({ "input": junk }))
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{junk:?}");
    }
    let (status, _) = t
        .player(1, "POST", "/v1/imports", json!({ "input": 5 }))
        .await;
    assert!(status.is_client_error());
    // Someone else's request is not there for them.
    let (_, mine) = t
        .player(1, "POST", "/v1/imports", json!({ "input": "sodium" }))
        .await;
    let (status, _) = t
        .player(
            2,
            "GET",
            &format!("/v1/imports/{}", mine["id"].as_str().unwrap()),
            Value::Null,
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    // An account adds a bounded number in an hour.
    let mut last = StatusCode::OK;
    for _ in 0..crate::api::IMPORTS_PER_HOUR {
        last = t
            .player(3, "POST", "/v1/imports", json!({ "input": "sodium" }))
            .await
            .0;
    }
    assert_eq!(last, StatusCode::ACCEPTED);
    let (status, _) = t
        .player(3, "POST", "/v1/imports", json!({ "input": "sodium" }))
        .await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn reading_checks_every_value() {
    let t = setup();
    for uri in [
        "/v1/search?kind=plugin",
        "/v1/search?game=1.21%27%3B%20DROP%20TABLE%20project",
        "/v1/search?sort=random",
        "/v1/search?loader=a%20b",
        "/v1/projects/sodium/versions?game=%22",
    ] {
        let (status, _) = t.get(uri).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{uri}");
    }
    for uri in [
        "/v1/search?q=%22sodium%22%20OR%20title%3A*%20NEAR(",
        "/v1/search?q=%F0%9F%98%80&offset=-5&limit=100000",
        "/v1/search?q=&kind=&game=",
    ] {
        let (status, answer) = t.get(uri).await;
        assert_eq!(status, StatusCode::OK, "{uri}: {answer}");
    }
    for uri in [
        "/v1/projects/..%2F..",
        "/v1/projects/%27",
        "/v1/versions/not-a-uuid",
        "/v1/projects/nothing",
    ] {
        let (status, _) = t.get(uri).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{uri}");
    }
    let (status, _) = t
        .call(
            None,
            "POST",
            "/v1/lookup",
            json!({ "hashes": ["'; DROP TABLE blob; --"] }),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let many: Vec<String> = (0..201).map(|_| "a".repeat(40)).collect();
    let (status, _) = t
        .call(None, "POST", "/v1/lookup", json!({ "hashes": many }))
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, health) = t.get("/v1/health").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(health["keys"], true);
}

#[test]
fn input_reads_addresses_and_names() {
    assert_eq!(
        input::parse("https://modrinth.com/mod/sodium"),
        Some(Input::Modrinth {
            project: "sodium".into(),
            version: None
        })
    );
    assert_eq!(
        input::parse("modrinth.com/shader/complementary-reimagined/version/r5.2.2"),
        Some(Input::Modrinth {
            project: "complementary-reimagined".into(),
            version: Some("r5.2.2".into())
        })
    );
    assert_eq!(
        input::parse("https://cdn.modrinth.com/data/AANobbMI/versions/OihdIimA/sodium.jar"),
        Some(Input::ModrinthFile {
            project: "AANobbMI".into(),
            version: "OihdIimA".into()
        })
    );
    assert_eq!(
        input::parse("https://www.curseforge.com/minecraft/mc-mods/jei"),
        Some(Input::CurseForge)
    );
    assert_eq!(
        input::parse("  Just Enough Items "),
        Some(Input::Name("Just Enough Items".into()))
    );
    assert_eq!(input::parse("https://modrinth.com/user/someone"), None);
    assert_eq!(input::parse("https://modrinth.com/mod"), None);
    assert_eq!(input::parse("https://evil.com/mod/sodium"), None);
    assert_eq!(input::parse("http://modrinth.com.evil.example/mod/x"), None);
    assert_eq!(input::parse("a\u{7}b"), None);
}

#[test]
fn modrinth_answers_are_checked() {
    let jar = file_json("AANobbMI", "OihdIimA", "a.jar", b"x");
    let bad_name =
        json!({ "filename": "../../a.jar", "url": "https://cdn.modrinth.com/a.jar", "hashes": {} });
    let elsewhere =
        json!({ "filename": "b.jar", "url": "https://evil.example/b.jar", "hashes": {} });
    let plain_http =
        json!({ "filename": "c.jar", "url": "http://cdn.modrinth.com/c.jar", "hashes": {} });
    let version = modrinth::version(&json!({
        "id": "OihdIimA", "version_number": "1\u{0}.0", "game_versions": ["1.21", "1.21; rm -rf", "1.21"],
        "loaders": ["Fabric"], "version_type": "nightly",
        "files": [jar, bad_name, elsewhere, plain_http],
        "dependencies": [{ "dependency_type": "required", "project_id": "../x" }, { "dependency_type": "weird", "project_id": "abc" }],
    }))
    .unwrap();
    assert_eq!(version.number, "1.0");
    assert_eq!(version.games, ["1.21"]);
    assert_eq!(version.loaders, ["fabric"]);
    assert_eq!(version.channel, "release");
    assert_eq!(version.files.len(), 1);
    assert!(version.dependencies.is_empty());
    assert!(modrinth::version(&json!({ "id": "../../x" })).is_none());
    assert!(modrinth::project(&json!({ "id": "abc", "project_type": "plugin" }), None).is_none());
    let project = modrinth::project(
        &json!({ "id": "abc", "slug": "My-Mod", "project_type": "mod", "icon_url": "javascript:alert(1)",
                 "gallery": [{ "url": "http://i.imgur.com/a.png" }] }),
        None,
    )
    .unwrap();
    assert_eq!(project.slug, "my-mod");
    assert!(project.icon_url.is_none());
    assert!(project.gallery.is_empty());
}

#[test]
fn the_newest_release_is_chosen_for_each_pair() {
    let v = |id: &str, channel: &str, published: i64, games: &[&str], loaders: &[&str]| {
        model::SourceVersion {
            source_id: id.into(),
            number: id.into(),
            name: id.into(),
            changelog: String::new(),
            channel: channel.into(),
            published,
            downloads: 0,
            games: games.iter().map(|s| s.to_string()).collect(),
            loaders: loaders.iter().map(|s| s.to_string()).collect(),
            files: Vec::new(),
            dependencies: Vec::new(),
        }
    };
    let versions = [
        v("a", "release", 1, &["1.20.1"], &["forge", "fabric"]),
        v("b", "release", 2, &["1.20.1"], &["fabric"]),
        v("c", "beta", 3, &["1.20.1"], &["fabric"]),
        v("d", "alpha", 4, &["1.21"], &["fabric"]),
        v("e", "beta", 5, &["1.21"], &["fabric"]),
    ];
    assert_eq!(model::newest_per_pair(&versions), [0, 1, 4]);
}

#[test]
fn curl_gets_its_arguments_one_by_one_and_no_proxy() {
    let curl = Curl {
        program: "curl_chrome150".into(),
        leading: vec!["--impersonate".into(), "chrome150".into()],
    };
    let mut request = fetch::Request {
        url: "https://cdn.modrinth.com/data/x/a b.jar".into(),
        to: "/data/tmp/part".into(),
        max_bytes: 1000,
        json: false,
        proxy: Some("socks5h://user:secret@proxy.example:1080".into()),
        resolve: None,
        timeout: Duration::from_secs(30),
    };
    let args = curl.arguments(&request);
    assert_eq!(&args[0..2], ["--impersonate", "chrome150"]);
    assert!(args.windows(2).any(|w| w == ["--proto", "=https"]));
    assert!(args.windows(2).any(|w| w == ["--max-redirs", "0"]));
    assert!(args.windows(2).any(|w| w == ["--max-filesize", "1000"]));
    assert_eq!(
        args.last().unwrap(),
        "https://cdn.modrinth.com/data/x/a b.jar"
    );
    assert!(
        !args
            .iter()
            .any(|a| a.contains("secret") || a.contains("--resolve"))
    );
    request.proxy = None;
    request.resolve = Some("93.184.216.34:443".parse().unwrap());
    let args = curl.arguments(&request);
    assert!(
        args.windows(2)
            .any(|w| w == ["--resolve", "cdn.modrinth.com:443:93.184.216.34"])
    );

    let reply = fetch::parse_write_out("302\t0\thttps://cdn.modrinth.com/next\t\n");
    assert_eq!(reply.status, 302);
    assert_eq!(
        reply.location.as_deref(),
        Some("https://cdn.modrinth.com/next")
    );
    assert!(fetch::parse_write_out("403\t10\t\tchallenge").challenge);
    assert_eq!(fetch::parse_write_out("junk").status, 0);
}

#[test]
fn only_listed_hosts_over_https() {
    assert!(fetch::site("https://api.modrinth.com/v2/project/x").is_some());
    assert!(fetch::site("https://mediafilez.forgecdn.net/files/1/2/a.jar").is_some());
    for url in [
        "http://api.modrinth.com/v2/project/x",
        "https://api.modrinth.com:444/v2",
        "https://user:pw@api.modrinth.com/v2",
        "https://api.modrinth.com.evil.example/",
        "https://evil.example/",
        "https://api.modrinth.com/a b",
        "ftp://api.modrinth.com/",
    ] {
        assert!(fetch::site(url).is_none(), "{url}");
    }
    assert!(!fetch::is_public("10.1.2.3".parse().unwrap()));
    assert!(!fetch::is_public("::ffff:127.0.0.1".parse().unwrap()));
    assert!(!fetch::is_public("100.64.0.1".parse().unwrap()));
    assert!(fetch::is_public("93.184.216.34".parse().unwrap()));
    assert_eq!(
        fetch::parse_proxies(
            "socks5h://u:p@a.example:1080, http://b.example:8080\nftp://c.example junk"
        ),
        ["socks5h://u:p@a.example:1080", "http://b.example:8080"]
    );
}

#[tokio::test]
async fn redirects_are_followed_only_to_listed_hosts() {
    let t = setup();
    let to = t.app.blobs.part();
    let start = "https://cdn.modrinth.com/start.jar";
    t.sites.put(
        start,
        Page {
            status: 302,
            location: Some("https://edge.forgecdn.net/next.jar".into()),
            ..Page::default()
        },
    );
    t.sites
        .put("https://edge.forgecdn.net/next.jar", ok(b"file".to_vec()));
    assert_eq!(t.app.client.get(start, &to, 100, false).await, Ok(4));
    t.sites.put(
        start,
        Page {
            status: 301,
            location: Some("https://169.254.169.254/latest/meta-data".into()),
            ..Page::default()
        },
    );
    assert_eq!(
        t.app.client.get(start, &to, 100, false).await,
        Err(FetchError::NotAllowed)
    );
    assert_eq!(
        t.app
            .client
            .get("https://evil.example/a", &to, 100, false)
            .await,
        Err(FetchError::NotAllowed)
    );
    t.sites
        .put("https://cdn.modrinth.com/big", ok(vec![0u8; 101]));
    assert_eq!(
        t.app
            .client
            .get("https://cdn.modrinth.com/big", &to, 100, false)
            .await,
        Err(FetchError::TooBig)
    );
    assert_eq!(
        t.app
            .client
            .json("https://api.modrinth.com/v2/project/none")
            .await,
        Ok(None)
    );
}

#[tokio::test]
async fn a_name_that_points_inside_is_refused() {
    let t = setup();
    let client = Client::new(
        Box::new(Shared(Arc::clone(&t.sites))),
        Vec::new(),
        true,
        t.app.blobs.tmp(),
    )
    .with_resolver(resolver([127, 0, 0, 1]));
    let to = t.app.blobs.part();
    assert_eq!(
        client
            .get("https://cdn.modrinth.com/a", &to, 100, false)
            .await,
        Err(FetchError::NotAllowed)
    );
    assert!(t.sites.asked.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_refused_exit_rests_and_the_next_is_tried() {
    let proxies: Vec<String> = (1..=3)
        .map(|i| format!("http://proxy{i}.example:8080"))
        .collect();
    let t = setup_with(proxies);
    let url = "https://www.curseforge.com/api/v1/mods/1/files";
    t.sites.put(
        url,
        Page {
            status: 403,
            challenge: true,
            ..Page::default()
        },
    );
    let to = t.app.blobs.part();
    assert_eq!(
        t.app.client.get(url, &to, 100, false).await,
        Err(FetchError::Blocked)
    );
    let used: Vec<Option<String>> = t
        .sites
        .asked
        .lock()
        .unwrap()
        .iter()
        .map(|r| r.proxy.clone())
        .collect();
    assert_eq!(used.len(), 3);
    assert!(used.iter().all(Option::is_some));
    assert_eq!(
        used.iter().collect::<std::collections::HashSet<_>>().len(),
        3
    );
    // All three rest now: nothing is sent.
    assert_eq!(
        t.app.client.get(url, &to, 100, false).await,
        Err(FetchError::NoExit)
    );
    assert_eq!(t.sites.asked(url), 3);
}

#[test]
fn pictures_are_known_by_their_bytes_and_made_small() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("in");
    std::fs::write(&input, png(1000, 500)).unwrap();
    assert_eq!(crate::thumb::sniff(&png(2, 2)), Some("image/png"));
    assert_eq!(crate::thumb::sniff(b"<svg onload=alert(1)>"), None);
    let output = dir.path().join("out");
    crate::thumb::make(&input, &output, 256, "webp").unwrap();
    let small = image::ImageReader::open(&output)
        .unwrap()
        .with_guessed_format()
        .unwrap()
        .decode()
        .unwrap();
    assert_eq!((small.width(), small.height()), (256, 128));
    crate::thumb::make(&input, &output, 512, "jpeg").unwrap();
    assert_eq!(
        crate::thumb::sniff(&std::fs::read(&output).unwrap()),
        Some("image/jpeg")
    );
    std::fs::write(&input, b"not a picture").unwrap();
    assert!(crate::thumb::make(&input, &output, 256, "webp").is_err());
    assert!(crate::thumb::make(Path::new("/nonexistent"), &output, 256, "webp").is_err());
}

#[test]
fn description_pictures_are_found() {
    let body = "![a](https://i.imgur.com/a.png) <img src=\"https://cdn.modrinth.com/data/x/images/b.webp?x=1\">\n\
                https://i.imgur.com/a.png https://evil.example/c.png https://i.imgur.com/page.html";
    assert_eq!(
        import::body_images(body),
        [
            "https://i.imgur.com/a.png",
            "https://cdn.modrinth.com/data/x/images/b.webp?x=1"
        ]
    );
}

#[test]
fn the_probe_reads_the_proxies_as_deploy_wrote_them() {
    let file = "LAPI_MODS_PROXIES=\"socks5://a:b\\\"c@1.2.3.4:8000 http://x\\\\y@5.6.7.8:80\"\n";
    assert_eq!(
        crate::env_file_value(file, "LAPI_MODS_PROXIES").as_deref(),
        Some("socks5://a:b\"c@1.2.3.4:8000 http://x\\y@5.6.7.8:80")
    );
    assert_eq!(crate::env_file_value(file, "LAPI_MODS"), None);
    assert_eq!(
        crate::env_file_value("LAPI_MODS_PROXIES=bare\n", "LAPI_MODS_PROXIES"),
        None
    );
    assert_eq!(crate::env_file_value("", "LAPI_MODS_PROXIES"), None);
}

#[tokio::test]
async fn the_probe_asks_every_exit_and_prints_no_answer_text() {
    let sites = Arc::new(Sites::default());
    let api = "https://www.curseforge.com/api/v1/mods/238222";
    sites.json(
        &format!("{api}/files?pageIndex=0&pageSize=1"),
        json!({ "data": [{ "id": 4_000_123, "displayName": "secret name" }] }),
    );
    sites.put(
        &format!("{api}/files/4000123/download"),
        Page {
            status: 307,
            location: Some("https://edge.forgecdn.net/files/4000/123/jei.jar".into()),
            ..Page::default()
        },
    );
    sites.put(
        "https://www.curseforge.com/minecraft/mc-mods/jei",
        Page {
            status: 403,
            body: b"<title>Just a moment...</title> secret".to_vec(),
            challenge: true,
            ..Page::default()
        },
    );
    sites.put(
        "https://www.curseforge.com/minecraft/search?search=jei",
        ok("<script id=\"__NEXT_DATA__\">{\"secret\":1}</script>"),
    );
    let dir = tempfile::tempdir().unwrap();
    let client = Client::new(
        Box::new(Shared(Arc::clone(&sites))),
        vec!["socks5://secret-user:secret@203.0.113.9:1080".into()],
        true,
        dir.path(),
    )
    .with_resolver(resolver([1, 1, 1, 1]));
    let lines = crate::probe::run(&client, dir.path()).await;
    assert_eq!(lines[0], "exits 2");
    for exit in ["proxy1", "direct"] {
        let of = |check: &str| {
            lines
                .iter()
                .find(|l| l.starts_with(&format!("{exit:<8} {check:<16} ")))
                .cloned()
                .unwrap_or_default()
        };
        assert!(of("cf_files").ends_with("file_found"), "{lines:?}");
        assert!(
            of("cf_download").ends_with("307 0B to_forgecdn"),
            "{lines:?}"
        );
        assert!(of("cf_project_page").contains("403 challenge"), "{lines:?}");
        assert!(of("cf_search_page").ends_with(" page_data"), "{lines:?}");
        assert!(
            of("cf_project_api").ends_with("not_found") || of("cf_project_api").contains(" 404 "),
            "{lines:?}"
        );
    }
    for line in &lines {
        assert!(
            !line.contains("secret") && !line.contains("203.0.113"),
            "{line}"
        );
        assert!(!line.contains("forgecdn.net"), "{line}");
    }
    // One request per check and exit: no retries, no redirects followed.
    assert_eq!(
        sites.asked("https://www.curseforge.com/minecraft/mc-mods/jei"),
        2
    );
    assert_eq!(
        sites.asked("https://edge.forgecdn.net/files/4000/123/jei.jar"),
        0
    );
}
