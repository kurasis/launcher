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
use crate::{App, Urls, curseforge, model, modrinth, pack, packwiz};

const ISSUER: &str = "https://oauth.example";
const FILES: &str = "https://files.example";
const CDN: &str = "https://cdn.example";

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
    setup_full(proxies, None, None, "", None)
}

/// With R2 and Cloudflare's cache played by fakes.
fn setup_r2() -> (T, Arc<FakeR2>, Arc<Purged>) {
    let r2 = Arc::new(FakeR2::default());
    let purged = Arc::new(Purged::default());
    let t = setup_full(
        Vec::new(),
        Some(Arc::clone(&r2) as Arc<dyn crate::r2::Bucket>),
        Some(Arc::clone(&purged) as Arc<dyn crate::owner::Purge>),
        CDN,
        None,
    );
    (t, r2, purged)
}

fn setup_full(
    proxies: Vec<String>,
    r2: Option<Arc<dyn crate::r2::Bucket>>,
    purge: Option<Arc<dyn crate::owner::Purge>>,
    cdn: &str,
    virustotal: Option<Arc<dyn crate::virustotal::Api>>,
) -> T {
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
        sessions: crate::auth::Sessions::default(),
        limiter: crate::api::Limiter::default(),
        uploads: crate::own::Uploads::default(),
        urls: Urls::new(FILES).with_cdn(cdn),
        thumbs: import::Thumbs::Inline,
        r2,
        purge,
        virustotal,
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
    // Pictures are asked as a browser asks for an <img>; files are not.
    {
        let asked = t.sites.asked.lock().unwrap();
        let headers = |url: &str| {
            asked
                .iter()
                .find(|r| r.url == url)
                .map(|r| r.headers)
                .unwrap()
        };
        assert_eq!(headers("https://i.imgur.com/body1.png"), fetch::PICTURE);
        assert_eq!(headers("https://i.imgur.com/gallery1.png"), fetch::PICTURE);
        assert!(
            asked
                .iter()
                .filter(|r| r.url.ends_with(".jar"))
                .all(|r| r.headers.is_empty())
        );
    }

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

/// Moves a catalog's last reads of its sources and its finished jobs back.
fn age(t: &T, seconds: i64) {
    let db = rusqlite::Connection::open(t.app.store.path()).unwrap();
    db.execute(
        "UPDATE project_source SET fetched = fetched - ?1",
        [seconds],
    )
    .unwrap();
    db.execute("UPDATE job SET finished = finished - ?1", [seconds])
        .unwrap();
}

#[tokio::test]
async fn projects_are_checked_for_new_versions_wanted_ones_more_often() {
    let t = setup();
    modrinth_site(&t.sites);
    t.player(
        1,
        "POST",
        "/v1/imports",
        json!({ "input": "https://modrinth.com/mod/sodium" }),
    )
    .await;
    t.drain().await;
    // The import read it by its slug; a check reads it by its ID.
    let by_id = modrinth::project_url(SODIUM);
    let checks = |t: &T| t.sites.asked(&by_id);
    assert_eq!(checks(&t), 0);
    // Just read: nothing is due.
    assert_eq!(import::refresh(&t.app, None).unwrap(), 0);

    // A day on: not yet for a project nobody wanted lately.
    age(&t, 25 * 3600);
    assert_eq!(import::refresh(&t.app, None).unwrap(), 0);
    // A player opens its page: wanted, and checked at once.
    let (status, _) = t.get("/v1/projects/sodium").await;
    assert_eq!(status, StatusCode::OK);
    t.drain().await;
    assert_eq!(checks(&t), 1);
    // Opened again soon after: not checked again.
    t.get("/v1/projects/sodium").await;
    t.drain().await;
    assert_eq!(checks(&t), 1);

    // Wanted: the background check comes after a day, not before.
    age(&t, 20 * 3600);
    assert_eq!(import::refresh(&t.app, None).unwrap(), 0);
    age(&t, 5 * 3600);
    assert_eq!(import::refresh(&t.app, None).unwrap(), 1);
    t.drain().await;
    assert_eq!(checks(&t), 2);

    // Nobody wants it for a month: once a week.
    let db = rusqlite::Connection::open(t.app.store.path()).unwrap();
    db.execute("UPDATE project_wanted SET at = at - ?1", [31 * 86_400])
        .unwrap();
    age(&t, 2 * 86_400);
    assert_eq!(import::refresh(&t.app, None).unwrap(), 0);
    // Sodium and the dependency it brought, which nobody opened either.
    age(&t, 6 * 86_400);
    assert_eq!(import::refresh(&t.app, None).unwrap(), 2);
}

/// A chain of `n` mods, each needing the next, the last needing the first.
fn chain_site(sites: &Sites, n: usize) {
    for i in 0..n {
        let id = format!("Chain{i:03}");
        let next = format!("Chain{:03}", (i + 1) % n);
        let version = format!("ver{i:05}");
        let name = format!("chain-{i}.jar");
        let jar = format!("jar of {name}").into_bytes();
        sites.put(
            &format!("https://cdn.modrinth.com/data/{id}/versions/{version}/{name}"),
            ok(jar.clone()),
        );
        let project = json!({ "id": id, "slug": format!("chain{i}"), "project_type": "mod", "title": format!("Chain {i}") });
        sites.json(
            &modrinth::project_url(&format!("chain{i}")),
            project.clone(),
        );
        sites.json(&modrinth::project_url(&id), project);
        sites.json(&modrinth::members_url(&id), json!([]));
        sites.json(
            &modrinth::versions_url(&id),
            json!([{
                "id": version, "version_number": "1.0", "version_type": "release",
                "date_published": "2024-01-01T00:00:00Z", "game_versions": ["1.21.1"], "loaders": ["fabric"],
                "files": [file_json(&id, &version, &name, &jar)],
                "dependencies": [{ "project_id": next, "dependency_type": "required" }],
            }]),
        );
    }
}

#[tokio::test]
async fn dependencies_are_followed_to_the_end_of_a_long_chain_and_a_loop_ends() {
    let t = setup();
    chain_site(&t.sites, 25);
    let (status, _) = t
        .player(
            1,
            "POST",
            "/v1/imports",
            json!({ "input": "https://modrinth.com/mod/chain0" }),
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    t.drain().await;
    // Every link is in the catalog with its file kept, and each was read once
    // (the first twice: asked by its address, then by its ID from the last).
    for i in 0..25 {
        let (status, project) = t.get(&format!("/v1/projects/chain{i}")).await;
        assert_eq!(status, StatusCode::OK, "chain{i}");
        let (_, versions) = t
            .get(&format!(
                "/v1/projects/{}/versions",
                project["id"].as_str().unwrap()
            ))
            .await;
        assert_eq!(versions["versions"][0]["saved"], "all", "chain{i}");
        assert_eq!(
            t.sites
                .asked(&modrinth::versions_url(&format!("Chain{i:03}"))),
            if i == 0 { 2 } else { 1 },
            "chain{i}"
        );
    }
}

#[tokio::test]
async fn a_name_offers_projects_to_choose_from() {
    let t = setup();
    modrinth_site(&t.sites);
    t.sites.json(
        &modrinth::search_url("sodium mod"),
        json!({ "hits": [
            { "project_id": SODIUM, "slug": "sodium", "project_type": "mod", "title": "Sodium",
              "author": "jellysquid3", "downloads": 5, "icon_url": "https://cdn.modrinth.com/x.png",
              "featured_gallery": "https://cdn.modrinth.com/poster.png" },
            { "project_id": "plugin01", "slug": "a-plugin", "project_type": "plugin", "title": "Not ours" },
        ]}),
    );
    t.sites
        .put("https://cdn.modrinth.com/x.png", ok(png(400, 400)));
    t.sites
        .put("https://cdn.modrinth.com/poster.png", ok(png(1200, 600)));
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
    // Pictures reach the launcher only from here.
    assert!(candidates[0].get("iconUrl").is_none());
    assert!(candidates[0].get("bannerUrl").is_none());
    let icon = candidates[0]["icon"].as_str().unwrap();
    assert!(
        icon.starts_with("https://files.example/i/") && icon.ends_with(".webp"),
        "{icon}"
    );
    let banner = candidates[0]["banner"].as_str().unwrap();
    assert!(
        banner.starts_with("https://files.example/i/") && banner.ends_with(".jpg"),
        "{banner}"
    );

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
            json!({ "input": "https://edge.forgecdn.net/files/9255/357/jei.jar" }),
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
        (curse, "curseforge_file_link"),
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
        Some(Input::CurseForge {
            class: "mc-mods".into(),
            slug: "jei".into(),
            file: None
        })
    );
    assert_eq!(
        input::parse("curseforge.com/minecraft/Texture-Packs/Faithful-32x/files/4567890"),
        Some(Input::CurseForge {
            class: "texture-packs".into(),
            slug: "faithful-32x".into(),
            file: Some(4_567_890)
        })
    );
    assert_eq!(
        input::parse("https://edge.forgecdn.net/files/9255/357/jei.jar"),
        Some(Input::CurseForgeFile)
    );
    for bad in [
        "https://www.curseforge.com/minecraft/bukkit-plugins/worldedit",
        "https://www.curseforge.com/minecraft/mc-mods/jei/files/0",
        "https://www.curseforge.com/minecraft/mc-mods/jei/files/12345678901",
        "https://www.curseforge.com/minecraft/mc-mods/j%22ei",
        "https://www.curseforge.com/wow/addons/details",
        "https://www.curseforge.com/minecraft/mc-mods",
    ] {
        assert_eq!(input::parse(bad), None, "{bad}");
    }
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
    // Modrinth calls a data pack a mod; only its loaders tell.
    let kind_with = |loaders: Value| {
        modrinth::project(
            &json!({ "id": "abc", "project_type": "mod", "loaders": loaders }),
            None,
        )
        .unwrap()
        .kind
    };
    assert_eq!(kind_with(json!(["datapack"])), "datapack");
    assert_eq!(kind_with(json!(["datapack", "fabric"])), "mod");
    assert_eq!(kind_with(json!([])), "mod");
    assert_eq!(kind_with(json!("datapack")), "mod");
    assert_eq!(kind_with(json!([null])), "mod");
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
            details: true,
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
        headers: &["Referer: https://www.curseforge.com/"],
    };
    let args = curl.arguments(&request);
    assert_eq!(&args[0..2], ["--impersonate", "chrome150"]);
    assert!(
        args.windows(2)
            .any(|w| w == ["--header", "Referer: https://www.curseforge.com/"])
    );
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
fn description_pictures_come_from_any_public_host() {
    let body = "![a](https://rhs.millered001.workers.dev/data/cached_images/a.webp) \
                [![b](https://github.com/user-attachments/assets/1234-abcd)](https://x.example.org/page) \
                <IMG alt=\"c\" SRC='https://pics.example.net/c'> <img src=https://pics.example.net/d> \
                <a href=\"https://pics.example.net/e\">e</a> [f](https://pics.example.net/f) \
                https://pics.example.net/g.PNG?size=1 <img src=\"https://10.0.0.1/h.png\"> \
                <img src=\"https://printer.local/i.png\"> <img src=\"https://localhost/j.png\"> \
                <img src=\"https://intranet/k.png\"> <img src=\"https://0x7f.1/l.png\"> \
                <img src=\"https://[::1]/m.png\"> <img src=\"https://pics.example.net:8443/n.png\"> \
                <img src=\"https://u@pics.example.net/o.png\"> <img src=\"ftp://pics.example.net/p.png\"> \
                <imgx src=\"https://pics.example.net/q\"> é<img src=\"https://pics.example.net/r\">";
    let fetched: Vec<String> = import::body_images(body)
        .into_iter()
        .map(|(_, url)| url)
        .collect();
    assert_eq!(
        fetched,
        [
            "https://rhs.millered001.workers.dev/data/cached_images/a.webp",
            "https://github.com/user-attachments/assets/1234-abcd",
            "https://pics.example.net/c",
            "https://pics.example.net/d",
            "https://pics.example.net/g.PNG?size=1",
            "https://pics.example.net/r",
        ]
    );
    for url in [
        "https://pics.example.net/a",
        "https://I.IMGUR.com/a.png",
        "https://xn--e1afmkfd.xn--p1ai/a.png",
        "https://pics.example.net./a.png",
    ] {
        assert_eq!(fetch::picture_site(url), Some(fetch::Site::Images), "{url}");
    }
    assert_eq!(
        fetch::picture_site("https://cdn.modrinth.com/a.png"),
        Some(fetch::Site::Modrinth)
    );
    for url in [
        "http://pics.example.net/a.png",
        "https://127.0.0.1/a.png",
        "https://2130706433/a.png",
        "https://[::1]/a.png",
        "https://localhost/a.png",
        "https://a.localhost/a.png",
        "https://nas.lan/a.png",
        "https://x.internal/a.png",
        "https://y.onion/a.png",
        "https://1.2.3.4.in-addr.arpa/a.png",
        "https://single/a.png",
        "https://a_b.example.net/a.png",
        "https://pics.example.net:444/a.png",
        "https://u:p@pics.example.net/a.png",
        "https://pics.example.net/a b.png",
        &format!("https://pics.example.net/{}", "a".repeat(2100)),
    ] {
        assert_eq!(fetch::picture_site(url), None, "{url}");
    }
    // Only pictures go anywhere: files and pages stay with listed hosts.
    assert!(fetch::site("https://pics.example.net/a.png").is_none());
}

#[tokio::test]
async fn a_picture_host_that_points_inside_is_refused() {
    // Directly and through a proxy alike: the name must be public.
    for proxies in [Vec::new(), vec!["http://proxy1.example:8080".to_owned()]] {
        let t = setup();
        let direct = proxies.is_empty();
        let client = Client::new(
            Box::new(Shared(Arc::clone(&t.sites))),
            proxies,
            direct,
            t.app.blobs.tmp(),
        )
        .with_resolver(resolver([192, 168, 1, 1]));
        let to = t.app.blobs.part();
        assert_eq!(
            client
                .picture("https://pics.example.net/a.png", &to, 100)
                .await,
            Err(FetchError::NotAllowed)
        );
        assert!(t.sites.asked.lock().unwrap().is_empty());
    }
}

#[tokio::test]
async fn a_picture_redirect_must_stay_public() {
    let t = setup();
    t.sites.put(
        "https://pics.example.net/a.png",
        Page {
            status: 302,
            location: Some("https://127.0.0.1/a.png".into()),
            ..Page::default()
        },
    );
    let to = t.app.blobs.part();
    assert_eq!(
        t.app
            .client
            .picture("https://pics.example.net/a.png", &to, 100)
            .await,
        Err(FetchError::NotAllowed)
    );
    // A file never comes from a host nobody listed.
    assert_eq!(
        t.app
            .client
            .get("https://pics.example.net/a.png", &to, 100, false)
            .await,
        Err(FetchError::NotAllowed)
    );
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
            "socks5h://u:p@a.example:1080, http://b.example:8080\nftp://c.example junk \
             socks5://u:p@203.0.113.7:8000"
        ),
        [
            "socks5h://u:p@a.example:1080",
            "http://b.example:8080",
            "socks5h://u:p@203.0.113.7:8000"
        ]
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
                https://i.imgur.com/a.png https://evil.example/c.png https://i.imgur.com/page.html \
                <img src=\"http://i.imgur.com/d.png\"> http://evil.example/e.png http://i.imgur.com:8080/f.png";
    assert_eq!(
        import::body_images(body),
        [
            (
                "https://i.imgur.com/a.png".to_owned(),
                "https://i.imgur.com/a.png".to_owned()
            ),
            (
                "https://cdn.modrinth.com/data/x/images/b.webp?x=1".to_owned(),
                "https://cdn.modrinth.com/data/x/images/b.webp?x=1".to_owned()
            ),
            (
                "http://i.imgur.com/d.png".to_owned(),
                "https://i.imgur.com/d.png".to_owned()
            ),
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
        Page {
            status: 308,
            location: Some("https://www.curseforge.com/minecraft/search?page=1&search=jei".into()),
            ..Page::default()
        },
    );
    sites.put(
        "https://www.curseforge.com/minecraft/search?page=1&search=jei",
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
        assert!(
            of("cf_search_page").ends_with(
                " 308 0B then 200 48B next_data hits:0,no_jei,authors:0,summaries:0,downloads:0"
            ),
            "{lines:?}"
        );
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

const JEI: u64 = 238_222;
const ARCHITECTURY: u64 = 419_699;

fn cf_file(project: u64, id: u64, name: &str, size: usize, release: u64, date: &str) -> Value {
    json!({
        "id": id, "projectId": project, "displayName": name, "fileName": name,
        "fileLength": size, "status": 4, "releaseType": release, "dateCreated": date,
        "gameVersions": ["Forge", "1.20.1", "Client", "Server"], "totalDownloads": 10,
        "isEarlyAccessContent": false,
    })
}

/// A file's download link: the site's redirect to forgecdn, and the file.
fn cf_download(sites: &Sites, project: u64, id: u64, name: &str, bytes: &[u8]) {
    let cdn = format!(
        "https://edge.forgecdn.net/files/{}/{}/{name}",
        id / 1000,
        id % 1000
    );
    sites.put(
        &curseforge::download_url(project, id),
        Page {
            status: 307,
            location: Some(cdn.clone()),
            ..Page::default()
        },
    );
    sites.put(&cdn, ok(bytes.to_vec()));
}

/// The page of a project as the site sends it: the App Router stream in two
/// pushes, with an object of the same slug before the project.
fn cf_page(project: &Value) -> String {
    let rsc = format!(
        "1:{}\n",
        json!(["$", "div", null, { "breadcrumb": { "slug": "jei", "name": "Decoy" }, "project": project }])
    );
    let (a, b) = rsc.split_at(rsc.len() / 2);
    format!(
        "<html><head><meta property=\"og:title\" content=\"JEI &amp; more\"></head><body>\
         <script>self.__next_f.push({})</script><script>self.__next_f.push({})</script>\
         </body></html>",
        json!([1, a]),
        json!([1, b])
    )
}

/// JEI with three files for one game version and loader, one early-access
/// and one not released; the newest release needs Architectury, whose page
/// is missing.
fn curseforge_site(sites: &Sites) -> HashMap<u64, Vec<u8>> {
    let mut jars = HashMap::new();
    let files = [
        (5002, "jei-15.3-beta.jar", 2, "2024-03-01T00:00:00Z"),
        (5001, "jei-15.2.jar", 1, "2024-02-01T00:00:00Z"),
        (5000, "jei-15.1.jar", 1, "2024-01-01T00:00:00Z"),
    ];
    let mut list = Vec::new();
    for (id, name, release, date) in files {
        let bytes = format!("jar of {name}").into_bytes();
        cf_download(sites, JEI, id, name, &bytes);
        let file = cf_file(JEI, id, name, bytes.len(), release, date);
        sites.json(&curseforge::file_url(JEI, id), json!({ "data": file }));
        list.push(file);
        jars.insert(id, bytes);
    }
    let mut early = cf_file(JEI, 5003, "jei-16.jar", 5, 1, "2024-04-01T00:00:00Z");
    early["isEarlyAccessContent"] = true.into();
    let mut held = cf_file(JEI, 5004, "jei-17.jar", 5, 1, "2024-05-01T00:00:00Z");
    held["status"] = 1.into();
    list.insert(0, early);
    list.insert(0, held);
    sites.json(
        &curseforge::files_url(JEI, 0),
        json!({ "data": list, "pagination": { "index": 0, "pageSize": 50, "totalCount": 5 } }),
    );
    sites.json(
        &curseforge::dependencies_url(JEI, 5001),
        json!({ "data": [
            { "id": ARCHITECTURY, "slug": "architectury-api", "type": "RequiredDependency",
              "categoryClass": { "slug": "mc-mods" } },
            { "id": 77, "slug": "some-addon", "type": "OptionalDependency",
              "categoryClass": { "slug": "mc-mods" } },
            { "id": 78, "slug": "a-tool", "type": "Tool", "categoryClass": { "slug": "mc-mods" } },
        ]}),
    );
    sites.json(
        &curseforge::changelog_url(JEI, 5001),
        json!({ "changelogBody": "<p>Fixed\u{202E} recipes</p>" }),
    );
    let project = json!({
        "id": JEI, "slug": "jei", "gameId": 432, "classId": 6,
        "name": "Just Enough Items", "summary": "View items and recipes",
        "logo": { "url": "https://media.forgecdn.net/avatars/1/1/jei.png" },
        "authors": [{ "name": "mezz" }],
        "screenshots": [{ "url": "https://media.forgecdn.net/attachments/1/2/shot.png", "title": "Shot" },
                        { "url": "https://evil.example/x.png" }],
        "categories": [{ "slug": "api" }, { "slug": "BAD CATEGORY!" }],
        "downloadCount": 5000,
        "dateCreated": "2015-11-21T00:00:00Z", "dateModified": "2024-03-01T00:00:00Z",
    });
    sites.put(
        &curseforge::page_url("mc-mods", "jei"),
        ok(cf_page(&project)),
    );
    sites.put(
        "https://media.forgecdn.net/avatars/1/1/jei.png",
        ok(png(200, 200)),
    );
    sites.put(
        "https://media.forgecdn.net/attachments/1/2/shot.png",
        ok(png(800, 400)),
    );
    // Architectury: one file, its page missing.
    let bytes = b"jar of architectury".to_vec();
    cf_download(sites, ARCHITECTURY, 6000, "architectury.jar", &bytes);
    sites.json(
        &curseforge::files_url(ARCHITECTURY, 0),
        json!({
            "data": [cf_file(ARCHITECTURY, 6000, "architectury.jar", bytes.len(), 1, "2024-01-01T00:00:00Z")],
            "pagination": { "index": 0, "pageSize": 50, "totalCount": 1 },
        }),
    );
    jars.insert(6000, bytes);
    jars
}

#[tokio::test]
async fn a_curseforge_address_brings_the_project_its_newest_files_and_dependencies() {
    let t = setup();
    let jars = curseforge_site(&t.sites);
    let (status, request) = t
        .player(
            1,
            "POST",
            "/v1/imports",
            json!({ "input": "https://www.curseforge.com/minecraft/mc-mods/jei" }),
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{request}");
    t.drain().await;
    let id = request["id"].as_str().unwrap();
    let (_, request) = t
        .player(1, "GET", &format!("/v1/imports/{id}"), Value::Null)
        .await;
    assert_eq!(request["state"], "ready", "{request}");

    let (status, project) = t.get("/v1/projects/jei").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(project["id"], request["projectId"]);
    assert_eq!(project["title"], "Just Enough Items");
    assert_eq!(project["summary"], "View items and recipes");
    assert_eq!(project["authors"][0]["name"], "mezz");
    assert_eq!(project["categories"], json!(["api"]));
    assert!(
        project["icon"]
            .as_str()
            .unwrap()
            .starts_with("https://files.example/i/")
    );
    // Pictures from sites we do not fetch are left out.
    assert_eq!(project["gallery"].as_array().unwrap().len(), 1);

    let (_, versions) = t
        .get("/v1/projects/jei/versions?game=1.20.1&loader=forge")
        .await;
    let versions = versions["versions"].as_array().unwrap().clone();
    let saved: Vec<(&str, &str)> = versions
        .iter()
        .map(|v| (v["number"].as_str().unwrap(), v["saved"].as_str().unwrap()))
        .collect();
    assert_eq!(
        saved,
        [
            ("jei-15.3-beta.jar", "none"),
            ("jei-15.2.jar", "all"),
            ("jei-15.1.jar", "none")
        ]
    );
    assert_eq!(versions[0]["channel"], "beta");
    assert_eq!(versions[1]["changelog"], "<p>Fixed recipes</p>");
    let (sha1, _, sha256) = hashes(&jars[&5001]);
    assert_eq!(versions[1]["files"][0]["sha256"], sha256.as_str());
    assert_eq!(
        std::fs::read(t.app.blobs.path(&sha256)).unwrap(),
        jars[&5001]
    );
    // Required dependencies came along without their page; optional ones
    // and tools did not.
    let dependencies = versions[1]["dependencies"].as_array().unwrap();
    assert_eq!(dependencies.len(), 2);
    let required = dependencies
        .iter()
        .find(|d| d["kind"] == "required")
        .unwrap();
    let (_, architectury) = t
        .get(&format!(
            "/v1/projects/{}",
            required["projectId"].as_str().unwrap()
        ))
        .await;
    assert_eq!(architectury["title"], "architectury-api");
    let (_, api_versions) = t
        .get(&format!(
            "/v1/projects/{}/versions",
            required["projectId"].as_str().unwrap()
        ))
        .await;
    assert_eq!(api_versions["versions"][0]["saved"], "all");
    assert_eq!(t.sites.asked(&curseforge::files_url(77, 0)), 0);

    // The launcher finds a kept file by its SHA-1, made here.
    let (_, found) = t
        .call(None, "POST", "/v1/lookup", json!({ "hashes": [sha1] }))
        .await;
    assert_eq!(found["found"].as_array().unwrap().len(), 1, "{found}");

    // Read again: the kept file stays kept and is not downloaded again.
    import::queue_project(&t.app, curseforge::SOURCE, "mc-mods/jei", 0, None, 0).unwrap();
    t.drain().await;
    assert_eq!(t.sites.asked(&curseforge::files_url(JEI, 0)), 2);
    let (_, again) = t
        .get("/v1/projects/jei/versions?game=1.20.1&loader=forge")
        .await;
    assert_eq!(again["versions"][1]["saved"], "all");
    assert_eq!(again["versions"][1]["files"][0]["sha256"], sha256.as_str());
    assert_eq!(again["versions"][1]["changelog"], "<p>Fixed recipes</p>");
    assert_eq!(t.sites.asked(&curseforge::download_url(JEI, 5001)), 1);

    // A file's address brings that file.
    let (_, older) = t
        .player(
            2,
            "POST",
            "/v1/imports",
            json!({ "input": "https://www.curseforge.com/minecraft/mc-mods/jei/files/5000" }),
        )
        .await;
    t.drain().await;
    let (_, older) = t
        .player(
            2,
            "GET",
            &format!("/v1/imports/{}", older["id"].as_str().unwrap()),
            Value::Null,
        )
        .await;
    assert_eq!(older["state"], "ready", "{older}");
    let (_, versions) = t
        .get("/v1/projects/jei/versions?game=1.20.1&loader=forge")
        .await;
    assert_eq!(versions["versions"][2]["saved"], "all");
}

#[tokio::test]
async fn a_curseforge_file_of_another_size_is_not_kept() {
    let t = setup();
    curseforge_site(&t.sites);
    let wrong = cf_file(JEI, 5001, "jei-15.2.jar", 3, 1, "2024-02-01T00:00:00Z");
    t.sites.json(
        &curseforge::files_url(JEI, 0),
        json!({ "data": [wrong], "pagination": { "totalCount": 1 } }),
    );
    t.player(
        1,
        "POST",
        "/v1/imports",
        json!({ "input": "https://www.curseforge.com/minecraft/mc-mods/jei" }),
    )
    .await;
    t.drain().await;
    let (_, versions) = t.get("/v1/projects/jei/versions").await;
    assert_ne!(versions["versions"][0]["saved"], "all", "{versions}");
}

#[test]
fn curseforge_answers_are_checked() {
    let good = json!({
        "id": 9, "projectId": 1, "displayName": "My Mod 1.0\u{0}", "fileName": "my-mod.jar",
        "fileLength": 100, "status": 10, "releaseType": 2, "dateCreated": "2024-01-01T00:00:00Z",
        "gameVersions": ["NeoForge", "Fabric", "1.21.1", "24w14a", "Client", "Java 21", "1.21; rm -rf"],
    });
    let file = curseforge::file(1, &good).unwrap();
    assert_eq!(file.source_id, "9");
    assert_eq!(file.number, "My Mod 1.0");
    assert_eq!(file.games, ["1.21.1", "24w14a"]);
    assert_eq!(file.loaders, ["fabric", "neoforge"]);
    assert_eq!(file.channel, "beta");
    assert_eq!(file.files[0].url, curseforge::download_url(1, 9));
    assert_eq!(file.files[0].size, 100);
    assert!(file.files[0].sha1.is_none());
    assert!(!file.details);
    for (key, value) in [
        ("projectId", json!(2)),
        ("status", json!(1)),
        ("isEarlyAccessContent", json!(true)),
        ("fileName", json!("../../a.jar")),
        ("fileName", json!(".hidden")),
        ("id", json!(-1)),
    ] {
        let mut bad = good.clone();
        bad[key] = value;
        assert!(curseforge::file(1, &bad).is_none(), "{key}");
    }
    let (list, entries, total) = curseforge::files(
        1,
        &json!({ "data": [good, { "id": "x" }], "pagination": { "totalCount": 1_000_000_000_000u64 } }),
    );
    assert_eq!((list.len(), entries, total), (1, 2, 100_000));
    assert_eq!(curseforge::files(1, &json!("junk")).1, 0);

    let needed = curseforge::dependencies(&json!({ "data": [
        { "id": 5, "slug": "lib", "type": "RequiredDependency", "categoryClass": { "slug": "mc-mods" } },
        { "id": 6, "slug": "pack", "type": "Incompatible", "categoryClass": { "slug": "texture-packs" } },
        { "id": 7, "slug": "../x", "type": "RequiredDependency", "categoryClass": { "slug": "mc-mods" } },
        { "id": 8, "slug": "plugin", "type": "RequiredDependency", "categoryClass": { "slug": "bukkit-plugins" } },
        { "id": 0, "slug": "zero", "type": "RequiredDependency", "categoryClass": { "slug": "mc-mods" } },
        { "id": 9, "slug": "tool", "type": "Tool", "categoryClass": { "slug": "mc-mods" } },
    ]}));
    let got: Vec<(&str, &str, &str, &str)> = needed
        .iter()
        .map(|n| {
            (
                n.dependency.kind.as_str(),
                n.dependency.project.as_deref().unwrap(),
                n.class,
                n.slug.as_str(),
            )
        })
        .collect();
    assert_eq!(
        got,
        [
            ("required", "5", "mc-mods", "lib"),
            ("incompatible", "6", "texture-packs", "pack")
        ]
    );
    assert_eq!(
        curseforge::changelog(&json!({ "changelogBody": "a\u{7}b\nc" })),
        "ab\nc"
    );

    let r = curseforge::Ref::parse("238222/mc-mods/jei").unwrap();
    assert_eq!(
        (r.id, r.class.as_str(), r.slug.as_str()),
        (Some(238_222), "mc-mods", "jei")
    );
    assert_eq!(r.text(), "238222/mc-mods/jei");
    assert_eq!(
        curseforge::Ref::parse("modpacks/all-the-mods").unwrap().id,
        None
    );
    for bad in [
        "0/mc-mods/jei",
        "12345678901/mc-mods/jei",
        "bukkit-plugins/x",
        "mc-mods/JEI",
        "mc-mods/",
        "1/2/mc-mods/jei",
        "",
    ] {
        assert!(curseforge::Ref::parse(bad).is_none(), "{bad}");
    }
}

#[test]
fn curseforge_pages_are_read_with_care() {
    let project = json!({
        "id": JEI, "slug": "jei", "gameId": 432, "name": "Just Enough Items",
        "description": "<p>About</p>",
    });
    let card = curseforge::page(&cf_page(&project), "jei");
    assert_eq!(card.id, Some(JEI));
    assert_eq!(card.title, "Just Enough Items");
    assert_eq!(card.body, "<p>About</p>");
    // Another slug's page has no ID; the tags still give a title.
    let other = curseforge::page(&cf_page(&project), "jeii");
    assert_eq!(other.id, None);
    assert_eq!(other.title, "JEI & more");

    // Values the stream gives by reference: a text chunk, a JSON chunk,
    // dates as the stream writes them and as milliseconds.
    let html_body = "<p>About\n<b>JEI</b> é</p>";
    let project = json!({
        "id": JEI, "slug": "jei", "gameId": 432, "name": "Just Enough Items",
        "description": "$1f", "screenshots": "$2a",
        "creationDate": "$D2015-11-21T00:00:00.000Z", "lastUpdated": 1_709_251_200_000u64,
    });
    let rsc = format!(
        "1:{}\n2a:{}\n1f:T{:x},{html_body}3:[]\n",
        json!(["$", "div", null, { "project": project }]),
        json!([{ "url": "https://media.forgecdn.net/attachments/1/2/shot.png", "title": "Shot" }]),
        html_body.len()
    );
    let html = format!("<script>self.__next_f.push({})</script>", json!([1, rsc]));
    let card = curseforge::page(&html, "jei");
    assert_eq!(card.body, html_body);
    assert_eq!(card.gallery.len(), 1);
    assert_eq!(card.gallery[0].title, "Shot");
    assert_eq!(card.published, 1_448_064_000);
    assert_eq!(card.updated, 1_709_251_200);
    assert_eq!(
        curseforge::shapes(&html, "jei"),
        [
            format!("description:ref_str{}", html_body.len()),
            "screenshots:ref_list1".to_owned(),
            "creationDate:str26".to_owned(),
            "lastUpdated:num".to_owned()
        ]
    );
    // A reference to nothing, or a length past the end, gives nothing.
    let broken = json!({ "id": JEI, "slug": "jei", "gameId": 432, "description": "$99", "screenshots": "$1f" });
    let rsc = format!("1:{}\n1f:Tffffff,<p>x", json!({ "project": broken }));
    let html = format!("<script>self.__next_f.push({})</script>", json!([1, rsc]));
    let card = curseforge::page(&html, "jei");
    assert_eq!(card.body, "");
    assert!(card.gallery.is_empty());

    // The description the page renders apart from the project: raw HTML
    // (by reference to a text chunk) wins over other text chunks, a script
    // is not a description, and a text chunk with paragraphs is the last
    // resort.
    let bare = json!({ "id": JEI, "slug": "jei", "gameId": 432, "name": "Just Enough Items" });
    let about = "<p>JEI shows <b>recipes</b></p><p><img src=\"https://media.forgecdn.net/attachments/1/2/a.png\"></p>";
    let other = "<p>Short</p>";
    let rsc = format!(
        "1:{}\n2:{}\n3:{}\n4:T{:x},{about}5:T{:x},{other}6:[]\n",
        json!(["$", "div", null, { "project": bare }]),
        json!(["$", "div", null, { "dangerouslySetInnerHTML": { "__html": "$4" } }]),
        json!(["$", "script", null, { "dangerouslySetInnerHTML": { "__html": "<script>window.x=1<2</script>".repeat(40) } }]),
        about.len(),
        other.len()
    );
    let html = format!("<script>self.__next_f.push({})</script>", json!([1, rsc]));
    assert_eq!(curseforge::page(&html, "jei").body, about);
    assert!(
        curseforge::media(&html)
            .starts_with(&format!("desc:{},raw_html:2,text_chunks_p:2,", about.len()))
    );
    assert!(curseforge::media(&html).ends_with("holders[]"));
    let rsc = format!(
        "1:{}\n5:T{:x},{other}6:[]\n",
        json!(["$", "div", null, { "project": bare }]),
        other.len()
    );
    let html = format!("<script>self.__next_f.push({})</script>", json!([1, rsc]));
    assert_eq!(curseforge::page(&html, "jei").body, other);
    // Screenshots listed beside the project, in their order, each once;
    // a picture elsewhere or without a thumbnail is not one.
    let shot = |index: u64, name: &str| {
        json!({ "url": format!("https://media.forgecdn.net/attachments/1/2/{name}.png"),
                "thumbnailUrl": "https://media.forgecdn.net/attachments/thumbnails/1/2/t.png",
                "title": name, "description": "", "displayIndex": index, "isFeatured": false })
    };
    let rsc = format!(
        "1:{}\n2:{}\n",
        json!(["$", "div", null, { "project": bare }]),
        json!({ "images": [shot(2, "b"), shot(1, "a"), shot(3, "a"),
                { "url": "https://media.forgecdn.net/attachments/1/2/c.png" },
                { "url": "https://evil.example/attachments/x.png", "thumbnailUrl": "x" }] })
    );
    let html = format!("<script>self.__next_f.push({})</script>", json!([1, rsc]));
    let titles: Vec<String> = curseforge::page(&html, "jei")
        .gallery
        .into_iter()
        .map(|g| g.title)
        .collect();
    assert_eq!(titles, ["a", "b"]);

    // Objects holding a picture are named by their keys.
    let rsc = format!(
        "1:{}\n",
        json!([{ "imageUrl": "https://media.forgecdn.net/attachments/1/2/s.png", "title": "S", "bad key": 1 }])
    );
    let html = format!("<script>self.__next_f.push({})</script>", json!([1, rsc]));
    assert_eq!(
        curseforge::media(&html),
        "desc:0,raw_html:0,text_chunks_p:0,holders[imageUrl+title]"
    );

    // JSON-LD and Open Graph when there is no stream.
    let html = r#"<script type="application/ld+json">{"@context":"https://schema.org","name":"Faithful","description":"32x","image":{"url":"https://media.forgecdn.net/a.png"},"author":[{"name":"xMrVizzy"}],"dateModified":"2024-01-01T00:00:00Z"}</script>
        <meta property="og:description" content="ignored">"#;
    let card = curseforge::page(html, "faithful-32x");
    assert_eq!(card.id, None);
    assert_eq!(card.title, "Faithful");
    assert_eq!(card.summary, "32x");
    assert_eq!(
        card.icon.as_deref(),
        Some("https://media.forgecdn.net/a.png")
    );
    assert_eq!(card.authors, ["xMrVizzy"]);
    assert_eq!(card.updated, 1_704_067_200);
    let html = r#"<meta content="x" property="og:image"><meta property="og:image" content="javascript:alert(1)">"#;
    assert_eq!(curseforge::page(html, "x").icon, None);

    // Junk ends quickly and gives nothing.
    let started = std::time::Instant::now();
    let mut junk = String::new();
    for _ in 0..20_000 {
        junk.push_str(
            "self.__next_f.push([1,\"{\\\"slug\\\":\\\"jei\\\",{\"]) {\"slug\":\"jei\" \u{e9}",
        );
    }
    junk.push_str(&"{".repeat(100_000));
    junk.push_str("<script type=\"application/ld+json\">");
    junk.push_str("<meta property=\"og:title\"");
    let card = curseforge::page(&junk, "jei");
    assert_eq!(card.id, None);
    assert_eq!(card.title, "");
    assert!(started.elapsed() < Duration::from_secs(10));
    assert_eq!(curseforge::page("", "jei"), curseforge::Card::default());
}

/// The search page as the site sends it: hits in the stream, one of a class
/// we do not keep, one of another game, one twice.
fn cf_search_page() -> String {
    let hit = |id: u64, slug: &str, name: &str, class: u64| {
        json!({ "id": id, "slug": slug, "name": name, "classId": class, "gameId": 432,
                "summary": format!("About {name}"), "author": { "name": "mezz" }, "downloads": 9 })
    };
    let mut other_game = hit(5, "wow-thing", "Not Minecraft", 6);
    other_game["gameId"] = 1.into();
    let rsc = format!(
        "1:{}\n2:{}\n",
        json!(["$", "div", null, { "results": [
            hit(JEI, "jei", "Just Enough Items", 6),
            hit(4, "a-plugin", "A plugin", 5),
            other_game,
            { "id": 6, "slug": "pack", "name": "A Pack", "class": { "slug": "texture-packs" },
              "summary": "Blocks", "authors": [{ "name": "artist" }] },
            // The site's categories look alike, with no summary or author.
            { "id": 423, "slug": "map-information", "name": "Map and Information", "classId": 6,
              "gameId": 432, "parentCategoryId": 6 },
        ]}]),
        json!({ "again": hit(JEI, "jei", "Just Enough Items", 6) })
    );
    format!("<script>self.__next_f.push({})</script>", json!([1, rsc]))
}

#[tokio::test]
async fn a_name_does_not_ask_curseforge() {
    let t = setup();
    curseforge_site(&t.sites);
    let page = curseforge::search_url("just enough");
    t.sites.put(&page, ok(cf_search_page()));
    let (_, request) = t
        .player(1, "POST", "/v1/imports", json!({ "input": "just enough" }))
        .await;
    let id = request["id"].as_str().unwrap().to_owned();
    t.drain().await;
    let (_, request) = t
        .player(1, "GET", &format!("/v1/imports/{id}"), Value::Null)
        .await;
    // Modrinth found nothing; CurseForge's search page is not read.
    assert_eq!(request["state"], "failed", "{request}");
    assert_eq!(request["error"], "nothing_found", "{request}");
    assert_eq!(t.sites.asked(&page), 0);
}

#[test]
fn curseforge_search_pages_are_read_with_care() {
    assert_eq!(
        curseforge::search_url("a&b c/\"\u{e9}"),
        "https://www.curseforge.com/minecraft/search?search=a%26b%20c%2F%22%C3%A9"
    );
    assert_eq!(curseforge::candidates(&cf_search_page()).len(), 2);
    assert!(curseforge::candidates("").is_empty());
    // Many hits: five at most.
    let many: Vec<Value> = (1..50)
        .map(|n| {
            json!({ "id": n, "slug": format!("m{n}"), "name": "M", "classId": 6,
                    "summary": "S", "author": "A" })
        })
        .collect();
    let rsc = format!("1:{}\n", json!(many));
    let html = format!("<script>self.__next_f.push({})</script>", json!([1, rsc]));
    assert_eq!(curseforge::candidates(&html).len(), 5);
    // Bad IDs, slugs and names are left out, and so are hits without a
    // summary or author.
    let bad = json!([
        { "id": 10, "slug": "a", "name": "A", "classId": 6, "author": "A" },
        { "id": 11, "slug": "a", "name": "A", "classId": 6, "summary": "S" },
        { "id": 0, "slug": "a", "name": "A", "classId": 6, "summary": "S", "author": "A" },
        { "id": 12_345_678_901u64, "slug": "a", "name": "A", "classId": 6, "summary": "S", "author": "A" },
        { "id": 7, "slug": "../a", "name": "A", "classId": 6, "summary": "S", "author": "A" },
        { "id": 8, "slug": "a", "name": "\u{7}", "classId": 6, "summary": "S", "author": "A" },
        { "id": "9", "slug": "a", "name": "A", "classId": 6, "summary": "S", "author": "A" },
    ]);
    let rsc = format!("1:{bad}\n");
    let html = format!("<script>self.__next_f.push({})</script>", json!([1, rsc]));
    assert!(curseforge::candidates(&html).is_empty());
    // Deep nesting and junk end quickly.
    let started = std::time::Instant::now();
    let mut junk = String::from("1:");
    junk.push_str(&"[".repeat(200_000));
    junk.push_str("\n2:{\"id\":1,");
    let html = format!("<script>self.__next_f.push({})</script>", json!([1, junk]));
    assert!(curseforge::candidates(&html).is_empty());
    assert!(started.elapsed() < Duration::from_secs(10));
}

// ----- modpacks -----

const PACK: &str = "PaCkPaCk";

fn mrpack(entries: &[(&str, &[u8])]) -> Vec<u8> {
    use std::io::Write as _;
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, bytes) in entries {
        zip.start_file(*name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

fn pack_entry(path: &str, url: &str, bytes: &[u8], client: &str) -> Value {
    let (sha1, sha512, _) = hashes(bytes);
    json!({
        "path": path, "hashes": { "sha1": sha1, "sha512": sha512 },
        "env": { "client": client, "server": "required" },
        "downloads": [url], "fileSize": bytes.len(),
    })
}

/// A modpack with one version whose archive is `archive`.
fn modpack_site(sites: &Sites, archive: &[u8]) {
    sites.json(
        &modrinth::project_url("pack"),
        json!({ "id": PACK, "slug": "pack", "project_type": "modpack", "title": "Pack" }),
    );
    sites.json(&modrinth::members_url(PACK), json!([]));
    sites.json(
        &modrinth::versions_url(PACK),
        json!([{
            "id": "pvaaaaaa", "version_number": "1.0", "version_type": "release",
            "date_published": "2024-01-01T00:00:00Z", "game_versions": ["1.21.1"], "loaders": ["fabric"],
            "files": [file_json(PACK, "pvaaaaaa", "pack-1.0.mrpack", archive)],
        }]),
    );
    sites.put(
        &format!("https://cdn.modrinth.com/data/{PACK}/versions/pvaaaaaa/pack-1.0.mrpack"),
        ok(archive.to_vec()),
    );
}

async fn pack_version(t: &T) -> String {
    t.player(
        1,
        "POST",
        "/v1/imports",
        json!({ "input": "https://modrinth.com/modpack/pack" }),
    )
    .await;
    t.drain().await;
    let (_, versions) = t.get("/v1/projects/pack/versions").await;
    versions["versions"][0]["id"].as_str().unwrap().to_owned()
}

#[tokio::test]
async fn a_modpack_is_read_and_all_its_files_are_kept_when_asked() {
    let t = setup();
    let sodium = b"jar of sodium in a pack".to_vec();
    let shader = b"shader from github".to_vec();
    let sodium_url = format!("https://cdn.modrinth.com/data/{SODIUM}/versions/v9/sodium.jar");
    let shader_url = "https://github.com/someone/shader/releases/download/1/shader.zip";
    t.sites.put(&sodium_url, ok(sodium.clone()));
    t.sites.put(shader_url, ok(shader.clone()));
    let index = json!({
        "formatVersion": 1, "game": "minecraft", "versionId": "1.0", "name": "Pack",
        "dependencies": { "minecraft": "1.21.1", "fabric-loader": "0.16.5" },
        "files": [
            pack_entry("mods/sodium.jar", &sodium_url, &sodium, "required"),
            pack_entry("shaderpacks/shader.zip", shader_url, &shader, "optional"),
            // Left out: outside the game folder, a program, a server's file.
            pack_entry("../escape.jar", &sodium_url, &sodium, "required"),
            pack_entry("config/run.exe", &sodium_url, &sodium, "required"),
            pack_entry("mods/server-only.jar", &sodium_url, &sodium, "unsupported"),
        ],
    });
    let archive = mrpack(&[
        ("modrinth.index.json", index.to_string().as_bytes()),
        ("overrides/config/a.toml", b"a = 1"),
    ]);
    modpack_site(&t.sites, &archive);
    let version = pack_version(&t).await;

    // Nothing of the pack's files is fetched before it is asked for.
    assert_eq!(t.sites.asked(&sodium_url), 0);
    let (status, pack) = t.get(&format!("/v1/versions/{version}/pack")).await;
    assert_eq!(status, StatusCode::OK, "{pack}");
    assert_eq!(pack["state"], "none");
    let (status, pack) = t
        .call(
            None,
            "POST",
            &format!("/v1/versions/{version}/pack"),
            Value::Null,
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{pack}");
    assert_eq!(pack["state"], "working");
    t.drain().await;

    let (status, pack) = t.get(&format!("/v1/versions/{version}/pack")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(pack["state"], "ready", "{pack}");
    assert_eq!(pack["game"], "1.21.1");
    assert_eq!(
        pack["loader"],
        json!({ "kind": "fabric", "version": "0.16.5" })
    );
    assert_eq!(pack["skipped"], 3);
    assert_eq!(pack["total"], 2);
    let files = pack["files"].as_array().unwrap();
    assert_eq!(files[0]["path"], "mods/sodium.jar");
    assert_eq!(files[0]["optional"], false);
    assert_eq!(files[1]["optional"], true);
    let (_, _, sha256) = hashes(&shader);
    assert_eq!(files[1]["sha256"], sha256.as_str());
    assert_eq!(
        files[1]["url"],
        format!("https://files.example/f/{sha256}/shader%2Ezip")
    );
    assert_eq!(std::fs::read(t.app.blobs.path(&sha256)).unwrap(), shader);
    let (_, _, archive_sha) = hashes(&archive);
    assert_eq!(pack["archive"]["sha256"], archive_sha.as_str());
    assert!(
        pack["archive"]["url"]
            .as_str()
            .unwrap()
            .starts_with("https://files.example/f/")
    );
    assert_eq!(t.sites.asked(&sodium_url), 1);

    // Ready: asked again, nothing more is fetched.
    let (status, _) = t
        .call(
            None,
            "POST",
            &format!("/v1/versions/{version}/pack"),
            Value::Null,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    t.drain().await;
    assert_eq!(t.sites.asked(&sodium_url), 1);

    // Not a modpack, not a version.
    let (_, versions) = t.get("/v1/projects/pack/versions").await;
    assert_eq!(versions["versions"][0]["saved"], "all");
    let (status, _) = t
        .get("/v1/versions/0190a000-0000-7000-8000-000000000000/pack")
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = t.get("/v1/versions/..%2Fx/pack").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_broken_modpack_fails_with_a_reason() {
    for (archive, code) in [
        (b"not a zip at all".to_vec(), "bad_pack"),
        (mrpack(&[("other.json", b"{}")]), "bad_pack"),
        (
            mrpack(&[("modrinth.index.json", b"{\"formatVersion\":1")]),
            "bad_pack",
        ),
    ] {
        let t = setup();
        modpack_site(&t.sites, &archive);
        let version = pack_version(&t).await;
        t.call(
            None,
            "POST",
            &format!("/v1/versions/{version}/pack"),
            Value::Null,
        )
        .await;
        t.drain().await;
        let (_, pack) = t.get(&format!("/v1/versions/{version}/pack")).await;
        assert_eq!(pack["state"], "failed", "{pack}");
        assert_eq!(pack["error"], code);
    }

    // A file that is not what the index says is not kept.
    let t = setup();
    let url = format!("https://cdn.modrinth.com/data/{SODIUM}/versions/v9/sodium.jar");
    t.sites.put(&url, ok(b"the real jaX".to_vec()));
    let index = json!({
        "formatVersion": 1, "game": "minecraft",
        "dependencies": { "minecraft": "1.21.1" },
        "files": [pack_entry("mods/sodium.jar", &url, b"the real jar", "required")],
    });
    modpack_site(
        &t.sites,
        &mrpack(&[("modrinth.index.json", index.to_string().as_bytes())]),
    );
    let version = pack_version(&t).await;
    t.call(
        None,
        "POST",
        &format!("/v1/versions/{version}/pack"),
        Value::Null,
    )
    .await;
    t.drain().await;
    let (_, pack) = t.get(&format!("/v1/versions/{version}/pack")).await;
    assert_eq!(pack["state"], "failed");
    assert_eq!(pack["error"], "hash_mismatch");
    assert!(pack["files"][0]["url"].is_null());
}

#[test]
fn pack_paths_stay_in_the_folders_a_pack_fills() {
    for path in [
        "mods/sodium.jar",
        "mods/sodium.jar.disabled",
        "mods/1.20.1/old.jar",
        "config/sodium-options.json",
        "config/.hidden",
        "kubejs/server_scripts/main.js",
        "saves/World 1/level.dat",
        "resourcepacks/Faithful 32x.zip",
        "options.txt",
        "servers.dat",
    ] {
        assert!(crate::pack::pack_path(path), "{path}");
    }
    for path in [
        "",
        "/mods/a.jar",
        "../mods/a.jar",
        "mods/../../a.jar",
        "mods/./a.jar",
        "mods//a.jar",
        "mods\\a.jar",
        "C:/mods/a.jar",
        "mods/a:b.jar",
        "mods/CON.jar",
        "mods/lpt1",
        "mods/a.jar ",
        "mods/a.",
        "mods/a\u{0}.jar",
        "mods/a\n.jar",
        "Mods/a.jar",
        "logs/latest.log",
        "natives/lwjgl.dll",
        "config/tool.jar",
        "config/run.exe",
        "mods/run.EXE",
        "mods/lib.dll",
        "kubejs/x.sh",
        "saves/w/x.bat.disabled",
        "launcher.json",
        "a/b/c/d/e/f/g/h/i/j/k/l/m/n/o/p/q.txt",
    ] {
        assert!(!crate::pack::pack_path(path), "{path:?}");
    }
    assert!(!crate::pack::pack_path(&format!(
        "config/{}",
        "a".repeat(400)
    )));
}

#[test]
fn pack_indexes_are_read_with_care() {
    let jar = b"jar";
    let good = |files: Value, dependencies: Value| json!({ "formatVersion": 1, "game": "minecraft", "files": files, "dependencies": dependencies });
    let entry = pack_entry(
        "mods/a.jar",
        "https://cdn.modrinth.com/data/x/a.jar",
        jar,
        "required",
    );
    let read = |v: Value| crate::pack::index(v.to_string().as_bytes());
    let index = read(good(
        json!([entry]),
        json!({ "minecraft": "1.20.1", "neoforge": "20.4.237" }),
    ))
    .unwrap();
    assert_eq!(index.game, "1.20.1");
    assert_eq!(
        index.loader,
        Some(("neoforge".to_owned(), "20.4.237".to_owned()))
    );
    assert_eq!(index.files.len(), 1);
    // An entry with no env is the client's.
    let mut no_env = entry.clone();
    no_env.as_object_mut().unwrap().remove("env");
    assert_eq!(
        read(good(json!([no_env]), json!({ "minecraft": "1.20.1" })))
            .unwrap()
            .files
            .len(),
        1
    );

    let mut other_host = entry.clone();
    other_host["downloads"] = json!([
        "https://evil.example/a.jar",
        "http://cdn.modrinth.com/a.jar"
    ]);
    let mut bad_hash = entry.clone();
    bad_hash["hashes"]["sha1"] = json!("XYZ");
    let mut no_sha512 = entry.clone();
    no_sha512["hashes"]
        .as_object_mut()
        .unwrap()
        .remove("sha512");
    let mut empty = entry.clone();
    empty["fileSize"] = json!(0);
    let mut huge = entry.clone();
    huge["fileSize"] = json!(3u64 * 1024 * 1024 * 1024);
    let mut same_path = entry.clone();
    same_path["path"] = json!("mods/A.jar");
    let fabric = json!({ "minecraft": "1.20.1", "fabric-loader": "0.15" });
    for (value, code) in [
        (json!("junk"), "bad_pack"),
        (
            json!({ "formatVersion": 2, "game": "minecraft" }),
            "unsupported_pack",
        ),
        (
            json!({ "formatVersion": 1, "game": "terraria" }),
            "unsupported_pack",
        ),
        (good(json!([]), json!({})), "bad_pack"),
        (
            good(json!([]), json!({ "minecraft": "1.20.1; rm -rf" })),
            "bad_pack",
        ),
        (
            good(
                json!([]),
                json!({ "minecraft": "1.20.1", "forge": "47", "fabric-loader": "0.15" }),
            ),
            "unsupported_pack",
        ),
        (
            good(json!([]), json!({ "minecraft": "1.20.1", "rift": "1" })),
            "unsupported_pack",
        ),
        (
            good(json!([other_host]), fabric.clone()),
            "unsupported_host",
        ),
        (good(json!([bad_hash]), fabric.clone()), "bad_pack"),
        (good(json!([no_sha512]), fabric.clone()), "bad_pack"),
        (good(json!([empty]), fabric.clone()), "bad_pack"),
        (good(json!([huge]), fabric.clone()), "too_big"),
        (
            good(json!([entry.clone(), same_path]), fabric.clone()),
            "bad_pack",
        ),
        (
            good(
                Value::Array(vec![entry.clone(); crate::pack::MAX_FILES + 1]),
                fabric.clone(),
            ),
            "too_big",
        ),
    ] {
        assert_eq!(read(value.clone()), Err(code), "{value}");
    }
    // Too much together.
    let many: Vec<Value> = (0..10)
        .map(|i| {
            let mut e = entry.clone();
            e["path"] = json!(format!("mods/{i}.jar"));
            e["fileSize"] = json!(2u64 * 1024 * 1024 * 1024);
            e
        })
        .collect();
    assert_eq!(read(good(json!(many), fabric)), Err("too_big"));
}

#[tokio::test]
async fn a_curseforge_modpack_has_its_files_found_kept_and_placed() {
    const ATM: u64 = 715_572;
    let t = setup();
    let jars = curseforge_site(&t.sites);
    // JEI is in the catalog already: its file is not fetched twice.
    t.player(
        1,
        "POST",
        "/v1/imports",
        json!({ "input": "https://www.curseforge.com/minecraft/mc-mods/jei" }),
    )
    .await;
    t.drain().await;
    let jei_download = curseforge::download_url(JEI, 5001);
    assert_eq!(t.sites.asked(&jei_download), 1);

    // A shader, a resource pack, a data pack and a jar packed in a folder.
    let shader = mrpack(&[("shaders/final.fsh", b"void main() {}")]);
    let resources = mrpack(&[
        ("Faithful/pack.mcmeta", b"{}"),
        ("Faithful/assets/minecraft/x.png", b"png"),
    ]);
    let data = mrpack(&[("pack.mcmeta", b"{}"), ("data/x/recipes/a.json", b"{}")]);
    for (project, id, name, bytes) in [
        (900, 9000, "shader.zip", &shader),
        (901, 9001, "faithful.zip", &resources),
        (902, 9002, "recipes.zip", &data),
    ] {
        cf_download(&t.sites, project, id, name, bytes);
        let mut file = cf_file(project, id, name, bytes.len(), 1, "2024-01-01T00:00:00Z");
        // A pack may choose a file the site no longer offers.
        file["status"] = 5.into();
        t.sites
            .json(&curseforge::file_url(project, id), json!({ "data": file }));
    }
    // A file whose name is not one to write.
    t.sites.json(
        &curseforge::file_url(903, 9003),
        json!({ "data": cf_file(903, 9003, "../escape.jar", 5, 1, "2024-01-01T00:00:00Z") }),
    );
    let manifest = json!({
        "minecraft": { "version": "1.20.1",
                       "modLoaders": [{ "id": "forge-47.2.0", "primary": true }] },
        "manifestType": "minecraftModpack", "manifestVersion": 1,
        "name": "All the Mods", "version": "1.0", "author": "someone",
        "files": [
            { "projectID": JEI, "fileID": 5001, "required": true },
            { "projectID": 900, "fileID": 9000, "required": false },
            { "projectID": 901, "fileID": 9001, "required": true },
            { "projectID": 902, "fileID": 9002, "required": true },
            { "projectID": 903, "fileID": 9003, "required": true },
            // Gone from the site.
            { "projectID": 904, "fileID": 9004, "required": true },
            { "projectID": JEI, "fileID": 5001, "required": true },
        ],
        "overrides": "overrides",
    });
    let archive = mrpack(&[
        ("manifest.json", manifest.to_string().as_bytes()),
        ("overrides/config/a.toml", b"a = 1"),
    ]);
    cf_download(&t.sites, ATM, 7000, "ATM-1.0.zip", &archive);
    let mut file = cf_file(
        ATM,
        7000,
        "ATM-1.0.zip",
        archive.len(),
        1,
        "2024-01-01T00:00:00Z",
    );
    file["gameVersions"] = json!(["Forge", "1.20.1"]);
    t.sites.json(
        &curseforge::files_url(ATM, 0),
        json!({ "data": [file], "pagination": { "index": 0, "pageSize": 50, "totalCount": 1 } }),
    );
    let project = json!({
        "id": ATM, "slug": "all-the-mods", "gameId": 432, "classId": 4471,
        "name": "All the Mods", "summary": "A pack",
    });
    t.sites.put(
        &curseforge::page_url("modpacks", "all-the-mods"),
        ok(cf_page(&project).replace("\"jei\"", "\"all-the-mods\"")),
    );
    let (status, request) = t
        .player(
            1,
            "POST",
            "/v1/imports",
            json!({ "input": "https://www.curseforge.com/minecraft/modpacks/all-the-mods" }),
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{request}");
    t.drain().await;
    let (_, versions) = t.get("/v1/projects/all-the-mods/versions").await;
    let version = versions["versions"][0]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("{versions}"))
        .to_owned();

    t.call(
        None,
        "POST",
        &format!("/v1/versions/{version}/pack"),
        Value::Null,
    )
    .await;
    t.drain().await;
    let (status, pack) = t.get(&format!("/v1/versions/{version}/pack")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(pack["state"], "ready", "{pack}");
    assert_eq!(pack["game"], "1.20.1");
    assert_eq!(
        pack["loader"],
        json!({ "kind": "forge", "version": "47.2.0" })
    );
    let paths: Vec<(&str, bool)> = pack["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| {
            (
                f["path"].as_str().unwrap(),
                f["optional"].as_bool().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        paths,
        [
            ("mods/jei-15.2.jar", false),
            ("shaderpacks/shader.zip", true),
            ("resourcepacks/faithful.zip", false),
        ]
    );
    // The data pack, the bad name and the gone file are left out.
    assert_eq!(pack["skipped"], 3);
    assert_eq!(pack["total"], 3);
    let (_, _, jei_sha) = hashes(&jars[&5001]);
    assert_eq!(pack["files"][0]["sha256"], jei_sha.as_str());
    assert_eq!(t.sites.asked(&jei_download), 1);
    let (_, _, shader_sha) = hashes(&shader);
    assert_eq!(
        std::fs::read(t.app.blobs.path(&shader_sha)).unwrap(),
        shader
    );
    assert!(pack["archive"]["url"].as_str().is_some());
}

#[test]
fn curseforge_manifests_are_read_with_care() {
    let base = json!({
        "minecraft": { "version": "1.21.1",
                       "modLoaders": [{ "id": "fabric-0.16.5", "primary": false },
                                      { "id": "neoforge-21.1.77", "primary": true }] },
        "manifestType": "minecraftModpack", "manifestVersion": 1,
        "files": [{ "projectID": 1, "fileID": 2 }],
    });
    let index = pack::manifest(base.to_string().as_bytes()).unwrap();
    assert_eq!(index.game, "1.21.1");
    assert_eq!(
        index.loader,
        Some(("neoforge".to_owned(), "21.1.77".to_owned()))
    );
    assert_eq!(
        index.curseforge,
        [pack::CurseForgeFile {
            project: 1,
            file: 2,
            optional: false
        }]
    );
    let with = |pointer: &str, value: Value| {
        let mut m = base.clone();
        *m.pointer_mut(pointer).unwrap() = value;
        pack::manifest(m.to_string().as_bytes())
    };
    assert_eq!(
        with("/manifestType", json!("other")),
        Err("unsupported_pack")
    );
    assert_eq!(with("/manifestVersion", json!(2)), Err("unsupported_pack"));
    assert_eq!(
        with("/minecraft/version", json!("1.21; rm -rf /")),
        Err("bad_pack")
    );
    assert_eq!(
        with("/minecraft/modLoaders", json!([{ "id": "liteloader-1" }])),
        Err("unsupported_pack")
    );
    assert_eq!(
        with("/minecraft/modLoaders", json!([{ "id": "forge-47/../x" }])),
        Err("unsupported_pack")
    );
    assert_eq!(
        with("/files", json!([{ "projectID": 0, "fileID": 2 }])),
        Err("bad_pack")
    );
    assert_eq!(
        with("/files", json!([{ "projectID": -1, "fileID": 2 }])),
        Err("bad_pack")
    );
    let mut m = base.clone();
    m["overrides"] = json!("../elsewhere");
    assert_eq!(
        pack::manifest(m.to_string().as_bytes()),
        Err("unsupported_pack")
    );
    let many: Vec<Value> = (1..=pack::MAX_FILES as u64 + 1)
        .map(|n| json!({ "projectID": n, "fileID": n }))
        .collect();
    assert_eq!(with("/files", json!(many)), Err("too_big"));
    assert_eq!(pack::manifest(b"\xff junk"), Err("bad_pack"));
    // No loader: a plain game.
    let none = with("/minecraft/modLoaders", json!([])).unwrap();
    assert_eq!(none.loader, None);

    assert_eq!(
        curseforge::download_ids(&curseforge::download_url(238_222, 5001)),
        Some((238_222, 5001))
    );
    for bad in [
        "https://www.curseforge.com/api/v1/mods/1/files/x/download",
        "https://www.curseforge.com/api/v1/mods/0/files/1/download",
        "https://evil.example/api/v1/mods/1/files/2/download",
        "https://www.curseforge.com/api/v1/mods/1/files/2",
    ] {
        assert_eq!(curseforge::download_ids(bad), None, "{bad}");
    }
}

#[tokio::test]
async fn the_same_project_on_both_sources_becomes_one() {
    const JEI_MODRINTH: &str = "JeIjEiJe";
    let t = setup();
    let jars = curseforge_site(&t.sites);
    t.player(
        1,
        "POST",
        "/v1/imports",
        json!({ "input": "https://www.curseforge.com/minecraft/mc-mods/jei" }),
    )
    .await;
    t.drain().await;
    let (_, cf) = t.get("/v1/projects/jei").await;
    let cf_id = cf["id"].as_str().unwrap().to_owned();

    // On Modrinth, with the same file as CurseForge's 15.2, and one more.
    let same = &jars[&5001];
    let newer = b"jar of jei 15.4".to_vec();
    for (version, name, bytes) in [
        ("mjv1aaaa", "jei-15.2.jar", same.clone()),
        ("mjv2bbbb", "jei-15.4.jar", newer.clone()),
    ] {
        t.sites.put(
            &format!("https://cdn.modrinth.com/data/{JEI_MODRINTH}/versions/{version}/{name}"),
            ok(bytes),
        );
    }
    t.sites.json(
        &modrinth::project_url("jei"),
        json!({ "id": JEI_MODRINTH, "slug": "jei", "project_type": "mod",
                "title": "JEI on Modrinth", "description": "Items" }),
    );
    t.sites
        .json(&modrinth::members_url(JEI_MODRINTH), json!([]));
    t.sites.json(
        &modrinth::versions_url(JEI_MODRINTH),
        json!([
            { "id": "mjv2bbbb", "version_number": "15.4", "version_type": "release",
              "date_published": "2024-06-01T00:00:00Z", "game_versions": ["1.21.1"], "loaders": ["neoforge"],
              "files": [file_json(JEI_MODRINTH, "mjv2bbbb", "jei-15.4.jar", &newer)] },
            { "id": "mjv1aaaa", "version_number": "15.2", "version_type": "release",
              "date_published": "2024-02-01T00:00:00Z", "game_versions": ["1.20.1"], "loaders": ["forge"],
              "files": [file_json(JEI_MODRINTH, "mjv1aaaa", "jei-15.2.jar", same)] },
        ]),
    );
    t.player(
        1,
        "POST",
        "/v1/imports",
        json!({ "input": "https://modrinth.com/mod/jei" }),
    )
    .await;
    t.drain().await;

    // One project with Modrinth's card and both sources; CurseForge's ID and
    // slug lead to it.
    let (status, project) = t.get(&format!("/v1/projects/{cf_id}")).await;
    assert_eq!(status, StatusCode::OK, "{project}");
    let id = project["id"].as_str().unwrap().to_owned();
    assert_ne!(id, cf_id);
    assert_eq!(project["title"], "JEI on Modrinth");
    let mut sources: Vec<&str> = project["sources"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["source"].as_str().unwrap())
        .collect();
    sources.sort_unstable();
    assert_eq!(sources, ["curseforge", "modrinth"]);
    // It takes the plain slug CurseForge's had first; its own still leads.
    assert_eq!(project["slug"], "jei", "{project}");
    let (_, by_slug) = t.get("/v1/projects/jei").await;
    assert_eq!(by_slug["id"], id.as_str());
    let (_, found) = t.get("/v1/search?q=jei").await;
    assert_eq!(found["hits"][0]["slug"], "jei", "{found}");

    // The shared file shows once, as Modrinth's; the others from both.
    let (_, versions) = t.get(&format!("/v1/projects/{id}/versions")).await;
    let numbers: Vec<&str> = versions["versions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["number"].as_str().unwrap())
        .collect();
    assert!(
        numbers.contains(&"15.2") && numbers.contains(&"15.4"),
        "{numbers:?}"
    );
    assert!(!numbers.contains(&"jei-15.2.jar"), "{numbers:?}");
    assert!(numbers.contains(&"jei-15.1.jar"), "{numbers:?}");

    // Searching finds it once.
    let (_, found) = t.get("/v1/search?q=jei").await;
    assert_eq!(found["total"], 1, "{found}");

    // CurseForge read again: the card stays Modrinth's.
    age(&t, 30 * 24 * 3600);
    t.player(
        2,
        "POST",
        "/v1/imports",
        json!({ "input": "https://www.curseforge.com/minecraft/mc-mods/jei" }),
    )
    .await;
    t.drain().await;
    let (_, project) = t.get(&format!("/v1/projects/{id}")).await;
    assert_eq!(project["title"], "JEI on Modrinth");

    // A project of one source is never joined with another of the same.
    let db = rusqlite::Connection::open(t.app.store.path()).unwrap();
    let joined: i64 = db
        .query_row("SELECT count(*) FROM project_joined", [], |r| r.get(0))
        .unwrap();
    assert_eq!(joined, 1);

    // A join made before that keeps the long slug until the next start.
    let gone: String = db
        .query_row("SELECT project_id FROM project_joined", [], |r| r.get(0))
        .unwrap();
    let long: String = db
        .query_row("SELECT slug FROM project WHERE id = ?1", [&gone], |r| {
            r.get(0)
        })
        .unwrap();
    assert_ne!(long, "jei");
    for (who, slug) in [(&gone, "aside"), (&id, long.as_str()), (&gone, "jei")] {
        db.execute(
            "UPDATE project SET slug = ?2 WHERE id = ?1",
            rusqlite::params![who, slug],
        )
        .unwrap();
    }
    drop(db);
    let data = t.app.store.path().parent().unwrap().parent().unwrap();
    crate::store::Store::open(data).unwrap();
    let (_, project) = t.get(&format!("/v1/projects/{id}")).await;
    assert_eq!(project["slug"], "jei", "{project}");
    let (_, by_long) = t.get(&format!("/v1/projects/{long}")).await;
    assert_eq!(by_long["id"], id.as_str());
}

// ----- packwiz packs (stage 5) -----

const PW: &str = "https://raw.githubusercontent.com/me/pack/main";

fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(sha2::Sha256::digest(bytes))
}

#[test]
fn packwiz_addresses_and_files_are_read_with_care() {
    let url = |t: &str| reqwest::Url::parse(t).unwrap();
    assert_eq!(
        packwiz::pack_url(&url("https://github.com/me/pack/blob/main/pack.toml")).as_deref(),
        Some("https://raw.githubusercontent.com/me/pack/main/pack.toml")
    );
    assert_eq!(
        packwiz::pack_url(&url("https://gitlab.com/me/pack/-/blob/main/x/pack.toml")).as_deref(),
        Some("https://gitlab.com/me/pack/-/raw/main/x/pack.toml")
    );
    for bad in [
        "https://evil.example/pack.toml",
        "https://github.com/me/pack/blob/main/other.toml",
        "https://raw.githubusercontent.com/me/pack/main/pack.toml?x=1",
        "https://github.com/me/pack.toml",
        "http://user@raw.githubusercontent.com/me/pack/main/pack.toml",
    ] {
        assert_eq!(packwiz::pack_url(&url(bad)), None, "{bad}");
    }
    assert_eq!(
        input::parse("github.com/me/pack/blob/main/pack.toml"),
        Some(Input::Packwiz(
            "https://raw.githubusercontent.com/me/pack/main/pack.toml".to_owned()
        ))
    );
    let base = format!("{PW}/index.toml");
    assert_eq!(
        packwiz::relative(&base, "mods/a.pw.toml").as_deref(),
        Some(format!("{PW}/mods/a.pw.toml").as_str())
    );
    for bad in [
        "../x",
        "/etc/passwd",
        "a//b",
        "https://evil.example/x",
        "a\\b",
        "./a",
    ] {
        assert_eq!(packwiz::relative(&base, bad), None, "{bad}");
    }

    let good = format!(
        "name = \"My Pack\"\nversion = \"1.2\"\n[index]\nfile = \"index.toml\"\nhash-format = \"sha256\"\nhash = \"{}\"\n[versions]\nminecraft = \"1.20.1\"\nforge = \"47.2.0\"\n",
        "a".repeat(64)
    );
    let found = packwiz::pack(&good).unwrap();
    assert_eq!(found.game, "1.20.1");
    assert_eq!(
        found.loader,
        Some(("forge".to_owned(), "47.2.0".to_owned()))
    );
    assert_eq!(packwiz::slug(&found.name), "my-pack");
    for (bad, code) in [
        (good.replace("forge", "liteloader"), "unsupported_pack"),
        (
            good.replace("[versions]\nminecraft = \"1.20.1\"\n", "[versions]\n"),
            "bad_pack",
        ),
        (good.replace("sha256", "murmur2"), "bad_pack"),
        (good.replace("47.2.0", "47 2"), "bad_pack"),
        ("not toml [".to_owned(), "bad_pack"),
    ] {
        assert_eq!(packwiz::pack(&bad), Err(code));
    }

    let index = format!(
        "hash-format = \"sha256\"\n[[files]]\nfile = \"mods/a.pw.toml\"\nhash = \"{h}\"\nmetafile = true\n[[files]]\nfile = \"config/a.toml\"\nhash = \"{h}\"\n[[files]]\nfile = \"../escape.txt\"\nhash = \"{h}\"\n[[files]]\nfile = \"config/run.exe\"\nhash = \"{h}\"\n[[files]]\nfile = \"a.pw.toml\"\nhash = \"{h}\"\nmetafile = true\n",
        h = "b".repeat(64)
    );
    let (files, skipped) = packwiz::index(&index).unwrap();
    assert_eq!(files.len(), 2);
    assert_eq!(skipped, 3);

    let meta = |text: &str| packwiz::meta(text, "mods/a.pw.toml");
    let sha512 = "c".repeat(128);
    let modrinth = format!(
        "filename = \"a.jar\"\nside = \"both\"\n[download]\nurl = \"https://cdn.modrinth.com/data/x/versions/y/a.jar\"\nhash-format = \"sha512\"\nhash = \"{sha512}\"\n"
    );
    let found = meta(&modrinth).unwrap().unwrap();
    assert_eq!(found.path, "mods/a.jar");
    assert!(!found.optional);
    assert_eq!(meta(&modrinth.replace("both", "server")), Ok(None));
    assert_eq!(
        meta(&modrinth.replace("a.jar\"\nside", "a.exe\"\nside")),
        Ok(None)
    );
    assert_eq!(
        meta(&modrinth.replace("\"a.jar\"\nside", "\"../a.jar\"\nside")),
        Err("bad_pack")
    );
    assert_eq!(
        meta(&modrinth.replace("https://cdn.modrinth.com", "https://evil.example")),
        Err("unsupported_host")
    );
    let cf = "filename = \"jei.jar\"\n[download]\nhash-format = \"murmur2\"\nhash = \"123\"\nmode = \"metadata:curseforge\"\n[update.curseforge]\nfile-id = 5\nproject-id = 7\n[option]\noptional = true\n";
    let found = meta(cf).unwrap().unwrap();
    assert_eq!(found.url, curseforge::download_url(7, 5));
    assert!(found.optional);
    // A hash that cannot be checked only from CurseForge.
    let unchecked = modrinth
        .replace("sha512", "md5")
        .replace(&sha512, "d41d8cd98f00b204e9800998ecf8427e");
    assert_eq!(meta(&unchecked), Err("bad_pack"));
}

#[tokio::test]
async fn a_packwiz_pack_becomes_a_modpack_with_every_file_kept() {
    let t = setup();
    let sodium = b"sodium jar for packwiz".to_vec();
    let jei = b"jei jar from curseforge".to_vec();
    let config = b"a = 1".to_vec();
    let sodium_url = format!("https://cdn.modrinth.com/data/{SODIUM}/versions/v1/sodium.jar");
    t.sites.put(&sodium_url, ok(sodium.clone()));
    cf_download(&t.sites, JEI, 4242, "jei.jar", &jei);
    let (_, sodium_512, _) = hashes(&sodium);
    let (jei_1, _, _) = hashes(&jei);
    let sodium_meta = format!(
        "name = \"Sodium\"\nfilename = \"sodium.jar\"\nside = \"client\"\n[download]\nurl = \"{sodium_url}\"\nhash-format = \"sha512\"\nhash = \"{sodium_512}\"\n"
    );
    let jei_meta = format!(
        "filename = \"jei.jar\"\n[download]\nhash-format = \"sha1\"\nhash = \"{jei_1}\"\nmode = \"metadata:curseforge\"\n[update.curseforge]\nfile-id = 4242\nproject-id = {JEI}\n"
    );
    let server_meta = format!(
        "filename = \"server.jar\"\nside = \"server\"\n[download]\nurl = \"{sodium_url}\"\nhash-format = \"sha512\"\nhash = \"{sodium_512}\"\n"
    );
    for (path, text) in [
        ("mods/sodium.pw.toml", &sodium_meta),
        ("mods/jei.pw.toml", &jei_meta),
        ("mods/server.pw.toml", &server_meta),
    ] {
        t.sites
            .put(&format!("{PW}/{path}"), ok(text.as_bytes().to_vec()));
    }
    t.sites
        .put(&format!("{PW}/config/a.toml"), ok(config.clone()));
    let index = format!(
        "hash-format = \"sha256\"\n\
         [[files]]\nfile = \"mods/sodium.pw.toml\"\nhash = \"{}\"\nmetafile = true\n\
         [[files]]\nfile = \"mods/jei.pw.toml\"\nhash = \"{}\"\nmetafile = true\n\
         [[files]]\nfile = \"mods/server.pw.toml\"\nhash = \"{}\"\nmetafile = true\n\
         [[files]]\nfile = \"config/a.toml\"\nhash = \"{}\"\n\
         [[files]]\nfile = \"run.sh\"\nhash = \"{}\"\n",
        sha256_hex(sodium_meta.as_bytes()),
        sha256_hex(jei_meta.as_bytes()),
        sha256_hex(server_meta.as_bytes()),
        sha256_hex(&config),
        sha256_hex(b"x"),
    );
    t.sites
        .put(&format!("{PW}/index.toml"), ok(index.as_bytes().to_vec()));
    let pack_toml = format!(
        "name = \"My Pack\"\nauthor = \"me\"\nversion = \"1.0\"\npack-format = \"packwiz:1.1.0\"\n[index]\nfile = \"index.toml\"\nhash-format = \"sha256\"\nhash = \"{}\"\n[versions]\nminecraft = \"1.20.1\"\nforge = \"47.2.0\"\n",
        sha256_hex(index.as_bytes())
    );
    t.sites.put(
        &format!("{PW}/pack.toml"),
        ok(pack_toml.as_bytes().to_vec()),
    );

    let (status, request) = t
        .player(
            1,
            "POST",
            "/v1/imports",
            json!({ "input": "https://github.com/me/pack/blob/main/pack.toml" }),
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{request}");
    t.drain().await;
    let id = request["id"].as_str().unwrap();
    let (_, request) = t
        .player(1, "GET", &format!("/v1/imports/{id}"), Value::Null)
        .await;
    assert_eq!(request["state"], "ready", "{request}");

    let (status, project) = t.get("/v1/projects/my-pack").await;
    assert_eq!(status, StatusCode::OK, "{project}");
    assert_eq!(project["kind"], "modpack");
    assert_eq!(project["title"], "My Pack");
    assert_eq!(project["authors"][0]["name"], "me");
    let (_, versions) = t.get("/v1/projects/my-pack/versions").await;
    let version = &versions["versions"][0];
    assert_eq!(version["number"], "1.0");
    assert_eq!(version["games"], json!(["1.20.1"]));
    assert_eq!(version["loaders"], json!(["forge"]));
    assert_eq!(version["saved"], "all", "{version}");
    let version = version["id"].as_str().unwrap().to_owned();

    // The launcher installs it like any modpack; nothing is fetched again.
    let (status, _) = t
        .call(
            None,
            "POST",
            &format!("/v1/versions/{version}/pack"),
            Value::Null,
        )
        .await;
    assert!(status.is_success());
    t.drain().await;
    let (_, pack) = t.get(&format!("/v1/versions/{version}/pack")).await;
    assert_eq!(pack["state"], "ready", "{pack}");
    assert_eq!(pack["game"], "1.20.1");
    assert_eq!(
        pack["loader"],
        json!({ "kind": "forge", "version": "47.2.0" })
    );
    let paths: Vec<&str> = pack["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["path"].as_str().unwrap())
        .collect();
    assert_eq!(paths, ["mods/sodium.jar", "mods/jei.jar"]);
    assert_eq!(t.sites.asked(&sodium_url), 1);
    let archive = t
        .app
        .blobs
        .path(pack["archive"]["sha256"].as_str().unwrap());
    let mut zip = zip::ZipArchive::new(std::fs::File::open(archive).unwrap()).unwrap();
    let mut text = String::new();
    std::io::Read::read_to_string(
        &mut zip.by_name("overrides/config/a.toml").unwrap(),
        &mut text,
    )
    .unwrap();
    assert_eq!(text, "a = 1");
    assert!(zip.by_name("overrides/run.sh").is_err());

    // Read again unchanged: the same version.
    import::queue_project(
        &t.app,
        packwiz::SOURCE,
        &format!("{PW}/pack.toml"),
        0,
        None,
        0,
    )
    .unwrap();
    t.drain().await;
    let (_, versions) = t.get("/v1/projects/my-pack/versions").await;
    assert_eq!(versions["versions"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn a_packwiz_file_that_is_not_what_its_index_says_fails_the_pack() {
    let t = setup();
    let index = "hash-format = \"sha256\"\n[[files]]\nfile = \"config/a.toml\"\nhash = \""
        .to_owned()
        + &sha256_hex(b"a = 1")
        + "\"\n";
    t.sites
        .put(&format!("{PW}/config/a.toml"), ok(b"a = 2".to_vec()));
    t.sites
        .put(&format!("{PW}/index.toml"), ok(index.as_bytes().to_vec()));
    let pack_toml = format!(
        "name = \"Bad\"\n[index]\nfile = \"index.toml\"\nhash-format = \"sha256\"\nhash = \"{}\"\n[versions]\nminecraft = \"1.20.1\"\n",
        sha256_hex(index.as_bytes())
    );
    t.sites.put(
        &format!("{PW}/pack.toml"),
        ok(pack_toml.as_bytes().to_vec()),
    );
    let (_, request) = t
        .player(
            1,
            "POST",
            "/v1/imports",
            json!({ "input": format!("{PW}/pack.toml") }),
        )
        .await;
    t.drain().await;
    let id = request["id"].as_str().unwrap();
    let (_, request) = t
        .player(1, "GET", &format!("/v1/imports/{id}"), Value::Null)
        .await;
    assert_eq!(request["state"], "failed", "{request}");
    let (status, _) = t.get("/v1/projects/bad").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

// ----- the scanner (D-066, stage 6) -----

/// A class file with the given method references and strings, and one
/// method whose code is `code`.
fn class_file(calls: &[(&str, &str)], strings: &[&str], code: &[u8]) -> Vec<u8> {
    let mut pool: Vec<Vec<u8>> = Vec::new();
    let mut utf8 = |pool: &mut Vec<Vec<u8>>, text: &str| -> u16 {
        let mut entry = vec![1];
        entry.extend((text.len() as u16).to_be_bytes());
        entry.extend(text.as_bytes());
        pool.push(entry);
        pool.len() as u16
    };
    let class = |pool: &mut Vec<Vec<u8>>,
                 utf8: &mut dyn FnMut(&mut Vec<Vec<u8>>, &str) -> u16,
                 name: &str|
     -> u16 {
        let at = utf8(pool, name);
        let mut entry = vec![7];
        entry.extend(at.to_be_bytes());
        pool.push(entry);
        pool.len() as u16
    };
    let this = class(&mut pool, &mut utf8, "a/Sample");
    let object = class(&mut pool, &mut utf8, "java/lang/Object");
    for (owner, name) in calls {
        let owner = class(&mut pool, &mut utf8, owner);
        let name = utf8(&mut pool, name);
        let descriptor = utf8(&mut pool, "()V");
        let mut nat = vec![12];
        nat.extend(name.to_be_bytes());
        nat.extend(descriptor.to_be_bytes());
        pool.push(nat);
        let nat = pool.len() as u16;
        let mut method = vec![10];
        method.extend(owner.to_be_bytes());
        method.extend(nat.to_be_bytes());
        pool.push(method);
    }
    for text in strings {
        let at = utf8(&mut pool, text);
        let mut entry = vec![8];
        entry.extend(at.to_be_bytes());
        pool.push(entry);
    }
    // A long constant takes two slots.
    pool.push(vec![5, 0, 0, 0, 0, 0, 0, 0, 1]);
    pool.push(Vec::new());
    let code_name = utf8(&mut pool, "Code");
    let method_name = utf8(&mut pool, "run");
    let descriptor = utf8(&mut pool, "()V");

    let mut out = vec![0xCA, 0xFE, 0xBA, 0xBE, 0, 0, 0, 52];
    out.extend(((pool.len() + 1) as u16).to_be_bytes());
    for entry in &pool {
        out.extend(entry);
    }
    out.extend([0, 0x21]);
    out.extend(this.to_be_bytes());
    out.extend(object.to_be_bytes());
    out.extend([0, 0, 0, 0, 0, 1, 0, 9]);
    out.extend(method_name.to_be_bytes());
    out.extend(descriptor.to_be_bytes());
    out.extend([0, 1]);
    out.extend(code_name.to_be_bytes());
    out.extend(((12 + code.len()) as u32).to_be_bytes());
    out.extend([0, 4, 0, 1]);
    out.extend((code.len() as u32).to_be_bytes());
    out.extend(code);
    out.extend([0, 0, 0, 0, 0, 0]);
    out
}

/// Code that builds `text` byte by byte, the way fractureiser hid its address.
fn built(text: &str) -> Vec<u8> {
    let mut code = vec![0x11];
    code.extend((text.len() as u16).to_be_bytes());
    code.extend([0xbc, 8]);
    for (i, b) in text.bytes().enumerate() {
        code.push(0x59);
        if i <= 5 {
            code.push(0x03 + i as u8);
        } else {
            code.extend([0x11]);
            code.extend((i as u16).to_be_bytes());
        }
        code.extend([0x10, b]);
        code.push(0x54);
    }
    code.push(0x57);
    code
}

fn jar(classes: &[(&str, Vec<u8>)]) -> Vec<u8> {
    let entries: Vec<(&str, &[u8])> = classes.iter().map(|(n, b)| (*n, b.as_slice())).collect();
    mrpack(&entries)
}

#[test]
fn honest_code_is_clean_and_weak_signs_are_only_noted() {
    use crate::scan::{Level, scan_bytes, sign};
    let honest = class_file(
        &[("java/lang/String", "valueOf")],
        &["https://modrinth.com/mod/sodium", "Loading config"],
        &[0xb1],
    );
    let verdict = scan_bytes(&jar(&[
        ("a/Sample.class", honest),
        ("fabric.mod.json", b"{}".to_vec()),
    ]));
    assert_eq!(verdict.level, Level::Clean, "{verdict:?}");

    // Starting a program, a connection to check for updates and a native
    // library are each common in honest mods.
    let runs = class_file(&[("java/lang/ProcessBuilder", "<init>")], &[], &[0xb1]);
    let updates = class_file(&[("java/net/URL", "openStream")], &[], &[0xb1]);
    let verdict = scan_bytes(&jar(&[
        ("a/Runs.class", runs),
        ("a/Updates.class", updates),
        ("natives/helper.exe", b"MZ".to_vec()),
    ]));
    assert_eq!(verdict.level, Level::Note, "{verdict:?}");
    assert_eq!(
        verdict.signs,
        vec![sign::NETWORK, sign::PROGRAMS_INSIDE, sign::RUNS_PROGRAMS]
    );
}

#[test]
fn loading_code_from_the_network_or_a_hidden_address_is_held() {
    use crate::scan::{Level, scan_bytes, sign};
    let dropper = class_file(
        &[
            ("java/net/URLClassLoader", "<init>"),
            ("java/net/URL", "openConnection"),
        ],
        &[],
        &[0xb1],
    );
    let verdict = scan_bytes(&jar(&[("a/Loader.class", dropper)]));
    assert_eq!(verdict.level, Level::Suspicious, "{verdict:?}");

    // The address is built from bytes, then a program is started.
    let mut code = built("https://downloads.example/stage1.jar");
    code.push(0xb1);
    let hidden = class_file(&[("java/lang/Runtime", "exec")], &[], &code);
    let verdict = scan_bytes(&jar(&[("a/Hidden.class", hidden)]));
    assert_eq!(verdict.level, Level::Suspicious, "{verdict:?}");
    assert!(verdict.signs.contains(&sign::HIDDEN_ADDRESS));

    // Apart, in two classes, the same calls are only noted.
    let loads = class_file(&[("java/net/URLClassLoader", "<init>")], &[], &[0xb1]);
    let net = class_file(&[("java/net/URL", "openConnection")], &[], &[0xb1]);
    let verdict = scan_bytes(&jar(&[("a/A.class", loads), ("a/B.class", net)]));
    assert_eq!(verdict.level, Level::Note, "{verdict:?}");

    // Sign-in files and the network anywhere in one jar: a stealer.
    let reads = class_file(
        &[],
        &["%APPDATA%/.minecraft/launcher_accounts.json"],
        &[0xb1],
    );
    let sends = class_file(&[("java/net/http/HttpClient", "send")], &[], &[0xb1]);
    let verdict = scan_bytes(&jar(&[("a/A.class", reads), ("a/B.class", sends)]));
    assert_eq!(verdict.level, Level::Suspicious, "{verdict:?}");
    assert!(verdict.signs.contains(&sign::SIGN_IN_FILES));
}

#[test]
fn a_known_marker_is_malware_even_hidden_or_nested() {
    use crate::scan::{Level, scan_bytes, sign};
    let mut code = built("http://85.217.144.130:8080/dl");
    code.push(0xb1);
    let stage0 = class_file(&[], &[], &code);
    let inner = jar(&[("dev/Utility.class", stage0)]);
    let outer = jar(&[
        ("META-INF/jars/inner.jar", inner),
        ("a/Plain.class", class_file(&[], &[], &[0xb1])),
    ]);
    let verdict = scan_bytes(&outer);
    assert_eq!(verdict.level, Level::Malicious, "{verdict:?}");
    assert!(verdict.signs.contains(&sign::KNOWN_MALWARE));
}

#[test]
fn junk_and_oversized_archives_do_not_trip_the_scanner() {
    use crate::scan::{Class, Level, scan_bytes, sign};
    assert_eq!(scan_bytes(b"not a zip").level, Level::Clean);
    assert!(Class::parse(b"").is_none());
    assert!(Class::parse(&[0xCA, 0xFE, 0xBA, 0xBE, 0, 0]).is_none());
    // Every cut of a real class file, and a pool count past the end.
    let real = class_file(
        &[("java/net/URL", "openStream")],
        &["text"],
        &built("abcdefgh"),
    );
    for cut in 0..real.len() {
        let _ = Class::parse(&real[..cut]);
    }
    let mut lying = real.clone();
    lying[8] = 0xff;
    lying[9] = 0xff;
    assert!(Class::parse(&lying).is_none());
    // Code with a switch whose bounds point far past the end.
    let mut code = vec![0x00, 0xaa, 0, 0];
    code.extend([0, 0, 0, 0, 0x80, 0, 0, 0, 0x7f, 0xff, 0xff, 0xff]);
    assert!(Class::parse(&class_file(&[], &[], &code)).is_some());
    // A class too big to read is noted as unread, not opened.
    let huge = vec![0u8; 9 * 1024 * 1024];
    let verdict = scan_bytes(&jar(&[
        ("a/Huge.class", huge),
        ("a/Junk.class", vec![7; 100]),
    ]));
    assert_eq!(verdict.signs, vec![sign::UNREAD], "{verdict:?}");
    // Jars nested deeper than the limit are not opened.
    let mut nested = jar(&[(
        "a/Loader.class",
        class_file(
            &[
                ("java/net/URLClassLoader", "<init>"),
                ("java/net/URL", "openStream"),
            ],
            &[],
            &[0xb1],
        ),
    )]);
    for _ in 0..4 {
        nested = jar(&[("inner.jar", nested)]);
    }
    assert_eq!(scan_bytes(&nested).level, Level::Clean);
}

fn dropper_jar() -> Vec<u8> {
    jar(&[(
        "a/Loader.class",
        class_file(
            &[
                ("java/net/URLClassLoader", "<init>"),
                ("java/net/URL", "openConnection"),
            ],
            &[],
            &[0xb1],
        ),
    )])
}

/// Puts `bytes` where the newest Sodium release downloads from.
fn sodium_release_is(t: &T, bytes: &[u8]) {
    let (sha1, sha512, _) = hashes(bytes);
    let url = format!("https://cdn.modrinth.com/data/{SODIUM}/versions/v2bbbbbb/sodium-0.6.jar");
    t.sites.put(&url, ok(bytes.to_vec()));
    let file = json!({
        "filename": "sodium-0.6.jar", "size": bytes.len(), "primary": true,
        "hashes": { "sha1": sha1, "sha512": sha512 }, "url": url,
    });
    let mut versions: Value = serde_json::from_slice(
        &t.sites.pages.lock().unwrap()[&modrinth::versions_url(SODIUM)].body,
    )
    .unwrap();
    versions[1]["files"] = json!([file]);
    t.sites.json(&modrinth::versions_url(SODIUM), versions);
}

#[tokio::test]
async fn a_suspicious_file_is_held_back_and_put_before_the_owner() {
    let t = setup();
    modrinth_site(&t.sites);
    let dropper = dropper_jar();
    sodium_release_is(&t, &dropper);
    t.player(
        1,
        "POST",
        "/v1/imports",
        json!({ "input": "https://modrinth.com/mod/sodium" }),
    )
    .await;
    t.drain().await;

    let (_, versions) = t.get("/v1/projects/sodium/versions").await;
    let file = &versions["versions"][1]["files"][0];
    assert_eq!(file["held"], true, "{file}");
    assert!(file["url"].is_null(), "{file}");
    let sha256 = file["sha256"].as_str().unwrap();
    assert!(!t.app.blobs.has(sha256));
    assert!(t.app.blobs.holds(sha256));
    assert_eq!(
        t.app.store.reviews(),
        vec![("scan".to_owned(), sha256.to_owned())]
    );
    let counts = t.app.store.counts().unwrap();
    assert_eq!(counts["blobsHeld"], 1);
    assert_eq!(counts["reviewOpen"], 1);
    // Fabric API is served as before.
    let (_, api) = t.get("/v1/projects/fabric-api/versions").await;
    assert_eq!(api["versions"][0]["files"][0]["held"], false);
    assert!(api["versions"][0]["files"][0]["url"].is_string());
    // Suspected is not known harmful: launchers are not told to warn yet.
    let (status, harmful) = t.get("/v1/harmful").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(harmful["files"], json!([]));

    // Asked again, the held file is not fetched again.
    let before = t.sites.asked(&format!(
        "https://cdn.modrinth.com/data/{SODIUM}/versions/v2bbbbbb/sodium-0.6.jar"
    ));
    let id = versions["versions"][1]["id"].as_str().unwrap();
    t.player(1, "POST", &format!("/v1/versions/{id}/fetch"), Value::Null)
        .await;
    t.drain().await;
    assert_eq!(
        t.sites.asked(&format!(
            "https://cdn.modrinth.com/data/{SODIUM}/versions/v2bbbbbb/sodium-0.6.jar"
        )),
        before
    );
}

#[tokio::test]
async fn kept_files_are_scanned_again_under_new_rules() {
    let t = setup();
    modrinth_site(&t.sites);
    t.player(
        1,
        "POST",
        "/v1/imports",
        json!({ "input": "https://modrinth.com/mod/sodium" }),
    )
    .await;
    t.drain().await;
    let (_, versions) = t.get("/v1/projects/sodium/versions").await;
    let sha256 = versions["versions"][1]["files"][0]["sha256"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(t.app.blobs.has(&sha256));
    // Nothing is due under the rules it was scanned with.
    assert_eq!(import::rescan(&t.app, 50).await.unwrap(), 0);

    // The kept bytes turn out to be malware once the rules know it.
    let mut code = built("https://files-8ie.pages.dev/dl");
    code.push(0xb1);
    std::fs::write(
        t.app.blobs.path(&sha256),
        jar(&[("a/Stage.class", class_file(&[], &[], &code))]),
    )
    .unwrap();
    t.app.store.forget_scans();
    // Only archives are scanned: the two jars, not the pictures.
    assert_eq!(import::rescan(&t.app, 50).await.unwrap(), 2);
    assert!(!t.app.blobs.has(&sha256));
    assert!(t.app.blobs.holds(&sha256));
    let (_, versions) = t.get("/v1/projects/sodium/versions").await;
    assert_eq!(versions["versions"][1]["files"][0]["held"], true);
    assert!(versions["versions"][1]["files"][0]["url"].is_null());
    // Known malware goes straight to launchers' warning list, not to review.
    assert!(t.app.store.reviews().is_empty());
    let (_, harmful) = t.get("/v1/harmful").await;
    assert_eq!(harmful["files"][0]["sha256"], sha256);
    assert!(harmful["next"].is_null());
    let since = harmful["files"][0]["since"].as_i64().unwrap();
    let (_, later) = t
        .get(&format!("/v1/harmful?since={since}&after={sha256}"))
        .await;
    assert_eq!(later["files"], json!([]));
    let (status, _) = t.get("/v1/harmful?since=-5&after=../x").await;
    assert_eq!(status, StatusCode::OK);
}

// ----- the owner's commands (D-066, stage 6) -----

#[derive(Default)]
struct Purged(Mutex<Vec<String>>);

impl crate::owner::Purge for Purged {
    fn purge(&self, urls: &[String]) -> Result<usize, String> {
        self.0.lock().unwrap().extend(urls.iter().cloned());
        Ok(urls.len())
    }
}

fn owner<'a>(t: &'a T, purged: &'a Purged) -> crate::owner::Owner<'a> {
    crate::owner::Owner {
        store: &t.app.store,
        blobs: &t.app.blobs,
        urls: &t.app.urls,
        purge: Some(purged),
        bucket: None,
        virustotal: t.app.virustotal.as_deref(),
        now: now(),
    }
}

fn sha(text: &str) -> crate::owner::Hash {
    crate::owner::Hash::parse(text).unwrap()
}

async fn sodium_release(t: &T) -> (String, Value) {
    let (_, versions) = t.get("/v1/projects/sodium/versions").await;
    let file = versions["versions"][1]["files"][0].clone();
    (file["sha256"].as_str().unwrap().to_owned(), file)
}

#[tokio::test]
async fn a_blocked_file_is_withdrawn_cleared_from_the_cache_and_named_to_launchers() {
    let t = setup();
    modrinth_site(&t.sites);
    t.player(
        1,
        "POST",
        "/v1/imports",
        json!({ "input": "https://modrinth.com/mod/sodium" }),
    )
    .await;
    t.drain().await;
    let (sha256, file) = sodium_release(&t).await;
    let sha1 = file["sha1"].as_str().unwrap().to_owned();
    assert!(t.app.blobs.has(&sha256));
    let purged = Purged::default();
    let o = owner(&t, &purged);

    let lines = o.block(&sha(&sha256), "steals tokens").unwrap();
    assert_eq!(
        lines,
        ["blocked 1", "withdrawn 1", "cache cleared 1 addresses"]
    );
    assert_eq!(
        *purged.0.lock().unwrap(),
        [format!("{FILES}/f/{sha256}/sodium%2D0%2E6%2Ejar")]
    );
    assert!(!t.app.blobs.has(&sha256));
    assert!(t.app.blobs.holds(&sha256));
    let (_, file) = sodium_release(&t).await;
    assert_eq!(file["held"], true);
    assert!(file["url"].is_null());
    let (_, harmful) = t.get("/v1/harmful").await;
    assert_eq!(harmful["files"].as_array().unwrap().len(), 1);
    assert_eq!(harmful["files"][0]["sha1"], sha1.as_str());
    assert_eq!(harmful["files"][0]["sha256"], sha256.as_str());
    // Again: nothing more to do. By its SHA-1: the same file.
    assert_eq!(o.block(&sha(&sha256), "").unwrap()[0], "blocked 0");
    assert_eq!(
        o.block(&sha(&sha1), "").unwrap()[..2],
        ["blocked 0", "withdrawn 0"]
    );
    let found = o.find(&sha256).unwrap().join("\n");
    assert!(found.contains("quarantined blocked"), "{found}");
    assert!(found.contains("blocked: steals tokens"), "{found}");
    assert!(found.contains("in version"), "{found}");
    assert!(found.contains("block steals tokens"), "{found}");
    assert_eq!(t.app.store.counts().unwrap()["blocked"], 1);

    // A mistake: the file is served again, and no later scan holds it.
    assert_eq!(
        o.unblock(&sha(&sha1)).unwrap(),
        ["unblocked 1", "released 1"]
    );
    assert!(t.app.blobs.has(&sha256));
    let (_, file) = sodium_release(&t).await;
    assert_eq!(file["held"], false);
    assert!(file["url"].is_string());
    let (_, harmful) = t.get("/v1/harmful").await;
    assert_eq!(harmful["files"], json!([]));
    t.app.store.forget_scans();
    import::rescan(&t.app, 50).await.unwrap();
    assert!(t.app.blobs.has(&sha256));
    let found = o.find(&sha256).unwrap().join("\n");
    assert!(found.contains("stored allowed"), "{found}");
    assert!(found.contains("allowed by the owner"), "{found}");
}

#[tokio::test]
async fn a_file_blocked_before_it_comes_is_held_back_when_it_does() {
    let t = setup();
    let site = modrinth_site(&t.sites);
    let (sha1, _, sha256) = hashes(&site["v2bbbbbb"]);
    let purged = Purged::default();
    let o = owner(&t, &purged);
    assert_eq!(
        o.block(&sha(&sha1.to_uppercase()), "public report")
            .unwrap(),
        ["blocked 1", "not here yet: held back if it ever comes"]
    );
    assert!(purged.0.lock().unwrap().is_empty());
    // Launchers hear of it at once, by its SHA-1.
    let (_, harmful) = t.get("/v1/harmful").await;
    assert_eq!(harmful["files"][0]["sha1"], sha1.as_str());
    assert!(harmful["files"][0]["sha256"].is_null());

    t.player(
        1,
        "POST",
        "/v1/imports",
        json!({ "input": "https://modrinth.com/mod/sodium" }),
    )
    .await;
    t.drain().await;
    let (kept, file) = sodium_release(&t).await;
    assert_eq!(kept, sha256);
    assert_eq!(file["held"], true);
    assert!(t.app.blobs.holds(&sha256) && !t.app.blobs.has(&sha256));
    // Blocked is decided: nothing waits for the owner.
    assert!(t.app.store.reviews().is_empty());
    let (_, harmful) = t.get("/v1/harmful").await;
    assert_eq!(harmful["files"].as_array().unwrap().len(), 1);
    assert_eq!(harmful["files"][0]["sha256"], sha256.as_str());
    // The other versions are served as before.
    let (_, versions) = t.get("/v1/projects/sodium/versions").await;
    assert_eq!(versions["versions"][0]["files"][0]["held"], false);
}

#[tokio::test]
async fn the_owner_decides_what_waits_removes_a_project_and_forgets_a_file() {
    let t = setup();
    modrinth_site(&t.sites);
    sodium_release_is(&t, &dropper_jar());
    t.player(
        1,
        "POST",
        "/v1/imports",
        json!({ "input": "https://modrinth.com/mod/sodium" }),
    )
    .await;
    t.drain().await;
    let (sha256, _) = sodium_release(&t).await;
    let purged = Purged::default();
    let o = owner(&t, &purged);
    let waiting = o.review().unwrap().join("\n");
    assert!(waiting.starts_with("#1 "), "{waiting}");
    assert!(
        waiting.contains(&format!("scan {sha256} suspicious")),
        "{waiting}"
    );
    assert!(waiting.contains("in version"), "{waiting}");
    assert_eq!(
        o.review_count().unwrap(),
        [
            "scan 1",
            "total 1",
            "virustotal checked 0, with detections 0, waiting 0"
        ]
    );
    assert!(o.decide(1, "maybe", "").is_err());
    assert!(o.decide(7, "keep", "").is_err());

    let lines = o.decide(1, "keep", "Essential-like, fine").unwrap();
    assert_eq!(lines.last().unwrap(), "decided #1 keep");
    assert!(t.app.blobs.has(&sha256));
    assert!(t.app.store.reviews().is_empty());
    assert_eq!(o.review().unwrap(), ["nothing waits"]);
    assert_eq!(
        o.review_count().unwrap(),
        [
            "total 0",
            "virustotal checked 0, with detections 0, waiting 0"
        ]
    );
    assert!(o.decide(1, "block", "").is_err(), "decided already");

    let (_, found) = t.get("/v1/search?q=sodium").await;
    assert_eq!(found["total"], 1, "{found}");

    // The author asks: the project goes, and the files only it needs.
    let lines = o
        .remove("https://modrinth.com/mod/sodium", "author asked")
        .unwrap();
    assert!(lines[0].starts_with("removed project "), "{lines:?}");
    assert_eq!(lines[1], "withdrawn 1");
    assert!(!t.app.blobs.has(&sha256));
    let (status, _) = t.get("/v1/projects/sodium").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (_, found) = t.get("/v1/search?q=sodium").await;
    assert_eq!(found["total"], 0, "{found}");
    // Its dependency stays.
    let (_, api) = t.get("/v1/projects/fabric-api/versions").await;
    let api_file = api["versions"][0]["files"][0]["sha256"].as_str().unwrap();
    assert!(t.app.blobs.has(api_file));
    // An import does not bring it back.
    let (_, request) = t
        .player(
            2,
            "POST",
            "/v1/imports",
            json!({ "input": "https://modrinth.com/mod/sodium" }),
        )
        .await;
    t.drain().await;
    let (_, request) = t
        .player(
            2,
            "GET",
            &format!("/v1/imports/{}", request["id"].as_str().unwrap()),
            Value::Null,
        )
        .await;
    assert_eq!(request["state"], "failed", "{request}");
    assert_eq!(request["error"], "removed");
    let (status, _) = t.get("/v1/projects/sodium").await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Deleted for good only when asked twice.
    assert!(o.forget(api_file, true).is_err(), "a served file stays");
    let lines = o.forget(&sha256, false).unwrap();
    assert!(lines[0].starts_with("would delete"));
    assert!(t.app.blobs.holds(&sha256));
    assert_eq!(o.forget(&sha256, true).unwrap(), ["forgotten 1"]);
    assert!(!t.app.blobs.holds(&sha256));
    assert_eq!(
        t.app.store.blob_state(&sha256).unwrap().as_deref(),
        Some("forgotten")
    );
    assert!(o.forget(&sha256, true).is_err());
    let counts = t.app.store.counts().unwrap();
    assert_eq!(counts["blobsForgotten"], 1);
    assert_eq!(counts["projectsRemoved"], 1);
    let found = o.find("sodium").unwrap().join("\n");
    assert!(found.contains("removed"), "{found}");
    assert!(found.contains("forgotten"), "{found}");
    assert!(found.contains("remove author asked"), "{found}");
}

#[test]
fn the_owners_input_is_checked() {
    use crate::owner::{Cloudflare, Hash, purge_config};
    assert_eq!(
        Hash::parse(&"AB".repeat(20)),
        Some(Hash::Sha1("ab".repeat(20)))
    );
    assert_eq!(
        Hash::parse(&"0f".repeat(32)),
        Some(Hash::Sha256("0f".repeat(32)))
    );
    for junk in [
        "",
        "abc",
        &"a".repeat(63),
        &"g".repeat(64),
        &"a".repeat(41),
        "'; DROP TABLE blob; --",
    ] {
        assert_eq!(Hash::parse(junk), None, "{junk}");
    }
    assert_eq!(
        crate::owner::reason(&["a\u{0}b".to_owned(), "c\nd".to_owned()]),
        "ab cd"
    );
    assert!(crate::owner::reason(&["x".repeat(5000)]).chars().count() <= 300);

    let curl = std::path::PathBuf::from("curl");
    let zone = "0123456789abcdef0123456789abcdef";
    let token = "Abc_def-0123456789xyzXYZ";
    assert!(Cloudflare::new(zone, token, curl.clone()).is_some());
    for (zone, token) in [
        ("", token),
        ("0123456789abcdef0123456789abcdeg", token),
        (zone, ""),
        (zone, "short"),
        (zone, "Abc_def-0123456789\"\nurl = \"http://evil"),
        (zone, "Abc def 0123456789xyzXYZ"),
    ] {
        assert!(
            Cloudflare::new(zone, token, curl.clone()).is_none(),
            "{token}"
        );
    }
    // The token travels on standard input; the body is quoted for curl.
    let config = purge_config(
        zone,
        token,
        &["https://files.example/f/ab/a%22b.jar".to_owned()],
    );
    assert!(config.contains(&format!("zones/{zone}/purge_cache")));
    assert!(config.contains(&format!("header = \"Authorization: Bearer {token}\"")));
    assert!(
        config
            .contains("data = \"{\\\"files\\\":[\\\"https://files.example/f/ab/a%22b.jar\\\"]}\"")
    );
    assert_eq!(config.lines().count(), 5);
}

#[tokio::test]
async fn finding_takes_addresses_slugs_and_hashes_and_nothing_else() {
    let t = setup();
    modrinth_site(&t.sites);
    t.player(
        1,
        "POST",
        "/v1/imports",
        json!({ "input": "https://modrinth.com/mod/sodium" }),
    )
    .await;
    t.drain().await;
    let purged = Purged::default();
    let o = owner(&t, &purged);
    for query in [
        "sodium",
        "SODIUM",
        SODIUM,
        "https://modrinth.com/mod/sodium",
    ] {
        let found = o.find(query).unwrap();
        assert!(found[0].starts_with("project "), "{query}: {found:?}");
        assert!(
            found.iter().any(|l| l.starts_with("  version ")),
            "{found:?}"
        );
    }
    assert_eq!(o.find("no-such-mod").unwrap(), ["nothing found"]);
    assert_eq!(o.find("' OR 1=1 --").unwrap(), ["nothing found"]);
    assert_eq!(o.find(&"0".repeat(64)).unwrap(), ["nothing found"]);
    assert!(o.find("https://example.com/mod/sodium").is_err());
    assert!(o.find("so\u{0}dium").is_err());
    assert!(o.remove("no-such-mod", "").is_err());
    assert!(o.unblock(&sha(&"0".repeat(40))).unwrap()[0] == "unblocked 0");
}

// ----- R2 (D-066, stage 6) -----

/// R2 in memory: objects by bucket and key. Like R2, it refuses bytes that
/// do not match the SHA-256 an upload names.
#[derive(Default)]
struct FakeR2 {
    objects: Mutex<HashMap<(bool, String), Vec<u8>>>,
    down: std::sync::atomic::AtomicBool,
}

impl FakeR2 {
    fn has(&self, public: bool, key: &str) -> Option<Vec<u8>> {
        self.objects
            .lock()
            .unwrap()
            .get(&(public, key.to_owned()))
            .cloned()
    }

    fn keys(&self) -> Vec<String> {
        let mut keys: Vec<String> = self
            .objects
            .lock()
            .unwrap()
            .keys()
            .map(|(public, key)| format!("{}:{key}", if *public { "public" } else { "private" }))
            .collect();
        keys.sort();
        keys
    }
}

impl crate::r2::Bucket for FakeR2 {
    fn put(
        &self,
        which: crate::r2::Which,
        key: &str,
        path: &Path,
        sha256: &str,
        _media_type: &str,
    ) -> Result<(), String> {
        if self.down.load(std::sync::atomic::Ordering::Relaxed) {
            return Err("R2 answered 503 to an upload".to_owned());
        }
        let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
        if sha256_hex(&bytes) != sha256 {
            return Err("R2 answered 400 to an upload".to_owned());
        }
        self.objects
            .lock()
            .unwrap()
            .insert((which == crate::r2::Which::Public, key.to_owned()), bytes);
        Ok(())
    }

    fn delete(&self, which: crate::r2::Which, key: &str) -> Result<(), String> {
        self.objects
            .lock()
            .unwrap()
            .remove(&(which == crate::r2::Which::Public, key.to_owned()));
        Ok(())
    }

    fn size(&self, which: crate::r2::Which, key: &str) -> Result<Option<u64>, String> {
        Ok(self
            .has(which == crate::r2::Which::Public, key)
            .map(|b| b.len() as u64))
    }

    fn get(&self, which: crate::r2::Which, key: &str, to: &Path) -> Result<bool, String> {
        match self.has(which == crate::r2::Which::Public, key) {
            Some(bytes) => {
                std::fs::write(to, bytes).map_err(|e| e.to_string())?;
                Ok(true)
            }
            None => Ok(false),
        }
    }
}

fn r2_owner<'a>(t: &'a T, purged: &'a Purged, r2: &'a FakeR2) -> crate::owner::Owner<'a> {
    crate::owner::Owner {
        bucket: Some(r2),
        ..owner(t, purged)
    }
}

async fn import_sodium(t: &T) {
    modrinth_site(&t.sites);
    t.player(
        1,
        "POST",
        "/v1/imports",
        json!({ "input": "https://modrinth.com/mod/sodium" }),
    )
    .await;
    t.drain().await;
}

#[tokio::test]
async fn kept_files_are_served_from_r2_once_copied_and_leave_it_when_held() {
    let (t, r2, purged) = setup_r2();
    import_sodium(&t).await;
    let (sha256, file) = sodium_release(&t).await;
    // Not copied yet: served from our disk.
    assert!(file["url"].as_str().unwrap().starts_with(FILES), "{file}");
    assert!(
        t.app.store.counts().unwrap()["blobsToCopy"]
            .as_i64()
            .unwrap()
            > 0
    );

    let copied = crate::upkeep::copy(&t.app, 100).await.unwrap();
    assert!(copied > 0);
    assert_eq!(crate::upkeep::copy(&t.app, 100).await.unwrap(), 0);
    assert_eq!(
        r2.has(true, &format!("f/{sha256}")),
        Some(std::fs::read(t.app.blobs.path(&sha256)).unwrap())
    );
    // Pictures stay on our disk.
    assert!(
        r2.keys().iter().all(|k| k.starts_with("public:f/")),
        "{:?}",
        r2.keys()
    );
    let (_, file) = sodium_release(&t).await;
    assert_eq!(file["url"], format!("{CDN}/f/{sha256}"));
    let counts = t.app.store.counts().unwrap();
    assert_eq!(counts["blobsToCopy"], 0);
    assert_eq!(counts["blobsInR2"].as_i64().unwrap() as usize, copied);

    // Blocked: gone from the public bucket at once, both addresses cleared.
    let lines = r2_owner(&t, &purged, &r2).block(&sha(&sha256), "").unwrap();
    assert_eq!(lines[2], "cache cleared 2 addresses");
    assert!(r2.has(true, &format!("f/{sha256}")).is_none());
    assert!(
        purged
            .0
            .lock()
            .unwrap()
            .contains(&format!("{CDN}/f/{sha256}"))
    );
    let (_, file) = sodium_release(&t).await;
    assert!(file["url"].is_null());
    // The service keeps a copy in the private bucket.
    assert_eq!(crate::upkeep::copy(&t.app, 100).await.unwrap(), 1);
    assert!(r2.has(false, &format!("q/{sha256}")).is_some());
    assert_eq!(t.app.store.counts().unwrap()["blobsHeldInR2"], 1);

    // Let go: public again, the private copy goes.
    r2_owner(&t, &purged, &r2).unblock(&sha(&sha256)).unwrap();
    assert_eq!(crate::upkeep::copy(&t.app, 100).await.unwrap(), 1);
    assert!(r2.has(true, &format!("f/{sha256}")).is_some());
    assert!(r2.has(false, &format!("q/{sha256}")).is_none());

    // Held by a scan in the background: the cache is cleared at once, and
    // the service takes the file out of R2.
    let (_, api) = t.get("/v1/projects/fabric-api/versions").await;
    let other = api["versions"][0]["files"][0]["sha256"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(r2.has(true, &format!("f/{other}")).is_some());
    purged.0.lock().unwrap().clear();
    let mut code = built("https://files-8ie.pages.dev/dl");
    code.push(0xb1);
    std::fs::write(
        t.app.blobs.path(&other),
        jar(&[("a/Stage.class", class_file(&[], &[], &code))]),
    )
    .unwrap();
    t.app.store.forget_scans();
    import::rescan(&t.app, 50).await.unwrap();
    assert!(
        purged
            .0
            .lock()
            .unwrap()
            .contains(&format!("{CDN}/f/{other}"))
    );
    crate::upkeep::copy(&t.app, 1).await.unwrap();
    assert!(r2.has(true, &format!("f/{other}")).is_none());
    assert_eq!(t.app.store.copy_of(&other).unwrap().as_deref(), Some(""));
}

#[tokio::test]
async fn a_copy_r2_refuses_leaves_the_file_served_from_our_disk() {
    let (t, r2, _) = setup_r2();
    import_sodium(&t).await;
    let (sha256, _) = sodium_release(&t).await;
    r2.down.store(true, std::sync::atomic::Ordering::Relaxed);
    assert!(crate::upkeep::copy(&t.app, 100).await.is_err());
    let (_, file) = sodium_release(&t).await;
    assert!(file["url"].as_str().unwrap().starts_with(FILES));
    r2.down.store(false, std::sync::atomic::Ordering::Relaxed);

    // Bytes that do not match their hash are refused, and stay on our disk.
    std::fs::write(t.app.blobs.path(&sha256), b"other bytes").unwrap();
    while crate::upkeep::copy(&t.app, 1).await.is_ok_and(|n| n > 0) {}
    assert!(r2.has(true, &format!("f/{sha256}")).is_none());
    let (_, file) = sodium_release(&t).await;
    assert!(file["url"].as_str().unwrap().starts_with(FILES), "{file}");
}

#[tokio::test]
async fn what_only_r2_has_comes_back_to_the_disk_checked() {
    let (t, r2, _) = setup_r2();
    import_sodium(&t).await;
    let (sha256, _) = sodium_release(&t).await;
    crate::upkeep::copy(&t.app, 100).await.unwrap();
    let bytes = std::fs::read(t.app.blobs.path(&sha256)).unwrap();
    std::fs::remove_file(t.app.blobs.path(&sha256)).unwrap();
    // Another file's copy in R2 was damaged.
    let (_, api) = t.get("/v1/projects/fabric-api/versions").await;
    let other = api["versions"][0]["files"][0]["sha256"]
        .as_str()
        .unwrap()
        .to_owned();
    std::fs::remove_file(t.app.blobs.path(&other)).unwrap();
    r2.objects
        .lock()
        .unwrap()
        .insert((true, format!("f/{other}")), b"damaged".to_vec());

    let lines = crate::upkeep::restore(&t.app.store, &t.app.blobs, r2.as_ref()).unwrap();
    assert_eq!(lines[0], "restored 1");
    assert_eq!(lines[3], "wrong in R2 1");
    assert_eq!(std::fs::read(t.app.blobs.path(&sha256)).unwrap(), bytes);
    assert!(!t.app.blobs.path(&other).exists());
}

#[tokio::test]
async fn the_database_goes_to_r2_daily_and_r2_is_checked_weekly() {
    let (t, r2, _) = setup_r2();
    import_sodium(&t).await;
    crate::upkeep::copy(&t.app, 100).await.unwrap();
    let backups = t._dir.path().join("backups");
    let copy = t.app.store.backup(&backups, 14).unwrap();
    assert!(
        crate::upkeep::copy_database(&t.app, backups.clone())
            .await
            .unwrap()
    );
    let (daily, monthly) = crate::r2::database_keys(chrono::Utc::now().date_naive());
    assert_eq!(r2.has(false, &daily), Some(std::fs::read(&copy).unwrap()));
    assert!(r2.has(false, &monthly).is_some());
    // Once a day only.
    assert!(!crate::upkeep::copy_database(&t.app, backups).await.unwrap());

    assert_eq!(crate::upkeep::check(&t.app).await.unwrap(), Some(0));
    assert_eq!(crate::upkeep::check(&t.app).await.unwrap(), None);
    // A copy goes missing: the next weekly check finds it, and it is made again.
    let (sha256, _) = sodium_release(&t).await;
    r2.objects
        .lock()
        .unwrap()
        .remove(&(true, format!("f/{sha256}")));
    t.app.store.set_upkeep("r2_checked", 0).unwrap();
    assert_eq!(crate::upkeep::check(&t.app).await.unwrap(), Some(1));
    assert_eq!(t.app.store.counts().unwrap()["r2Mismatched"], 1);
    crate::upkeep::copy(&t.app, 100).await.unwrap();
    assert!(r2.has(true, &format!("f/{sha256}")).is_some());
}

#[test]
fn r2_settings_and_requests_are_checked() {
    use crate::r2::{R2, credentials, put_args};
    let curl = std::path::PathBuf::from("curl");
    let account = "0123456789abcdef0123456789abcdef";
    let key = "fedcba9876543210fedcba9876543210";
    let secret = "ab".repeat(32);
    assert!(
        R2::new(
            account,
            key,
            &secret,
            "mods-files",
            "mods-private",
            curl.clone()
        )
        .is_some()
    );
    for (account, key, secret, public, private) in [
        ("", key, secret.as_str(), "mods-files", "mods-private"),
        (
            account,
            "short",
            secret.as_str(),
            "mods-files",
            "mods-private",
        ),
        (
            account,
            key,
            "x\"\nurl = \"http://evil",
            "mods-files",
            "mods-private",
        ),
        (account, key, secret.as_str(), "Mods_Files", "mods-private"),
        (account, key, secret.as_str(), "mods-files", "mods-files"),
        (account, key, secret.as_str(), "mods-files", "../x"),
    ] {
        assert!(R2::new(account, key, secret, public, private, curl.clone()).is_none());
    }
    assert_eq!(
        credentials(key, &secret),
        format!("user = \"{key}:{secret}\"\n")
    );
    let args = put_args(
        "/data/f",
        &"0".repeat(64),
        "application/java-archive",
        "https://x/f/0",
    );
    assert!(args.contains(&format!("x-amz-content-sha256: {}", "0".repeat(64))));
    assert!(args.contains(&"Content-Disposition: attachment".to_owned()));
    assert_eq!(args.last().unwrap(), "https://x/f/0");
    // No secret in the arguments.
    assert!(!args.iter().any(|a| a.contains(&secret)));

    // The public address: https only, a plain host.
    for (cdn, served) in [
        ("https://cdn-mods.lapi.sh", true),
        ("https://cdn-mods.lapi.sh/", true),
        ("", false),
        ("http://cdn-mods.lapi.sh", false),
        ("https://cdn.example/path?x", false),
        ("https://user@cdn.example", false),
    ] {
        let urls = Urls::new(FILES).with_cdn(cdn);
        let url = urls.served(&"a".repeat(64), "x.jar", true);
        assert_eq!(url.starts_with("https://cdn"), served, "{cdn}: {url}");
    }
}

// ----- players' own packs (stage 6) -----

impl T {
    /// Sends an archive as a request's body.
    async fn upload(&self, who: Option<u8>, uri: &str, bytes: Vec<u8>) -> (StatusCode, Value) {
        self.send_bytes(who, "POST", uri, bytes).await
    }

    async fn send_bytes(
        &self,
        who: Option<u8>,
        method: &str,
        uri: &str,
        bytes: Vec<u8>,
    ) -> (StatusCode, Value) {
        let mut request = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::CONTENT_TYPE, "application/zip")
            // Each from its own address: the tests count accounts.
            .header("x-real-ip", {
                static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
                let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                format!("198.51.{}.{}", (n / 250) % 250, n % 250 + 1)
            });
        if let Some(who) = who {
            let token = self.token_with(&["account", "mods"], who);
            request = request.header(header::AUTHORIZATION, format!("Bearer {token}"));
        }
        let response = self
            .router
            .clone()
            .oneshot(request.body(Body::from(bytes)).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }
}

fn own_meta(title: &str, version: &str) -> Vec<u8> {
    json!({
        "title": title, "summary": "My settings for a fast game",
        "body": "# Mine\nWith <script>alert(1)</script> nothing.", "version": version,
        "changes": "First",
    })
    .to_string()
    .into_bytes()
}

/// An own pack: Sodium 0.6 from the catalog, a config and the options.
fn own_pack(version: &str, extra: &[(&str, &[u8])]) -> Vec<u8> {
    let sodium = b"jar of sodium-0.6.jar";
    let index = json!({
        "formatVersion": 1, "game": "minecraft", "versionId": version, "name": "Mine",
        "files": [pack_entry("mods/sodium-0.6.jar", "https://example.com/x.jar", sodium, "required")],
        "dependencies": { "minecraft": "1.21.1", "fabric-loader": "0.16.0" },
    })
    .to_string();
    let meta = own_meta("My Pack", version);
    let icon = png(64, 64);
    let mut entries: Vec<(&str, &[u8])> = vec![
        (pack::INDEX, index.as_bytes()),
        (crate::own::META, &meta),
        ("icon.png", &icon),
        ("overrides/config/sodium-options.json", b"{\"fast\":true}"),
        ("overrides/options.txt", b"fov:0.5"),
    ];
    entries.extend_from_slice(extra);
    mrpack(&entries)
}

#[tokio::test]
async fn a_large_pack_comes_in_parts_and_is_put_together() {
    let t = setup();
    import_sodium(&t).await;
    // Bytes that do not compress: about three parts.
    let big: Vec<u8> = (0..20_000u32)
        .flat_map(|n| sha2::Sha256::digest(n.to_le_bytes()))
        .collect();
    let archive = own_pack("1.0", &[("overrides/config/big.dat", &big)]);
    let chunk = crate::own::CHUNK as usize;
    let count = archive.len().div_ceil(chunk);
    assert!(count >= 3, "{}", archive.len());
    let sha256 = hex::encode(sha2::Sha256::digest(&archive));
    let start = json!({ "size": archive.len(), "sha256": sha256 });

    let (status, _) = t
        .call(None, "POST", "/v1/packs/uploads", start.clone())
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, answer) = t
        .player(
            1,
            "POST",
            "/v1/packs/uploads",
            json!({ "size": archive.len(), "sha256": "x" }),
        )
        .await;
    assert_eq!(
        (status, &answer["error"]),
        (StatusCode::BAD_REQUEST, &json!("bad_upload"))
    );
    let (status, answer) = t
        .player(
            1,
            "POST",
            "/v1/packs/uploads",
            json!({ "size": crate::own::MAX_ARCHIVE + 1, "sha256": sha256 }),
        )
        .await;
    assert_eq!(
        (status, &answer["error"]),
        (StatusCode::BAD_REQUEST, &json!("too_big"))
    );
    let (status, answer) = t
        .player(1, "POST", "/v1/packs/uploads", start.clone())
        .await;
    assert_eq!(status, StatusCode::CREATED, "{answer}");
    assert_eq!(answer["parts"], count);
    assert_eq!(answer["partSize"], crate::own::CHUNK);
    let id = answer["upload"].as_str().unwrap().to_owned();
    let part = |n: usize| archive[n * chunk..((n + 1) * chunk).min(archive.len())].to_vec();
    let uri = |n: usize| format!("/v1/packs/uploads/{id}/{n}");

    // Only its account sends parts, each of its length, none beyond the last.
    let (status, _) = t.send_bytes(Some(2), "PUT", &uri(0), part(0)).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, answer) = t.send_bytes(Some(1), "PUT", &uri(0), part(count - 1)).await;
    assert_eq!(
        (status, &answer["error"]),
        (StatusCode::BAD_REQUEST, &json!("bad_part"))
    );
    let (status, _) = t.send_bytes(Some(1), "PUT", &uri(count), part(0)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let finish = format!("/v1/packs/uploads/{id}/finish");
    let (status, answer) = t.player(1, "POST", &finish, Value::Null).await;
    assert_eq!(
        (status, &answer["error"]),
        (StatusCode::BAD_REQUEST, &json!("not_complete"))
    );

    // In any order, a part sent twice replaces the first.
    for n in (0..count).rev() {
        let (status, answer) = t.send_bytes(Some(1), "PUT", &uri(n), part(n)).await;
        assert_eq!(status, StatusCode::NO_CONTENT, "{answer}");
    }
    let (status, _) = t.send_bytes(Some(1), "PUT", &uri(0), part(0)).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = t.player(2, "POST", &finish, Value::Null).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, answer) = t.player(1, "POST", &finish, Value::Null).await;
    assert_eq!(status, StatusCode::CREATED, "{answer}");
    assert_eq!(answer["project"]["title"], "My Pack");
    let (status, _) = t.player(1, "POST", &finish, Value::Null).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Parts that do not make the announced archive are refused.
    let project = answer["project"]["id"].as_str().unwrap().to_owned();
    let other = own_pack("1.1", &[("overrides/config/big.dat", &big)]);
    let (status, answer) = t
        .player(
            1,
            "POST",
            &format!("/v1/packs/uploads?project={project}"),
            json!({ "size": other.len(), "sha256": sha256 }),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{answer}");
    let id = answer["upload"].as_str().unwrap().to_owned();
    for n in 0..other.len().div_ceil(chunk) {
        let bytes = other[n * chunk..((n + 1) * chunk).min(other.len())].to_vec();
        let (status, _) = t
            .send_bytes(
                Some(1),
                "PUT",
                &format!("/v1/packs/uploads/{id}/{n}"),
                bytes,
            )
            .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
    }
    let (status, answer) = t
        .player(
            1,
            "POST",
            &format!("/v1/packs/uploads/{id}/finish"),
            Value::Null,
        )
        .await;
    assert_eq!(
        (status, &answer["error"]),
        (StatusCode::BAD_REQUEST, &json!("bad_upload"))
    );
    // Nothing is left behind in tmp/.
    assert_eq!(std::fs::read_dir(t.app.blobs.tmp()).unwrap().count(), 0);
}

#[tokio::test]
async fn a_player_publishes_a_pack_of_catalog_mods_and_their_settings() {
    let t = setup();
    import_sodium(&t).await;
    let (status, _) = t.upload(None, "/v1/packs", own_pack("1.0", &[])).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, answer) = t.upload(Some(1), "/v1/packs", own_pack("1.0", &[])).await;
    assert_eq!(status, StatusCode::CREATED, "{answer}");
    let project = &answer["project"];
    let id = project["id"].as_str().unwrap().to_owned();
    assert_eq!(project["kind"], "modpack");
    assert_eq!(project["status"], "ready");
    assert_eq!(project["title"], "My Pack");
    assert_eq!(project["slug"], "my-pack");
    assert_eq!(project["sources"][0]["source"], "own");
    assert_eq!(project["authors"][0]["name"], "Player");
    assert!(project["icon"].as_str().unwrap().starts_with(FILES));
    let body = project["body"].as_str().unwrap();
    assert!(
        !body.contains('<') && body.contains("&lt;script>"),
        "{body}"
    );
    // Installed like any pack: every file served by the catalog.
    let (_, versions) = t.get(&format!("/v1/projects/{id}/versions")).await;
    let version = versions["versions"][0]["id"].as_str().unwrap().to_owned();
    assert_eq!(versions["versions"][0]["number"], "1.0");
    let (status, pack) = t.get(&format!("/v1/versions/{version}/pack")).await;
    assert_eq!(status, StatusCode::OK, "{pack}");
    assert_eq!(pack["state"], "ready", "{pack}");
    let file = &pack["files"][0];
    assert_eq!(file["path"], "mods/sodium-0.6.jar");
    assert!(file["url"].as_str().unwrap().starts_with(FILES), "{file}");
    let (_, found) = t.get("/v1/search?q=my%20pack&kind=modpack").await;
    assert_eq!(found["hits"][0]["id"], id.as_str(), "{found}");
    // A new version of the same pack; a number once only; only its author.
    let uri = format!("/v1/packs?project={id}");
    // Bytes that do not compress.
    let big: Vec<u8> = (0..40_000u32)
        .flat_map(|n| sha2::Sha256::digest(n.to_le_bytes()))
        .collect();
    let archive = own_pack("1.1", &[("overrides/config/big.dat", &big)]);
    assert!(archive.len() > 1_000_000);
    let (status, answer) = t.upload(Some(1), &uri, archive).await;
    assert_eq!(status, StatusCode::CREATED, "{answer}");
    assert_eq!(answer["project"]["id"], id.as_str());
    let (status, answer) = t.upload(Some(1), &uri, own_pack("1.1", &[])).await;
    assert_eq!(
        (status, &answer["error"]),
        (StatusCode::CONFLICT, &json!("version_exists"))
    );
    let (status, _) = t.upload(Some(2), &uri, own_pack("1.2", &[])).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = t
        .upload(Some(1), "/v1/packs?project=sodium", own_pack("1.2", &[]))
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (_, mine) = t.player(1, "GET", "/v1/packs/mine", Value::Null).await;
    assert_eq!(mine["packs"][0]["id"], id.as_str());
    assert_eq!(mine["packs"][0]["versions"], 2);
    assert_eq!(mine["packs"][0]["latest"], "1.1");
    let (_, theirs) = t.player(2, "GET", "/v1/packs/mine", Value::Null).await;
    assert_eq!(theirs["packs"], json!([]));
    // Hidden by its author, and shown again.
    let hide = format!("/v1/packs/{id}/hidden");
    let (status, _) = t.player(2, "POST", &hide, json!({ "hidden": true })).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (_, answer) = t.player(1, "POST", &hide, json!({ "hidden": true })).await;
    assert_eq!(answer["status"], "hidden");
    let (_, found) = t.get("/v1/search?q=my%20pack&kind=modpack").await;
    assert_eq!(found["total"], 0, "{found}");
    let (status, _) = t.upload(Some(1), &uri, own_pack("1.2", &[])).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(
        t.app.store.project_status(&id).unwrap().as_deref(),
        Some("hidden")
    );
    let (_, answer) = t.player(1, "POST", &hide, json!({ "hidden": false })).await;
    assert_eq!(answer["status"], "ready");
    let (status, _) = t.player(1, "POST", &hide, json!({ "hidden": "no" })).await;
    assert!(status.is_client_error());
    // Never fetched from a source.
    age(&t, 30 * 86_400);
    import::refresh(&t.app, None).unwrap();
    t.drain().await;
    let counts = t.app.store.counts().unwrap();
    assert_eq!(counts["ownPacks"], 1);
    assert_eq!(counts["ownVersions"], 3);
}

#[tokio::test]
async fn a_room_pack_is_installed_by_its_version_and_never_shown() {
    let t = setup();
    import_sodium(&t).await;
    let (status, answer) = t
        .upload(Some(1), "/v1/packs?room=true", own_pack("1", &[]))
        .await;
    assert_eq!(status, StatusCode::CREATED, "{answer}");
    let id = answer["project"]["id"].as_str().unwrap().to_owned();
    let version = answer["version"].as_str().unwrap().to_owned();
    assert_eq!(answer["project"]["status"], "room");
    // Not in the catalog: not searched, not a project, not among the
    // author's packs, and the author cannot show it.
    let (_, found) = t.get("/v1/search?q=my%20pack&kind=modpack").await;
    assert_eq!(found["total"], 0, "{found}");
    let (status, _) = t.get(&format!("/v1/projects/{id}")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (_, mine) = t.player(1, "GET", "/v1/packs/mine", Value::Null).await;
    assert_eq!(mine["packs"], json!([]));
    let hide = format!("/v1/packs/{id}/hidden");
    t.player(1, "POST", &hide, json!({ "hidden": false })).await;
    assert_eq!(
        t.app.store.project_status(&id).unwrap().as_deref(),
        Some("room")
    );
    // Its guests install it by the version the world's session names.
    let (status, pack) = t.get(&format!("/v1/versions/{version}/pack")).await;
    assert_eq!(status, StatusCode::OK, "{pack}");
    assert_eq!(pack["state"], "ready", "{pack}");
    assert_eq!(pack["files"][0]["path"], "mods/sodium-0.6.jar");
    let sha256 = pack["archive"]["sha256"].as_str().unwrap();
    assert_eq!(sha256.len(), 64);
    // New generations are its versions; a room pack takes no catalog
    // version and a catalog pack no room version.
    let uri = format!("/v1/packs?room=true&project={id}");
    let (status, answer) = t.upload(Some(1), &uri, own_pack("2", &[])).await;
    assert_eq!(status, StatusCode::CREATED, "{answer}");
    assert_ne!(answer["version"], version.as_str());
    let (status, _) = t
        .upload(
            Some(1),
            &format!("/v1/packs?project={id}"),
            own_pack("3", &[]),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (_, shown) = t.upload(Some(1), "/v1/packs", own_pack("1", &[])).await;
    let shown = shown["project"]["id"].as_str().unwrap().to_owned();
    let (status, _) = t
        .upload(
            Some(1),
            &format!("/v1/packs?room=true&project={shown}"),
            own_pack("2", &[]),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    // Checked as any own pack: a mod the catalog lacks refuses it.
    let stranger = own_pack("4", &[("overrides/mods/x.jar", b"code")]);
    let (status, _) = t.upload(Some(1), &uri, stranger).await;
    assert!(status.is_client_error(), "{status}");
}

#[tokio::test]
async fn own_packs_bring_only_catalog_files_and_settings() {
    let t = setup();
    import_sodium(&t).await;
    let refused = |extra: &[(&str, &[u8])]| own_pack("1.0", extra);
    for (n, (name, bytes)) in [
        ("overrides/mods/extra.jar", &b"a mod"[..]),
        ("overrides/config/Evil.class", b"code"),
        ("overrides/config/run.sh", b"rm"),
        ("overrides/config/a.jar.disabled", b"a mod"),
        ("overrides/config/x.dll", b"code"),
        ("overrides/../escape.txt", b"x"),
        ("overrides/C:/x.txt", b"x"),
        ("overrides/unknown-folder/x.txt", b"x"),
        ("client-overrides/config/x.txt", b"x"),
        ("evil.txt", b"x"),
    ]
    .into_iter()
    .enumerate()
    {
        let who = Some(10 + n as u8);
        let (status, answer) = t.upload(who, "/v1/packs", refused(&[(name, bytes)])).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{name}: {answer}");
        assert_eq!(answer["error"], "not_allowed", "{name}");
        assert_eq!(answer["paths"].as_array().unwrap().len(), 1, "{name}");
    }
    // A file the catalog does not have: named, to add first.
    let other = b"a mod nobody added";
    let index = json!({
        "formatVersion": 1, "game": "minecraft",
        "files": [pack_entry("mods/other.jar", "https://cdn.modrinth.com/x", other, "required")],
        "dependencies": { "minecraft": "1.21.1" },
    })
    .to_string();
    let meta = own_meta("Other pack", "1");
    let archive = mrpack(&[(pack::INDEX, index.as_bytes()), (crate::own::META, &meta)]);
    let (status, answer) = t.upload(Some(1), "/v1/packs", archive).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        answer,
        json!({ "error": "not_in_catalog", "paths": ["mods/other.jar"] })
    );
    // Broken archives, texts and icons.
    let index = json!({
        "formatVersion": 1, "game": "minecraft", "files": [],
        "dependencies": { "minecraft": "1.21.1" },
    })
    .to_string();
    let with = |meta: &[u8], extra: &[(&str, &[u8])]| {
        let mut entries: Vec<(&str, &[u8])> = vec![(pack::INDEX, index.as_bytes())];
        if !meta.is_empty() {
            entries.push((crate::own::META, meta));
        }
        entries.extend_from_slice(extra);
        mrpack(&entries)
    };
    let meta_of = |v: Value| v.to_string().into_bytes();
    let big = png(3000, 4);
    let unsupported = json!({
        "formatVersion": 1, "game": "minecraft",
        "files": [pack_entry("mods/sodium-0.6.jar", "x", b"jar of sodium-0.6.jar", "unsupported")],
        "dependencies": { "minecraft": "1.21.1" },
    })
    .to_string();
    for (n, (archive, code)) in [
        (b"not a zip".to_vec(), "bad_pack"),
        (Vec::new(), "too_big"),
        (with(b"", &[]), "bad_meta"),
        (with(b"{}", &[]), "bad_meta"),
        (
            with(&meta_of(json!({ "title": "x", "version": "1" })), &[]),
            "bad_title",
        ),
        (
            with(&meta_of(json!({ "title": "  !!  ", "version": "1" })), &[]),
            "bad_title",
        ),
        (
            with(&meta_of(json!({ "title": "Fine", "version": "1 0" })), &[]),
            "bad_version",
        ),
        (
            with(&meta_of(json!({ "title": "Fine", "version": "../1" })), &[]),
            "bad_version",
        ),
        (
            with(
                &meta_of(json!({ "title": "x".repeat(81), "version": "1" })),
                &[],
            ),
            "too_long",
        ),
        (
            with(
                &meta_of(json!({ "title": "Fine", "version": "1", "body": "x".repeat(70_000) })),
                &[],
            ),
            "too_long",
        ),
        (
            with(
                &meta_of(json!({ "title": "Fine", "version": "1", "evil": true })),
                &[],
            ),
            "bad_meta",
        ),
        (
            with(&own_meta("Fine", "1"), &[("icon.png", b"not a picture")]),
            "bad_icon",
        ),
        (
            with(&own_meta("Fine", "1"), &[("icon.png", &big)]),
            "bad_icon",
        ),
        (
            mrpack(&[
                (pack::INDEX, unsupported.as_bytes()),
                (crate::own::META, &own_meta("Fine", "1")),
            ]),
            "bad_pack",
        ),
        (
            mrpack(&[(crate::own::META, &own_meta("Fine", "1"))]),
            "bad_pack",
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let (status, answer) = t.upload(Some(30 + n as u8), "/v1/packs", archive).await;
        let expected = if code == "too_big" {
            StatusCode::PAYLOAD_TOO_LARGE
        } else {
            StatusCode::BAD_REQUEST
        };
        assert_eq!((status, answer["error"].as_str()), (expected, Some(code)));
    }
    let (status, _) = t
        .upload(
            Some(1),
            "/v1/packs",
            vec![0; crate::own::MAX_ARCHIVE as usize + 1],
        )
        .await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    // A file held back: the pack waits for the owner's decision.
    let (sha256, _) = sodium_release(&t).await;
    let purged = Purged::default();
    owner(&t, &purged).block(&sha(&sha256), "").unwrap();
    let (status, answer) = t.upload(Some(1), "/v1/packs", own_pack("1.0", &[])).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        answer,
        json!({ "error": "held", "paths": ["mods/sodium-0.6.jar"] })
    );
    owner(&t, &purged).unblock(&sha(&sha256)).unwrap();
    // A bounded number of new packs a day.
    for n in 0..crate::own::PACKS_PER_DAY {
        let archive = own_pack(&format!("1.{n}"), &[]);
        let (status, answer) = t.upload(Some(3), "/v1/packs", archive).await;
        assert_eq!(status, StatusCode::CREATED, "{answer}");
    }
    let (status, answer) = t.upload(Some(3), "/v1/packs", own_pack("2.0", &[])).await;
    assert_eq!(
        (status, &answer["error"]),
        (StatusCode::TOO_MANY_REQUESTS, &json!("too_many_packs"))
    );
    assert_eq!(
        t.app.store.counts().unwrap()["ownPacks"],
        crate::own::PACKS_PER_DAY
    );
}

#[test]
fn own_pack_texts_and_paths_are_checked() {
    use crate::own::{meta, overridable, slug};
    assert_eq!(slug("My Pack!"), "my-pack");
    assert_eq!(slug("Сборка"), "pack");
    assert_eq!(slug("  a  "), "pack");
    assert!(slug(&"long ".repeat(30)).len() <= 40);
    for ok in [
        "config/a.json",
        "options.txt",
        "resourcepacks/x.zip",
        "saves/w/level.dat",
    ] {
        assert!(overridable(ok), "{ok}");
    }
    for bad in [
        "mods/a.jar",
        "mods/a.txt",
        "config/a.jar",
        "config/a.JAR.disabled",
        "config/a.class",
        "config/a.exe.txt",
        "config/a.ps1",
        "config/../a.txt",
        "/config/a.txt",
        "config\\a.txt",
        "nowhere/a.txt",
    ] {
        assert!(!overridable(bad), "{bad}");
    }
    let m = meta(br#"{"title":" Mine\u202e\n","version":"V1.0","body":"<b>x</b>"}"#).unwrap();
    assert_eq!((m.title.as_str(), m.number.as_str()), ("Mine", "v1.0"));
    assert_eq!(m.body, "&lt;b>x&lt;/b>");
    assert!(meta(b"[]").is_err());
    assert!(meta(br#"{"title":"Mine"}"#).is_err());
}

#[tokio::test]
async fn players_report_own_packs_to_the_owner() {
    let t = setup();
    import_sodium(&t).await;
    let (_, answer) = t.upload(Some(1), "/v1/packs", own_pack("1.0", &[])).await;
    let id = answer["project"]["id"].as_str().unwrap().to_owned();
    let uri = format!("/v1/projects/{id}/report");
    let (status, _) = t
        .call(None, "POST", &uri, json!({ "reason": "malware" }))
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    for junk in [
        json!({ "reason": "boring" }),
        json!({ "reason": "other", "text": "x".repeat(501) }),
        json!({ "reason": "other", "extra": 1 }),
        json!({ "text": "no reason" }),
    ] {
        let (status, _) = t.player(2, "POST", &uri, junk.clone()).await;
        assert!(status.is_client_error(), "{junk}");
    }
    // Only players' own packs are reported here.
    let (_, sodium) = t.get("/v1/projects/sodium").await;
    let sodium_uri = format!("/v1/projects/{}/report", sodium["id"].as_str().unwrap());
    let (status, _) = t
        .player(2, "POST", &sodium_uri, json!({ "reason": "malware" }))
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = t
        .player(
            2,
            "POST",
            &uri,
            json!({ "reason": "malware", "text": "steals\u{0} passwords" }),
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    let (status, _) = t
        .player(2, "POST", &uri, json!({ "reason": "other" }))
        .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    let purged = Purged::default();
    let o = owner(&t, &purged);
    assert_eq!(
        o.review_count().unwrap(),
        [
            "report 1",
            "total 1",
            "virustotal checked 0, with detections 0, waiting 0"
        ]
    );
    let review = o.review().unwrap();
    assert!(
        review[0].contains("report") && review[0].contains("malware 1"),
        "{review:?}"
    );
    assert!(
        review.iter().any(|l| l.contains("\"steals passwords\"")),
        "{review:?}"
    );
    let item: i64 = review[0][1..].split(' ').next().unwrap().parse().unwrap();
    o.decide(item, "keep", "").unwrap();
    assert_eq!(
        o.review_count().unwrap(),
        [
            "total 0",
            "virustotal checked 0, with detections 0, waiting 0"
        ]
    );
    // Another player's report puts it back; `block` removes the pack.
    t.player(3, "POST", &uri, json!({ "reason": "stolen" }))
        .await;
    let review = o.review().unwrap();
    let item: i64 = review[0][1..].split(' ').next().unwrap().parse().unwrap();
    o.decide(item, "block", "stolen").unwrap();
    assert_eq!(
        t.app.store.project_status(&id).unwrap().as_deref(),
        Some("removed")
    );
    let (status, answer) = t
        .upload(
            Some(1),
            &format!("/v1/packs?project={id}"),
            own_pack("1.1", &[]),
        )
        .await;
    assert_eq!(
        (status, &answer["error"]),
        (StatusCode::GONE, &json!("removed"))
    );
    let (status, _) = t
        .player(4, "POST", &uri, json!({ "reason": "other" }))
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let hide = format!("/v1/packs/{id}/hidden");
    let (_, answer) = t.player(1, "POST", &hide, json!({ "hidden": false })).await;
    assert_eq!(answer["status"], "removed");
}

// ----- projects from sites that are not catalogs (external.rs) -----

/// A row of optifine.net's downloads page, as the site writes it.
fn optifine_row(class: &str, title: &str, file: &str, forge: &str, date: &str) -> String {
    format!(
        "<tr class='downloadLine {class}'>
           <td class='colFile'>{title}</td>
           <td class='colDownload'><a href=\"http://adfoc.us/serve/sitelinks/?id=1&url=http://optifine.net/adloadx?f={file}&x=9ec7\">Download</a></td>
           <td class='colMirror'><a href=\"http://optifine.net/adloadx?f={file}\">(Mirror)</a></td>
           <td class='colChangelog'><a href='changelog?f={file}'>Changelog</a></td>
           <td class='colForge'>{forge}</td>
           <td class='colDate'>{date}</td>
         </tr>"
    )
}

fn optifine_page() -> String {
    let mut html = String::from("<html><body><table class='tableRoot'>");
    html += "<h2>Minecraft 26.2</h2><table class='downloadTable'>";
    html += &optifine_row(
        "downloadLinePreview",
        "OptiFine HD U K2 pre5",
        "preview_OptiFine_26.2_HD_U_K2_pre5.jar",
        "Forge N/A",
        "20.10.2026",
    );
    html += "</table><table class='downloadTable mainTable'></table>";
    html += "<h2>Minecraft 1.21.11</h2><table class='downloadTable mainTable'>";
    html += &optifine_row(
        "downloadLineMain",
        "OptiFine HD U J9",
        "OptiFine_1.21.11_HD_U_J9.jar",
        "Forge 61.0.8",
        "05.02.2026",
    );
    html += &optifine_row(
        "downloadLineMore",
        "OptiFine HD U J8",
        "OptiFine_1.21.11_HD_U_J8.jar",
        "Forge 61.0.2",
        "05.01.2026",
    );
    // A file of another game version under this heading, and junk.
    html += &optifine_row(
        "downloadLineMore",
        "OptiFine HD U J7",
        "OptiFine_1.21.10_HD_U_J7.jar",
        "Forge 60.0.1",
        "05.12.2025",
    );
    html += &optifine_row(
        "downloadLineMore",
        "OptiFine HD U J6",
        "OptiFine_1.21.11_HD_U_J6.jar?x=../../etc",
        "Forge 60",
        "05.11.2025",
    );
    html += &optifine_row(
        "downloadLineMore",
        "OptiFine HD U J5",
        "OptiFine_1.21.11_HD_U_J5.jar",
        "Forge <script>alert(1)</script>",
        "31.02.2025",
    );
    html += "</table><h2>Minecraft 1.8.0</h2><table class='downloadTable mainTable'>";
    html += &optifine_row(
        "downloadLineMain",
        "OptiFine HD U E7",
        "OptiFine_1.8.0_HD_U_E7.jar",
        "Forge #1614",
        "07.01.2016",
    );
    html += "</table><h2>Minecraft <b>x</b></h2><table>";
    html += &optifine_row(
        "downloadLineMain",
        "OptiFine HD U Z1",
        "OptiFine_x_HD_U_Z1.jar",
        "Forge N/A",
        "07.01.2016",
    );
    html += "</table></table></body></html>";
    html
}

#[test]
fn optifine_downloads_page_is_read_row_by_row() {
    use crate::external::{optifine_rows, optifine_version};
    let rows = optifine_rows(&optifine_page());
    let files: Vec<&str> = rows.iter().map(|r| r.file.as_str()).collect();
    assert_eq!(
        files,
        [
            "preview_OptiFine_26.2_HD_U_K2_pre5.jar",
            "OptiFine_1.21.11_HD_U_J9.jar",
            "OptiFine_1.21.11_HD_U_J8.jar",
            "OptiFine_1.8.0_HD_U_E7.jar",
        ]
    );
    assert!(rows[0].preview && rows[0].forge.is_none());
    assert_eq!(rows[1].forge.as_deref(), Some("61.0.8"));
    assert_eq!(rows[3].forge.as_deref(), Some("#1614"));
    assert_eq!(rows[3].game, "1.8");
    assert_eq!(rows[1].published, 1_770_249_600);

    let version = optifine_version(
        &rows[1],
        Some("OptiFine 1.21.11_HD_U_J9\n - fixed\u{202E}x"),
    );
    assert_eq!(version.number, "1.21.11 HD U J9");
    assert_eq!(version.loaders, ["forge"]);
    assert_eq!(version.channel, "release");
    assert_eq!(version.games, ["1.21.11"]);
    assert_eq!(
        version.files[0].url,
        "https://optifine.net/adloadx?f=OptiFine_1.21.11_HD_U_J9.jar"
    );
    assert!(version.changelog.starts_with("For Forge 61.0.8.\n\n"));
    assert!(!version.changelog.contains('\u{202E}'));
    let preview = optifine_version(&rows[0], None);
    assert_eq!(preview.channel, "beta");
    assert!(preview.loaders.is_empty());
    assert!(!preview.details);

    // Junk is no page.
    assert!(optifine_rows("").is_empty());
    assert!(optifine_rows("<h2>Minecraft 1.21</h2><tr class='downloadLine'><td").is_empty());
    assert!(optifine_rows(&"<h2>".repeat(10_000)).is_empty());
}

#[test]
fn optifine_link_and_names_take_nothing_else() {
    use crate::external::{optifine_file, optifine_link, optifine_name};
    let name = "OptiFine_1.21.1_HD_U_J1.jar";
    let page = format!(
        "<a href='downloadx?f={name}&x=3650e05d246f7a2889a76764d27ce3eb' onclick='onDownload()'>"
    );
    assert_eq!(
        optifine_link(&page, name).as_deref(),
        Some(
            "https://optifine.net/downloadx?f=OptiFine_1.21.1_HD_U_J1.jar&x=3650e05d246f7a2889a76764d27ce3eb"
        )
    );
    // Another file's link, a short or long token, other characters.
    let other = page.replace("1.21.1_", "1.21.4_");
    assert_eq!(optifine_link(&other, name), None);
    assert_eq!(
        optifine_link(&page.replace("3650e05d", "3650e05"), name),
        None
    );
    assert_eq!(
        optifine_link(&page.replace("ce3eb'", "ce3eb0'"), name),
        None
    );
    assert_eq!(
        optifine_link(&page.replace("3650e05d", "3650E05D"), name),
        None
    );
    assert_eq!(
        optifine_link(&page.replace("3650e05d", "3650é05"), name),
        None
    );
    assert_eq!(optifine_link("", name), None);
    assert_eq!(optifine_link(&format!("downloadx?f={name}&x="), name), None);

    for bad in [
        "",
        "OptiFine_.jar",
        "OptiFine_1.21_HD.jar ",
        "OptiFine_1.21_HD_U_J1.jar/../x",
        "OptiFine_1.21_HD_U_J1.jar&x=1",
        "OptiFine_1.21_HD U_J1.jar",
        "OptiFine_1.a_HD_U_J1.jar",
        "other_OptiFine_1.21_HD_U_J1.jar",
        "OptiFine_1.21_HD_U_J1.zip",
    ] {
        assert!(!optifine_name(bad), "{bad:?}");
    }
    assert!(optifine_name("preview_OptiFine_26.2_HD_U_K2_pre5.jar"));
    assert_eq!(
        optifine_file("https://optifine.net/adloadx?f=OptiFine_1.21.1_HD_U_J1.jar").as_deref(),
        Some(name)
    );
    for bad in [
        "http://optifine.net/adloadx?f=OptiFine_1.21.1_HD_U_J1.jar",
        "https://optifine.net.evil.example/adloadx?f=OptiFine_1.21.1_HD_U_J1.jar",
        "https://optifine.net/adloadx?f=OptiFine_1.21.1_HD_U_J1.jar&x=1",
    ] {
        assert_eq!(optifine_file(bad), None, "{bad}");
    }
}

/// A jar big enough to be one.
fn optifine_jar(seed: u8) -> Vec<u8> {
    let filler: Vec<u8> = (0..4000u32)
        .map(|i| (i.wrapping_mul(2_654_435_761) >> 13) as u8 ^ seed)
        .collect();
    mrpack(&[
        ("META-INF/mods.toml", b"modLoader=\"javafml\"".as_slice()),
        ("optifine/Filler.bin", &filler),
    ])
}

/// optifine.net: its downloads page, changelogs, and each file's page with
/// a link that gives the jar.
fn optifine_site(sites: &Sites) -> HashMap<String, Vec<u8>> {
    sites.put(crate::external::OPTIFINE_DOWNLOADS, ok(optifine_page()));
    let mut jars = HashMap::new();
    for (seed, file) in [
        "preview_OptiFine_26.2_HD_U_K2_pre5.jar",
        "OptiFine_1.21.11_HD_U_J9.jar",
        "OptiFine_1.21.11_HD_U_J8.jar",
        "OptiFine_1.8.0_HD_U_E7.jar",
    ]
    .into_iter()
    .enumerate()
    {
        let token = format!("{:032x}", seed + 1);
        sites.put(
            &crate::external::optifine_file_url(file),
            ok(format!(
                "<a href='downloadx?f={file}&x={token}'>Download</a>"
            )),
        );
        let jar = optifine_jar(seed as u8);
        sites.put(
            &format!("https://optifine.net/downloadx?f={file}&x={token}"),
            ok(jar.clone()),
        );
        sites.put(
            &crate::external::optifine_changelog_url(file),
            ok(format!("OptiFine {file}\n - fixed things")),
        );
        jars.insert(file.to_owned(), jar);
    }
    jars
}

fn external(t: &T, args: &[&str]) -> Result<Vec<String>, String> {
    let args: Vec<String> = args.iter().map(|a| (*a).to_owned()).collect();
    crate::external::command(&t.app.store, &t.app.blobs, &args, now())
}

#[tokio::test]
async fn optifine_is_kept_from_its_site_and_checked_every_three_days() {
    let t = setup();
    let jars = optifine_site(&t.sites);
    let lines = external(&t, &["add", "https://optifine.net/home"]).unwrap();
    assert_eq!(lines[0], "optifine: every 3 days");
    t.drain().await;

    let (status, project) = t.get("/v1/projects/optifine").await;
    assert_eq!(status, StatusCode::OK, "{project}");
    assert_eq!(project["title"], "OptiFine");
    assert_eq!(project["sources"][0]["source"], "external");
    assert_eq!(project["sources"][0]["url"], "https://optifine.net/home");
    let (_, list) = t.get("/v1/projects/optifine/versions").await;
    let versions = list["versions"].as_array().unwrap();
    assert_eq!(versions.len(), 4);
    let by_name = |name: &str| {
        versions
            .iter()
            .find(|v| v["name"] == name)
            .unwrap_or_else(|| panic!("{name}"))
            .clone()
    };
    // The newest for each game version and loader is kept, with our hashes
    // and size; the older one waits until asked for.
    let j9 = by_name("OptiFine HD U J9");
    assert_eq!(j9["saved"], "all");
    let jar = &jars["OptiFine_1.21.11_HD_U_J9.jar"];
    assert_eq!(j9["files"][0]["size"], jar.len());
    assert_eq!(j9["files"][0]["sha1"], hashes(jar).0);
    assert!(j9["changelog"].as_str().unwrap().contains("- fixed things"));
    assert_eq!(by_name("OptiFine HD U J8")["saved"], "none");
    assert_eq!(by_name("OptiFine HD U K2 pre5")["saved"], "all");
    assert_eq!(by_name("OptiFine HD U E7")["games"], json!(["1.8"]));
    let changelogs = |t: &T| {
        t.sites.asked(&crate::external::optifine_changelog_url(
            "OptiFine_1.21.11_HD_U_J9.jar",
        ))
    };
    assert_eq!(changelogs(&t), 1);

    // Two days on: not yet. Three: read again, its files not fetched again
    // and its changelogs not read again.
    let pages = |t: &T| t.sites.asked(crate::external::OPTIFINE_DOWNLOADS);
    age(&t, 2 * 86_400);
    assert_eq!(import::refresh(&t.app, None).unwrap(), 0);
    age(&t, 86_400 + 60);
    assert_eq!(import::refresh(&t.app, None).unwrap(), 1);
    t.drain().await;
    assert_eq!(pages(&t), 2);
    assert_eq!(changelogs(&t), 1);
    let (_, list) = t.get("/v1/projects/optifine/versions").await;
    assert_eq!(list["versions"].as_array().unwrap().len(), 4);
    let again = list["versions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["name"] == "OptiFine HD U J9")
        .unwrap()
        .clone();
    assert_eq!(again["files"][0]["size"], jar.len());
    assert_eq!(again["saved"], "all");

    // The owner sets other days, or none.
    assert_eq!(
        external(&t, &["days", "optifine", "1"]).unwrap(),
        ["optifine: every 1 days"]
    );
    age(&t, 86_400 + 60);
    assert_eq!(import::refresh(&t.app, None).unwrap(), 1);
    t.drain().await;
    external(&t, &["days", "optifine", "0"]).unwrap();
    age(&t, 400 * 86_400);
    assert_eq!(import::refresh(&t.app, None).unwrap(), 0);
    // A check by hand still runs.
    external(&t, &["check", "optifine"]).unwrap();
    t.drain().await;
    assert_eq!(pages(&t), 4);
    // A page that lost most of its rows marks nothing gone.
    let mut short = optifine_page();
    let cut = short.find("<h2>Minecraft 1.21.11").unwrap();
    short.truncate(cut);
    t.sites.put(crate::external::OPTIFINE_DOWNLOADS, ok(short));
    external(&t, &["check", "optifine"]).unwrap();
    t.drain().await;
    let (_, list) = t.get("/v1/projects/optifine/versions").await;
    assert!(
        list["versions"]
            .as_array()
            .unwrap()
            .iter()
            .all(|v| v["goneAt"].is_null())
    );
    t.sites
        .put(crate::external::OPTIFINE_DOWNLOADS, ok(optifine_page()));
    let listed = external(&t, &["list"]).unwrap();
    assert_eq!(listed[0], "1 projects");
    assert!(
        listed[1].starts_with("optifine mod every 0 days, 4 versions, 3 kept"),
        "{}",
        listed[1]
    );
    assert!(external(&t, &["days", "optifine", "default"]).is_err());
    assert!(external(&t, &["days", "optifine", "366"]).is_err());
}

#[tokio::test]
async fn an_answer_that_is_no_jar_is_not_kept() {
    let t = setup();
    optifine_site(&t.sites);
    // A token the site no longer knows: a 200 with a line of text.
    let file = "OptiFine_1.21.11_HD_U_J9.jar";
    sites_put_token(&t.sites, file, b"Request not found.".to_vec());
    // A page without the link.
    t.sites.put(
        &crate::external::optifine_file_url("OptiFine_1.8.0_HD_U_E7.jar"),
        ok("<html>Please wait</html>"),
    );
    external(&t, &["add", "optifine"]).unwrap();
    t.drain().await;
    let (_, list) = t.get("/v1/projects/optifine/versions").await;
    let saved: Vec<(String, String)> = list["versions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| {
            (
                v["name"].as_str().unwrap().to_owned(),
                v["saved"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    assert!(
        saved.contains(&("OptiFine HD U J9".into(), "none".into())),
        "{saved:?}"
    );
    assert!(
        saved.contains(&("OptiFine HD U E7".into(), "none".into())),
        "{saved:?}"
    );
    assert!(
        saved.contains(&("OptiFine HD U K2 pre5".into(), "all".into())),
        "{saved:?}"
    );
}

fn sites_put_token(sites: &Sites, file: &str, body: Vec<u8>) {
    let token = "f".repeat(32);
    sites.put(
        &crate::external::optifine_file_url(file),
        ok(format!(
            "<a href='downloadx?f={file}&x={token}'>Download</a>"
        )),
    );
    sites.put(
        &format!("https://optifine.net/downloadx?f={file}&x={token}"),
        ok(body),
    );
}

#[tokio::test]
async fn the_owner_adds_a_project_and_its_versions_by_hand() {
    let t = setup();
    let jar = optifine_jar(9);
    t.sites.put(
        "https://files.author.org/dl/Better%20Mod-1.2.jar",
        ok(jar.clone()),
    );
    t.sites
        .put("https://files.author.org/icon.png", ok(png(64, 64)));
    external(
        &t,
        &[
            "manual",
            "better-mod",
            "--title",
            "Better Mod",
            "--kind",
            "mod",
            "--page",
            "https://author.org/better",
            "--summary",
            "Makes things better.",
            "--icon",
            "https://files.author.org/icon.png",
        ],
    )
    .unwrap();
    let lines = external(
        &t,
        &[
            "version",
            "better-mod",
            "--file",
            "https://files.author.org/dl/Better%20Mod-1.2.jar",
            "--game",
            "1.21.1,1.21",
            "--loader",
            "fabric",
        ],
    )
    .unwrap();
    assert_eq!(lines[0], "manual:better-mod: 1 versions added by hand");
    t.drain().await;
    let (status, project) = t.get("/v1/projects/better-mod").await;
    assert_eq!(status, StatusCode::OK, "{project}");
    assert_eq!(project["summary"], "Makes things better.");
    assert!(project["icon"].is_string(), "{project}");
    let (_, list) = t.get("/v1/projects/better-mod/versions").await;
    let version = &list["versions"][0];
    assert_eq!(version["saved"], "all");
    assert_eq!(version["number"], "BetterMod-1.2");
    assert_eq!(version["games"], json!(["1.21", "1.21.1"]));
    assert_eq!(version["files"][0]["name"], "BetterMod-1.2.jar");
    assert_eq!(version["files"][0]["size"], jar.len());

    // A file that is no archive is not kept.
    t.sites
        .put("https://files.author.org/dl/fake.jar", ok(vec![b'x'; 5000]));
    external(
        &t,
        &[
            "version",
            "manual:better-mod",
            "--file",
            "https://files.author.org/dl/fake.jar",
            "--game",
            "1.21.1",
            "--number",
            "1.3",
            "--channel",
            "beta",
        ],
    )
    .unwrap();
    t.drain().await;
    let (_, list) = t.get("/v1/projects/better-mod/versions").await;
    let fake = list["versions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["number"] == "1.3")
        .unwrap()
        .clone();
    assert_eq!(fake["saved"], "none");
    assert_eq!(fake["channel"], "beta");
}

#[test]
fn the_owners_commands_refuse_junk() {
    let t = setup();
    for args in [
        &[
            "manual",
            "Bad Slug",
            "--title",
            "x",
            "--kind",
            "mod",
            "--page",
            "https://a.org/",
        ][..],
        &[
            "manual",
            "ok-slug",
            "--title",
            "x",
            "--kind",
            "virus",
            "--page",
            "https://a.org/",
        ],
        &[
            "manual",
            "ok-slug",
            "--title",
            "x",
            "--kind",
            "mod",
            "--page",
            "http://a.org/",
        ],
        &[
            "manual",
            "ok-slug",
            "--title",
            "x",
            "--kind",
            "mod",
            "--page",
            "https://10.0.0.1/",
        ],
        &[
            "manual",
            "ok-slug",
            "--title",
            "x",
            "--kind",
            "mod",
            "--page",
            "https://router.lan/",
        ],
        &[
            "manual",
            "ok-slug",
            "--title",
            "\u{7}",
            "--kind",
            "mod",
            "--page",
            "https://a.org/",
        ],
        &[
            "manual",
            "ok-slug",
            "--title",
            "x",
            "--kind",
            "mod",
            "--page",
            "https://a.org/",
            "--days",
            "-1",
        ],
        &[
            "manual",
            "ok-slug",
            "--title",
            "x",
            "--kind",
            "mod",
            "--page",
            "https://a.org/",
            "--evil",
            "1",
        ],
        &[
            "manual",
            "ok-slug",
            "--title",
            "x",
            "--title",
            "y",
            "--kind",
            "mod",
            "--page",
            "https://a.org/",
        ],
        &["manual", "ok-slug", "--title"],
        &["add", "https://modrinth.com/mod/sodium"],
        &["add", "https://optifine.net.evil.org/"],
        &[
            "version",
            "nobody",
            "--file",
            "https://a.org/a.jar",
            "--game",
            "1.21",
        ],
        &["check", "nobody"],
        &["nonsense"],
        &[],
    ] {
        assert!(external(&t, args).is_err(), "{args:?}");
    }
    external(
        &t,
        &[
            "manual",
            "ok-slug",
            "--title",
            "x",
            "--kind",
            "mod",
            "--page",
            "https://a.org/",
        ],
    )
    .unwrap();
    for args in [
        &[
            "version",
            "ok-slug",
            "--file",
            "https://a.org/a.exe",
            "--game",
            "1.21",
        ][..],
        &[
            "version",
            "ok-slug",
            "--file",
            "https://127.0.0.1/a.jar",
            "--game",
            "1.21",
        ],
        &[
            "version",
            "ok-slug",
            "--file",
            "https://a.org/a.jar",
            "--game",
            "1.21;drop",
        ],
        &["version", "ok-slug", "--file", "https://a.org/a.jar"],
        &[
            "version",
            "ok-slug",
            "--file",
            "https://a.org/a.jar",
            "--game",
            "1.21",
            "--channel",
            "gold",
        ],
        &[
            "version",
            "optifine",
            "--file",
            "https://a.org/a.jar",
            "--game",
            "1.21",
        ],
    ] {
        assert!(external(&t, args).is_err(), "{args:?}");
    }
    assert!(
        t.app
            .store
            .external_versions("manual:ok-slug")
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn days_set_for_a_catalog_project_bound_its_checks() {
    let t = setup();
    modrinth_site(&t.sites);
    t.player(
        1,
        "POST",
        "/v1/imports",
        json!({ "input": "https://modrinth.com/mod/sodium" }),
    )
    .await;
    t.drain().await;
    let id = t.app.store.find_project("sodium").unwrap().unwrap();
    external(&t, &["days", "sodium", "2"]).unwrap();
    assert_eq!(t.app.store.recheck_days(&id).unwrap(), Some(2));
    age(&t, 2 * 86_400 + 60);
    // Sodium only: its dependency keeps the usual week.
    assert_eq!(import::refresh(&t.app, None).unwrap(), 1);
    external(&t, &["days", "sodium", "default"]).unwrap();
    assert_eq!(t.app.store.recheck_days(&id).unwrap(), None);
}

/// OptiFine's site with its J9 build suspected (it loads code from the
/// network) and its K2 preview carrying a known malware marker; the SHA-1
/// of J9's jar.
fn optifine_site_with_suspects(sites: &Sites) -> String {
    optifine_site(sites);
    let filler: Vec<u8> = (0..4000u32)
        .map(|i| (i.wrapping_mul(2_654_435_761) >> 13) as u8)
        .collect();
    let suspected = jar(&[
        (
            "a/Loader.class",
            class_file(
                &[
                    ("java/net/URLClassLoader", "<init>"),
                    ("java/net/URL", "openConnection"),
                ],
                &[],
                &[0xb1],
            ),
        ),
        ("optifine/Filler.bin", filler.clone()),
    ]);
    let mut code = built("http://85.217.144.130:8080/dl");
    code.push(0xb1);
    let malware = jar(&[
        ("dev/Utility.class", class_file(&[], &[], &code)),
        ("optifine/Filler.bin", filler),
    ]);
    for (seed, file, bytes) in [
        (0, "preview_OptiFine_26.2_HD_U_K2_pre5.jar", &malware),
        (1, "OptiFine_1.21.11_HD_U_J9.jar", &suspected),
    ] {
        let token = format!("{:032x}", seed + 1);
        sites.put(
            &format!("https://optifine.net/downloadx?f={file}&x={token}"),
            ok(bytes.clone()),
        );
    }
    hashes(&suspected).0
}

/// The SHA-256 a version's first file is kept under, and whether it is held.
async fn optifine_file(t: &T, name: &str) -> (String, bool) {
    let (_, list) = t.get("/v1/projects/optifine/versions").await;
    let version = list["versions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["name"] == name)
        .unwrap_or_else(|| panic!("{name}"))
        .clone();
    let file = &version["files"][0];
    (
        file["sha256"].as_str().unwrap().to_owned(),
        file["held"].as_bool().unwrap(),
    )
}

#[tokio::test]
async fn trusting_a_project_lets_its_suspected_files_go_but_not_malware() {
    let t = setup();
    let suspected_sha1 = optifine_site_with_suspects(&t.sites);
    external(&t, &["add", "optifine"]).unwrap();
    t.drain().await;
    let (j9, held) = optifine_file(&t, "OptiFine HD U J9").await;
    assert!(held);
    let (k2, held) = optifine_file(&t, "OptiFine HD U K2 pre5").await;
    assert!(held);
    assert_eq!(t.app.store.reviews(), vec![("scan".to_owned(), j9.clone())]);

    let lines = external(&t, &["trust", "optifine", "on"]).unwrap();
    assert_eq!(lines, vec!["optifine: trusted", "released 1"]);
    assert_eq!(
        optifine_file(&t, "OptiFine HD U J9").await,
        (j9.clone(), false)
    );
    assert!(t.app.blobs.has(&j9) && !t.app.blobs.held_path(&j9).is_file());
    assert!(t.app.store.reviews().is_empty());
    assert_eq!(
        optifine_file(&t, "OptiFine HD U K2 pre5").await,
        (k2.clone(), true)
    );
    assert!(!t.app.blobs.has(&k2));
    // Again: nothing more to let go.
    let lines = external(&t, &["trust", "optifine", "on"]).unwrap();
    assert_eq!(lines[1], "released 0");
    let list = external(&t, &["list"]).unwrap();
    assert!(list[1].contains(", trusted,"), "{list:?}");
    assert_eq!(
        t.app.store.decisions(&j9).unwrap()[0].1,
        "trust",
        "the journal says why"
    );

    // A block by the owner holds a trusted project's file all the same.
    let blocked = crate::owner::Hash::parse(&suspected_sha1).unwrap();
    let owner = crate::owner::Owner {
        store: &t.app.store,
        blobs: &t.app.blobs,
        urls: &t.app.urls,
        purge: None,
        bucket: None,
        virustotal: None,
        now: now(),
    };
    owner.block(&blocked, "test").unwrap();
    assert_eq!(
        external(&t, &["trust", "optifine", "on"]).unwrap()[1],
        "released 0"
    );
    assert!(!t.app.blobs.has(&j9));

    let lines = external(&t, &["trust", "optifine", "off"]).unwrap();
    assert!(lines[0].starts_with("optifine: not trusted"), "{lines:?}");
    assert!(!external(&t, &["list"]).unwrap()[1].contains("trusted"));
}

#[tokio::test]
async fn a_trusted_projects_new_files_are_served_at_once() {
    let t = setup();
    optifine_site_with_suspects(&t.sites);
    external(&t, &["add", "optifine"]).unwrap();
    external(&t, &["trust", "optifine", "on"]).unwrap();
    t.drain().await;
    let (j9, held) = optifine_file(&t, "OptiFine HD U J9").await;
    assert!(!held);
    assert!(t.app.blobs.has(&j9));
    assert!(t.app.store.reviews().is_empty());
    // Known malware is held whatever the owner trusts.
    let (k2, held) = optifine_file(&t, "OptiFine HD U K2 pre5").await;
    assert!(held);
    assert!(t.app.blobs.holds(&k2));
}

#[test]
fn trust_refuses_junk() {
    let t = setup();
    for args in [
        &["trust", "optifine", "on"][..],
        &["trust", "nobody", "on"],
        &["trust", "optifine"],
        &["trust", "optifine", "yes", "please"],
    ] {
        assert!(external(&t, args).is_err(), "{args:?}");
    }
    external(&t, &["add", "optifine"]).unwrap();
    for value in ["yes", "ON ", "1", "", "on; rm -rf /"] {
        assert!(
            external(&t, &["trust", "optifine", value]).is_err(),
            "{value:?}"
        );
    }
}

/// VirusTotal played by a fake: answers by file, and what it was asked.
#[derive(Default)]
struct FakeVirusTotal {
    answers: Mutex<HashMap<String, crate::virustotal::Answer>>,
    lookups: Mutex<Vec<String>>,
    uploads: Mutex<Vec<String>>,
}

impl crate::virustotal::Api for FakeVirusTotal {
    fn lookup(&self, sha256: &str) -> crate::virustotal::Answer {
        self.lookups.lock().unwrap().push(sha256.to_owned());
        self.answers
            .lock()
            .unwrap()
            .get(sha256)
            .cloned()
            .unwrap_or(crate::virustotal::Answer::Unknown)
    }

    fn upload_url(&self) -> crate::virustotal::Answer {
        crate::virustotal::Answer::Failed
    }

    fn upload(&self, url: &str, path: &std::path::Path) -> crate::virustotal::Answer {
        assert_eq!(url, "https://www.virustotal.com/api/v3/files");
        assert!(path.is_file());
        self.uploads
            .lock()
            .unwrap()
            .push(path.file_name().unwrap().to_string_lossy().into_owned());
        crate::virustotal::Answer::Sent
    }
}

impl FakeVirusTotal {
    fn knows(&self, sha256: &str, malicious: i64, suspicious: i64) {
        self.answers.lock().unwrap().insert(
            sha256.to_owned(),
            crate::virustotal::Answer::Found(crate::virustotal::Stats {
                malicious,
                suspicious,
                undetected: 60 - malicious - suspicious,
                harmless: 0,
                analysed: Some(1_792_000_000),
            }),
        );
    }

    fn lookups(&self) -> usize {
        self.lookups.lock().unwrap().len()
    }
}

/// Sodium's newest release is a dropper the scanner holds; VirusTotal is a
/// fake. The held file's SHA-256.
async fn suspected_with_virustotal() -> (T, Arc<FakeVirusTotal>, String) {
    let vt = Arc::new(FakeVirusTotal::default());
    let t = setup_full(
        Vec::new(),
        None,
        None,
        "",
        Some(Arc::clone(&vt) as Arc<dyn crate::virustotal::Api>),
    );
    modrinth_site(&t.sites);
    sodium_release_is(&t, &dropper_jar());
    t.player(
        1,
        "POST",
        "/v1/imports",
        json!({ "input": "https://modrinth.com/mod/sodium" }),
    )
    .await;
    t.drain().await;
    let (_, sha256) = t.app.store.reviews().pop().unwrap();
    (t, vt, sha256)
}

fn vt_step(t: &T, at: i64) -> bool {
    crate::virustotal::step(
        &t.app.store,
        &t.app.blobs,
        t.app.virustotal.as_deref().unwrap(),
        at,
    )
    .unwrap()
}

#[tokio::test]
async fn a_suspected_file_known_to_virustotal_shows_its_count_once_checked() {
    let (t, vt, sha256) = suspected_with_virustotal().await;
    let purged = Purged::default();
    let review = owner(&t, &purged).review().unwrap();
    assert!(
        review.contains(&"  virustotal: waiting".to_owned()),
        "{review:?}"
    );

    vt.knows(&sha256, 2, 1);
    assert!(vt_step(&t, now()));
    let review = owner(&t, &purged).review().unwrap();
    assert!(
        review.contains(&"  virustotal: 2 / 60, 1 suspicious (2026-10-14)".to_owned()),
        "{review:?}"
    );
    let found = owner(&t, &purged).find(&sha256).unwrap();
    assert!(
        found.contains(&"  virustotal: 2 / 60, 1 suspicious (2026-10-14)".to_owned()),
        "{found:?}"
    );
    // Asked once: no re-checks for now, and clean files are never asked.
    assert!(!vt_step(&t, now() + 400 * 86_400));
    assert_eq!(vt.lookups(), 1);
    let count = owner(&t, &purged).review_count().unwrap();
    assert!(
        count.contains(&"virustotal checked 1, with detections 1, waiting 0".to_owned()),
        "{count:?}"
    );
    // The owner asks again by hand.
    vt.knows(&sha256, 5, 0);
    let lines = owner(&t, &purged)
        .virustotal_check(&crate::owner::Hash::Sha256(sha256.clone()))
        .unwrap();
    assert_eq!(lines[1], "virustotal: 5 / 60 (2026-10-14)");
    assert_eq!(vt.lookups(), 2);
}

#[tokio::test]
async fn an_unknown_public_file_is_sent_then_looked_up_again() {
    let (t, vt, sha256) = suspected_with_virustotal().await;
    let purged = Purged::default();
    let start = now();
    assert!(vt_step(&t, start));
    assert_eq!(*vt.uploads.lock().unwrap(), vec![sha256.clone()]);
    let row = t.app.store.virustotal(&sha256).unwrap().unwrap();
    assert_eq!(
        crate::virustotal::line(&row, true),
        "virustotal: sent, analysing"
    );
    // Not before half an hour; then still analysing, then known.
    assert!(!vt_step(&t, start + 60));
    t.app.store.virustotal_spent("1970-01-01", 0).unwrap();
    vt.answers
        .lock()
        .unwrap()
        .insert(sha256.clone(), crate::virustotal::Answer::Analysing);
    assert!(vt_step(&t, start + 31 * 60));
    assert_eq!(vt.uploads.lock().unwrap().len(), 1, "not sent twice");
    vt.knows(&sha256, 0, 0);
    assert!(!vt_step(&t, start + 60 * 60), "waits two hours");
    assert!(vt_step(&t, start + 31 * 60 + 2 * 3600 + 1));
    let review = owner(&t, &purged).review().unwrap();
    assert!(
        review.iter().any(|l| l.starts_with("  virustotal: 0 / 60")),
        "{review:?}"
    );
}

#[tokio::test]
async fn virustotal_stays_within_its_quotas() {
    let (t, vt, sha256) = suspected_with_virustotal().await;
    vt.knows(&sha256, 1, 0);
    let purged = Purged::default();
    let at = now();
    let (day, month) = crate::virustotal::day_and_month(at);
    // The background's share of the day is spent: it asks nothing.
    t.app.store.virustotal_spent(&day, 450).unwrap();
    assert!(!vt_step(&t, at));
    assert_eq!(vt.lookups(), 0);
    // The owner still may, up to the day's quota.
    let lines = owner(&t, &purged)
        .virustotal_check(&crate::owner::Hash::Sha256(sha256.clone()))
        .unwrap();
    assert_eq!(lines[1], "virustotal: 1 / 60 (2026-10-14)");
    assert_eq!(
        t.app.store.virustotal_usage(&day, &month).unwrap(),
        (451, 451)
    );
    t.app.store.virustotal_spent(&day, 500).unwrap();
    let lines = owner(&t, &purged)
        .virustotal_check(&crate::owner::Hash::Sha256(sha256.clone()))
        .unwrap();
    assert_eq!(lines[1], "no quota left: try again tomorrow (UTC)");
    assert_eq!(vt.lookups(), 1);
    // The month's quota counts the other days of the month too.
    let store = &t.app.store;
    store.virustotal_spent("2030-01-01", 14_999).unwrap();
    assert!(
        store
            .take_virustotal("2030-01-15", "2030-01", 450, 15_000)
            .unwrap()
    );
    assert!(
        !store
            .take_virustotal("2030-01-15", "2030-01", 450, 15_000)
            .unwrap()
    );
    assert!(
        store
            .take_virustotal("2030-01-15", "2030-01", 500, 15_500)
            .unwrap()
    );
    assert_eq!(
        store.virustotal_usage("2030-01-15", "2030-01").unwrap(),
        (2, 15_001)
    );
    // VirusTotal's own word that the quota is spent ends the day.
    let later = at + 3 * 86_400;
    vt.answers
        .lock()
        .unwrap()
        .insert(sha256.clone(), crate::virustotal::Answer::Quota);
    let later_owner = crate::owner::Owner {
        now: later,
        ..owner(&t, &purged)
    };
    let lines = later_owner
        .virustotal_check(&crate::owner::Hash::Sha256(sha256.clone()))
        .unwrap();
    assert_eq!(lines[1], "no quota left: try again tomorrow (UTC)");
    let status = later_owner.virustotal_status().unwrap();
    assert_eq!(status[0], "virustotal on");
    assert_eq!(
        status[1].split(',').next().unwrap(),
        "requests today 500 of 500"
    );
    assert_eq!(status[2], "found 1");
    assert_eq!(status[3], "with detections 1");
    assert!(!vt_step(&t, later));
}

#[test]
fn virustotal_answers_are_read_strictly() {
    use crate::virustotal::{
        Answer, Curl, address_answer, lookup_answer, sent_answer, upload_address,
    };
    let report = |stats: Value| {
        json!({ "data": { "attributes": {
            "last_analysis_stats": stats, "last_analysis_date": 1_792_000_000,
            "last_analysis_results": { "Engine": { "result": "Evil.Name \u{1b}[31m" } },
        }}})
        .to_string()
    };
    let Answer::Found(stats) = lookup_answer(
        200,
        report(
            json!({ "malicious": 2, "suspicious": 1, "undetected": 57, "harmless": 0,
            "timeout": 3, "type-unsupported": 9 }),
        )
        .as_bytes(),
    ) else {
        panic!()
    };
    assert_eq!((stats.malicious, stats.total()), (2, 60));
    for junk in [
        report(json!({ "malicious": -1, "undetected": 5 })),
        report(json!({ "malicious": 99999, "undetected": 5 })),
        report(json!({ "malicious": "2", "undetected": 5 })),
        report(json!({ "malicious": 0, "undetected": 0 })),
        report(json!([1, 2])),
        "{}".to_owned(),
        "not json".to_owned(),
        String::new(),
    ] {
        assert_eq!(
            lookup_answer(200, junk.as_bytes()),
            Answer::Analysing,
            "{junk}"
        );
    }
    assert_eq!(lookup_answer(404, b"{}"), Answer::Unknown);
    assert_eq!(lookup_answer(429, b"{}"), Answer::Quota);
    for code in [0, 204, 301, 401, 403, 500, 503] {
        assert_eq!(lookup_answer(code, b"{}"), Answer::Failed, "{code}");
    }
    let good = "https://www.virustotal.com/_ah/upload/abc";
    assert!(upload_address(good));
    assert_eq!(
        address_answer(200, json!({ "data": good }).to_string().as_bytes()),
        Answer::Address(good.to_owned())
    );
    for bad in [
        "http://www.virustotal.com/upload",
        "https://virustotal.com.evil.org/upload",
        "https://evilvirustotal.com/upload",
        "https://user@www.virustotal.com/upload",
        "https://www.virustotal.com:8443/upload",
        "file:///etc/passwd",
        "https://10.0.0.1/upload",
        "",
    ] {
        assert!(!upload_address(bad), "{bad}");
        assert_eq!(
            address_answer(200, json!({ "data": bad }).to_string().as_bytes()),
            Answer::Failed,
            "{bad}"
        );
    }
    assert_eq!(address_answer(200, b"[]"), Answer::Failed);
    assert_eq!(sent_answer(200), Answer::Sent);
    assert_eq!(sent_answer(429), Answer::Quota);
    assert_eq!(sent_answer(413), Answer::Failed);
    // A key is letters and digits; anything else turns VirusTotal off.
    let curl = std::path::PathBuf::from("curl");
    assert!(Curl::new(&"a1".repeat(32), curl.clone()).is_some());
    for bad in [
        String::new(),
        "a1".repeat(15),
        "a1".repeat(65),
        format!("{} {}", "a1".repeat(16), "b2".repeat(16)),
        format!("{}\"\nurl = \"http://evil", "a1".repeat(32)),
    ] {
        assert!(Curl::new(&bad, curl.clone()).is_none(), "{bad:?}");
    }
}

#[tokio::test]
async fn the_virustotal_commands_refuse_junk() {
    let (t, _vt, _) = suspected_with_virustotal().await;
    let purged = Purged::default();
    let owner = owner(&t, &purged);
    assert!(
        owner
            .virustotal_check(&crate::owner::Hash::Sha256("ab".repeat(32)))
            .is_err()
    );
    let off = crate::owner::Owner {
        virustotal: None,
        ..owner
    };
    assert!(
        off.virustotal_check(&crate::owner::Hash::Sha256("ab".repeat(32)))
            .is_err()
    );
    assert_eq!(off.virustotal_status().unwrap()[0], "virustotal off");
    let review = off.review().unwrap();
    assert!(
        review.contains(&"  virustotal: off".to_owned()),
        "{review:?}"
    );
}

#[cfg(unix)]
#[test]
fn virustotal_through_curl_keeps_the_key_off_the_command_line() {
    use crate::virustotal::{Answer, Api, Curl};
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let script = dir.path().join("curl");
    let seen = dir.path().join("seen");
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\necho \"$@\" > {0}.args\ncat > {0}.config\n\
             printf '%s' '{{\"data\":{{\"attributes\":{{\"last_analysis_stats\":\
             {{\"malicious\":3,\"undetected\":57}}}}}}}}'\nprintf '\\n200'\n",
            seen.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    let key = "0123456789abcdef".repeat(4);
    let curl = Curl::new(&key, script).unwrap();
    let sha256 = "ab".repeat(32);
    let Answer::Found(stats) = curl.lookup(&sha256) else {
        panic!("not read")
    };
    assert_eq!((stats.malicious, stats.total()), (3, 60));
    let args = std::fs::read_to_string(dir.path().join("seen.args")).unwrap();
    let config = std::fs::read_to_string(dir.path().join("seen.config")).unwrap();
    assert!(!args.contains(&key), "{args}");
    assert!(config.contains(&format!("header = \"x-apikey: {key}\"")));
    assert!(config.contains(&format!(
        "url = \"https://www.virustotal.com/api/v3/files/{sha256}\""
    )));
    // Junk is refused before curl runs.
    assert_eq!(curl.lookup("../etc"), Answer::Failed);
    let odd = dir.path().join("a\"b");
    std::fs::write(&odd, b"x").unwrap();
    assert_eq!(
        curl.upload("https://www.virustotal.com/api/v3/files", &odd),
        Answer::Failed
    );
    assert_eq!(
        curl.upload("https://evil.example/upload", &dir.path().join("seen.args")),
        Answer::Failed
    );
}

#[tokio::test]
async fn the_owner_adds_projects_by_their_address() {
    let t = setup();
    modrinth_site(&t.sites);
    let purged = Purged::default();
    let owner = owner(&t, &purged);
    assert_eq!(owner.add_status().unwrap(), vec!["no additions yet"]);
    let added = owner
        .add(&["https://modrinth.com/mod/sodium".to_owned()])
        .unwrap();
    assert_eq!(added, vec!["queued https://modrinth.com/mod/sodium"]);
    t.drain().await;
    let (status, project) = t.get("/v1/projects/sodium").await;
    assert_eq!(status, StatusCode::OK, "{project}");
    assert_eq!(owner.add_status().unwrap(), vec!["ready 1"]);
}

#[test]
fn the_owner_adds_only_addresses_of_projects() {
    let t = setup();
    let purged = Purged::default();
    let owner = owner(&t, &purged);
    let one = |text: &str| owner.add(&[text.to_owned()]);
    for junk in [
        "",
        "sodium",
        "https://example.com/mod/sodium",
        "https://edge.forgecdn.net/files/1/2/a.jar",
        "javascript:alert(1)",
        "https://modrinth.com/mod/so\ndium",
        &"x".repeat(600),
    ] {
        assert!(one(junk).is_err(), "{junk:?}");
    }
    assert!(owner.add(&[]).is_err());
    let many = vec!["https://modrinth.com/mod/sodium".to_owned(); 33];
    assert!(owner.add(&many).is_err());
    // A refused list queues none of it.
    let mixed = vec![
        "https://modrinth.com/mod/sodium".to_owned(),
        "sodium".to_owned(),
    ];
    assert!(owner.add(&mixed).is_err());
    assert_eq!(owner.add_status().unwrap(), vec!["no additions yet"]);
}

#[test]
fn optifine_has_an_icon_of_its_own() {
    let card = crate::external::Card::optifine(3);
    let icon = card.icon.as_deref().unwrap();
    assert!(crate::fetch::picture_site(icon).is_some(), "{icon}");
    assert_eq!(card.project(now()).icon_url.as_deref(), Some(icon));
}

/// Player `who` starts an upload of `size` bytes from `ip`.
async fn start_upload_from(t: &T, who: u8, ip: &str, size: u64) -> (StatusCode, Value) {
    let token = t.token_with(&["account", "mods"], who);
    let request = Request::builder()
        .method("POST")
        .uri("/v1/packs/uploads")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .header("x-real-ip", ip)
        .body(Body::from(
            json!({ "size": size, "sha256": "ab".repeat(32) }).to_string(),
        ))
        .unwrap();
    let response = t.router.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

#[tokio::test]
async fn idle_uploads_of_a_few_accounts_keep_no_one_out() {
    let t = setup();
    let small = 1000;
    let large = crate::own::CHUNK * 3;
    // An address has two at a time; an IPv6 /64 is one address.
    for (who, ip, expected) in [
        (1, "203.0.113.20", StatusCode::CREATED),
        (2, "203.0.113.20", StatusCode::CREATED),
        (3, "203.0.113.20", StatusCode::TOO_MANY_REQUESTS),
        (4, "2001:db8:1:2::1", StatusCode::CREATED),
        (5, "2001:db8:1:2::ffff", StatusCode::CREATED),
        (6, "2001:db8:1:2:aaaa::1", StatusCode::TOO_MANY_REQUESTS),
    ] {
        let (status, answer) = start_upload_from(&t, who, ip, small).await;
        assert_eq!(status, expected, "{who} from {ip}: {answer}");
    }
    // Idle large uploads fill the large slots, and small packs still come.
    for who in 10..18 {
        let (status, answer) =
            start_upload_from(&t, who, &format!("198.51.100.{who}"), large).await;
        assert_eq!(status, StatusCode::CREATED, "{answer}");
    }
    let (status, answer) = start_upload_from(&t, 18, "198.51.100.18", large).await;
    assert_eq!(
        (status, &answer["error"]),
        (StatusCode::TOO_MANY_REQUESTS, &json!("busy"))
    );
    let (status, answer) = start_upload_from(&t, 19, "198.51.100.19", small).await;
    assert_eq!(status, StatusCode::CREATED, "{answer}");
}

#[tokio::test]
async fn signed_out_and_deleted_accounts_are_refused_and_forgotten() {
    let t = setup();
    import_sodium(&t).await;
    let (status, answer) = t.upload(Some(2), "/v1/packs", own_pack("1.0", &[])).await;
    assert_eq!(status, StatusCode::CREATED, "{answer}");
    let pack = answer["project"]["id"].as_str().unwrap().to_owned();
    let (status, _) = t
        .player(
            2,
            "POST",
            "/v1/imports",
            json!({ "input": "https://modrinth.com/mod/lithium" }),
        )
        .await;
    assert!(status.is_success(), "{status}");
    let (_, other) = t.upload(Some(3), "/v1/packs", own_pack("1.0", &[])).await;
    let other = other["project"]["id"].as_str().unwrap().to_owned();
    let (status, _) = t
        .player(
            2,
            "POST",
            &format!("/v1/projects/{other}/report"),
            json!({ "reason": "broken" }),
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED);

    // Player 1 signed out: refused; player 3 is not.
    t.app.sessions.end(&["s1".to_owned()]);
    assert_eq!(
        t.player(1, "GET", "/v1/packs/mine", Value::Null).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        t.player(3, "GET", "/v1/packs/mine", Value::Null).await.0,
        StatusCode::OK
    );

    let found = |hits: &Value, id: &str| {
        hits["hits"]
            .as_array()
            .unwrap()
            .iter()
            .any(|h| h["id"] == id)
    };
    let (_, hits) = t.get("/v1/search?q=Player").await;
    assert!(found(&hits, &pack), "{hits}");

    // Player 2's account is deleted.
    crate::api::purge(&t.app, &[(account(2), now())], 0);
    assert_eq!(
        t.player(2, "GET", "/v1/packs/mine", Value::Null).await.0,
        StatusCode::UNAUTHORIZED
    );
    let db = rusqlite::Connection::open(t.app.store.path()).unwrap();
    for (table, column) in [
        ("import_request", "account_id"),
        ("report", "account_id"),
        ("own_pack", "account_id"),
        ("own_version", "account_id"),
    ] {
        let left: i64 = db
            .query_row(
                &format!("SELECT count(*) FROM {table} WHERE {column} = ?1"),
                [account(2)],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(left, 0, "{table}");
    }
    let authors: i64 = db
        .query_row(
            "SELECT count(*) FROM project_author WHERE project_id = ?1",
            [&pack],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(authors, 0);
    // Its pack stays in the catalog, named by no one and found by no name.
    let (status, project) = t.get(&format!("/v1/projects/{pack}")).await;
    assert_eq!(status, StatusCode::OK, "{project}");
    assert_eq!(project["authors"], json!([]), "{project}");
    let (_, hits) = t.get("/v1/search?q=Player").await;
    assert!(!found(&hits, &pack), "{hits}");
    assert!(found(&hits, &other), "{hits}");
    // No one can change it, and players can still report it.
    let (status, _) = t
        .player(
            3,
            "POST",
            &format!("/v1/packs/{pack}/hidden"),
            json!({ "hidden": true }),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = t
        .player(
            3,
            "POST",
            &format!("/v1/projects/{pack}/report"),
            json!({ "reason": "broken" }),
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    // Player 3's pack stays.
    assert_eq!(
        t.get(&format!("/v1/projects/{other}")).await.0,
        StatusCode::OK
    );
}

// ----- slugs, the most downloaded, health, search order -----

/// A project straight in the store, as an import would put it.
fn stored_project(
    t: &T,
    source_id: &str,
    slug: &str,
    downloads: i64,
    categories: &[&str],
    loaders: &[&str],
) -> String {
    let (id, _) = t
        .app
        .store
        .upsert_project(
            &model::SourceProject {
                source: modrinth::SOURCE,
                source_id: source_id.to_owned(),
                slug: slug.to_owned(),
                kind: "mod",
                title: slug.to_owned(),
                summary: String::new(),
                body: String::new(),
                license: String::new(),
                icon_url: None,
                client_side: "unknown".to_owned(),
                server_side: "unknown".to_owned(),
                downloads,
                followers: 0,
                categories: categories.iter().map(|c| (*c).to_owned()).collect(),
                authors: Vec::new(),
                gallery: Vec::new(),
                url: format!("https://modrinth.com/mod/{slug}"),
                published: 0,
                updated: 0,
            },
            now(),
        )
        .unwrap();
    let versions: Vec<model::SourceVersion> = loaders
        .iter()
        .enumerate()
        .map(|(n, loader)| model::SourceVersion {
            source_id: format!("{source_id}v{n}"),
            number: format!("1.{n}"),
            name: String::new(),
            changelog: String::new(),
            channel: "release".to_owned(),
            published: 0,
            downloads: 0,
            games: vec!["1.21.1".to_owned()],
            loaders: vec![(*loader).to_owned()],
            files: Vec::new(),
            dependencies: Vec::new(),
            details: true,
        })
        .collect();
    t.app
        .store
        .upsert_versions(&id, modrinth::SOURCE, &versions)
        .unwrap();
    id
}

async fn slugs_of(t: &T, query: &str) -> Vec<String> {
    let (status, found) = t.get(&format!("/v1/search?{query}")).await;
    assert_eq!(status, StatusCode::OK, "{found}");
    found["hits"]
        .as_array()
        .unwrap()
        .iter()
        .map(|h| h["slug"].as_str().unwrap().to_owned())
        .collect()
}

#[tokio::test]
async fn libraries_come_last_in_lists_by_downloads() {
    let t = setup();
    stored_project(
        &t,
        "LibAAAAA",
        "big-library",
        9000,
        &["library"],
        &["fabric"],
    );
    stored_project(
        &t,
        "LibBBBBB",
        "cf-library",
        8000,
        &["library-api"],
        &["fabric"],
    );
    stored_project(&t, "ModAAAAA", "small-mod", 10, &["utility"], &["fabric"]);
    assert_eq!(
        slugs_of(&t, "kind=mod").await,
        ["small-mod", "big-library", "cf-library"]
    );
    assert_eq!(
        slugs_of(&t, "kind=mod&sort=downloads").await,
        ["small-mod", "big-library", "cf-library"]
    );
    // Words keep relevance, other orders theirs, and a list of libraries the
    // downloads.
    assert_eq!(
        slugs_of(&t, "q=library&sort=downloads").await[0],
        "big-library"
    );
    assert_eq!(slugs_of(&t, "kind=mod&sort=title").await[0], "big-library");
    assert_eq!(slugs_of(&t, "category=library").await, ["big-library"]);
}

#[tokio::test]
async fn mods_that_are_data_packs_too_are_found_with_data_packs() {
    let t = setup();
    stored_project(
        &t,
        "HybAAAAA",
        "terralith",
        500,
        &["worldgen"],
        &["fabric", "datapack"],
    );
    stored_project(
        &t,
        "ModAAAAA",
        "sodium-like",
        900,
        &["optimization"],
        &["fabric"],
    );
    assert_eq!(slugs_of(&t, "kind=datapack").await, ["terralith"]);
    let (_, found) = t.get("/v1/search?kind=datapack").await;
    // Still a mod: the launcher puts its data pack version into a world.
    assert_eq!(found["hits"][0]["kind"], "mod");
    assert_eq!(found["total"], 1);
    assert_eq!(slugs_of(&t, "kind=mod").await, ["sodium-like", "terralith"]);
}

#[tokio::test]
async fn the_owner_gives_a_project_another_slug() {
    let t = setup();
    let id = stored_project(
        &t,
        "FabAAAAA",
        "fabric-api-modrinth-20953b",
        1,
        &[],
        &["fabric"],
    );
    stored_project(&t, "OthAAAAA", "other", 1, &[], &["fabric"]);
    let purged = Purged::default();
    let owner = owner(&t, &purged);
    assert_eq!(
        owner
            .slug("fabric-api-modrinth-20953b", "fabric-api")
            .unwrap(),
        ["slug fabric-api-modrinth-20953b is now fabric-api"]
    );
    let (status, project) = t.get("/v1/projects/fabric-api").await;
    assert_eq!(status, StatusCode::OK, "{project}");
    assert_eq!(project["id"], id.as_str());
    assert_eq!(slugs_of(&t, "q=fabric").await, ["fabric-api"]);
    // Its source's slug still finds it by address; another's slug is taken.
    assert!(
        owner
            .slug("https://modrinth.com/mod/fabric-api", "fapi")
            .is_ok()
    );
    assert!(owner.slug("fapi", "other").is_err());
    assert!(owner.slug("nothing-here", "free").is_err());
    for junk in [
        "",
        "Fabric",
        "-fabric",
        "fabric-",
        "fab ric",
        "fab/ric",
        "fab\nric",
        "fäbric",
        &"x".repeat(65),
        "01a16010-29a4-7019-8eb8-1dad2b20953b",
    ] {
        assert!(owner.slug("fapi", junk).is_err(), "{junk:?}");
    }
}

#[test]
fn the_most_downloaded_are_read_from_modrinths_answer() {
    let url = modrinth::popular_url("shader", 500);
    assert!(url.contains("limit=100&index=downloads"), "{url}");
    assert!(url.contains("project%5Ftype%3Ashader"), "{url}");
    let ids = modrinth::project_ids(&json!({ "hits": [
        { "project_id": "AANobbMI" },
        { "project_id": "../../x" },
        { "project_id": 7 },
        { "slug": "no-id" },
        { "project_id": "P7dR8mSH" },
    ]}));
    assert_eq!(ids, ["AANobbMI", "P7dR8mSH"]);
    assert!(modrinth::project_ids(&json!({ "hits": "x" })).is_empty());
}

#[tokio::test]
async fn the_most_downloaded_are_added_once() {
    let t = setup();
    modrinth_site(&t.sites);
    for project_type in modrinth::POPULAR_TYPES {
        let hits = if project_type == "mod" {
            json!([{ "project_id": SODIUM }])
        } else {
            json!([])
        };
        t.sites.json(
            &modrinth::popular_url(project_type, 50),
            json!({ "hits": hits }),
        );
    }
    let mut missing = import::popular(&t.app, 50).await.unwrap();
    assert_eq!(missing, [SODIUM]);
    assert!(t.app.store.upkeep("popular_seeded").unwrap().is_some());
    assert_eq!(import::seed(&t.app, &mut missing).unwrap(), 1);
    assert!(missing.is_empty());
    t.drain().await;
    let (status, project) = t.get("/v1/projects/sodium").await;
    assert_eq!(status, StatusCode::OK, "{project}");
    // Known now: not looked for again.
    assert!(import::popular(&t.app, 50).await.unwrap().is_empty());
}

#[tokio::test]
async fn the_most_downloaded_wait_while_players_requests_are_queued() {
    let t = setup();
    for n in 0..import::SEED_WHEN_BELOW {
        t.app
            .store
            .enqueue(
                &format!("busy:{n}"),
                "body",
                &json!({}),
                None,
                None,
                now(),
                None,
            )
            .unwrap();
    }
    let mut missing: Vec<String> = ["AANobbMI", "P7dR8mSH", "gvQqBUqZ"]
        .map(str::to_owned)
        .to_vec();
    assert_eq!(import::seed(&t.app, &mut missing).unwrap(), 0);
    assert_eq!(missing.len(), 3);
    // Nearly empty again: a few at a time.
    t.drain().await;
    assert_eq!(
        import::seed(&t.app, &mut missing).unwrap(),
        import::SEED_AT_ONCE
    );
    assert_eq!(missing.len(), 3 - import::SEED_AT_ONCE);
}

#[tokio::test]
async fn health_names_its_problems_in_words_and_numbers() {
    let t = setup();
    let purged = Purged::default();
    let owner = owner(&t, &purged);
    // No copy of the database yet: the backup timer never ran.
    let backups = t._dir.path().join("backups");
    let lines = owner.health(0, &backups).unwrap();
    assert_eq!(lines.last().unwrap(), "problems 1", "{lines:?}");
    assert!(
        lines.contains(&"backups problem 0 copies, newest never".to_owned()),
        "{lines:?}"
    );
    t.app.store.backup(&backups, 14).unwrap();
    let lines = owner.health(0, &backups).unwrap();
    assert_eq!(lines.last().unwrap(), "all ok", "{lines:?}");
    assert!(lines.contains(&"popular off".to_owned()), "{lines:?}");
    assert!(
        lines.contains(&"backups ok 1 copies, newest 0 h ago".to_owned()),
        "{lines:?}"
    );
    // Popular projects never looked for, and a job due for a day.
    t.app
        .store
        .enqueue(
            "stuck",
            "project",
            &json!({}),
            None,
            None,
            now() - 86_400,
            None,
        )
        .unwrap();
    let lines = owner.health(50, &backups).unwrap();
    assert_eq!(lines.last().unwrap(), "problems 2", "{lines:?}");
    assert!(
        lines.contains(&"popular problem looked never".to_owned()),
        "{lines:?}"
    );
    assert!(
        lines
            .iter()
            .any(|l| l.starts_with("queue problem oldest waits 24 h")),
        "{lines:?}"
    );
    assert!(
        lines.iter().any(|l| l.starts_with("popular problem")),
        "{lines:?}"
    );
    for line in &lines {
        assert!(
            line.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b" ,".contains(&b)),
            "{line}"
        );
    }
}

#[tokio::test]
async fn a_first_copy_of_the_database_is_made_once() {
    let t = setup();
    let backups = t._dir.path().join("backups");
    assert!(crate::upkeep::first_backup(&t.app.store, &backups).unwrap());
    assert!(!crate::upkeep::first_backup(&t.app.store, &backups).unwrap());
    let names: Vec<String> = std::fs::read_dir(&backups)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names.len(), 1, "{names:?}");
    assert!(
        names[0].starts_with("catalog-") && names[0].ends_with(".db"),
        "{names:?}"
    );
    // A copy left half made is not taken for one.
    std::fs::write(backups.join("catalog-29991231-000000-000.db.part"), b"half").unwrap();
    assert_eq!(crate::store::backup_copies(&backups).len(), 1);
}
