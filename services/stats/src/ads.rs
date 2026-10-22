//! The ads the ads service (servers.lapi.sh, D-069) shows, asked on this
//! server every few minutes. Counts are taken only for an ad that showed on
//! their day, and the report for the ad register needs each day's erid.
//! The ads service stays read only: nothing goes to it.

use std::collections::{BTreeMap, HashSet};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use chrono::NaiveDate;
use serde_json::Value;

use crate::store::{ShownAd, Store};

pub const POLL_EVERY: Duration = Duration::from_secs(300);
/// Counts of ads wait while the list is older than this; the launcher sends
/// them again later.
pub const FRESH_FOR: Duration = Duration::from_secs(1800);
/// Days of lists kept in memory: a batch may wait a week in a launcher.
const KEEP_DAYS: i64 = 10;
/// The languages of the ads service: an ad may show to some of them only.
const LANGS: [&str; 3] = ["en", "ru", "ar"];
const MAX_NAME: usize = 40;
const MAX_ERID: usize = 64;
const MAX_BYTES: usize = 512 * 1024;

#[derive(Default)]
pub struct Shown {
    days: Mutex<BTreeMap<NaiveDate, HashSet<i64>>>,
    fetched: Mutex<Option<Instant>>,
}

impl Shown {
    /// The lists of the last days, from the database.
    pub fn load(store: &Store, today: NaiveDate) -> Result<Self, String> {
        let days = store.ads_seen(today - chrono::Duration::days(KEEP_DAYS))?;
        Ok(Self {
            days: Mutex::new(days),
            fetched: Mutex::new(None),
        })
    }

    fn days(&self) -> std::sync::MutexGuard<'_, BTreeMap<NaiveDate, HashSet<i64>>> {
        self.days
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Whether the ad showed on the day, or the day before or after (an ad
    /// can end just before midnight, a launcher's clock be off).
    pub fn known(&self, ad: i64, day: NaiveDate) -> bool {
        let days = self.days();
        (-1..=1).any(|d| {
            days.get(&(day + chrono::Duration::days(d)))
                .is_some_and(|ads| ads.contains(&ad))
        })
    }

    /// The list was fetched lately.
    pub fn fresh(&self) -> bool {
        self.fetched
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .is_some_and(|at| at.elapsed() < FRESH_FOR)
    }

    pub fn note(&self, day: NaiveDate, ads: &[ShownAd]) {
        let mut days = self.days();
        days.entry(day)
            .or_default()
            .extend(ads.iter().map(|a| a.id));
        days.retain(|d, _| *d >= day - chrono::Duration::days(KEEP_DAYS));
        drop(days);
        *self
            .fetched
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Instant::now());
    }
}

/// HTTP to this server only: no proxies, short waits.
pub fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .no_proxy()
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::none())
        .user_agent(concat!("lapi-stats/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| format!("http client: {e}"))
}

async fn get(http: &reqwest::Client, url: &str) -> Result<Value, String> {
    let mut response = http
        .get(url)
        .send()
        .await
        .map_err(|_| "the ads service does not answer".to_owned())?;
    if !response.status().is_success() {
        return Err(format!(
            "the ads service answered {}",
            response.status().as_u16()
        ));
    }
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| "the ads service's answer broke off".to_owned())?
    {
        body.extend_from_slice(&chunk);
        if body.len() > MAX_BYTES {
            return Err("the ads service's answer is too large".to_owned());
        }
    }
    serde_json::from_slice(&body).map_err(|_| "the ads service's answer is not JSON".to_owned())
}

/// Every ad showing now, in any language.
pub async fn fetch(http: &reqwest::Client, base: &str) -> Result<Vec<ShownAd>, String> {
    let mut ads: Vec<ShownAd> = Vec::new();
    for lang in LANGS {
        let answer = get(http, &format!("{base}/v1/ads?place=feed&lang={lang}")).await?;
        for ad in parse(&answer) {
            if !ads.iter().any(|a| a.id == ad.id) {
                ads.push(ad);
            }
        }
    }
    Ok(ads)
}

/// The ads of an answer of `/v1/ads`; one that breaks a rule is left out.
pub fn parse(answer: &Value) -> Vec<ShownAd> {
    let Some(list) = answer.get("ads").and_then(Value::as_array) else {
        return Vec::new();
    };
    list.iter()
        .take(100)
        .filter_map(|ad| {
            let id = ad.get("id")?.as_str()?;
            if id.is_empty() || id.len() > 12 || !id.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            let id = id.parse().ok().filter(|id: &i64| *id > 0)?;
            let name = ad.get("name")?.as_str()?;
            let name: String = name
                .chars()
                .filter(|c| !c.is_control() && !crate::invisible(*c))
                .take(MAX_NAME)
                .collect();
            let erid = match ad.get("erid") {
                None | Some(Value::Null) => None,
                Some(erid) => {
                    let erid = erid.as_str()?;
                    if erid.is_empty()
                        || erid.len() > MAX_ERID
                        || !erid.bytes().all(|b| b.is_ascii_alphanumeric())
                    {
                        return None;
                    }
                    Some(erid.to_owned())
                }
            };
            Some(ShownAd {
                id,
                name: name.trim().to_owned(),
                erid,
            })
        })
        .collect()
}

/// An ad's online players by hour, from the ads service: the hours as
/// (unix seconds, players or none when it did not answer), and when the ad
/// starts and ends.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Online {
    pub starts: i64,
    pub ends: i64,
    pub hours: Vec<(i64, Option<i64>)>,
}

pub async fn online(http: &reqwest::Client, base: &str, ad: i64) -> Result<Online, String> {
    let answer = get(http, &format!("{base}/v1/online/{ad}")).await?;
    let number = |key: &str| answer.get(key).and_then(Value::as_i64).unwrap_or_default();
    let hours = answer
        .get("hours")
        .and_then(Value::as_array)
        .map(|hours| {
            hours
                .iter()
                .take(24 * 400)
                .filter_map(|hour| {
                    let at = hour.get("at")?.as_i64()?;
                    let online = hour
                        .get("online")
                        .and_then(Value::as_i64)
                        .filter(|n| (0..=10_000_000).contains(n));
                    Some((at, online))
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(Online {
        starts: number("starts"),
        ends: number("ends"),
        hours,
    })
}
