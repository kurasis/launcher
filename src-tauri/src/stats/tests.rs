use std::sync::{Arc, Mutex};
use std::time::{Duration as Wait, Instant};

use super::*;
use crate::state::dto::{MutationRequest, SettingsPatch};
use crate::validation::MemoryPolicy;

/// Records what would be sent and answers from a list, then with 204.
/// Status 0 stands for no connection.
#[derive(Clone, Default)]
struct Fake {
    sent: Arc<Mutex<Vec<(String, Value)>>>,
    answers: Arc<Mutex<Vec<(u16, Value)>>>,
}

impl Fake {
    fn answer(&self, status: u16, body: Value) {
        self.answers.lock().unwrap().push((status, body));
    }

    fn bodies(&self, path: &str) -> Vec<Value> {
        self.sent
            .lock()
            .unwrap()
            .iter()
            .filter(|(p, _)| p == path)
            .map(|(_, body)| body.clone())
            .collect()
    }

    fn count(&self) -> usize {
        self.sent.lock().unwrap().len()
    }
}

impl Transport for Fake {
    fn post<'a>(
        &'a self,
        path: &'a str,
        gzip_json: Vec<u8>,
    ) -> Boxed<'a, Result<(u16, Vec<u8>), String>> {
        Box::pin(async move {
            let mut json = Vec::new();
            std::io::Read::read_to_end(
                &mut flate2::read::GzDecoder::new(&gzip_json[..]),
                &mut json,
            )
            .unwrap();
            self.sent
                .lock()
                .unwrap()
                .push((path.to_owned(), serde_json::from_slice(&json).unwrap()));
            let next = {
                let mut answers = self.answers.lock().unwrap();
                (!answers.is_empty()).then(|| answers.remove(0))
            };
            match next {
                Some((0, _)) => Err("connect".to_owned()),
                Some((status, body)) => Ok((status, serde_json::to_vec(&body).unwrap())),
                None => Ok((204, Vec::new())),
            }
        })
    }
}

/// The tests' time.
#[derive(Clone)]
struct Clock(Arc<Mutex<DateTime<Utc>>>);

impl Clock {
    fn set(&self, at: &str) {
        *self.0.lock().unwrap() = at.parse().unwrap();
    }

    fn advance(&self, minutes: i64) {
        *self.0.lock().unwrap() += chrono::Duration::minutes(minutes);
    }
}

/// A launcher on Thursday 2026-10-22 (ISO week 43), 16 GiB of memory.
fn setup() -> (tempfile::TempDir, AppState, Arc<Stats>, Fake, Clock) {
    let dir = tempfile::tempdir().unwrap();
    let root = DataRoot::new(dir.path().join("data")).unwrap();
    let state = AppState::initialize(
        Ok(root),
        "1.4.0",
        MemoryPolicy::from_physical_bytes(16 * 1024 * 1024 * 1024),
        Box::new(crate::state::NoopSink),
    );
    let fake = Fake::default();
    let clock = Clock(Arc::new(Mutex::new(
        "2026-10-22T12:00:00Z".parse().unwrap(),
    )));
    let time = clock.clone();
    let stats = Arc::new(Stats::with_clock(
        Box::new(fake.clone()),
        Box::new(move || *time.0.lock().unwrap()),
    ));
    // Light work: the tests do not wait for 20 bits.
    stats.edit(&state.data_root().unwrap(), |saved| {
        saved.bits = Some(8);
        (true, ())
    });
    (dir, state, stats, fake, clock)
}

fn set(state: &AppState, patch: SettingsPatch) {
    state
        .set_app_settings(MutationRequest {
            request_id: uuid::Uuid::new_v4().to_string(),
            payload: patch,
        })
        .unwrap();
}

/// The file as it is on disk.
fn on_disk(state: &AppState) -> Saved {
    read(&state_file(&state.data_root().unwrap()))
}

fn total(state: &AppState, day: &str, ad: &str) -> AdCounts {
    on_disk(state)
        .days
        .get(day)
        .and_then(|d| d.total.get(ad))
        .cloned()
        .unwrap_or_default()
}

fn worked(body: &Value, bits: u32) -> bool {
    let text = |key: &str| body[key].as_str().unwrap().to_owned();
    let digest = Sha256::digest(
        format!(
            "stats.lapi.sh|{}|{}|{}",
            text("batch"),
            text("day"),
            text("nonce")
        )
        .as_bytes(),
    );
    zeros(&digest) >= bits
}

#[tokio::test]
async fn a_new_launcher_counts_once_a_day_with_the_flags_of_its_first_times() {
    let (_dir, state, stats, fake, clock) = setup();
    stats.init(&state);
    stats.send(&state).await;
    let pings = fake.bodies("/v1/ping");
    assert_eq!(pings.len(), 1);
    assert_eq!(pings[0]["first"], json!(["day", "week", "month", "ever"]));
    assert_eq!(pings[0]["cohort"], "2026-W43");
    assert_eq!(pings[0]["day"], "2026-10-22");
    assert_eq!(pings[0]["schema"], 1);
    assert!(worked(&pings[0], 8));

    // The same day again: nothing.
    clock.advance(180);
    stats.send(&state).await;
    assert_eq!(fake.bodies("/v1/ping").len(), 1);

    for (at, first) in [
        ("2026-10-23T09:00:00Z", json!(["day"])),
        // Monday: a new week.
        ("2026-10-26T09:00:00Z", json!(["day", "week"])),
        // Sunday of that week, and a new month.
        ("2026-11-01T09:00:00Z", json!(["day", "month"])),
        ("2026-11-09T09:00:00Z", json!(["day", "week"])),
    ] {
        clock.set(at);
        stats.send(&state).await;
        let ping = fake.bodies("/v1/ping").pop().unwrap();
        assert_eq!(ping["first"], first, "{at}");
        assert_eq!(ping["day"], &at[..10]);
        // The install week stays the same: it gives the retention.
        assert_eq!(ping["cohort"], "2026-W43");
    }
    assert_eq!(fake.bodies("/v1/ping").len(), 5);
}

#[tokio::test]
async fn a_launcher_that_ran_before_is_old_and_never_new() {
    let (_dir, state, stats, fake, _clock) = setup();
    state
        .edit_settings(|s| s.last_version = Some("1.3.0".into()))
        .unwrap();
    stats.init(&state);
    stats.send(&state).await;
    let ping = &fake.bodies("/v1/ping")[0];
    assert_eq!(ping["first"], json!(["day", "week", "month"]));
    assert_eq!(ping["cohort"], "old");
    // A second start does not change the week.
    stats.init(&state);
    assert_eq!(on_disk(&state).cohort.as_deref(), Some("old"));
}

#[tokio::test]
async fn a_ping_says_nothing_that_names_the_computer_or_the_player() {
    let (dir, state, stats, fake, clock) = setup();
    stats.init(&state);
    stats.send(&state).await;
    let ping = fake.bodies("/v1/ping").pop().unwrap();
    let mut keys: Vec<&str> = ping
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        [
            "accounts",
            "arch",
            "batch",
            "bundle",
            "channel",
            "cohort",
            "cores",
            "day",
            "first",
            "launcher",
            "nonce",
            "os",
            "osVersion",
            "ram",
            "schema",
            "system",
            "tz",
            "ui",
        ]
    );
    assert_eq!(ping["launcher"], "1.4.0");
    assert_eq!(ping["channel"], "stable");
    assert_eq!(ping["ui"], "en");
    assert_eq!(ping["ram"], "16-32");
    assert_eq!(ping["accounts"], json!([]));
    assert_eq!(ping["os"], system::os(std::env::consts::OS));

    // A player name never goes, only that there is such a profile.
    state
        .update_accounts_file(Box::new(|accounts| {
            accounts
                .notoken_profiles
                .push(crate::storage::models::NoTokenRecord {
                    id: uuid::Uuid::new_v4().to_string(),
                    name: "Steve_Secret".into(),
                });
        }))
        .unwrap();
    clock.set("2026-10-23T09:00:00Z");
    stats.send(&state).await;
    let ping = fake.bodies("/v1/ping").pop().unwrap();
    assert_eq!(ping["accounts"], json!(["notoken"]));
    let text = ping.to_string();
    assert!(!text.contains("Steve_Secret"));
    assert!(!text.contains(&*dir.path().to_string_lossy()));
}

#[tokio::test]
async fn ad_counts_go_in_hourly_batches() {
    let (_dir, state, stats, fake, clock) = setup();
    stats.init(&state);
    stats.ad_event(&state, "7", AdEvent::Shown);
    // Still in view a minute later: the same impression.
    clock.advance(1);
    stats.ad_event(&state, "7", AdEvent::Shown);
    // Five minutes after it counted: a new one.
    clock.advance(5);
    stats.ad_event(&state, "7", AdEvent::Shown);
    stats.ad_event(&state, "7", AdEvent::Menu);
    stats.ad_event(&state, "7", AdEvent::VersionList);
    stats.ad_event(&state, "7", AdEvent::Play);
    stats.ad_event(&state, "8", AdEvent::Shown);
    stats.ad_event(&state, "8", AdEvent::Copy);
    stats.send(&state).await;
    let batches = fake.bodies("/v1/counts");
    assert_eq!(batches.len(), 1);
    let batch = &batches[0];
    assert!(worked(batch, 8));
    assert_eq!(batch["day"], "2026-10-22");
    assert_eq!(batch["ui"], "en");
    assert_eq!(
        batch["ads"],
        json!([
            { "ad": "7", "shown": 2, "reach": ["day", "ever"], "menu": 1, "versionList": 1, "play": 1 },
            { "ad": "8", "shown": 1, "reach": ["day", "ever"], "copy": 1 },
        ])
    );

    // Counted since: they wait for the hour.
    clock.advance(10);
    stats.ad_event(&state, "7", AdEvent::Shown);
    stats.send(&state).await;
    assert_eq!(fake.bodies("/v1/counts").len(), 1);
    clock.advance(55);
    stats.send(&state).await;
    let batches = fake.bodies("/v1/counts");
    assert_eq!(batches.len(), 2);
    // Seen already today: no reach.
    assert_eq!(batches[1]["ads"], json!([{ "ad": "7", "shown": 1 }]));

    // The next day it reaches this launcher again, but not for the first time.
    clock.set("2026-10-23T09:00:00Z");
    stats.ad_event(&state, "7", AdEvent::Shown);
    stats.send(&state).await;
    let last = fake.bodies("/v1/counts").pop().unwrap();
    assert_eq!(last["day"], "2026-10-23");
    assert_eq!(
        last["ads"],
        json!([{ "ad": "7", "shown": 1, "reach": ["day"] }])
    );
}

#[tokio::test]
async fn an_ads_day_stays_within_the_limits() {
    let (_dir, state, stats, _fake, _clock) = setup();
    for _ in 0..400 {
        stats.count(&state, "7", &Change::Event(AdEvent::Shown));
        stats.count(&state, "7", &Change::Event(AdEvent::Report));
        stats.count(&state, "7", &Change::Event(AdEvent::Play));
        stats.count(&state, "7", &Change::Refused("full"));
        stats.count(&state, "7", &Change::Played(100));
    }
    for n in 0..30 {
        stats.count(&state, "7", &Change::Started(&format!("1.21.{n}")));
    }
    let counted = total(&state, "2026-10-22", "7");
    assert_eq!(counted.shown, MAX_SHOWN);
    assert_eq!(counted.report, MAX_CLICKS);
    assert_eq!(counted.play, MAX_PLAYS);
    assert_eq!(counted.refused["full"], MAX_PLAYS);
    assert_eq!(counted.minutes, MAX_MINUTES);
    assert_eq!(counted.played, MAX_PLAYS);
    assert_eq!(counted.started, MAX_PLAYS);
    assert_eq!(counted.picked.len(), MAX_PLAYS as usize);
    // A version that is not one is never counted.
    stats.count(&state, "9", &Change::Started("../../x"));
    let other = total(&state, "2026-10-22", "9");
    assert_eq!((other.started, other.picked.len()), (1, 0));
}

#[tokio::test]
async fn play_again_a_day_and_a_week_later_counts_once_each() {
    let (_dir, state, stats, _fake, clock) = setup();
    let back = |state: &AppState, day: &str| total(state, day, "7").back;
    stats.ad_event(&state, "7", AdEvent::Play);
    stats.ad_event(&state, "7", AdEvent::Play);
    assert!(back(&state, "2026-10-22").is_empty());
    clock.set("2026-10-23T10:00:00Z");
    stats.ad_event(&state, "7", AdEvent::Play);
    assert_eq!(back(&state, "2026-10-23"), ["day"]);
    clock.set("2026-10-24T10:00:00Z");
    stats.ad_event(&state, "7", AdEvent::Play);
    assert!(back(&state, "2026-10-24").is_empty());
    clock.set("2026-10-30T10:00:00Z");
    stats.ad_event(&state, "7", AdEvent::Play);
    assert_eq!(back(&state, "2026-10-30"), ["week"]);
    clock.set("2026-11-06T10:00:00Z");
    stats.ad_event(&state, "7", AdEvent::Play);
    assert!(back(&state, "2026-11-06").is_empty());
}

#[tokio::test]
async fn a_run_from_an_ad_counts_its_join_refusal_and_minutes() {
    let (_dir, state, stats, _fake, _clock) = setup();
    let connect = "[12:00:01] [Render thread/INFO]: Connecting to play.example.net, 25565";
    let minutes = |m: u64| Wait::from_secs(m * 60);
    let t0 = Instant::now();

    // In through the chat, 25 minutes.
    let run = stats.ad_run("7");
    run.started(&state, "1.21.4");
    run.line_at(&state, connect, t0);
    run.line_at(
        &state,
        "[Render thread/INFO]: [System] [CHAT] Welcome",
        t0 + minutes(1),
    );
    run.line_at(
        &state,
        "[Render thread/INFO]: [System] [CHAT] Hi again",
        t0 + minutes(2),
    );
    run.ended_at(&state, t0 + minutes(25));
    run.ended_at(&state, t0 + minutes(90));
    let counted = total(&state, "2026-10-22", "7");
    assert_eq!(
        (
            counted.started,
            counted.joined,
            counted.minutes,
            counted.played
        ),
        (1, 1, 25, 1)
    );
    assert_eq!(counted.picked["1.21.4"], 1);

    // Turned away for its version: a refusal, no join, no minutes.
    let run = stats.ad_run("7");
    run.line_at(&state, connect, t0);
    run.line_at(
        &state,
        "[Render thread/WARN]: Client disconnected with reason: Outdated client! Please use 1.21.4",
        t0 + Wait::from_secs(2),
    );
    run.ended_at(&state, t0 + minutes(10));
    let counted = total(&state, "2026-10-22", "7");
    assert_eq!(counted.refused, BTreeMap::from([("version".to_owned(), 1)]));
    assert_eq!((counted.joined, counted.minutes), (1, 25));

    // Quiet, but two minutes without a refusal: in, 4 minutes, not played.
    let run = stats.ad_run("7");
    run.line_at(&state, connect, t0);
    run.line_at(
        &state,
        "[Render thread/INFO]: Loaded 12 advancements",
        t0 + minutes(3),
    );
    run.ended_at(&state, t0 + minutes(4));
    let counted = total(&state, "2026-10-22", "7");
    assert_eq!(
        (counted.joined, counted.minutes, counted.played),
        (2, 29, 1)
    );

    // Kicked after 12 minutes, the game kept open: the time ends there.
    let run = stats.ad_run("7");
    run.line_at(&state, connect, t0);
    run.line_at(
        &state,
        "[Render thread/INFO]: [CHAT] Hello",
        t0 + minutes(1),
    );
    run.line_at(
        &state,
        "[Render thread/WARN]: Client disconnected with reason: Kicked for spamming",
        t0 + minutes(12),
    );
    run.ended_at(&state, t0 + minutes(60));
    let counted = total(&state, "2026-10-22", "7");
    assert_eq!(
        (counted.joined, counted.minutes, counted.played),
        (3, 41, 2)
    );
    assert_eq!(counted.refused.values().sum::<u32>(), 1);

    // Closed at the menu, never connected: nothing.
    let run = stats.ad_run("7");
    run.ended_at(&state, t0 + minutes(30));
    let counted = total(&state, "2026-10-22", "7");
    assert_eq!((counted.joined, counted.minutes), (3, 41));
}

#[tokio::test]
async fn turning_off_stops_sending_and_erases_but_keeps_the_dates() {
    let (_dir, state, stats, fake, clock) = setup();
    stats.init(&state);
    stats.send(&state).await;
    stats.ad_event(&state, "7", AdEvent::Shown);
    stats.ad_event(&state, "7", AdEvent::Play);
    stats.seal(&state);
    assert_eq!(on_disk(&state).batches.len(), 1);

    set(
        &state,
        SettingsPatch {
            stats: Some(false),
            ..SettingsPatch::default()
        },
    );
    stats.forget(&state);
    let saved = on_disk(&state);
    assert!(saved.batches.is_empty() && saved.pings.is_empty());
    assert!(saved.days.is_empty() && saved.seen.is_empty() && saved.played.is_empty());
    assert!(saved.last_ping.is_none() && saved.last_counts.is_none());
    assert_eq!(saved.cohort.as_deref(), Some("2026-W43"));
    assert_eq!(saved.sent_day.as_deref(), Some("2026-10-22"));
    assert!(saved.ever_sent);

    // Off: nothing is counted or sent.
    let before = fake.count();
    clock.set("2026-10-23T09:00:00Z");
    stats.ad_event(&state, "7", AdEvent::Shown);
    stats.send(&state).await;
    assert_eq!(fake.count(), before);
    assert!(on_disk(&state).days.is_empty());
    assert_eq!(stats.sent(&state).next, None);

    // On again: not a new install.
    set(
        &state,
        SettingsPatch {
            stats: Some(true),
            ..SettingsPatch::default()
        },
    );
    stats.send(&state).await;
    let ping = fake.bodies("/v1/ping").pop().unwrap();
    assert_eq!(ping["first"], json!(["day"]));
}

#[tokio::test]
async fn more_work_is_done_when_the_service_asks_for_it() {
    let (_dir, state, stats, fake, _clock) = setup();
    stats.init(&state);
    fake.answer(422, json!({ "error": "work", "bits": 12 }));
    stats.send(&state).await;
    let pings = fake.bodies("/v1/ping");
    assert_eq!(pings.len(), 2);
    assert_eq!(pings[0]["batch"], pings[1]["batch"]);
    assert!(worked(&pings[1], 12));
    let saved = on_disk(&state);
    assert_eq!(saved.bits, Some(12));
    assert_eq!(saved.sent_day.as_deref(), Some("2026-10-22"));

    // More than this launcher does: it waits, it does not drop.
    stats.ad_event(&state, "7", AdEvent::Shown);
    stats.seal(&state);
    fake.answer(422, json!({ "error": "work", "bits": 40 }));
    stats.send(&state).await;
    assert_eq!(on_disk(&state).batches.len(), 1);
}

#[tokio::test]
async fn refusals_are_dropped_and_failures_wait_with_their_batch_and_work() {
    let (_dir, state, stats, fake, _clock) = setup();
    stats.init(&state);
    stats.ad_event(&state, "7", AdEvent::Shown);
    // The service is down: the pass stops at the ping.
    fake.answer(503, json!({ "error": "unavailable" }));
    stats.send(&state).await;
    assert_eq!(fake.count(), 1);
    let first = fake.bodies("/v1/ping")[0].clone();
    // No connection: the same again.
    fake.answer(0, Value::Null);
    stats.send(&state).await;
    // The counts come back as made up for an unknown day: dropped.
    fake.answer(204, Value::Null);
    fake.answer(422, json!({ "error": "day" }));
    stats.send(&state).await;
    let pings = fake.bodies("/v1/ping");
    assert_eq!(pings.len(), 3);
    for ping in &pings {
        assert_eq!(ping["batch"], first["batch"]);
        assert_eq!(ping["nonce"], first["nonce"]);
    }
    assert_eq!(fake.bodies("/v1/counts").len(), 1);
    let saved = on_disk(&state);
    assert!(saved.batches.is_empty() && saved.pings.is_empty());
    assert!(saved.last_counts.is_none(), "a refused batch was not sent");
    assert_eq!(saved.last_ping.as_ref(), Some(&pings[2]));
}

#[tokio::test]
async fn what_the_service_no_longer_takes_is_dropped() {
    let (_dir, state, stats, fake, clock) = setup();
    stats.init(&state);
    stats.ad_event(&state, "7", AdEvent::Shown);
    stats.seal(&state);
    // Eight days offline.
    clock.set("2026-10-30T12:00:00Z");
    stats.send(&state).await;
    assert!(fake.bodies("/v1/counts").is_empty());
    let pings = fake.bodies("/v1/ping");
    assert_eq!(pings.len(), 1);
    assert_eq!(pings[0]["day"], "2026-10-30");
    assert!(on_disk(&state).batches.is_empty());
}

#[tokio::test]
async fn what_is_sent_shows_the_last_ones_as_they_went() {
    let (_dir, state, stats, fake, _clock) = setup();
    stats.init(&state);
    let before = stats.sent(&state);
    assert_eq!((before.ping.as_ref(), before.counts.as_ref()), (None, None));
    let next = before.next.unwrap();
    assert_eq!(next["first"], json!(["day", "week", "month", "ever"]));
    assert!(next.get("nonce").is_none());

    stats.ad_event(&state, "7", AdEvent::Shown);
    stats.send(&state).await;
    let after = stats.sent(&state);
    assert_eq!(after.ping, fake.bodies("/v1/ping").pop());
    assert_eq!(after.counts, fake.bodies("/v1/counts").pop());
    assert_eq!((after.next, after.waiting), (None, 0));
}

#[tokio::test]
async fn only_ad_numbers_count() {
    let (_dir, state, stats, _fake, _clock) = setup();
    for id in ["", "0", "07", "../7", "7 ", "-7", "1234567890123", "７"] {
        stats.ad_event(&state, id, AdEvent::Shown);
        stats.count(&state, id, &Change::Joined);
    }
    assert!(on_disk(&state).days.is_empty());
    assert!(ad_id("7") && ad_id("123456789012"));
}

#[test]
fn a_damaged_file_is_read_within_its_limits() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    fs::write(&path, b"{not json").unwrap();
    assert!(read(&path).pings.is_empty());

    let good = |n: usize| Waiting::new("2026-10-22", None).batch + &"x".repeat(n);
    let pings: Vec<Value> = (0..50)
        .map(|_| json!({ "batch": good(0), "day": "2026-10-22" }))
        .chain([
            json!({ "batch": good(1), "day": "2026-10-22" }),
            json!({ "batch": good(0), "day": "yesterday" }),
            json!({ "batch": good(0), "day": "2026-10-22", "work": [99, "1"] }),
            json!({ "batch": good(0), "day": "2026-10-22", "counts": "x" }),
        ])
        .collect();
    let seen: serde_json::Map<String, Value> = (1..5000)
        .map(|n| (n.to_string(), json!("2026-10-22")))
        .chain([("../x".to_owned(), json!("2026-10-22"))])
        .collect();
    fs::write(
        &path,
        serde_json::to_vec(&json!({
            "cohort": "W43; DROP TABLE",
            "sentDay": "not a day",
            "bits": 200,
            "pings": pings,
            "seen": seen,
            "days": { "x": {}, "2026-10-22": { "open": { "../7": { "shown": 3 } } } },
        }))
        .unwrap(),
    )
    .unwrap();
    let saved = read(&path);
    assert_eq!(saved.pings.len(), 4);
    assert_eq!(saved.seen.len(), MAX_REMEMBERED);
    assert!(!saved.seen.contains_key("../x"));
    assert_eq!(saved.cohort.as_deref(), Some("old"));
    assert_eq!((saved.sent_day, saved.bits), (None, None));
    assert_eq!(saved.days.len(), 1);
    assert!(saved.days["2026-10-22"].open.is_empty());
}

#[test]
fn answers_are_read_by_their_status() {
    assert_eq!(answer(204, b""), Answer::Taken);
    assert_eq!(
        answer(422, br#"{"error":"work","bits":22}"#),
        Answer::Work(22)
    );
    assert_eq!(answer(422, br#"{"error":"work"}"#), Answer::Work(u32::MAX));
    assert_eq!(answer(422, br#"{"error":"day"}"#), Answer::Refused);
    assert_eq!(answer(422, b"<html>"), Answer::Refused);
    assert_eq!(answer(413, b""), Answer::Refused);
    for status in [403, 404, 429, 500, 503] {
        assert_eq!(answer(status, b""), Answer::Later, "{status}");
    }
}

#[test]
fn only_the_statistics_service_is_asked() {
    let ok = |url: &str| crate::net::SourcePolicy::Stats.allows(&url::Url::parse(url).unwrap());
    assert!(ok("https://stats.lapi.sh/v1/ping"));
    assert!(!ok("http://stats.lapi.sh/v1/ping"));
    assert!(!ok("https://stats.lapi.sh.evil.example/v1/ping"));
    assert!(!ok("https://crash.lapi.sh/v1/stats"));
    assert!(!crate::net::SourcePolicy::Stats.mirrored());
}
