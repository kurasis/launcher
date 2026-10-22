//! YouTube's Data API (v3), with the key from `LAPI_FEED_YOUTUBE_KEY`: the
//! length of videos and channels' pictures, which the RSS feeds do not give.
//! Each request costs one unit of Google's free daily quota (10 000); the
//! service stops at `DAY_UNITS` and goes on without lengths until the next
//! day. Without the key lengths are left out.

use std::collections::HashMap;
use std::sync::Mutex;

use serde::Deserialize;

use crate::images::video_id;
use crate::net;
use crate::youtube::{channel_id, picture_url};

/// Units spent in a day at most (Google counts a Pacific day).
pub const DAY_UNITS: u32 = 8000;
/// Videos or channels named in one request.
pub const IDS_PER_REQUEST: usize = 50;
/// The longest length taken, in seconds (30 days); anything longer is junk.
pub const MAX_SECONDS: u64 = 30 * 24 * 3600;
const MAX_ANSWER: usize = 1 << 20;
const BASE: &str = "https://www.googleapis.com/youtube/v3";

pub struct DataApi {
    key: String,
    /// The day (Pacific, roughly) and the units spent in it.
    spent: Mutex<(String, u32)>,
}

#[derive(Deserialize)]
struct Videos {
    #[serde(default)]
    items: Vec<VideoItem>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct VideoItem {
    id: String,
    content_details: Option<ContentDetails>,
}

#[derive(Deserialize)]
struct ContentDetails {
    duration: Option<String>,
}

#[derive(Deserialize)]
struct Channels {
    #[serde(default)]
    items: Vec<ChannelItem>,
}

#[derive(Deserialize)]
struct ChannelItem {
    snippet: Option<Snippet>,
}

#[derive(Deserialize)]
struct Snippet {
    thumbnails: Option<HashMap<String, Thumbnail>>,
}

#[derive(Deserialize)]
struct Thumbnail {
    url: String,
}

/// An ISO 8601 length as YouTube gives it (`PT15M33S`, `P1DT2H`) in
/// seconds; `None` for anything else, for nothing (`P0D`, a live stream or
/// one not yet aired) and for more than `MAX_SECONDS`.
pub fn seconds(text: &str) -> Option<u32> {
    let rest = text.strip_prefix('P')?;
    if rest.is_empty() || text.len() > 40 {
        return None;
    }
    let (mut total, mut number, mut in_time, mut last) = (0u64, None::<u64>, false, 0u8);
    for c in rest.chars() {
        if c == 'T' {
            if in_time || number.is_some() {
                return None;
            }
            in_time = true;
            continue;
        }
        if let Some(digit) = c.to_digit(10) {
            number = Some(number.unwrap_or(0).checked_mul(10)? + u64::from(digit));
            if number? > MAX_SECONDS {
                return None;
            }
            continue;
        }
        // Units in order: W D before T, H M S after it, each once.
        let (rank, unit) = match (c, in_time) {
            ('W', false) => (1, 7 * 24 * 3600),
            ('D', false) => (2, 24 * 3600),
            ('H', true) => (3, 3600),
            ('M', true) => (4, 60),
            ('S', true) => (5, 1),
            _ => return None,
        };
        if rank <= last {
            return None;
        }
        last = rank;
        total = total.checked_add(number.take()?.checked_mul(unit)?)?;
    }
    if number.is_some() || (in_time && last < 3) {
        return None;
    }
    (total > 0 && total <= MAX_SECONDS)
        .then_some(total)
        .and_then(|t| u32::try_from(t).ok())
}

/// The day Google's quota counts in: Pacific, taken as UTC−8.
fn quota_day() -> String {
    (chrono::Utc::now() - chrono::Duration::hours(8))
        .format("%Y-%m-%d")
        .to_string()
}

impl DataApi {
    pub fn new(key: String) -> Option<Self> {
        let key = key.trim().to_owned();
        (!key.is_empty()).then(|| Self {
            key,
            spent: Mutex::new((String::new(), 0)),
        })
    }

    /// Takes one unit of the day's quota, when there is one left.
    pub fn spend(&self) -> bool {
        let mut spent = self
            .spent
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let today = quota_day();
        if spent.0 != today {
            *spent = (today, 0);
        }
        if spent.1 >= DAY_UNITS {
            return false;
        }
        spent.1 += 1;
        true
    }

    /// Units spent today.
    pub fn spent(&self) -> u32 {
        let spent = self
            .spent
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if spent.0 == quota_day() { spent.1 } else { 0 }
    }

    /// One request; the key travels in a header, never in an address that
    /// an error could print.
    async fn get(
        &self,
        http: &reqwest::Client,
        path: &str,
        params: &[(&str, &str)],
    ) -> Result<Vec<u8>, String> {
        if !self.spend() {
            return Err("the day's quota is used".to_owned());
        }
        let url = url::Url::parse_with_params(&format!("{BASE}/{path}"), params)
            .map_err(|e| e.to_string())?;
        let response = http
            .get(url)
            .header("X-goog-api-key", &self.key)
            .send()
            .await
            .map_err(|e| e.without_url().to_string())?;
        net::read(response, MAX_ANSWER).await
    }

    /// The lengths of these videos, in seconds, as many as YouTube knows
    /// (no live streams, nothing not yet aired, nothing removed).
    pub async fn lengths(
        &self,
        http: &reqwest::Client,
        ids: &[String],
    ) -> Result<HashMap<String, u32>, String> {
        let ids: Vec<&str> = ids
            .iter()
            .map(String::as_str)
            .filter(|id| video_id(id))
            .collect();
        let mut out = HashMap::new();
        for chunk in ids.chunks(IDS_PER_REQUEST) {
            let joined = chunk.join(",");
            let answer = self
                .get(
                    http,
                    "videos",
                    &[
                        ("part", "contentDetails"),
                        ("id", &joined),
                        ("fields", "items(id,contentDetails/duration)"),
                    ],
                )
                .await?;
            out.extend(read_lengths(&answer, chunk)?);
        }
        Ok(out)
    }

    /// The address of a channel's picture (240×240), when YouTube has one.
    pub async fn channel_picture(
        &self,
        http: &reqwest::Client,
        id: &str,
    ) -> Result<Option<String>, String> {
        if !channel_id(id) {
            return Err("bad channel id".to_owned());
        }
        let answer = self
            .get(
                http,
                "channels",
                &[
                    ("part", "snippet"),
                    ("id", id),
                    ("fields", "items(snippet/thumbnails)"),
                ],
            )
            .await?;
        read_picture(&answer)
    }
}

/// The lengths in an answer of `videos`, of the videos asked only.
pub fn read_lengths(answer: &[u8], asked: &[&str]) -> Result<HashMap<String, u32>, String> {
    let videos: Videos =
        serde_json::from_slice(answer).map_err(|_| "bad Data API answer".to_owned())?;
    Ok(videos
        .items
        .into_iter()
        .filter(|item| asked.contains(&item.id.as_str()))
        .filter_map(|item| {
            let s = seconds(item.content_details?.duration.as_deref()?)?;
            Some((item.id, s))
        })
        .collect())
}

/// The channel picture's address in an answer of `channels`: the 240×240
/// one, else another, on YouTube's picture hosts only.
pub fn read_picture(answer: &[u8]) -> Result<Option<String>, String> {
    let channels: Channels =
        serde_json::from_slice(answer).map_err(|_| "bad Data API answer".to_owned())?;
    Ok(channels
        .items
        .into_iter()
        .filter_map(|c| c.snippet?.thumbnails)
        .find_map(|mut t| {
            ["medium", "high", "default"]
                .iter()
                .find_map(|size| t.remove(*size))
        })
        .and_then(|t| picture_url(&t.url)))
}
