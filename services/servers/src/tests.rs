use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use tokio::io::AsyncWriteExt;
use tower::ServiceExt;

use crate::ads::{self, Flags};
use crate::icons::{self, Icons, Kind};
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
        .live("feed", "en", NOW)
        .unwrap()
        .iter()
        .map(|a| a.id)
        .collect();
    assert_eq!(ids, [live.id]);
    assert_eq!(store::state(&later, NOW), "scheduled");
    assert_eq!(
        store.live("feed", "en", later.starts).unwrap()[0].id,
        later.id
    );
    assert_eq!(
        later.ends,
        later.starts + 7 * 86_400,
        "the run counts from the start"
    );
    assert!(store.live("home", "en", NOW).unwrap().is_empty());
    let counts = store.counts(NOW).unwrap();
    assert_eq!(counts["live"], 1);
    assert_eq!(counts["scheduled"], 1);
    assert_eq!(counts["paused"], 1);
    assert_eq!(counts["ended"], 1);
    // An ad of a minute is gone a minute later.
    let short = add(&store, &["--for", "1m"]);
    assert!(
        store
            .live("feed", "en", NOW + 59)
            .unwrap()
            .iter()
            .any(|a| a.id == short.id)
    );
    assert!(
        !store
            .live("feed", "en", NOW + 60)
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
    let sha = icons.keep(&clean, Kind::Png).unwrap();
    assert!(icons::sha_name(&sha));
    assert!(icons.read(&sha, Kind::Png).is_some());
    assert!(icons.read(&sha, Kind::Jpeg).is_none());
    assert!(icons.read("../servers.db", Kind::Png).is_none());
    icons.clean(&[]);
    assert!(icons.read(&sha, Kind::Png).is_none());
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
    // The sign-in check finds no one listening: the ping stands without it.
    let (port, _task) = fake_servers(vec![status_packet(&json)]).await;
    let mut pinger = Pinger::new();
    pinger.allow_local = true;
    pinger.pause = Duration::ZERO;
    let ad = store::Ad {
        address: format!("127.0.0.1:{port}"),
        ..ad
    };
    let ping = ads::ping(&store, &icons, &pinger, &ad, NOW, false)
        .await
        .unwrap();
    assert!(ping.up);
    assert!(
        icons
            .read(ping.icon.as_deref().unwrap(), Kind::Png)
            .is_some()
    );
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

#[test]
fn ads_can_be_shown_to_some_languages_only() {
    assert_eq!(text::langs("ru").unwrap(), ["ru"]);
    assert_eq!(text::langs(" RU, en ,ru").unwrap(), ["en", "ru"]);
    assert!(text::langs("all").unwrap().is_empty());
    for bad in ["", "de", "ru;en", "ru,,en", "*"] {
        assert!(text::langs(bad).is_err(), "{bad:?}");
    }
    let (_dir, store) = store();
    let everyone = add(&store, &[]);
    let russian = add(&store, &["--langs", "ru", "--weight", "50"]);
    assert_eq!(russian.langs, ["ru"]);
    let ids = |lang: &str| -> Vec<i64> {
        store
            .live("feed", lang, NOW)
            .unwrap()
            .iter()
            .map(|a| a.id)
            .collect()
    };
    assert_eq!(ids("ru"), [russian.id, everyone.id]);
    assert_eq!(ids("en"), [everyone.id]);
    assert_eq!(ids("ar"), [everyone.id]);
    let back = ads::build(
        &store,
        Some(&russian),
        &flags(&["--langs", "all"]),
        zone(),
        NOW,
    )
    .unwrap();
    store.update(&back, NOW).unwrap();
    assert_eq!(ids("en"), [russian.id, everyone.id]);
    // At most MAX_LIVE go out, counted after the language is matched.
    for _ in 0..store::MAX_LIVE {
        add(&store, &["--langs", "ar"]);
    }
    assert_eq!(ids("en").len(), 2);
    assert_eq!(ids("ar").len(), store::MAX_LIVE);
}

#[test]
fn a_database_from_before_gets_the_new_columns() {
    let dir = tempfile::tempdir().unwrap();
    {
        let db = rusqlite::Connection::open(dir.path().join("servers.db")).unwrap();
        db.execute_batch(
            "CREATE TABLE ads (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL,
                address TEXT NOT NULL, version TEXT NOT NULL, oldest TEXT NOT NULL,
                newest TEXT NOT NULL, texts TEXT NOT NULL, starts INTEGER NOT NULL,
                ends INTEGER NOT NULL, weight INTEGER NOT NULL, place TEXT NOT NULL, erid TEXT,
                paused INTEGER NOT NULL DEFAULT 0, icon TEXT, created INTEGER NOT NULL,
                updated INTEGER NOT NULL);
             CREATE TABLE pings (ad INTEGER PRIMARY KEY, at INTEGER NOT NULL, up INTEGER NOT NULL,
                online INTEGER, max INTEGER, protocol INTEGER, server_version TEXT, icon TEXT);
             INSERT INTO ads (name, address, version, oldest, newest, texts, starts, ends,
                weight, place, created, updated)
             VALUES ('Old', 'old.example.net', '1.21.4', '1.21.4', '1.21.4', '{\"*\":\"Old.\"}',
                0, 1900000000, 10, 'feed', 0, 0);
             INSERT INTO pings (ad, at, up, icon) VALUES (1, 5, 1, 'kept');",
        )
        .unwrap();
    }
    let store = Store::open(dir.path()).unwrap();
    let old = store.ad(1).unwrap().unwrap();
    assert!(old.langs.is_empty());
    assert_eq!(old.banner, None);
    let ping = store.ping(1).unwrap().unwrap();
    assert_eq!((ping.licensed, ping.checked_at), (None, None));
    assert_eq!(ping.icon.as_deref(), Some("kept"));
    // Opening again finds the columns there.
    drop(store);
    assert!(Store::open(dir.path()).is_ok());
}

#[test]
fn a_ping_without_a_check_keeps_the_last_check() {
    let (_dir, store) = store();
    let ad = add(&store, &[]);
    let checked = store::Ping {
        at: NOW,
        up: true,
        licensed: Some(true),
        checked_at: Some(NOW),
        ..Default::default()
    };
    store.save_ping(ad.id, &checked).unwrap();
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
    let kept = store.ping(ad.id).unwrap().unwrap();
    assert_eq!((kept.licensed, kept.checked_at), (Some(true), Some(NOW)));
    assert_eq!(ads::public(&store, &ad, "en").unwrap()["licensed"], true);
    // A check that found nothing is a check too.
    store
        .save_ping(
            ad.id,
            &store::Ping {
                at: NOW + 7200,
                up: true,
                licensed: None,
                checked_at: Some(NOW + 7200),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(
        ads::public(&store, &ad, "en").unwrap()["licensed"],
        serde_json::Value::Null
    );
}

fn picture(width: u32, height: u32, format: image::ImageFormat) -> Vec<u8> {
    let picture = image::RgbImage::from_fn(width, height, |x, y| {
        image::Rgb([(x % 256) as u8, (y % 256) as u8, 90])
    });
    let mut out = Vec::new();
    image::DynamicImage::ImageRgb8(picture)
        .write_to(&mut std::io::Cursor::new(&mut out), format)
        .unwrap();
    out
}

#[test]
fn banners_are_cut_to_16_9_and_written_again() {
    use image::ImageFormat::{Jpeg, Png};
    for (width, height, format) in [
        (1280, 720, Png),
        (800, 800, Jpeg),
        (2000, 400, Png),
        (480, 270, Jpeg),
    ] {
        let jpeg = icons::banner(&picture(width, height, format)).unwrap();
        assert!(jpeg.starts_with(&[0xff, 0xd8, 0xff]));
        let decoded = image::load_from_memory(&jpeg).unwrap();
        assert_eq!(
            (decoded.width(), decoded.height()),
            icons::BANNER,
            "{width}×{height}"
        );
    }
    assert!(
        icons::banner(&picture(400, 300, image::ImageFormat::Png)).is_err(),
        "too small"
    );
    assert!(icons::banner(b"GIF89a.......").is_err());
    assert!(icons::banner(b"<svg onload=alert(1)>").is_err());
    assert!(
        icons::banner(&[0xff, 0xd8, 0xff, 0xe0, 1, 2, 3]).is_err(),
        "a broken JPEG"
    );
    assert!(icons::banner(&vec![0xff; icons::BANNER_MAX_BYTES + 1]).is_err());
    assert!(
        icons::banner(&picture(4100, 270, image::ImageFormat::Png)).is_err(),
        "too wide to decode"
    );
}

/// A server on this machine that answers each of `answers` to one
/// connection in turn.
async fn fake_servers(answers: Vec<Vec<u8>>) -> (u16, tokio::task::JoinHandle<Vec<Vec<u8>>>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let task = tokio::spawn(async move {
        let mut heard = Vec::new();
        for answer in answers {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut buffer = [0u8; 512];
            let read = tokio::io::AsyncReadExt::read(&mut stream, &mut buffer)
                .await
                .unwrap_or(0);
            heard.push(buffer[..read].to_vec());
            let _ = stream.write_all(&answer).await;
            let _ = stream.flush().await;
        }
        heard
    });
    (port, task)
}

fn login_packet(id: i32, body: &[u8]) -> Vec<u8> {
    let mut packet = Vec::new();
    ping::write_varint(&mut packet, id);
    packet.extend_from_slice(body);
    let mut wire = Vec::new();
    ping::write_varint(&mut wire, packet.len() as i32);
    wire.extend_from_slice(&packet);
    wire
}

#[tokio::test]
async fn the_sign_in_check_reads_the_first_login_answer() {
    let mut pinger = Pinger::new();
    pinger.allow_local = true;
    pinger.pause = Duration::ZERO;
    for (answer, expected) in [
        (login_packet(0x01, b"\x00\x05key..\x04tokn"), Some(true)),
        (login_packet(0x02, &[0; 20]), Some(false)),
        (login_packet(0x03, &[0x80, 0x02]), Some(false)),
        (login_packet(0x00, b"\x0f\"whitelisted\""), None),
        (login_packet(0x04, &[1, 0]), None),
    ] {
        let (port, task) = fake_servers(vec![answer]).await;
        let got = pinger
            .licensed(&format!("127.0.0.1:{port}"), &[769])
            .await
            .unwrap();
        assert_eq!(got, expected);
        let heard = task.await.unwrap();
        // The handshake asks for login, then the check's name follows.
        assert!(
            heard[0]
                .windows(ping::CHECK_NAME.len())
                .any(|w| w == ping::CHECK_NAME.as_bytes())
        );
    }
    let (port, _task) = fake_servers(vec![vec![0xff; 8]]).await;
    assert!(
        pinger
            .licensed(&format!("127.0.0.1:{port}"), &[767])
            .await
            .is_err()
    );
    let mut huge = Vec::new();
    ping::write_varint(&mut huge, i32::MAX);
    let (port, _task) = fake_servers(vec![huge]).await;
    assert!(
        pinger
            .licensed(&format!("127.0.0.1:{port}"), &[767])
            .await
            .is_err()
    );
    pinger.allow_local = false;
    let (port, _task) = fake_servers(vec![login_packet(0x01, &[])]).await;
    assert!(
        pinger
            .licensed(&format!("127.0.0.1:{port}"), &[767])
            .await
            .is_err(),
        "this machine is never asked"
    );
}

#[tokio::test]
async fn the_sign_in_check_tries_the_next_protocol_after_a_kick() {
    let mut pinger = Pinger::new();
    pinger.allow_local = true;
    pinger.pause = Duration::ZERO;
    let kick = login_packet(0x00, b"\x0f\"use 1.21.7 on\"");
    // A version kick, then an encryption request in the next protocol.
    let (port, task) = fake_servers(vec![kick.clone(), login_packet(0x01, &[])]).await;
    let got = pinger
        .licensed(&format!("127.0.0.1:{port}"), &[767, 774])
        .await
        .unwrap();
    assert_eq!(got, Some(true));
    let heard = task.await.unwrap();
    // The handshake's length and id, then its protocol.
    let speaks = |wire: &[u8], protocol: i32| {
        let mut varint = Vec::new();
        ping::write_varint(&mut varint, protocol);
        wire[2..].starts_with(&varint)
    };
    assert!(speaks(&heard[0], 767) && speaks(&heard[1], 774));
    // Kicked in every protocol: unknown, and three logins at most.
    let (port, task) = fake_servers(vec![kick.clone(), kick.clone(), kick.clone()]).await;
    let got = pinger
        .licensed(&format!("127.0.0.1:{port}"), &[774, 767, 777, 763])
        .await
        .unwrap();
    assert_eq!(got, None);
    assert_eq!(task.await.unwrap().len(), 3);
    // Nothing to try is no answer.
    assert!(pinger.licensed("127.0.0.1:1", &[]).await.is_err());
}

#[test]
fn the_sign_in_check_starts_with_the_ads_version() {
    // CubeCraft-like: the status repeats 1.21.1's protocol it was asked in.
    assert_eq!(ping::check_protocols("1.21.11", Some(767)), [774, 767, 777]);
    assert_eq!(ping::check_protocols("1.20.1", Some(763)), [763, 777]);
    assert_eq!(ping::check_protocols("26.3", Some(777)), [777]);
    // A release we do not know, and odd status protocols.
    assert_eq!(ping::check_protocols("26.9", Some(790)), [790, 777]);
    assert_eq!(ping::check_protocols("1.8.9", None), [47, 777]);
    assert_eq!(ping::check_protocols("1.12.2", Some(-1)), [340, 777]);
    assert_eq!(ping::check_protocols("1.7.10", Some(i64::MAX)), [5, 777]);
}

#[test]
fn login_start_has_the_shape_of_each_protocol() {
    let body = |protocol: i32| {
        let wire = ping::login_start(protocol);
        // Length, id 0, name length, the name, then what the protocol adds.
        let start = 3 + ping::CHECK_NAME.len();
        assert_eq!(wire[0] as usize, wire.len() - 1);
        assert_eq!(&wire[3..start], ping::CHECK_NAME.as_bytes());
        wire[start..].to_vec()
    };
    assert!(body(47).is_empty(), "1.8");
    assert!(body(758).is_empty(), "1.18.2");
    assert_eq!(body(759), [0], "1.19");
    assert_eq!(body(760), [0, 0], "1.19.2");
    assert_eq!(body(763), [0], "1.20.1");
    assert_eq!(body(764).len(), 16, "1.20.2");
    assert_eq!(body(774).len(), 16, "1.21.11");
}

#[tokio::test]
async fn an_ad_ping_checks_the_sign_in_once_a_day() {
    let (dir, store) = store();
    let icons = Icons::new(dir.path()).unwrap();
    let mut pinger = Pinger::new();
    pinger.allow_local = true;
    pinger.pause = Duration::ZERO;
    let status = status_packet(
        r#"{"version":{"name":"Paper","protocol":769},"players":{"online":3,"max":10}}"#,
    );
    let (port, task) = fake_servers(vec![status.clone(), login_packet(0x01, &[])]).await;
    let ad = store::Ad {
        address: format!("127.0.0.1:{port}"),
        ..add(&store, &[])
    };
    let first = ads::ping(&store, &icons, &pinger, &ad, NOW, false)
        .await
        .unwrap();
    assert_eq!((first.licensed, first.checked_at), (Some(true), Some(NOW)));
    assert_eq!(task.await.unwrap().len(), 2);
    // An hour later: the status only.
    let (port, task) = fake_servers(vec![status.clone()]).await;
    let ad = store::Ad {
        address: format!("127.0.0.1:{port}"),
        ..ad
    };
    let later = ads::ping(&store, &icons, &pinger, &ad, NOW + 3600, false)
        .await
        .unwrap();
    assert_eq!((later.licensed, later.checked_at), (Some(true), Some(NOW)));
    assert_eq!(task.await.unwrap().len(), 1);
    // A day later, or when asked: checked again.
    let (port, task) = fake_servers(vec![status, login_packet(0x02, &[0; 20])]).await;
    let ad = store::Ad {
        address: format!("127.0.0.1:{port}"),
        ..ad
    };
    let next = ads::ping(&store, &icons, &pinger, &ad, NOW + ads::CHECK_EVERY, false)
        .await
        .unwrap();
    assert_eq!(next.licensed, Some(false));
    assert_eq!(task.await.unwrap().len(), 2);
}

#[tokio::test]
async fn the_api_serves_banners_and_the_sign_in_check() {
    let (dir, store) = store();
    let mut ad = add(&store, &[]);
    let icons = Icons::new(dir.path()).unwrap();
    let jpeg = icons::banner(&picture(1280, 720, image::ImageFormat::Png)).unwrap();
    ad.banner = Some(icons.keep(&jpeg, Kind::Jpeg).unwrap());
    store.update(&ad, NOW).unwrap();
    let sha = ad.banner.clone().unwrap();
    let app = app(store, dir.path());
    let (_, body) = call(&app, "GET", "/v1/ads?place=feed&lang=en", "").await;
    let value: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(value["ads"][0]["banner"], sha.as_str());
    assert_eq!(value["ads"][0]["licensed"], serde_json::Value::Null);
    let response = api::router(Arc::clone(&app))
        .oneshot(
            Request::builder()
                .uri(format!("/v1/icons/{sha}.jpg"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["content-type"], "image/jpeg");
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(bytes.as_ref(), jpeg.as_slice());
    // The banner is no PNG, and no other name finds it.
    for uri in [
        format!("/v1/icons/{sha}.png"),
        format!("/v1/icons/{sha}jpg"),
        format!("/v1/icons/{sha}.jpeg"),
        format!("/v1/icons/{sha}.jpg.png"),
    ] {
        assert_eq!(
            call(&app, "GET", &uri, "").await.0,
            StatusCode::NOT_FOUND,
            "{uri}"
        );
    }
}

#[tokio::test]
async fn players_online_are_kept_by_hour_from_when_an_ad_is_added() {
    let (dir, store) = store();
    let now_ad = add(&store, &[]);
    // Added a week before it starts: pinged from now on, for "before".
    let later = add(
        &store,
        &[
            "--start",
            "2027-01-22 15:00",
            "--for",
            "7d",
            "--name",
            "Later",
        ],
    );
    assert!(later.starts > NOW + 86_400);
    let due: Vec<i64> = store.to_ping(NOW).unwrap().iter().map(|a| a.id).collect();
    assert_eq!(due, [now_ad.id, later.id]);
    assert!(
        store.to_ping(later.ends).unwrap().is_empty(),
        "ended ads are not pinged"
    );
    let ping = |at: i64, up: bool, online: Option<i64>| store::Ping {
        at,
        up,
        online,
        max: Some(100),
        ..Default::default()
    };
    store
        .save_ping(now_ad.id, &ping(NOW, true, Some(12)))
        .unwrap();
    // Asked again within the hour: one row, the newest answer.
    store
        .save_ping(now_ad.id, &ping(NOW + 60, true, Some(15)))
        .unwrap();
    store
        .save_ping(now_ad.id, &ping(NOW + 3600, false, Some(99)))
        .unwrap();
    let hours = store.online(now_ad.id, NOW - 86_400).unwrap();
    let hour = NOW - NOW % 3600;
    assert_eq!(
        hours,
        [
            store::Hour {
                at: hour,
                up: true,
                online: Some(15),
                max: Some(100)
            },
            store::Hour {
                at: hour + 3600,
                up: false,
                online: None,
                max: None
            },
        ]
    );
    let lines = ads::online_days(&hours, &now_ad, zone());
    assert_eq!(lines.len(), 1);
    assert!(lines[0].contains("    15     15"), "{lines:?}");
    assert!(lines[0].contains(" 1 of 2"), "{lines:?}");
    assert!(lines[0].ends_with("while the ad shows"), "{lines:?}");

    let app = app(store, dir.path());
    let (status, body) = call(&app, "GET", &format!("/v1/online/{}", now_ad.id), "").await;
    assert_eq!(status, StatusCode::OK);
    let value: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(value["ad"], now_ad.id.to_string());
    assert_eq!(value["starts"], now_ad.starts);
    assert_eq!(value["hours"][0]["online"], 15);
    assert_eq!(value["hours"][1]["up"], false);
    for uri in [
        "/v1/online/999",
        "/v1/online/x1",
        "/v1/online/-1",
        "/v1/online/1234567890123",
    ] {
        assert_eq!(
            call(&app, "GET", uri, "").await.0,
            StatusCode::NOT_FOUND,
            "{uri}"
        );
    }
    app.store.remove(now_ad.id).unwrap();
    assert!(app.store.online(now_ad.id, 0).unwrap().is_empty());
}

#[test]
fn the_history_of_players_online_is_not_served_outside() {
    // nginx passes three paths on; the statistics service asks this one on
    // the server itself.
    let site = include_str!("../../../deploy/servers/nginx/servers-site.inc");
    assert!(!site.contains("online"));
    assert!(site.contains("location / {\n    return 404;\n}"));
}
