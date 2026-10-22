//! What the launcher sends (D-071) and how each value is read. Nothing is
//! kept as it came: a ping adds one to each of a few independent cuts, and a
//! batch of ad counts to each ad's events, so no stored row describes one
//! launcher. A value outside its known words or form counts as `other`;
//! only a request whose shape is wrong is refused.

use std::collections::BTreeMap;

use chrono::{Datelike, NaiveDate, Weekday};
use serde::Deserialize;

pub const SCHEMA: u32 = 1;
/// Ads in one batch: the ads service sends at most 50.
pub const MAX_ADS: usize = 50;
/// What one launcher may count for one ad in one batch; the launcher keeps
/// to these for a whole day.
pub const MAX_SHOWN: u32 = 300;
pub const MAX_PLAYS: u32 = 20;
pub const MAX_CLICKS: u32 = 50;
pub const MAX_MINUTES: u32 = 24 * 60;
/// Days a batch of ad counts may wait in an offline launcher.
pub const MAX_AGE_DAYS: i64 = 7;

/// What a ping counts towards: every ping is its day's; the first one of a
/// calendar week, of a month, and the very first also count there.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Period {
    Day,
    Week,
    Month,
    New,
}

impl Period {
    pub const ALL: [Self; 4] = [Self::Day, Self::Week, Self::Month, Self::New];

    pub fn code(self) -> &'static str {
        match self {
            Self::Day => "d",
            Self::Week => "w",
            Self::Month => "m",
            Self::New => "n",
        }
    }
}

/// The cuts of a ping, each one value per launcher and day.
pub const CUTS: [&str; 9] = [
    // Country (Cloudflare's).
    "country",
    // os/version/arch: windows/11/x86_64.
    "system",
    // launcher/channel/bundle: 1.4.0/stable/nsis.
    "version",
    // country/interface language: RU/ru.
    "country_ui",
    // system language/interface language: kk/ru.
    "language",
    // country/time zone: RU/Asia/Yekaterinburg (a VPN shows here).
    "country_tz",
    // memory/cores: 8-16/5-8.
    "hardware",
    // accounts signed in/country: microsoft+notoken/RU.
    "accounts",
    // install week/country: 2026-W41/RU; the week's pings give retention.
    "cohort",
];

pub const RAM: [&str; 6] = ["<4", "4-8", "8-16", "16-32", "32-64", "64+"];
pub const CORES: [&str; 6] = ["1-2", "3-4", "5-8", "9-16", "17-32", "33+"];
const OS: [&str; 3] = ["windows", "macos", "linux"];
const ARCH: [&str; 2] = ["x86_64", "aarch64"];
const CHANNELS: [&str; 2] = ["stable", "beta"];
const BUNDLES: [&str; 8] = [
    "nsis", "msi", "appimage", "deb", "rpm", "app", "dmg", "unknown",
];
const ACCOUNTS: [&str; 3] = ["microsoft", "launcher", "notoken"];
const FIRSTS: [&str; 4] = ["day", "week", "month", "ever"];
/// Why a server turned the player away, as the launcher names it.
pub const REFUSALS: [&str; 6] = ["version", "full", "whitelist", "ban", "noanswer", "other"];

/// The flags sent once a day.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Ping {
    pub schema: u32,
    pub batch: String,
    pub day: String,
    pub nonce: String,
    #[serde(default)]
    pub first: Vec<String>,
    #[serde(default)]
    pub cohort: String,
    #[serde(default)]
    pub launcher: String,
    #[serde(default)]
    pub channel: String,
    #[serde(default)]
    pub os: String,
    #[serde(default)]
    pub os_version: String,
    #[serde(default)]
    pub arch: String,
    #[serde(default)]
    pub bundle: String,
    #[serde(default)]
    pub ui: String,
    #[serde(default)]
    pub system: String,
    #[serde(default)]
    pub tz: String,
    #[serde(default)]
    pub ram: String,
    #[serde(default)]
    pub cores: String,
    #[serde(default)]
    pub accounts: Vec<String>,
}

/// One ad's counts in a batch.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AdCounts {
    pub ad: String,
    #[serde(default)]
    pub shown: u32,
    /// `day`: the first time this launcher saw it today; `ever`: at all.
    #[serde(default)]
    pub reach: Vec<String>,
    #[serde(default)]
    pub menu: u32,
    #[serde(default)]
    pub version_list: u32,
    #[serde(default)]
    pub copy: u32,
    #[serde(default)]
    pub report: u32,
    #[serde(default)]
    pub play: u32,
    /// Play stopped: the server wants a Microsoft account.
    #[serde(default)]
    pub license: u32,
    #[serde(default)]
    pub installed: u32,
    #[serde(default)]
    pub install_failed: u32,
    #[serde(default)]
    pub started: u32,
    #[serde(default)]
    pub joined: u32,
    #[serde(default)]
    pub refused: BTreeMap<String, u32>,
    /// Ten minutes on the server or more.
    #[serde(default)]
    pub played: u32,
    #[serde(default)]
    pub minutes: u32,
    /// `day`, `week`: Play pressed again a day or a week after the first time.
    #[serde(default)]
    pub back: Vec<String>,
    /// The versions started, with how often.
    #[serde(default)]
    pub picked: BTreeMap<String, u32>,
}

/// Counts of a day, sent hourly while the launcher runs.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Counts {
    pub schema: u32,
    pub batch: String,
    pub day: String,
    pub nonce: String,
    #[serde(default)]
    pub ui: String,
    #[serde(default)]
    pub ads: Vec<AdCounts>,
}

/// A ping, read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PingRead {
    pub day: NaiveDate,
    pub periods: Vec<Period>,
    pub cuts: Vec<(&'static str, String)>,
}

/// One ad's events of a batch, read: (event, count), and versions started.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdRead {
    pub ad: i64,
    pub events: Vec<(&'static str, u32)>,
    pub picked: Vec<(String, u32)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CountsRead {
    pub day: NaiveDate,
    pub ui: String,
    pub ads: Vec<AdRead>,
}

const OTHER: &str = "other";

fn known(value: &str, words: &[&'static str]) -> &'static str {
    words
        .iter()
        .find(|w| **w == value)
        .copied()
        .unwrap_or(OTHER)
}

/// The country Cloudflare saw (`CF-IPCountry`): two letters or digits
/// (`T1` is Tor), `XX` when unknown.
pub fn country(header: &str) -> String {
    let code = header.trim();
    if code.len() == 2 && code.bytes().all(|b| b.is_ascii_alphanumeric()) {
        code.to_ascii_uppercase()
    } else {
        "XX".to_owned()
    }
}

/// `YYYY-MM-DD`, exactly.
pub fn day(text: &str) -> Option<NaiveDate> {
    (text.len() == 10)
        .then(|| NaiveDate::parse_from_str(text, "%Y-%m-%d").ok())
        .flatten()
}

/// `2026-W41`, the ISO week of a day.
pub fn week_of(day: NaiveDate) -> String {
    let week = day.iso_week();
    format!("{}-W{:02}", week.year(), week.week())
}

/// The Monday of an ISO week written as `2026-W41`.
pub fn week_start(text: &str) -> Option<NaiveDate> {
    let (year, week) = text.split_once("-W")?;
    if year.len() != 4 || week.len() != 2 {
        return None;
    }
    let (year, week) = (year.parse().ok()?, week.parse().ok()?);
    NaiveDate::from_isoywd_opt(year, week, Weekday::Mon)
}

/// `1.4.0` or `1.4.0-beta.2`.
pub fn launcher_version(text: &str) -> String {
    let (core, pre) = match text.split_once('-') {
        Some((core, pre)) => (core, Some(pre)),
        None => (text, None),
    };
    let numbers = core.split('.').count() == 3
        && core
            .split('.')
            .all(|p| (1..=5).contains(&p.len()) && p.bytes().all(|b| b.is_ascii_digit()));
    let label = pre.is_none_or(|p| {
        (1..=16).contains(&p.len())
            && p.bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'.')
    });
    if numbers && label {
        text.to_owned()
    } else {
        OTHER.to_owned()
    }
}

/// A short lowercase word: `11`, `15`, `ubuntu`, `fedora`.
fn word(text: &str, max: usize) -> String {
    let ok = (1..=max).contains(&text.len())
        && text
            .bytes()
            .next()
            .is_some_and(|b| b.is_ascii_alphanumeric())
        && text
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"._-".contains(&b));
    if ok {
        text.to_owned()
    } else {
        OTHER.to_owned()
    }
}

/// A language's primary subtag: `ru`, `kk`, `fil`.
pub fn language(text: &str) -> String {
    if (2..=3).contains(&text.len()) && text.bytes().all(|b| b.is_ascii_lowercase()) {
        text.to_owned()
    } else {
        OTHER.to_owned()
    }
}

/// A time zone of the IANA database: `Europe/Moscow`.
fn time_zone(text: &str) -> String {
    if text.len() <= 40 && text.parse::<chrono_tz::Tz>().is_ok() {
        text.to_owned()
    } else {
        OTHER.to_owned()
    }
}

/// The week the launcher first counted itself, `old` for one that ran before
/// the statistics; never after the ping's own week.
fn cohort(text: &str, day: NaiveDate) -> String {
    if text == "old" {
        return text.to_owned();
    }
    match week_start(text) {
        Some(monday) if monday.year() >= 2025 && monday <= day && week_of(monday) == text => {
            text.to_owned()
        }
        _ => OTHER.to_owned(),
    }
}

/// A version ID as the ads service sends them: `1.21.11`, `24w14a`.
pub fn version_id(id: &str) -> bool {
    (1..=32).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        && id.bytes().next().is_some_and(|b| b.is_ascii_alphanumeric())
}

/// The day of a request, within `back` days before today and one after
/// (clocks differ).
fn day_within(text: &str, today: NaiveDate, back: i64) -> Result<NaiveDate, &'static str> {
    let day = day(text).ok_or("day")?;
    let age = (today - day).num_days();
    if !(-1..=back).contains(&age) {
        return Err("day");
    }
    Ok(day)
}

fn batch_id(text: &str) -> Result<uuid::Uuid, &'static str> {
    // Exactly the hyphenated form the launcher writes.
    if text.len() != 36 {
        return Err("batch");
    }
    uuid::Uuid::parse_str(text).map_err(|_| "batch")
}

impl Ping {
    pub fn batch(&self) -> Result<uuid::Uuid, &'static str> {
        batch_id(&self.batch)
    }

    pub fn read(&self, country: &str, today: NaiveDate) -> Result<PingRead, &'static str> {
        if self.schema != SCHEMA {
            return Err("schema");
        }
        self.batch()?;
        let day = day_within(&self.day, today, 1)?;
        if self.first.len() > FIRSTS.len()
            || self.first.iter().any(|f| !FIRSTS.contains(&f.as_str()))
        {
            return Err("first");
        }
        if !self.first.iter().any(|f| f == "day") {
            return Err("first");
        }
        let has = |word: &str| self.first.iter().any(|f| f == word);
        let periods = [
            (Period::Day, true),
            (Period::Week, has("week")),
            (Period::Month, has("month")),
            (Period::New, has("ever")),
        ]
        .into_iter()
        .filter_map(|(period, on)| on.then_some(period))
        .collect();
        if self.accounts.len() > 8 {
            return Err("accounts");
        }
        let mut accounts: Vec<&str> = self.accounts.iter().map(|a| known(a, &ACCOUNTS)).collect();
        accounts.sort_unstable();
        accounts.dedup();
        let accounts = if accounts.is_empty() {
            "none".to_owned()
        } else {
            accounts.join("+")
        };
        let ui = language(&self.ui);
        let cuts = vec![
            ("country", country.to_owned()),
            (
                "system",
                format!(
                    "{}/{}/{}",
                    known(&self.os, &OS),
                    word(&self.os_version, 20),
                    known(&self.arch, &ARCH)
                ),
            ),
            (
                "version",
                format!(
                    "{}/{}/{}",
                    launcher_version(&self.launcher),
                    known(&self.channel, &CHANNELS),
                    known(&self.bundle, &BUNDLES)
                ),
            ),
            ("country_ui", format!("{country}/{ui}")),
            ("language", format!("{}/{ui}", language(&self.system))),
            ("country_tz", format!("{country}/{}", time_zone(&self.tz))),
            (
                "hardware",
                format!("{}/{}", known(&self.ram, &RAM), known(&self.cores, &CORES)),
            ),
            ("accounts", format!("{accounts}/{country}")),
            ("cohort", format!("{}/{country}", cohort(&self.cohort, day))),
        ];
        Ok(PingRead { day, periods, cuts })
    }
}

impl AdCounts {
    fn read(&self) -> Result<AdRead, &'static str> {
        let ad = (1..=12)
            .contains(&self.ad.len())
            .then(|| self.ad.parse::<i64>().ok())
            .flatten()
            .filter(|id| *id > 0 && self.ad.bytes().all(|b| b.is_ascii_digit()))
            .ok_or("ad")?;
        if self.reach.len() > 2 || self.back.len() > 2 {
            return Err("flags");
        }
        if self.refused.len() > REFUSALS.len() || self.picked.len() > MAX_PLAYS as usize {
            return Err("ads");
        }
        let flag = |flags: &[String], word: &str| u32::from(flags.iter().any(|f| f == word));
        let mut events = vec![
            ("shown", self.shown.min(MAX_SHOWN)),
            ("reach_day", flag(&self.reach, "day")),
            ("reach_ever", flag(&self.reach, "ever")),
            ("menu", self.menu.min(MAX_CLICKS)),
            ("version_list", self.version_list.min(MAX_CLICKS)),
            ("copy", self.copy.min(MAX_CLICKS)),
            ("report", self.report.min(MAX_CLICKS)),
            ("play", self.play.min(MAX_PLAYS)),
            ("license", self.license.min(MAX_PLAYS)),
            ("installed", self.installed.min(MAX_PLAYS)),
            ("install_failed", self.install_failed.min(MAX_PLAYS)),
            ("started", self.started.min(MAX_PLAYS)),
            ("joined", self.joined.min(MAX_PLAYS)),
            ("played", self.played.min(MAX_PLAYS)),
            ("minutes", self.minutes.min(MAX_MINUTES)),
            ("back_day", flag(&self.back, "day")),
            ("back_week", flag(&self.back, "week")),
        ];
        let mut refused: BTreeMap<&'static str, u32> = BTreeMap::new();
        for (reason, n) in &self.refused {
            let total = refused.entry(refusal(reason)).or_default();
            *total = total.saturating_add(*n).min(MAX_PLAYS);
        }
        for (reason, n) in refused {
            events.push((refused_event(reason), n));
        }
        events.retain(|(_, n)| *n > 0);
        let picked = self
            .picked
            .iter()
            .filter(|(version, n)| version_id(version) && **n > 0)
            .map(|(version, n)| (version.clone(), (*n).min(MAX_PLAYS)))
            .collect();
        Ok(AdRead { ad, events, picked })
    }
}

fn refusal(reason: &str) -> &'static str {
    known(reason, &REFUSALS)
}

/// The event a refusal is counted as: `refused_full`.
pub fn refused_event(reason: &str) -> &'static str {
    match refusal(reason) {
        "version" => "refused_version",
        "full" => "refused_full",
        "whitelist" => "refused_whitelist",
        "ban" => "refused_ban",
        "noanswer" => "refused_noanswer",
        _ => "refused_other",
    }
}

impl Counts {
    pub fn batch(&self) -> Result<uuid::Uuid, &'static str> {
        batch_id(&self.batch)
    }

    pub fn read(&self, today: NaiveDate) -> Result<CountsRead, &'static str> {
        if self.schema != SCHEMA {
            return Err("schema");
        }
        self.batch()?;
        let day = day_within(&self.day, today, MAX_AGE_DAYS)?;
        if self.ads.len() > MAX_ADS {
            return Err("ads");
        }
        let mut ads: Vec<AdRead> = Vec::with_capacity(self.ads.len());
        for counts in &self.ads {
            let read = counts.read()?;
            if ads.iter().any(|a| a.ad == read.ad) {
                return Err("ads");
            }
            ads.push(read);
        }
        Ok(CountsRead {
            day,
            ui: language(&self.ui),
            ads,
        })
    }
}
