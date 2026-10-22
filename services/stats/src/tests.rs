use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use chrono::{DateTime, Duration, NaiveDate, TimeZone, Utc};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

use crate::api::{self, App};
use crate::model::{self, Counts, Period, Ping};
use crate::store::{ShownAd, Store};
use crate::{ads, pow, report};

const BITS: u32 = 8;

fn clock() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 10, 22, 12, 0, 0).unwrap()
}

fn today() -> NaiveDate {
    clock().date_naive()
}

fn app() -> (tempfile::TempDir, Arc<App>) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path()).unwrap();
    (dir, Arc::new(App::new(store, BITS, clock).unwrap()))
}

fn batch() -> String {
    uuid_v4()
}

/// A random-looking v4 UUID without the `v4` feature: tests only need them
/// different.
fn uuid_v4() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(1);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    format!("00000000-0000-4000-8000-{n:012x}")
}

fn ping(first: &[&str]) -> Value {
    let batch = batch();
    let day = today().to_string();
    let nonce = pow::solve(&batch, &day, BITS);
    json!({
        "schema": 1,
        "batch": batch,
        "day": day,
        "nonce": nonce,
        "first": first,
        "cohort": "2026-W41",
        "launcher": "1.4.0",
        "channel": "stable",
        "os": "windows",
        "osVersion": "11",
        "arch": "x86_64",
        "bundle": "nsis",
        "ui": "ru",
        "system": "kk",
        "tz": "Asia/Almaty",
        "ram": "8-16",
        "cores": "5-8",
        "accounts": ["notoken", "microsoft"],
    })
}

/// Fills the work in for a body changed after `ping` or `counts` made it.
fn rework(mut body: Value) -> Value {
    let batch = body["batch"].as_str().unwrap().to_owned();
    let day = body["day"].as_str().unwrap().to_owned();
    body["nonce"] = json!(pow::solve(&batch, &day, BITS));
    body
}

fn counts(ads: Value) -> Value {
    let batch = batch();
    let day = today().to_string();
    let nonce = pow::solve(&batch, &day, BITS);
    json!({ "schema": 1, "batch": batch, "day": day, "nonce": nonce, "ui": "ru", "ads": ads })
}

async fn post(app: &Arc<App>, path: &str, body: &Value, country: &str) -> (StatusCode, Value) {
    post_from(app, path, body, country, "203.0.113.7").await
}

async fn post_from(
    app: &Arc<App>,
    path: &str,
    body: &Value,
    country: &str,
    ip: &str,
) -> (StatusCode, Value) {
    let request = Request::post(path)
        .header("content-type", "application/json")
        .header("x-real-ip", ip)
        .header("x-country", country)
        .body(Body::from(serde_json::to_vec(body).unwrap()))
        .unwrap();
    answer(app, request).await
}

async fn answer(app: &Arc<App>, request: Request<Body>) -> (StatusCode, Value) {
    let response = api::router(Arc::clone(app)).oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

fn values(app: &App, period: Period, cut: &str) -> Vec<(String, i64)> {
    app.flush().unwrap();
    app.store.values(today(), today(), period, cut).unwrap()
}

fn show(app: &App, ids: &[i64]) {
    let shown: Vec<ShownAd> = ids
        .iter()
        .map(|id| ShownAd {
            id: *id,
            name: format!("Server {id}"),
            erid: (*id == 3).then(|| "2SDnjeXYZ".to_owned()),
        })
        .collect();
    app.store.save_ads(today(), &shown).unwrap();
    app.shown.note(today(), &shown);
}

#[tokio::test]
async fn a_ping_counts_once_in_each_cut_of_its_periods() {
    let (_dir, app) = app();
    let body = ping(&["day", "week", "ever"]);
    let (status, _) = post(&app, "/v1/ping", &body, "kz").await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    // The same batch again (a resend after a lost answer) is not counted.
    let (status, _) = post(&app, "/v1/ping", &body, "KZ").await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(values(&app, Period::Day, "country"), [("KZ".to_owned(), 1)]);
    assert_eq!(
        values(&app, Period::Week, "country"),
        [("KZ".to_owned(), 1)]
    );
    assert_eq!(values(&app, Period::New, "country"), [("KZ".to_owned(), 1)]);
    assert!(values(&app, Period::Month, "country").is_empty());
    let cut = |cut: &str| values(&app, Period::Day, cut)[0].0.clone();
    assert_eq!(cut("system"), "windows/11/x86_64");
    assert_eq!(cut("version"), "1.4.0/stable/nsis");
    assert_eq!(cut("country_ui"), "KZ/ru");
    assert_eq!(cut("language"), "kk/ru");
    assert_eq!(cut("country_tz"), "KZ/Asia/Almaty");
    assert_eq!(cut("hardware"), "8-16/5-8");
    assert_eq!(cut("accounts"), "microsoft+notoken/KZ");
    assert_eq!(cut("cohort"), "2026-W41/KZ");
    // One row per cut and period: nothing stored describes the launcher whole.
    assert_eq!(app.store.rows("players"), 9 * 3);
    assert_eq!(app.store.rows("batches"), 1);
    let left = app.store.dropped(today(), today()).unwrap();
    assert_eq!(left, [("again".to_owned(), 1)]);
}

#[tokio::test]
async fn values_outside_their_words_count_as_other() {
    let (_dir, app) = app();
    let mut body = ping(&["day"]);
    for (key, value) in [
        ("launcher", json!("1.4.0; DROP TABLE players")),
        ("channel", json!("nightly")),
        ("os", json!("plan9")),
        ("osVersion", json!("Windows 11 Pro\u{202E}")),
        ("arch", json!("riscv64")),
        ("bundle", json!("flatpak")),
        ("ui", json!("RU")),
        ("system", json!("x")),
        ("tz", json!("Mars/Olympus_Mons")),
        ("ram", json!("1TB")),
        ("cores", json!("lots")),
        ("cohort", json!("2027-W01")),
        ("accounts", json!(["xbox", "microsoft", "microsoft"])),
    ] {
        body[key] = value;
    }
    let (status, _) = post(&app, "/v1/ping", &rework(body), "<script>").await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let cut = |cut: &str| values(&app, Period::Day, cut)[0].0.clone();
    assert_eq!(cut("country"), "XX");
    assert_eq!(cut("system"), "other/other/other");
    assert_eq!(cut("version"), "other/other/other");
    assert_eq!(cut("country_ui"), "XX/other");
    assert_eq!(cut("language"), "other/other");
    assert_eq!(cut("country_tz"), "XX/other");
    assert_eq!(cut("hardware"), "other/other");
    assert_eq!(cut("accounts"), "microsoft+other/XX");
    // A week after the ping's own is not an install week.
    assert_eq!(cut("cohort"), "other/XX");
}

#[tokio::test]
async fn requests_of_a_wrong_shape_are_refused_and_counted_nowhere() {
    let (_dir, app) = app();
    let good = ping(&["day"]);
    let yesterday = (today() - Duration::days(1)).to_string();
    let old = (today() - Duration::days(2)).to_string();
    let future = (today() + Duration::days(2)).to_string();
    let cases: Vec<(&str, Value)> = vec![
        ("schema", json!(2)),
        ("batch", json!("not-a-uuid")),
        ("batch", json!("00000000000040008000000000000001")),
        ("day", json!(old)),
        ("day", json!(future)),
        ("day", json!("2026-10-22T00:00")),
        ("first", json!(["week"])),
        ("first", json!(["day", "decade"])),
        ("first", json!(["day", "day", "day", "day", "day"])),
        (
            "accounts",
            json!(["a", "b", "c", "d", "e", "f", "g", "h", "i"]),
        ),
    ];
    for (key, value) in cases {
        let mut body = good.clone();
        body[key] = value.clone();
        let (status, _) = post(&app, "/v1/ping", &rework(body), "RU").await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{key} = {value}");
    }
    // A wrong type, a missing field, not JSON.
    for raw in [
        r#"{"schema": "1"}"#.to_owned(),
        r#"{"schema": 1, "day": "2026-10-22"}"#.to_owned(),
        "not json".to_owned(),
        format!(
            "{{\"schema\":1,\"batch\":\"{}\",\"day\":\"{}\",\"nonce\":\"1\",\"first\":\"day\"}}",
            batch(),
            today()
        ),
    ] {
        let request = Request::post("/v1/ping")
            .header("content-type", "application/json")
            .body(Body::from(raw.clone()))
            .unwrap();
        let (status, answer) = answer(&app, request).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{raw}");
        // Our word, not the sender's text echoed back.
        assert_eq!(answer, json!({ "error": "json" }));
    }
    // Yesterday is still fine: a launcher's clock may be behind.
    let mut late = good.clone();
    late["day"] = json!(yesterday);
    let (status, _) = post(&app, "/v1/ping", &rework(late), "RU").await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    app.flush().unwrap();
    assert!(
        app.store
            .values(today(), today(), Period::Day, "country")
            .unwrap()
            .is_empty()
    );
    assert_eq!(app.store.rows("batches"), 1);
}

#[tokio::test]
async fn a_request_without_its_work_is_refused_with_the_bits_asked() {
    let (_dir, app) = app();
    let mut body = ping(&["day"]);
    body["nonce"] = json!("0");
    if pow::done(
        body["batch"].as_str().unwrap(),
        body["day"].as_str().unwrap(),
        "0",
        BITS,
    ) {
        body["nonce"] = json!("x");
    }
    let (status, answer) = post(&app, "/v1/ping", &body, "RU").await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(answer, json!({ "error": "work", "bits": BITS }));
    for nonce in ["", "12a", "123456789012345678901"] {
        body["nonce"] = json!(nonce);
        let (status, _) = post(&app, "/v1/ping", &body, "RU").await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{nonce:?}");
    }
    // The work is bound to the batch and day: another batch's nonce fails.
    let other = ping(&["day"]);
    body["nonce"] = other["nonce"].clone();
    let (status, _) = post(&app, "/v1/ping", &body, "RU").await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[test]
fn the_work_counts_leading_zero_bits() {
    assert_eq!(pow::zeros(&[0, 0, 0x0f, 0xff]), 20);
    assert_eq!(pow::zeros(&[0x80]), 0);
    assert_eq!(pow::zeros(&[0, 0]), 16);
    let nonce = pow::solve("b", "2026-10-22", 12);
    assert!(pow::done("b", "2026-10-22", &nonce, 12));
    assert!(!pow::done("b", "2026-10-23", &nonce, 24));
}

fn ad(id: &str) -> Value {
    json!({
        "ad": id,
        "shown": 4,
        "reach": ["day", "ever"],
        "menu": 1,
        "versionList": 1,
        "copy": 1,
        "report": 0,
        "play": 2,
        "license": 1,
        "installed": 1,
        "installFailed": 0,
        "started": 1,
        "joined": 1,
        "refused": { "full": 1, "mystery": 2 },
        "played": 1,
        "minutes": 35,
        "back": ["day"],
        "picked": { "1.21.11": 1, "1.21.4": 1, "bad version!": 3 },
    })
}

#[tokio::test]
async fn ad_counts_are_taken_for_ads_that_showed_that_day_only() {
    let (_dir, app) = app();
    // Before the first list of the ads service: sent again later.
    let early = counts(json!([ad("3")]));
    let (status, _) = post(&app, "/v1/counts", &early, "RU").await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    show(&app, &[3]);
    let (status, _) = post(&app, "/v1/counts", &early, "RU").await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let body = counts(json!([ad("3"), ad("999")]));
    let (status, _) = post(&app, "/v1/counts", &body, "KZ").await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = post(&app, "/v1/counts", &body, "KZ").await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    app.flush().unwrap();
    let totals = app.store.ad_totals(today(), today(), None).unwrap();
    let n = |event: &str| totals.get(&(3, event.to_owned())).copied().unwrap_or(0);
    assert_eq!(n("shown"), 8);
    assert_eq!(n("reach_day"), 2);
    assert_eq!(n("play"), 4);
    assert_eq!(n("minutes"), 70);
    assert_eq!(n("refused_full"), 2);
    assert_eq!(n("refused_other"), 4);
    assert_eq!(n("back_day"), 2);
    assert_eq!(n("back_week"), 0);
    assert!(
        totals.keys().all(|(id, _)| *id == 3),
        "the unknown ad is left out"
    );
    let countries = app
        .store
        .ad_values(today(), today(), 3, "shown", "country")
        .unwrap();
    assert_eq!(countries, [("KZ".to_owned(), 4), ("RU".to_owned(), 4)]);
    let versions = app
        .store
        .ad_values(today(), today(), 3, "picked", "version")
        .unwrap();
    assert_eq!(
        versions,
        [("1.21.11".to_owned(), 2), ("1.21.4".to_owned(), 2)]
    );
    let left = app.store.dropped(today(), today()).unwrap();
    assert!(left.contains(&("unknown_ad".to_owned(), 1)), "{left:?}");
    assert!(left.contains(&("again".to_owned(), 1)), "{left:?}");
}

#[tokio::test]
async fn ad_counts_are_capped_and_malformed_batches_refused() {
    let (_dir, app) = app();
    show(&app, &[3, 4]);
    let mut big = ad("3");
    big["shown"] = json!(100_000);
    big["play"] = json!(4_000_000_000u64);
    big["minutes"] = json!(99_999);
    big["refused"] = json!({ "full": 4_000_000_000u64, "ban": 4_000_000_000u64 });
    let (status, _) = post(&app, "/v1/counts", &counts(json!([big])), "RU").await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    app.flush().unwrap();
    let totals = app.store.ad_totals(today(), today(), Some(3)).unwrap();
    let n = |event: &str| totals.get(&(3, event.to_owned())).copied().unwrap_or(0);
    assert_eq!(n("shown"), i64::from(model::MAX_SHOWN));
    assert_eq!(n("play"), i64::from(model::MAX_PLAYS));
    assert_eq!(n("minutes"), i64::from(model::MAX_MINUTES));
    assert_eq!(n("refused_full"), i64::from(model::MAX_PLAYS));
    let too_many: Vec<Value> = (1..=51).map(|i| ad(&i.to_string())).collect();
    let mut negative = ad("3");
    negative["shown"] = json!(-1);
    let mut flags = ad("3");
    flags["reach"] = json!(["day", "ever", "day"]);
    for (label, body) in [
        ("same ad twice", counts(json!([ad("3"), ad("3")]))),
        ("51 ads", counts(json!(too_many))),
        ("id 0", counts(json!([ad("0")]))),
        ("id with a sign", counts(json!([ad("+3")]))),
        ("long id", counts(json!([ad("1234567890123")]))),
        ("negative", counts(json!([negative]))),
        ("flags", counts(json!([flags]))),
    ] {
        let (status, _) = post(&app, "/v1/counts", &body, "RU").await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{label}");
    }
    // A week-old batch from an offline launcher is fine; older is not.
    let mut week = counts(json!([]));
    week["day"] = json!((today() - Duration::days(7)).to_string());
    let (status, _) = post(&app, "/v1/counts", &rework(week.clone()), "RU").await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    week["day"] = json!((today() - Duration::days(8)).to_string());
    let (status, _) = post(&app, "/v1/counts", &rework(week), "RU").await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn new_values_past_a_limit_go_to_other() {
    let (_dir, app) = app();
    show(&app, &[3]);
    for chunk in (0..crate::store::MAX_AD_VALUES + 20)
        .collect::<Vec<_>>()
        .chunks(20)
    {
        let picked: serde_json::Map<String, Value> =
            chunk.iter().map(|i| (format!("1.{i}"), json!(1))).collect();
        let mut entry = ad("3");
        entry["picked"] = Value::Object(picked);
        let (status, _) = post(&app, "/v1/counts", &counts(json!([entry])), "RU").await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        app.flush().unwrap();
    }
    let versions = app
        .store
        .ad_values(today(), today(), 3, "picked", "version")
        .unwrap();
    assert_eq!(versions.len() as i64, crate::store::MAX_AD_VALUES + 1);
    assert!(versions.contains(&("other".to_owned(), 20)), "{versions:?}");
    let total: i64 = versions.iter().map(|(_, n)| n).sum();
    assert_eq!(total, crate::store::MAX_AD_VALUES + 20, "nothing is lost");
}

#[tokio::test]
async fn bodies_come_gzipped_and_bounded() {
    use std::io::Write;
    let (_dir, app) = app();
    let gzip = |bytes: &[u8]| {
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        gz.write_all(bytes).unwrap();
        gz.finish().unwrap()
    };
    let send = |body: Vec<u8>| {
        Request::post("/v1/ping")
            .header("content-type", "application/json")
            .header("content-encoding", "gzip")
            .header("x-country", "DE")
            .body(Body::from(body))
            .unwrap()
    };
    let body = serde_json::to_vec(&ping(&["day"])).unwrap();
    let (status, _) = answer(&app, send(gzip(&body))).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    // Small packed, huge unpacked: refused once the limit is read.
    let mut bomb = br#"{"schema":1,"batch":""#.to_vec();
    bomb.extend(std::iter::repeat_n(b'a', 4 * 1024 * 1024));
    let packed = gzip(&bomb);
    assert!(packed.len() < 64 * 1024);
    let (status, _) = answer(&app, send(packed)).await;
    assert!(status.is_client_error(), "{status}");
    assert_eq!(values(&app, Period::Day, "country"), [("DE".to_owned(), 1)]);
}

#[tokio::test]
async fn one_address_is_limited_per_hour() {
    let (_dir, app) = app();
    let mut refused = 0;
    let body = json!({});
    for _ in 0..305 {
        let (status, _) = post_from(&app, "/v1/ping", &body, "RU", "198.51.100.9").await;
        if status == StatusCode::TOO_MANY_REQUESTS {
            refused += 1;
        }
    }
    assert_eq!(refused, 5);
    // Another address is not.
    let (status, _) = post_from(&app, "/v1/ping", &body, "RU", "198.51.100.10").await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    // Neither are reads of the health check.
    let request = Request::get("/v1/health").body(Body::empty()).unwrap();
    let (status, health) = answer(&app, request).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(health["service"], "lapi-stats");
}

#[tokio::test]
async fn nothing_can_be_read_from_outside() {
    let (_dir, app) = app();
    for path in ["/", "/v1/stats", "/v1/ads", "/v1/ping/1", "/v1/../stats.db"] {
        let request = Request::get(path).body(Body::empty()).unwrap();
        let (status, _) = answer(&app, request).await;
        assert!(
            status == StatusCode::NOT_FOUND || status == StatusCode::METHOD_NOT_ALLOWED,
            "{path}: {status}"
        );
    }
    let request = Request::get("/v1/ping").body(Body::empty()).unwrap();
    let (status, _) = answer(&app, request).await;
    assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
}

#[test]
fn days_weeks_and_versions_are_read_strictly() {
    assert_eq!(
        model::day("2026-10-22"),
        NaiveDate::from_ymd_opt(2026, 10, 22)
    );
    assert_eq!(model::day("2026-1-22"), None);
    assert_eq!(model::day("2026-02-30"), None);
    let monday = NaiveDate::from_ymd_opt(2026, 10, 5).unwrap();
    assert_eq!(model::week_of(monday), "2026-W41");
    assert_eq!(model::week_start("2026-W41"), Some(monday));
    assert_eq!(
        model::week_of(NaiveDate::from_ymd_opt(2026, 1, 1).unwrap()),
        "2026-W01"
    );
    assert_eq!(model::week_start("2026-W54"), None);
    assert_eq!(model::week_start("2026-W4"), None);
    for good in ["1.4.0", "0.1.0", "1.4.0-beta.2", "10.20.30"] {
        assert_eq!(model::launcher_version(good), good);
    }
    for bad in [
        "1.4",
        "1.4.0-",
        "1.4.0-Beta",
        "1.4.0.1",
        "v1.4.0",
        "1.4.0-a b",
        "123456.0.0",
    ] {
        assert_eq!(model::launcher_version(bad), "other", "{bad}");
    }
    assert_eq!(model::country(" ru "), "RU");
    assert_eq!(model::country("T1"), "T1");
    assert_eq!(model::country("RUS"), "XX");
    assert_eq!(model::country("R\u{0}"), "XX");
}

#[test]
fn a_ping_reads_its_periods_from_its_flags() {
    let mut body = Ping {
        schema: 1,
        batch: batch(),
        day: today().to_string(),
        first: vec!["day".into(), "month".into()],
        cohort: "old".into(),
        ..Ping::default()
    };
    let read = body.read("RU", today()).unwrap();
    assert_eq!(read.periods, [Period::Day, Period::Month]);
    assert!(read.cuts.contains(&("cohort", "old/RU".to_owned())));
    assert!(read.cuts.contains(&("accounts", "none/RU".to_owned())));
    body.cohort = "2024-W10".into();
    let read = body.read("RU", today()).unwrap();
    assert!(read.cuts.contains(&("cohort", "other/RU".to_owned())));
    let counts = Counts {
        schema: 1,
        batch: batch(),
        day: today().to_string(),
        ..Counts::default()
    };
    assert!(counts.read(today()).unwrap().ads.is_empty());
}

fn day_ping(app: &App, day: NaiveDate, first: &[&str], cohort: &str, country: &str) {
    let ping = Ping {
        schema: 1,
        batch: batch(),
        day: day.to_string(),
        first: first.iter().map(|f| (*f).to_owned()).collect(),
        cohort: cohort.to_owned(),
        ..Ping::default()
    };
    app.tally.ping(&ping.read(country, day).unwrap());
}

#[test]
fn reports_add_the_days_up() {
    let (_dir, app) = app();
    let cohort_day = NaiveDate::from_ymd_opt(2026, 9, 14).unwrap(); // 2026-W38
    for i in 0..4 {
        day_ping(
            &app,
            cohort_day,
            &["day", "week", "month", "ever"],
            "2026-W38",
            if i == 0 { "KZ" } else { "RU" },
        );
    }
    // Four weeks later two are back.
    let later = cohort_day + Duration::days(28);
    for _ in 0..2 {
        day_ping(&app, later, &["day", "week", "month"], "2026-W38", "RU");
    }
    for _ in 0..3 {
        day_ping(&app, today(), &["day"], "old", "RU");
    }
    app.flush().unwrap();
    let overview = report::overview(&app.store, today(), 30).unwrap();
    assert!(overview.contains("today 2026-10-22 (so far)"), "{overview}");
    assert!(overview.contains("still there after 4 weeks"), "{overview}");
    let retention = report::retention(&app.store, today(), 6, None, true).unwrap();
    let w38 = retention
        .lines()
        .find(|l| l.starts_with("2026-W38"))
        .unwrap();
    assert_eq!(w38, "2026-W38,4,0,0,2,,");
    let kz = report::retention(&app.store, today(), 6, Some("KZ"), true).unwrap();
    assert!(kz.lines().any(|l| l == "2026-W38,1,0,0,0,,"), "{kz}");
    let players = report::players(&app.store, today(), 1, true).unwrap();
    assert_eq!(
        players,
        "day,players,first_of_week,first_of_month,new\n2026-10-22,3,0,0,0\n"
    );
    let countries = report::cut(&app.store, later, ("country", Period::Day), 60, 30, true).unwrap();
    assert!(countries.contains("RU,5"), "{countries}");
}

#[tokio::test]
async fn ord_sums_impressions_per_erid_with_russia_apart() {
    let (_dir, app) = app();
    show(&app, &[3, 4]);
    for country in ["RU", "RU", "KZ"] {
        let (status, _) = post(
            &app,
            "/v1/counts",
            &counts(json!([ad("3"), ad("4")])),
            country,
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
    }
    app.flush().unwrap();
    let text = report::ord(&app.store, "2026-10", true).unwrap();
    assert_eq!(
        text,
        "erid,ads,shown,shown_in_russia\n,,12,8\n2SDnjeXYZ,3,12,8\n"
    );
    assert!(report::ord(&app.store, "2026-13", false).is_err());
    let ads = report::ads(&app.store, today(), 30, false).unwrap();
    assert!(ads.contains("Server 3"), "{ads}");
    let one = report::ad(&app.store, today(), (3, 30), Err("not asked".into()), false).unwrap();
    assert!(one.contains("joined the server"), "{one}");
    assert!(
        one.contains("Countries (shown): RU 66.7%, KZ 33.3%"),
        "{one}"
    );
}

#[test]
fn csv_fields_are_never_formulas() {
    let (_dir, app) = app();
    let shown = [ShownAd {
        id: 5,
        name: "=HYPERLINK(\"x\"),\"y\"".into(),
        erid: None,
    }];
    app.store.save_ads(today(), &shown).unwrap();
    let csv = report::ads(&app.store, today(), 1, true).unwrap();
    assert!(!csv.contains("\n=") && !csv.contains(",="), "{csv}");
}

#[test]
fn old_batches_go_and_old_days_are_summed_by_month() {
    let (_dir, app) = app();
    let long_ago = today() - Duration::days(800);
    day_ping(&app, long_ago, &["day", "month"], "old", "RU");
    day_ping(&app, long_ago + Duration::days(1), &["day"], "old", "RU");
    app.flush().unwrap();
    app.store
        .first_time(
            uuid::Uuid::parse_str(&batch()).unwrap(),
            today() - Duration::days(9),
        )
        .unwrap();
    app.store
        .first_time(uuid::Uuid::parse_str(&batch()).unwrap(), today())
        .unwrap();
    app.store.prune(today()).unwrap();
    assert_eq!(app.store.rows("batches"), 1);
    assert_eq!(app.store.rows("players"), 0);
    assert_eq!(app.store.rows("players_monthly"), 9 * 2);
}

#[test]
fn the_ads_list_is_read_by_its_rules() {
    let answer = json!({ "now": 1, "ads": [
        { "id": "3", "name": "North\u{202E}ern\u{0007} Land", "erid": "2SDnjeXYZ" },
        { "id": "4", "name": "No erid", "erid": null },
        { "id": "0", "name": "zero" },
        { "id": "x5", "name": "letters" },
        { "id": "6", "name": "bad erid", "erid": "2SD njeXYZ" },
        { "id": 7, "name": "number id" },
        { "id": "8" },
    ]});
    let ads = ads::parse(&answer);
    assert_eq!(
        ads,
        [
            ShownAd {
                id: 3,
                name: "Northern Land".into(),
                erid: Some("2SDnjeXYZ".into())
            },
            ShownAd {
                id: 4,
                name: "No erid".into(),
                erid: None
            },
        ]
    );
    assert!(ads::parse(&json!({ "ads": "no" })).is_empty());
}

#[test]
fn an_ad_is_known_the_day_it_showed_and_the_days_around() {
    let (_dir, app) = app();
    assert!(!app.shown.fresh());
    show(&app, &[3]);
    assert!(app.shown.fresh());
    assert!(app.shown.known(3, today()));
    assert!(app.shown.known(3, today() + Duration::days(1)));
    assert!(app.shown.known(3, today() - Duration::days(1)));
    assert!(!app.shown.known(3, today() - Duration::days(2)));
    assert!(!app.shown.known(4, today()));
    // A restart reads the lists back.
    let again = crate::ads::Shown::load(&app.store, today()).unwrap();
    assert!(again.known(3, today()) && !again.fresh());
}
