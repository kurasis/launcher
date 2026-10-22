//! Anonymous statistics without IDs (D-071), sent to stats.lapi.sh: once a
//! UTC day a ping with flags (the first of the day, the week, the month,
//! ever) and the week this launcher was installed, and the counts of the
//! server ads (D-069) the player saw and played. Nothing names the computer
//! or the player: no ID, account, nick, address, path or text of a server;
//! the service adds the country. On by default; Settings → Privacy turns
//! them off, which stops sending and erases what waits here. The day of the
//! last ping and the install week stay, so turning them on again is not a
//! new install.
//!
//! Everything waits in `stats/state.json` until the service has it: the
//! pings of today and yesterday, and the ad counts, sealed into a batch every
//! hour and at exit and kept up to 7 days. Each request carries a random
//! batch ID, so a resend after a lost answer counts once, and a small proof
//! of work. The player sees the last ones as they went in Settings →
//! Privacy → What is sent.

pub mod run;
pub mod system;

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::{DateTime, Datelike, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::error::LauncherResult;
use crate::state::AppState;
use crate::state::dto::NetworkStatus;
use crate::storage::paths::DataRoot;
use crate::sync::lock;

pub use run::AdRun;

pub const BASE: &str = "https://stats.lapi.sh";
const SCHEMA: u32 = 1;
/// How often what waits is tried.
pub const EVERY: Duration = Duration::from_secs(10 * 60);
/// Ad counts go into a batch this often, and at exit.
const SEAL_EVERY_SECS: i64 = 3600;
/// Batches the service still takes: counts of the last 7 days.
const KEEP_DAYS: i64 = 7;
const MAX_BATCHES: usize = 200;
/// Ads in one batch (the service's limit).
const MAX_ADS: usize = 50;
/// What one ad may count here in a day (the service's limits of a batch).
const MAX_SHOWN: u32 = 300;
const MAX_CLICKS: u32 = 50;
const MAX_PLAYS: u32 = 20;
const MAX_MINUTES: u32 = 24 * 60;
/// Ten minutes on a server count as having played there.
const PLAYED_MINUTES: u32 = 10;
/// The same ad counts as shown again 5 minutes later at the earliest.
const SHOWN_AGAIN_SECS: i64 = 5 * 60;
/// Ads remembered for "seen today", "seen before" and "played before".
const MAX_REMEMBERED: usize = 1000;
const REMEMBER_DAYS: i64 = 400;
/// The work asked for unless the service says otherwise, and the most this
/// launcher does (a few seconds).
const DEFAULT_BITS: u32 = 20;
const MAX_BITS: u32 = 24;
const MAX_ANSWER_BYTES: usize = 16 * 1024;
const MAX_FILE_BYTES: u64 = 8 * 1024 * 1024;

/// A press or a view in a server ad's tile, as the interface tells it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AdEvent {
    /// Half of the tile was in view for a second.
    Shown,
    /// The ⋯ menu opened.
    Menu,
    /// The list of the server's versions opened.
    VersionList,
    Copy,
    Report,
    Play,
    /// Play stopped: the server wants a Microsoft account.
    License,
    /// The version Play needed was installed, or its install failed.
    Installed,
    InstallFailed,
}

fn zero(n: &u32) -> bool {
    *n == 0
}

/// What one ad counted on one day, in the service's words.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AdCounts {
    #[serde(default, skip_serializing_if = "zero")]
    pub shown: u32,
    /// `day`: seen here for the first time today; `ever`: at all.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reach: Vec<String>,
    #[serde(default, skip_serializing_if = "zero")]
    pub menu: u32,
    #[serde(default, skip_serializing_if = "zero")]
    pub version_list: u32,
    #[serde(default, skip_serializing_if = "zero")]
    pub copy: u32,
    #[serde(default, skip_serializing_if = "zero")]
    pub report: u32,
    #[serde(default, skip_serializing_if = "zero")]
    pub play: u32,
    #[serde(default, skip_serializing_if = "zero")]
    pub license: u32,
    #[serde(default, skip_serializing_if = "zero")]
    pub installed: u32,
    #[serde(default, skip_serializing_if = "zero")]
    pub install_failed: u32,
    #[serde(default, skip_serializing_if = "zero")]
    pub started: u32,
    #[serde(default, skip_serializing_if = "zero")]
    pub joined: u32,
    /// Why the server turned the game away, one word each.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub refused: BTreeMap<String, u32>,
    /// Ten minutes on the server or more.
    #[serde(default, skip_serializing_if = "zero")]
    pub played: u32,
    #[serde(default, skip_serializing_if = "zero")]
    pub minutes: u32,
    /// `day`, `week`: Play pressed again a day or a week after the first time.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub back: Vec<String>,
    /// The versions started, with how often.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub picked: BTreeMap<String, u32>,
}

impl AdCounts {
    fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    fn add(&mut self, other: &Self) {
        let sum = |a: &mut u32, b: u32| *a = a.saturating_add(b);
        sum(&mut self.shown, other.shown);
        sum(&mut self.menu, other.menu);
        sum(&mut self.version_list, other.version_list);
        sum(&mut self.copy, other.copy);
        sum(&mut self.report, other.report);
        sum(&mut self.play, other.play);
        sum(&mut self.license, other.license);
        sum(&mut self.installed, other.installed);
        sum(&mut self.install_failed, other.install_failed);
        sum(&mut self.started, other.started);
        sum(&mut self.joined, other.joined);
        sum(&mut self.played, other.played);
        sum(&mut self.minutes, other.minutes);
        for (flags, more) in [
            (&mut self.reach, &other.reach),
            (&mut self.back, &other.back),
        ] {
            for flag in more {
                if !flags.contains(flag) {
                    flags.push(flag.clone());
                }
            }
        }
        for (map, more) in [
            (&mut self.refused, &other.refused),
            (&mut self.picked, &other.picked),
        ] {
            for (key, n) in more {
                sum(map.entry(key.clone()).or_default(), *n);
            }
        }
    }
}

/// Something counted for an ad.
enum Change<'a> {
    Event(AdEvent),
    /// The game started with this version.
    Started(&'a str),
    Joined,
    Refused(&'static str),
    /// Minutes on the server.
    Played(u32),
}

impl Change<'_> {
    /// What this adds to an ad's day that counted `total` so far: nothing
    /// past the day's limits.
    fn within(&self, total: &AdCounts) -> AdCounts {
        let one = |n: u32, max: u32| u32::from(n < max);
        let mut add = AdCounts::default();
        match *self {
            Change::Event(event) => match event {
                AdEvent::Shown => add.shown = one(total.shown, MAX_SHOWN),
                AdEvent::Menu => add.menu = one(total.menu, MAX_CLICKS),
                AdEvent::VersionList => add.version_list = one(total.version_list, MAX_CLICKS),
                AdEvent::Copy => add.copy = one(total.copy, MAX_CLICKS),
                AdEvent::Report => add.report = one(total.report, MAX_CLICKS),
                AdEvent::Play => add.play = one(total.play, MAX_PLAYS),
                AdEvent::License => add.license = one(total.license, MAX_PLAYS),
                AdEvent::Installed => add.installed = one(total.installed, MAX_PLAYS),
                AdEvent::InstallFailed => {
                    add.install_failed = one(total.install_failed, MAX_PLAYS);
                }
            },
            Change::Started(version) => {
                add.started = one(total.started, MAX_PLAYS);
                let room =
                    total.picked.len() < MAX_PLAYS as usize || total.picked.contains_key(version);
                if add.started > 0 && room && crate::feed::ads::version_id(version) {
                    add.picked.insert(version.to_owned(), 1);
                }
            }
            Change::Joined => add.joined = one(total.joined, MAX_PLAYS),
            Change::Refused(reason) => {
                if total.refused.values().sum::<u32>() < MAX_PLAYS {
                    add.refused.insert(reason.to_owned(), 1);
                }
            }
            Change::Played(minutes) => {
                add.minutes = minutes.min(MAX_MINUTES.saturating_sub(total.minutes));
                if minutes >= PLAYED_MINUTES {
                    add.played = one(total.played, MAX_PLAYS);
                }
            }
        }
        add
    }
}

/// A ping or a batch of counts waiting for the service.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Waiting {
    batch: String,
    day: String,
    /// The work done, and for how many bits.
    #[serde(default)]
    work: Option<(u32, String)>,
    /// The interface language and the ads of a batch; none for a ping, which
    /// is written when it goes.
    #[serde(default)]
    counts: Option<Value>,
}

impl Waiting {
    fn new(day: &str, counts: Option<Value>) -> Self {
        Self {
            batch: uuid::Uuid::new_v4().to_string(),
            day: day.to_owned(),
            work: None,
            counts,
        }
    }

    fn valid(&self) -> bool {
        self.batch.len() == 36
            && uuid::Uuid::parse_str(&self.batch).is_ok()
            && parse_day(&self.day).is_some()
            && self.counts.as_ref().is_none_or(Value::is_object)
            && self.work.as_ref().is_none_or(|(bits, nonce)| {
                *bits <= MAX_BITS
                    && (1..=20).contains(&nonce.len())
                    && nonce.bytes().all(|b| b.is_ascii_digit())
            })
    }
}

/// The ad counts of one day.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Day {
    /// Counted since the last batch.
    #[serde(default)]
    open: BTreeMap<String, AdCounts>,
    /// Counted all day, for the limits.
    #[serde(default)]
    total: BTreeMap<String, AdCounts>,
}

/// An ad played here: the first day, and which returns were counted.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Played {
    first: String,
    #[serde(default)]
    back_day: bool,
    #[serde(default)]
    back_week: bool,
}

/// `stats/state.json`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Saved {
    /// The week this launcher first ran (`2026-W43`), `old` for one that ran
    /// before the statistics.
    #[serde(default)]
    cohort: Option<String>,
    /// The day of the last ping the service took, and whether one carried
    /// `ever`.
    #[serde(default)]
    sent_day: Option<String>,
    #[serde(default)]
    ever_sent: bool,
    /// The work the service asked for last.
    #[serde(default)]
    bits: Option<u32>,
    #[serde(default)]
    pings: Vec<Waiting>,
    #[serde(default)]
    batches: Vec<Waiting>,
    #[serde(default)]
    days: BTreeMap<String, Day>,
    /// Ads seen here, with the last day.
    #[serde(default)]
    seen: BTreeMap<String, String>,
    #[serde(default)]
    played: BTreeMap<String, Played>,
    /// When counts last went into a batch.
    #[serde(default)]
    sealed: Option<DateTime<Utc>>,
    /// The last ping and batch the service took, as they went.
    #[serde(default)]
    last_ping: Option<Value>,
    #[serde(default)]
    last_counts: Option<Value>,
}

/// An ad's number as the ads service writes it.
pub fn ad_id(id: &str) -> bool {
    (1..=12).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_digit()) && !id.starts_with('0')
}

fn day_text(day: NaiveDate) -> String {
    day.format("%Y-%m-%d").to_string()
}

fn parse_day(text: &str) -> Option<NaiveDate> {
    (text.len() == 10)
        .then(|| NaiveDate::parse_from_str(text, "%Y-%m-%d").ok())
        .flatten()
}

/// `2026-W43`, the ISO week of a day.
pub fn week_of(day: NaiveDate) -> String {
    let week = day.iso_week();
    format!("{}-W{:02}", week.year(), week.week())
}

fn cohort_valid(text: &str) -> bool {
    text == "old"
        || text.split_once("-W").is_some_and(|(year, week)| {
            year.len() == 4
                && week.len() == 2
                && year.bytes().chain(week.bytes()).all(|b| b.is_ascii_digit())
        })
}

impl Saved {
    /// Whatever the file says, within the limits.
    fn bound(&mut self) {
        self.pings.retain(Waiting::valid);
        self.pings.truncate(4);
        self.batches.retain(Waiting::valid);
        let extra = self.batches.len().saturating_sub(MAX_BATCHES);
        self.batches.drain(..extra);
        self.days.retain(|day, _| parse_day(day).is_some());
        while self.days.len() > KEEP_DAYS as usize + 2 {
            self.days.pop_first();
        }
        for day in self.days.values_mut() {
            for ads in [&mut day.open, &mut day.total] {
                ads.retain(|ad, _| ad_id(ad));
                while ads.len() > MAX_REMEMBERED {
                    ads.pop_last();
                }
            }
        }
        self.seen
            .retain(|ad, day| ad_id(ad) && parse_day(day).is_some());
        self.played
            .retain(|ad, played| ad_id(ad) && parse_day(&played.first).is_some());
        // A file that lost its week is not a new install.
        if self.cohort.as_deref().is_some_and(|c| !cohort_valid(c)) {
            self.cohort = Some("old".to_owned());
        }
        if self
            .sent_day
            .as_deref()
            .is_some_and(|d| parse_day(d).is_none())
        {
            self.sent_day = None;
        }
        self.bits = self.bits.filter(|b| *b <= MAX_BITS);
        self.forget_old(None);
    }

    /// Ads not seen or played for a long time, and the oldest past the limit.
    fn forget_old(&mut self, today: Option<NaiveDate>) {
        if let Some(today) = today {
            let recent =
                |day: &str| parse_day(day).is_some_and(|d| (today - d).num_days() <= REMEMBER_DAYS);
            self.seen.retain(|_, day| recent(day));
            self.played.retain(|_, played| recent(&played.first));
        }
        while self.seen.len() > MAX_REMEMBERED {
            let oldest = self
                .seen
                .iter()
                .min_by(|a, b| a.1.cmp(b.1))
                .map(|(ad, _)| ad.clone());
            if let Some(ad) = oldest {
                self.seen.remove(&ad);
            }
        }
        while self.played.len() > MAX_REMEMBERED {
            let oldest = self
                .played
                .iter()
                .min_by(|a, b| a.1.first.cmp(&b.1.first))
                .map(|(ad, _)| ad.clone());
            if let Some(ad) = oldest {
                self.played.remove(&ad);
            }
        }
    }

    /// The counts of each day into batches of at most 50 ads.
    fn seal(&mut self, now: DateTime<Utc>, ui: &str) {
        for (day, counts) in &mut self.days {
            let ads: Vec<Value> = std::mem::take(&mut counts.open)
                .into_iter()
                .filter(|(_, c)| !c.is_empty())
                .filter_map(|(ad, c)| {
                    let mut value = serde_json::to_value(c).ok()?;
                    value["ad"] = json!(ad);
                    Some(value)
                })
                .collect();
            for chunk in ads.chunks(MAX_ADS) {
                self.batches
                    .push(Waiting::new(day, Some(json!({ "ui": ui, "ads": chunk }))));
            }
        }
        self.sealed = Some(now);
    }

    /// Today's ping, the counts of the past hour (`seal` at once), and
    /// nothing the service no longer takes.
    fn tidy(&mut self, now: DateTime<Utc>, ui: &str, seal: bool) {
        let today = now.date_naive();
        let today_text = day_text(today);
        // The service takes pings of yesterday, today and tomorrow (clocks
        // differ).
        self.pings.retain(|p| {
            parse_day(&p.day).is_some_and(|d| (-1..=1).contains(&(today - d).num_days()))
        });
        if self.sent_day.as_deref() != Some(today_text.as_str())
            && !self.pings.iter().any(|p| p.day == today_text)
        {
            self.pings.push(Waiting::new(&today_text, None));
        }
        let due = seal
            || self
                .sealed
                .is_none_or(|at| at > now || (now - at).num_seconds() >= SEAL_EVERY_SECS);
        if due {
            self.seal(now, ui);
        }
        self.batches.retain(|b| {
            parse_day(&b.day).is_some_and(|d| (-1..=KEEP_DAYS).contains(&(today - d).num_days()))
        });
        let extra = self.batches.len().saturating_sub(MAX_BATCHES);
        self.batches.drain(..extra);
        // Past days are in batches now; today's stay for its limits.
        self.days
            .retain(|day, counts| *day == today_text || !counts.open.is_empty());
        self.forget_old(Some(today));
    }

    /// Erases what waits; the dates and the week stay.
    fn forget(&mut self) {
        *self = Self {
            cohort: self.cohort.take(),
            sent_day: self.sent_day.take(),
            ever_sent: self.ever_sent,
            bits: self.bits,
            ..Self::default()
        };
    }

    fn waiting(&mut self, kind: Kind, batch: &str) -> Option<&mut Waiting> {
        match kind {
            Kind::Ping => &mut self.pings,
            Kind::Counts => &mut self.batches,
        }
        .iter_mut()
        .find(|w| w.batch == batch)
    }

    fn remove(&mut self, kind: Kind, batch: &str) {
        match kind {
            Kind::Ping => &mut self.pings,
            Kind::Counts => &mut self.batches,
        }
        .retain(|w| w.batch != batch);
    }

    /// The flags of a ping of `day`, after the last one the service took.
    fn first(&self, day: NaiveDate) -> Vec<&'static str> {
        let sent = self.sent_day.as_deref().and_then(parse_day);
        let mut first = vec!["day"];
        if sent.is_none_or(|s| s.iso_week() != day.iso_week()) {
            first.push("week");
        }
        if sent.is_none_or(|s| (s.year(), s.month()) != (day.year(), day.month())) {
            first.push("month");
        }
        if !self.ever_sent && self.cohort.as_deref().is_some_and(|c| c != "old") {
            first.push("ever");
        }
        first
    }
}

/// What a ping says besides its flags.
struct About {
    launcher: String,
    channel: &'static str,
    ui: String,
    ram: &'static str,
    accounts: Vec<&'static str>,
    facts: &'static system::Facts,
}

/// The primary subtag of the interface language: `pt` of `pt-BR`.
fn ui_language(state: &AppState) -> String {
    let tag = state.settings().language;
    tag.as_str()
        .split('-')
        .next()
        .unwrap_or(crate::storage::models::Language::ENGLISH)
        .to_owned()
}

fn about(state: &AppState, facts: &'static system::Facts) -> About {
    let settings = state.settings();
    let accounts = state.accounts_file();
    About {
        launcher: state.app_version().to_owned(),
        channel: settings.update_channel.path(),
        ui: ui_language(state),
        ram: system::ram(state.physical_mib()),
        accounts: [
            (!accounts.accounts.is_empty(), "microsoft"),
            (accounts.launcher_account.is_some(), "launcher"),
            (!accounts.notoken_profiles.is_empty(), "notoken"),
        ]
        .into_iter()
        .filter_map(|(has, name)| has.then_some(name))
        .collect(),
        facts,
    }
}

fn ping_body(saved: &Saved, waiting: &Waiting, nonce: Option<&str>, about: &About) -> Value {
    let day = parse_day(&waiting.day).unwrap_or_default();
    let mut body = json!({
        "schema": SCHEMA,
        "batch": waiting.batch,
        "day": waiting.day,
        "first": saved.first(day),
        "cohort": saved.cohort.as_deref().unwrap_or("old"),
        "launcher": about.launcher,
        "channel": about.channel,
        "os": about.facts.os,
        "osVersion": about.facts.os_version,
        "arch": about.facts.arch,
        "bundle": about.facts.bundle,
        "ui": about.ui,
        "system": about.facts.language,
        "tz": about.facts.time_zone,
        "ram": about.ram,
        "cores": about.facts.cores,
        "accounts": about.accounts,
    });
    if let Some(nonce) = nonce {
        body["nonce"] = json!(nonce);
    }
    body
}

fn counts_body(waiting: &Waiting, nonce: &str) -> Value {
    let mut body = waiting.counts.clone().unwrap_or_else(|| json!({}));
    body["schema"] = json!(SCHEMA);
    body["batch"] = json!(waiting.batch);
    body["day"] = json!(waiting.day);
    body["nonce"] = json!(nonce);
    body
}

/// Leading zero bits of a digest.
fn zeros(digest: &[u8]) -> u32 {
    let mut bits = 0;
    for byte in digest {
        if *byte == 0 {
            bits += 8;
        } else {
            return bits + byte.leading_zeros();
        }
    }
    bits
}

/// The service's proof of work: a number whose SHA-256 with the batch and
/// the day starts with `bits` zero bits.
pub fn solve(batch: &str, day: &str, bits: u32) -> String {
    let bits = bits.min(MAX_BITS);
    let mut n: u64 = 0;
    loop {
        let nonce = n.to_string();
        let digest = Sha256::digest(format!("stats.lapi.sh|{batch}|{day}|{nonce}").as_bytes());
        if zeros(&digest) >= bits {
            return nonce;
        }
        n += 1;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Ping,
    Counts,
}

impl Kind {
    fn path(self) -> &'static str {
        match self {
            Self::Ping => "/v1/ping",
            Self::Counts => "/v1/counts",
        }
    }
}

/// What the service said.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Answer {
    Taken,
    /// More work: this many bits.
    Work(u32),
    /// Never taken: dropped.
    Refused,
    Later,
}

fn answer(status: u16, body: &[u8]) -> Answer {
    match status {
        200..=299 => Answer::Taken,
        422 => {
            let value: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
            if value["error"] == "work" {
                Answer::Work(
                    value["bits"]
                        .as_u64()
                        .and_then(|b| u32::try_from(b).ok())
                        .unwrap_or(u32::MAX),
                )
            } else {
                Answer::Refused
            }
        }
        400 | 413 | 415 => Answer::Refused,
        _ => Answer::Later,
    }
}

type Boxed<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Posts gzipped JSON to the service: the answer's status and body. Tests
/// put a fake service here.
pub trait Transport: Send + Sync {
    fn post<'a>(
        &'a self,
        path: &'a str,
        gzip_json: Vec<u8>,
    ) -> Boxed<'a, Result<(u16, Vec<u8>), String>>;
}

pub struct Http(crate::net::PublicClient);

impl Http {
    pub fn new(app_version: &str) -> LauncherResult<Self> {
        crate::net::PublicClient::new(crate::net::SourcePolicy::Stats, app_version).map(Self)
    }
}

impl Transport for Http {
    fn post<'a>(
        &'a self,
        path: &'a str,
        gzip_json: Vec<u8>,
    ) -> Boxed<'a, Result<(u16, Vec<u8>), String>> {
        Box::pin(async move {
            let url = url::Url::parse(&format!("{BASE}{path}")).map_err(|e| e.to_string())?;
            let request = self
                .0
                .post(&url)
                .map_err(|e| e.message)?
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .header(reqwest::header::CONTENT_ENCODING, "gzip")
                .timeout(Duration::from_secs(30))
                .body(gzip_json);
            let (status, _, body) = self
                .0
                .get_bounded(request, MAX_ANSWER_BYTES)
                .await
                .map_err(|e| e.message)?;
            Ok((status.as_u16(), body))
        })
    }
}

/// What Settings → Privacy → What is sent shows.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SentView {
    /// The last ping and the last batch of ad counts the service took, as
    /// they went.
    pub ping: Option<Value>,
    pub counts: Option<Value>,
    /// Today's ping when it has not gone yet.
    pub next: Option<Value>,
    /// Pings and batches waiting here.
    pub waiting: usize,
}

fn state_file(root: &DataRoot) -> PathBuf {
    root.path().join("stats").join("state.json")
}

fn read(path: &Path) -> Saved {
    let mut saved: Saved = fs::metadata(path)
        .ok()
        .filter(|m| m.len() <= MAX_FILE_BYTES)
        .and_then(|_| fs::read(path).ok())
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default();
    saved.bound();
    saved
}

fn write(path: &Path, saved: &Saved) {
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let _ = crate::storage::atomic::write(path, &serde_json::to_vec(saved).unwrap_or_default());
}

pub struct Stats {
    transport: Box<dyn Transport>,
    clock: Box<dyn Fn() -> DateTime<Utc> + Send + Sync>,
    /// The file, read once per data folder.
    saved: Mutex<Option<(PathBuf, Saved)>>,
    /// When each ad last counted as shown.
    shown: Mutex<HashMap<String, DateTime<Utc>>>,
    /// One sending at a time.
    sending: tokio::sync::Mutex<()>,
}

impl Stats {
    pub fn new(transport: Box<dyn Transport>) -> Self {
        Self::with_clock(transport, Box::new(Utc::now))
    }

    pub fn with_clock(
        transport: Box<dyn Transport>,
        clock: Box<dyn Fn() -> DateTime<Utc> + Send + Sync>,
    ) -> Self {
        Self {
            transport,
            clock,
            saved: Mutex::new(None),
            shown: Mutex::new(HashMap::new()),
            sending: tokio::sync::Mutex::new(()),
        }
    }

    /// Changes the file; written when `change` says it changed something.
    fn edit<T>(&self, root: &DataRoot, change: impl FnOnce(&mut Saved) -> (bool, T)) -> T {
        let path = state_file(root);
        let mut held = lock(&self.saved);
        if held.as_ref().is_some_and(|(at, _)| *at != path) {
            *held = None;
        }
        let (_, saved) = held.get_or_insert_with(|| (path.clone(), read(&path)));
        let (changed, result) = change(saved);
        if changed {
            write(&path, saved);
        }
        result
    }

    fn look<T>(&self, root: &DataRoot, see: impl FnOnce(&Saved) -> T) -> T {
        self.edit(root, |saved| (false, see(saved)))
    }

    /// At start, before the update check notes this version: the install
    /// week of a launcher that never ran before, `old` for one that did.
    pub fn init(&self, state: &AppState) {
        let Some(root) = state.data_root() else {
            return;
        };
        // Settings of an earlier version, installations or accounts: it ran.
        let accounts = state.accounts_file();
        let ran_before = state.settings().last_version.is_some()
            || !state.instance_ids().is_empty()
            || !accounts.accounts.is_empty()
            || !accounts.notoken_profiles.is_empty();
        let week = week_of((self.clock)().date_naive());
        self.edit(&root, |saved| {
            if saved.cohort.is_some() {
                return (false, ());
            }
            saved.cohort = Some(if ran_before { "old".to_owned() } else { week });
            (true, ())
        });
    }

    /// A run of the game started from the ad `ad`.
    pub fn ad_run(self: &Arc<Self>, ad: &str) -> AdRun {
        AdRun::new(Arc::clone(self), ad.to_owned())
    }

    /// A press or a view in an ad's tile. A view counts again 5 minutes
    /// later at the earliest.
    pub fn ad_event(&self, state: &AppState, ad: &str, event: AdEvent) {
        if !state.settings().stats || !ad_id(ad) {
            return;
        }
        if event == AdEvent::Shown {
            let now = (self.clock)();
            let mut shown = lock(&self.shown);
            if shown
                .get(ad)
                .is_some_and(|at| (now - *at).num_seconds().abs() < SHOWN_AGAIN_SECS)
            {
                return;
            }
            if shown.len() >= MAX_REMEMBERED {
                shown.clear();
            }
            shown.insert(ad.to_owned(), now);
        }
        self.count(state, ad, &Change::Event(event));
    }

    fn count(&self, state: &AppState, ad: &str, change: &Change) {
        if !state.settings().stats || !ad_id(ad) {
            return;
        }
        let Some(root) = state.data_root() else {
            return;
        };
        let today = (self.clock)().date_naive();
        let day = day_text(today);
        self.edit(&root, |saved| {
            let total = saved
                .days
                .get(&day)
                .and_then(|d| d.total.get(ad))
                .cloned()
                .unwrap_or_default();
            let mut add = change.within(&total);
            match change {
                Change::Event(AdEvent::Shown) if add.shown > 0 => {
                    match saved.seen.get(ad) {
                        None => add.reach = vec!["day".into(), "ever".into()],
                        Some(last) if *last != day => add.reach = vec!["day".into()],
                        Some(_) => {}
                    }
                    saved.seen.insert(ad.to_owned(), day.clone());
                }
                Change::Event(AdEvent::Play) => {
                    let played = saved.played.entry(ad.to_owned()).or_insert_with(|| Played {
                        first: day.clone(),
                        back_day: false,
                        back_week: false,
                    });
                    let after =
                        parse_day(&played.first).map_or(0, |first| (today - first).num_days());
                    if after >= 1 && !played.back_day {
                        played.back_day = true;
                        add.back.push("day".into());
                    }
                    if after >= 7 && !played.back_week {
                        played.back_week = true;
                        add.back.push("week".into());
                    }
                }
                _ => {}
            }
            if add.is_empty() {
                return (false, ());
            }
            let counts = saved.days.entry(day).or_default();
            if counts.total.len() >= MAX_REMEMBERED && !counts.total.contains_key(ad) {
                return (false, ());
            }
            counts.open.entry(ad.to_owned()).or_default().add(&add);
            counts.total.entry(ad.to_owned()).or_default().add(&add);
            (true, ())
        });
    }

    /// Puts what was counted into a batch, to go now or at the next start.
    pub fn seal(&self, state: &AppState) {
        if !state.settings().stats {
            return;
        }
        let Some(root) = state.data_root() else {
            return;
        };
        let (now, ui) = ((self.clock)(), ui_language(state));
        self.edit(&root, |saved| {
            saved.tidy(now, &ui, true);
            (true, ())
        });
    }

    /// Settings → Privacy turned the statistics off: what waits goes.
    pub fn forget(&self, state: &AppState) {
        lock(&self.shown).clear();
        let Some(root) = state.data_root() else {
            return;
        };
        self.edit(&root, |saved| {
            saved.forget();
            (true, ())
        });
    }

    pub fn sent(&self, state: &AppState) -> SentView {
        let Some(root) = state.data_root() else {
            return SentView {
                ping: None,
                counts: None,
                next: None,
                waiting: 0,
            };
        };
        let on = state.settings().stats;
        let about = about(state, system::facts());
        let today = day_text((self.clock)().date_naive());
        self.look(&root, |saved| {
            let next = on
                .then(|| {
                    if saved.sent_day.as_deref() == Some(today.as_str()) {
                        return None;
                    }
                    let waiting = saved
                        .pings
                        .iter()
                        .find(|p| p.day == today)
                        .cloned()
                        .unwrap_or_else(|| Waiting::new(&today, None));
                    let nonce = waiting.work.as_ref().map(|(_, n)| n.as_str());
                    Some(ping_body(saved, &waiting, nonce, &about))
                })
                .flatten();
            SentView {
                ping: saved.last_ping.clone(),
                counts: saved.last_counts.clone(),
                next,
                waiting: saved.pings.len() + saved.batches.len(),
            }
        })
    }

    /// Sends what waits: today's ping and the ad counts, oldest first. Stops
    /// at the first failure; the next pass tries again.
    pub async fn send(&self, state: &AppState) {
        let _one = self.sending.lock().await;
        if !state.settings().stats || state.network() == NetworkStatus::Offline {
            return;
        }
        let Some(root) = state.data_root() else {
            return;
        };
        let (now, ui) = ((self.clock)(), ui_language(state));
        let queue: Vec<(Kind, String)> = self.edit(&root, |saved| {
            saved.tidy(now, &ui, false);
            let queue = saved
                .pings
                .iter()
                .map(|w| (Kind::Ping, w.batch.clone()))
                .chain(
                    saved
                        .batches
                        .iter()
                        .map(|w| (Kind::Counts, w.batch.clone())),
                )
                .collect();
            (true, queue)
        });
        if queue.is_empty() {
            return;
        }
        let Ok(facts) = tokio::task::spawn_blocking(system::facts).await else {
            return;
        };
        let about = about(state, facts);
        for (kind, batch) in queue {
            if !self.deliver(state, &root, kind, &batch, &about).await {
                break;
            }
        }
    }

    /// One ping or batch: its work, then the request. False to stop the
    /// pass and try later.
    async fn deliver(
        &self,
        state: &AppState,
        root: &DataRoot,
        kind: Kind,
        batch: &str,
        about: &About,
    ) -> bool {
        // A second round when the service asks for more work.
        for _ in 0..2 {
            let found = self.look(root, |saved| {
                let list = match kind {
                    Kind::Ping => &saved.pings,
                    Kind::Counts => &saved.batches,
                };
                let bits = saved.bits.unwrap_or(DEFAULT_BITS);
                list.iter()
                    .find(|w| w.batch == batch)
                    .cloned()
                    .map(|w| (w, bits))
            });
            let Some((waiting, bits)) = found else {
                return true;
            };
            let nonce = match &waiting.work {
                Some((done, nonce)) if *done >= bits => nonce.clone(),
                _ => {
                    let (b, day) = (waiting.batch.clone(), waiting.day.clone());
                    let Ok(nonce) =
                        tokio::task::spawn_blocking(move || solve(&b, &day, bits)).await
                    else {
                        return false;
                    };
                    self.edit(root, |saved| match saved.waiting(kind, batch) {
                        Some(w) => {
                            w.work = Some((bits, nonce.clone()));
                            (true, ())
                        }
                        None => (false, ()),
                    });
                    nonce
                }
            };
            let body = match kind {
                Kind::Ping => self.look(root, |saved| {
                    ping_body(saved, &waiting, Some(nonce.as_str()), about)
                }),
                Kind::Counts => counts_body(&waiting, &nonce),
            };
            let bytes = crate::reports::gzip(&serde_json::to_vec(&body).unwrap_or_default());
            let result = self.transport.post(kind.path(), bytes).await;
            let said = match &result {
                Ok((status, answer_body)) => answer(*status, answer_body),
                Err(_) => Answer::Later,
            };
            match said {
                Answer::Taken => {
                    self.edit(root, |saved| {
                        saved.remove(kind, batch);
                        match kind {
                            Kind::Ping => {
                                if saved
                                    .sent_day
                                    .as_deref()
                                    .is_none_or(|d| d < waiting.day.as_str())
                                {
                                    saved.sent_day = Some(waiting.day.clone());
                                }
                                saved.ever_sent |= body["first"]
                                    .as_array()
                                    .is_some_and(|f| f.iter().any(|v| v == "ever"));
                                saved.last_ping = Some(body.clone());
                            }
                            Kind::Counts => saved.last_counts = Some(body.clone()),
                        }
                        (true, ())
                    });
                    return true;
                }
                Answer::Work(asked) if asked <= MAX_BITS => {
                    self.edit(root, |saved| {
                        saved.bits = Some(asked);
                        (true, ())
                    });
                }
                Answer::Work(_) | Answer::Later => return false,
                Answer::Refused => {
                    let status = result.map(|(s, _)| s).unwrap_or_default();
                    state.console().warn(format!(
                        "The statistics service refused {} (HTTP {status}); it was dropped",
                        match kind {
                            Kind::Ping => "a ping",
                            Kind::Counts => "a batch of ad counts",
                        }
                    ));
                    self.edit(root, |saved| {
                        saved.remove(kind, batch);
                        (true, ())
                    });
                    return true;
                }
            }
        }
        false
    }
}

#[cfg(test)]
mod tests;
