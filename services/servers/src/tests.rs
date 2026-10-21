use std::collections::BTreeMap;
use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use tokio::io::AsyncWriteExt;
use tower::ServiceExt;

use crate::ads::{self, Flags};
use crate::icons::{self, Icons};
use crate::ping::{self, Pinger};
use crate::store::{self, Store};
use crate::text;
use crate::{api, releases};

const NOW: i64 = 1_800_000_000;

fn zone() -> chrono::FixedOffset {
    text::offset("+03:00").unwrap()
}

fn store() -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path()).unwrap();
    let ids: Vec<String> = [
        "26.3", "1.21.11", "1.21.4", "1.21", "1.20.1", "1.20", "1.16.5", "1.8.9",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    store.set_releases(&ids, NOW).unwrap();
    (dir, store)
}

fn flags(args: &[&str]) -> Flags {
    let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    let (plain, flags) = Flags::parse(&args).unwrap();
    assert!(plain.is_empty());
    flags
}

const AD: [&str; 14] = [
    "--name",
    "Test Server",
    "--address",
    "Play.Example.net",
    "--version",
    "1.21.4",
    "--from",
    "1.20",
    "--to",
    "1.21.11",
    "--text",
    "Survival with friends.",
    "--for",
    "7d",
];

fn add(store: &Store, extra: &[&str]) -> store::Ad {
    let mut args = AD.to_vec();
    for pair in extra.chunks(2) {
        match args.iter().position(|a| *a == pair[0]) {
            Some(at) => args[at + 1] = pair[1],
            None => args.extend_from_slice(pair),
        }
    }
    let ad = ads::build(store, None, &flags(&args), zone(), NOW).unwrap();
    let id = store.insert(&ad, NOW).unwrap();
    store.ad(id).unwrap().unwrap()
}

#[test]
fn names_and_texts_are_cleaned_and_bounded() {
    assert_eq!(text::name("  Северный   Край ").unwrap(), "Северный Край");
    assert!(text::name("").is_err());
    assert!(text::name(&"x".repeat(41)).is_err());
    assert!(text::name("a\u{202E}b").is_err(), "bidi override");
    assert!(text::name("a\u{0007}b").is_err());
    assert!(text::name("a\nb").is_err());
    assert_eq!(
        text::ad_text(" One  line \r\nTwo ").unwrap(),
        "One line\nTwo"
    );
    assert!(text::ad_text("a\nb\nc\nd").is_err(), "four lines");
    assert!(text::ad_text("go to https://evil.example").is_err());
    assert!(text::ad_text("see www.example.com").is_err());
    assert!(text::ad_text("javascript://x").is_err());
    assert!(text::ad_text(&"я".repeat(201)).is_err());
    assert_eq!(
        text::ad_text(&"я".repeat(200)).unwrap().chars().count(),
        200
    );
    assert!(text::ad_text("a\u{200B}b").is_err(), "zero width");
    assert!(text::ad_text("a\u{0000}b").is_err());
    assert!(
        text::ad_text("<script>alert(1)</script>").is_ok(),
        "shown as text, never markup"
    );
}

#[test]
fn addresses_are_what_the_launcher_accepts() {
    assert_eq!(
        text::address("Play.Example.NET").unwrap(),
        "play.example.net"
    );
    assert_eq!(
        text::address("play.example.net:25565").unwrap(),
        "play.example.net"
    );
    assert_eq!(
        text::address("play.example.net:25566").unwrap(),
        "play.example.net:25566"
    );
    assert_eq!(text::address("192.0.2.7").unwrap(), "192.0.2.7");
    assert_eq!(
        text::address("[2001:db8::1]:25570").unwrap(),
        "[2001:db8::1]:25570"
    );
    for junk in [
        "",
        " ",
        "play example.net",
        "--server evil",
        "a;rm -rf /",
        "host:0",
        "host:70000",
        "a..b",
        "-a.com",
        "123.456",
        "[::1",
        "[zz::1]",
        "\"q\".com",
        "a\nb.com",
        "a/b.com",
        "a.com:",
        "a.com:abc",
        "x.y\u{0}",
    ] {
        assert!(text::address(junk).is_err(), "{junk:?}");
    }
    assert!(text::address(&format!("{}.com", "a".repeat(64))).is_err());
}

#[test]
fn durations_and_times() {
    assert_eq!(text::duration("1m").unwrap(), 60);
    assert_eq!(text::duration("90m").unwrap(), 5400);
    assert_eq!(text::duration("12h").unwrap(), 43_200);
    assert_eq!(text::duration("2w").unwrap(), 14 * 86_400);
    assert_eq!(text::duration("3mo").unwrap(), 90 * 86_400);
    assert_eq!(text::duration("1y").unwrap(), 365 * 86_400);
    assert_eq!(text::duration("5y").unwrap(), text::MAX_RUN);
    for bad in [
        "0m",
        "6y",
        "",
        "m",
        "7",
        "7x",
        "-1d",
        "99999999999999999999y",
        "1 d",
    ] {
        assert!(text::duration(bad).is_err(), "{bad:?}");
    }
    let t = text::time("2026-10-21 14:00", zone(), NOW).unwrap();
    assert_eq!(t, text::time("2026-10-21T11:00:00Z", zone(), NOW).unwrap());
    assert_eq!(text::time("now", zone(), NOW).unwrap(), NOW);
    assert!(text::time("tomorrow", zone(), NOW).is_err());
    assert!(text::offset("+03:00").is_ok());
    assert!(text::offset("03:00").is_err());
    assert!(text::offset("+15:00").is_err());
}

#[test]
fn an_ad_is_checked_against_the_releases() {
    let (_dir, store) = store();
    let ad = add(&store, &[]);
    assert_eq!(ad.address, "play.example.net");
    assert_eq!(ad.ends, NOW + 7 * 86_400);
    assert_eq!(ad.texts.get("*").unwrap(), "Survival with friends.");
    assert_eq!(
        store.range(&ad.oldest, &ad.newest).unwrap(),
        ["1.21.11", "1.21.4", "1.21", "1.20.1", "1.20"]
    );
    let build = |args: &[&str]| {
        let mut all = AD.to_vec();
        for pair in args.chunks(2) {
            let at = all.iter().position(|a| *a == pair[0]).unwrap();
            all[at + 1] = pair[1];
        }
        ads::build(&store, None, &flags(&all), zone(), NOW)
    };
    assert!(
        build(&["--version", "1.16.5"]).is_err(),
        "outside the range"
    );
    assert!(
        build(&["--from", "1.21.11", "--to", "1.20"]).is_err(),
        "from newer than to"
    );
    assert!(
        build(&["--version", "1.21.5"]).is_err(),
        "not a known release"
    );
    assert!(build(&["--version", "26.4-snapshot-5"]).is_err());
    assert!(build(&["--version", "../../x"]).is_err());
    assert!(build(&["--for", "0m"]).is_err());
    assert!(build(&["--for", "6y"]).is_err());
    assert!(build(&["--text", "https://x.example"]).is_err());
    // Missing parts.
    assert!(ads::build(&store, None, &flags(&["--name", "x"]), zone(), NOW).is_err());
    let mut no_end = AD.to_vec();
    no_end.truncate(12);
    assert!(ads::build(&store, None, &flags(&no_end), zone(), NOW).is_err());
    // Unknown flags and languages are refused, not ignored.
    let mut typo = AD.to_vec();
    typo.extend(["--wieght", "5"]);
    assert!(ads::build(&store, None, &flags(&typo), zone(), NOW).is_err());
    let mut lang = AD.to_vec();
    lang.extend(["--text-xx", "hi"]);
    assert!(ads::build(&store, None, &flags(&lang), zone(), NOW).is_err());
    let args: Vec<String> = ["--name"].iter().map(|s| s.to_string()).collect();
    assert!(Flags::parse(&args).is_err(), "a flag without a value");
    let args: Vec<String> = ["--name", "a", "--name", "b"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    assert!(Flags::parse(&args).is_err(), "a flag twice");
}

#[test]
fn edits_change_only_what_is_given() {
    let (_dir, store) = store();
    let ad = add(&store, &["--text-ru", "Выживание.", "--erid", "abc123"]);
    let changed = ads::build(
        &store,
        Some(&ad),
        &flags(&["--weight", "50", "--text-en", "Survival."]),
        zone(),
        NOW,
    )
    .unwrap();
    assert_eq!(changed.weight, 50);
    assert_eq!(changed.name, ad.name);
    assert_eq!(changed.ends, ad.ends);
    assert_eq!(changed.texts.len(), 3);
    let cleared = ads::build(&store, Some(&ad), &flags(&["--clear-erid"]), zone(), NOW).unwrap();
    assert_eq!(cleared.erid, None);
    assert!(ads::build(&store, Some(&ad), &flags(&["--erid", "a-b"]), zone(), NOW).is_err());
}

#[test]
fn only_ads_showing_now_go_out() {
    let (_dir, store) = store();
    let live = add(&store, &[]);
    let later = add(&store, &["--start", "2030-01-01 00:00"]);
    let mut paused = add(&store, &[]);
    paused.paused = true;
    store.update(&paused, NOW).unwrap();
    let mut ended = add(&store, &[]);
    ended.ends = NOW;
    store.update(&ended, NOW).unwrap();
    let ids: Vec<i64> = store
        .live("feed", NOW)
        .unwrap()
        .iter()
        .map(|a| a.id)
        .collect();
    assert_eq!(ids, [live.id]);
    assert_eq!(store::state(&later, NOW), "scheduled");
    assert_eq!(store.live("feed", later.starts).unwrap()[0].id, later.id);
    assert_eq!(
        later.ends,
        later.starts + 7 * 86_400,
        "the run counts from the start"
    );
    assert!(store.live("home", NOW).unwrap().is_empty());
    let counts = store.counts(NOW).unwrap();
    assert_eq!(counts["live"], 1);
    assert_eq!(counts["scheduled"], 1);
    assert_eq!(counts["paused"], 1);
    assert_eq!(counts["ended"], 1);
    // An ad of a minute is gone a minute later.
    let short = add(&store, &["--for", "1m"]);
    assert!(
        store
            .live("feed", NOW + 59)
            .unwrap()
            .iter()
            .any(|a| a.id == short.id)
    );
    assert!(
        !store
            .live("feed", NOW + 60)
            .unwrap()
            .iter()
            .any(|a| a.id == short.id)
    );
}

#[test]
fn launchers_get_their_language_and_the_versions() {
    let (_dir, store) = store();
    let ad = add(&store, &["--text-ru", "Выживание."]);
    let ru = ads::public(&store, &ad, "ru").unwrap();
    assert_eq!(ru["text"], "Выживание.");
    let ar = ads::public(&store, &ad, ads::lang(Some("ar"))).unwrap();
    assert_eq!(ar["text"], "Survival with friends.");
    assert_eq!(ads::lang(Some("de")), "en");
    assert_eq!(ru["versions"][0], "1.21.11");
    assert_eq!(ru["version"], "1.21.4");
    assert_eq!(ru["up"], serde_json::Value::Null);
    store
        .save_ping(
            ad.id,
            &store::Ping {
                at: NOW,
                up: true,
                online: Some(12),
                max: Some(100),
                icon: Some("a".repeat(64)),
                ..Default::default()
            },
        )
        .unwrap();
    // A failed ping keeps the last good icon, and shows no players.
    store
        .save_ping(
            ad.id,
            &store::Ping {
                at: NOW + 3600,
                up: false,
                ..Default::default()
            },
        )
        .unwrap();
    let after = ads::public(&store, &ad, "en").unwrap();
    assert_eq!(after["icon"], "a".repeat(64));
    assert_eq!(after["up"], false);
    assert_eq!(after["online"], serde_json::Value::Null);
}

fn png(side: u32) -> Vec<u8> {
    let picture = image::RgbaImage::from_pixel(side, side, image::Rgba([10, 200, 30, 255]));
    let mut out = Vec::new();
    image::DynamicImage::ImageRgba8(picture)
        .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
        .unwrap();
    out
}

#[test]
fn icons_are_decoded_and_written_again() {
    let clean = icons::clean(&png(128)).unwrap();
    let decoded = image::load_from_memory(&clean).unwrap();
    assert_eq!((decoded.width(), decoded.height()), (64, 64));
    assert!(icons::clean(b"GIF89a....").is_err());
    assert!(icons::clean(b"\x89PNG\r\n\x1a\njunk").is_err());
    let wide = image::RgbaImage::new(64, 32);
    let mut bytes = Vec::new();
    image::DynamicImage::ImageRgba8(wide)
        .write_to(
            &mut std::io::Cursor::new(&mut bytes),
            image::ImageFormat::Png,
        )
        .unwrap();
    assert!(icons::clean(&bytes).is_err(), "not square");
    assert!(icons::clean(&png(2048)).is_err(), "too large");
    use base64::Engine;
    let url = format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(png(64))
    );
    assert!(icons::from_status(&url).is_ok());
    assert!(icons::from_status("data:image/svg+xml;base64,PHN2Zz4=").is_err());
    let dir = tempfile::tempdir().unwrap();
    let icons = Icons::new(dir.path()).unwrap();
    let sha = icons.keep(&clean).unwrap();
    assert!(icons::sha_name(&sha));
    assert!(icons.read(&sha).is_some());
    assert!(icons.read("../servers.db").is_none());
    icons.clean(&[]);
    assert!(icons.read(&sha).is_none());
}

#[test]
fn only_public_addresses_are_pinged() {
    for ip in [
        "127.0.0.1",
        "10.1.2.3",
        "172.16.0.1",
        "192.168.1.1",
        "169.254.169.254",
        "0.0.0.0",
        "100.64.0.1",
        "224.0.0.1",
        "255.255.255.255",
        "::1",
        "fc00::1",
        "fe80::1",
        "::ffff:127.0.0.1",
        "::ffff:10.0.0.1",
        "2001:db8::1",
    ] {
        assert!(!ping::public(ip.parse().unwrap()), "{ip}");
    }
    for ip in ["1.1.1.1", "8.8.8.8", "2606:4700::1111", "::ffff:1.1.1.1"] {
        assert!(ping::public(ip.parse().unwrap()), "{ip}");
    }
}

/// A server on this machine that answers one status ping with `answer`.
async fn fake_server(answer: Vec<u8>) -> u16 {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut buffer = [0u8; 512];
        let _ = tokio::io::AsyncReadExt::read(&mut stream, &mut buffer).await;
        let _ = stream.write_all(&answer).await;
        let _ = stream.flush().await;
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    });
    port
}

fn status_packet(json: &str) -> Vec<u8> {
    let mut body = Vec::new();
    ping::write_varint(&mut body, 0);
    ping::write_string(&mut body, json);
    let mut wire = Vec::new();
    ping::write_varint(&mut wire, body.len() as i32);
    wire.extend_from_slice(&body);
    wire
}

#[tokio::test]
async fn a_status_is_read_with_limits() {
    use base64::Engine;
    let favicon = format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(png(64))
    );
    let json = serde_json::json!({
        "version": {"name": "Paper 1.21.4", "protocol": 769},
        "players": {"online": 42, "max": 1000},
        "description": "x",
        "favicon": favicon,
    })
    .to_string();
    let mut pinger = Pinger::new();
    let port = fake_server(status_packet(&json)).await;
    assert!(
        pinger.ping(&format!("127.0.0.1:{port}")).await.is_err(),
        "this machine is never pinged"
    );
    pinger.allow_local = true;
    let port = fake_server(status_packet(&json)).await;
    let status = pinger.ping(&format!("127.0.0.1:{port}")).await.unwrap();
    assert_eq!(status.online, Some(42));
    assert_eq!(status.max, Some(1000));
    assert_eq!(status.version.as_deref(), Some("Paper 1.21.4"));
    assert!(icons::from_status(status.favicon.as_deref().unwrap()).is_ok());

    // A server that claims a huge answer is refused before anything is read.
    let mut huge = Vec::new();
    ping::write_varint(&mut huge, i32::MAX);
    let port = fake_server(huge).await;
    assert!(pinger.ping(&format!("127.0.0.1:{port}")).await.is_err());
    let port = fake_server(vec![0xff; 16]).await;
    assert!(pinger.ping(&format!("127.0.0.1:{port}")).await.is_err());
    let port = fake_server(status_packet("not json")).await;
    assert!(pinger.ping(&format!("127.0.0.1:{port}")).await.is_err());
    let port = fake_server(status_packet(r#"{"players":{"online":-5,"max":1e300}}"#)).await;
    let odd = pinger.ping(&format!("127.0.0.1:{port}")).await.unwrap();
    assert_eq!((odd.online, odd.max), (None, None));
}

#[tokio::test]
async fn an_ad_ping_keeps_the_icon() {
    let (dir, store) = store();
    let ad = add(&store, &[]);
    let icons = Icons::new(dir.path()).unwrap();
    use base64::Engine;
    let favicon = format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(png(64))
    );
    let json =
        serde_json::json!({"players": {"online": 1, "max": 2}, "favicon": favicon}).to_string();
    let port = fake_server(status_packet(&json)).await;
    let mut pinger = Pinger::new();
    pinger.allow_local = true;
    let ad = store::Ad {
        address: format!("127.0.0.1:{port}"),
        ..ad
    };
    let ping = ads::ping(&store, &icons, &pinger, &ad, NOW).await.unwrap();
    assert!(ping.up);
    assert!(icons.read(ping.icon.as_deref().unwrap()).is_some());
}

#[test]
fn releases_come_from_the_manifest() {
    let manifest = br#"{"latest":{},"versions":[
        {"id":"26.4-snapshot-5","type":"snapshot"},
        {"id":"26.3","type":"release"},
        {"id":"bad id","type":"release"},
        {"id":"1.21.11","type":"release"},
        {"id":"b1.7.3","type":"old_beta"}]}"#;
    assert_eq!(releases::parse(manifest).unwrap(), ["26.3", "1.21.11"]);
    assert!(releases::parse(b"{}").is_err());
    assert!(releases::parse(br#"{"versions":[]}"#).is_err());
}

fn app(store: Store, dir: &std::path::Path) -> Arc<api::App> {
    fn clock() -> i64 {
        NOW
    }
    Arc::new(api::App {
        store,
        icons: Icons::new(dir).unwrap(),
        clock,
    })
}

async fn call(app: &Arc<api::App>, method: &str, uri: &str, body: &str) -> (StatusCode, String) {
    let response = api::router(Arc::clone(app))
        .oneshot(
            Request::builder()
                .method(method)
                .uri(uri)
                .header("content-type", "application/json")
                .body(Body::from(body.to_owned()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (status, String::from_utf8_lossy(&bytes).into_owned())
}

#[tokio::test]
async fn the_api_serves_live_ads() {
    let (dir, store) = store();
    add(&store, &["--text-ru", "Выживание."]);
    let app = app(store, dir.path());
    let (status, body) = call(&app, "GET", "/v1/ads?place=feed&lang=ru", "").await;
    assert_eq!(status, StatusCode::OK);
    let value: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(value["ads"].as_array().unwrap().len(), 1);
    assert_eq!(value["ads"][0]["text"], "Выживание.");
    assert_eq!(value["ads"][0]["address"], "play.example.net");
    let (status, _) = call(&app, "GET", "/v1/ads?place=nowhere", "").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, body) = call(&app, "GET", "/v1/health", "").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("\"service\":\"lapi-servers\""));
    for uri in [
        "/v1/icons/..%2Fservers.db",
        "/v1/icons/servers.db",
        &format!("/v1/icons/{}.png", "0".repeat(64)),
        "/v1/icons/ABC.png",
    ] {
        assert_eq!(
            call(&app, "GET", uri, "").await.0,
            StatusCode::NOT_FOUND,
            "{uri}"
        );
    }
}

#[tokio::test]
async fn nothing_can_be_written_from_outside() {
    let (dir, store) = store();
    add(&store, &[]);
    let app = app(store, dir.path());
    let ad = serde_json::json!({"name": "x", "address": "evil.example", "text": "x"}).to_string();
    for method in ["POST", "PUT", "PATCH", "DELETE"] {
        for uri in [
            "/v1/ads",
            "/v1/ads/1",
            "/v1/health",
            "/v1/icons/x.png",
            "/admin",
            "/",
            "/v1/ads?place=feed",
        ] {
            let (status, _) = call(&app, method, uri, &ad).await;
            assert!(
                status == StatusCode::METHOD_NOT_ALLOWED || status == StatusCode::NOT_FOUND,
                "{method} {uri}: {status}"
            );
        }
    }
    let ads = app.store.all().unwrap();
    assert_eq!(ads.len(), 1, "nothing was added or removed");
    let texts: BTreeMap<String, String> = ads[0].texts.clone();
    assert_eq!(texts.get("*").unwrap(), "Survival with friends.");
}
