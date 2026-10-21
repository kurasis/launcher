//! VirusTotal's word on the files the scanner suspects (D-066, Ai
//! 2026-10-21). Shown to the owner only (`find`, `review`, `virustotal`):
//! it holds back or lets go nothing by itself.
//!
//! Each suspected file is looked up by its SHA-256 once. One VirusTotal does
//! not know is sent to it when it comes from a public site (Modrinth,
//! CurseForge, the owner's projects), then looked up again after a while;
//! files only players' own packs carry are never sent. No later re-checks
//! for now (Ai): the owner asks again with `virustotal check`.
//!
//! The quotas are the public API's, counted here for every request: 500 a
//! day and 15 500 a month (UTC); the background takes at most 450 and 15 000
//! of them, the rest is the owner's. The key (`LAPI_MODS_VIRUSTOTAL_KEY`)
//! goes to curl on its standard input, never in a command line or a log.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::blobs::{Blobs, is_sha256};
use crate::store::Store;

const API: &str = "https://www.virustotal.com/api/v3";

/// A share of the quotas: requests a UTC day and a UTC month.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub day: i64,
    pub month: i64,
}

/// The public API's quotas.
pub const QUOTA: Limits = Limits {
    day: 500,
    month: 15_500,
};
/// What the background may use; the rest is kept for the owner's checks.
pub const BACKGROUND: Limits = Limits {
    day: 450,
    month: 15_000,
};

/// Seconds between the background's checks (one file each).
pub const PACE: u64 = 180;
/// Sent as is up to this; larger files go to an address asked for first.
const DIRECT_UPLOAD: i64 = 32 * 1024 * 1024;
/// VirusTotal takes no larger file.
pub const MAX_UPLOAD: i64 = 650 * 1024 * 1024;
/// A file report is a few hundred KB.
const MAX_ANSWER: usize = 8 * 1024 * 1024;
/// After sending a file: looked up again after these many seconds, then
/// once a day, at most `MAX_WAITS` times.
const FIRST_WAIT: i64 = 30 * 60;
const WAITS: [i64; 2] = [2 * 3600, 86_400];
const MAX_WAITS: i64 = 8;
/// A file sent this long ago and still unknown may be sent again.
const SEND_AGAIN: i64 = 7 * 86_400;
const DAY: i64 = 86_400;

/// The engines' counts in VirusTotal's last analysis of a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Stats {
    pub malicious: i64,
    pub suspicious: i64,
    pub undetected: i64,
    pub harmless: i64,
    /// When VirusTotal analysed it (Unix seconds).
    pub analysed: Option<i64>,
}

impl Stats {
    /// The engines that gave an answer (timeouts and unsupported types left
    /// out): the 60 of "2 / 60".
    pub fn total(&self) -> i64 {
        self.malicious + self.suspicious + self.undetected + self.harmless
    }
}

/// What one request to VirusTotal came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Answer {
    /// The file's report.
    Found(Stats),
    /// Known, but with no finished analysis yet.
    Analysing,
    /// VirusTotal does not know the file.
    Unknown,
    /// Where to send a large file.
    Address(String),
    /// The file was taken for analysis.
    Sent,
    /// The quota is spent.
    Quota,
    /// Anything else: a refused key, a junk answer, no network.
    Failed,
}

/// VirusTotal's API, one request per call (a fake in the tests).
pub trait Api: Send + Sync {
    fn lookup(&self, sha256: &str) -> Answer;
    fn upload_url(&self) -> Answer;
    fn upload(&self, url: &str, path: &Path) -> Answer;
}

/// The API through curl, the key given on its standard input.
pub struct Curl {
    key: String,
    curl: PathBuf,
}

impl Curl {
    /// `None` unless the key looks like one: 32 to 128 letters and digits
    /// (VirusTotal's are 64 hex digits).
    pub fn new(key: &str, curl: PathBuf) -> Option<Self> {
        let key = key.trim();
        ((32..=128).contains(&key.len()) && key.bytes().all(|b| b.is_ascii_alphanumeric())).then(
            || Self {
                key: key.to_owned(),
                curl,
            },
        )
    }

    /// One request: its status and body (empty when too large).
    fn call(&self, config: &str, seconds: u32) -> Option<(u16, Vec<u8>)> {
        let mut child = Command::new(&self.curl)
            .args([
                "-sS",
                "--max-time",
                &seconds.to_string(),
                "--max-filesize",
                &MAX_ANSWER.to_string(),
                "-w",
                "\n%{http_code}",
                "-K",
                "-",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        child.stdin.take()?.write_all(config.as_bytes()).ok()?;
        let output = child.wait_with_output().ok()?;
        let out = output.stdout;
        let split = out.iter().rposition(|&b| b == b'\n')?;
        let code = std::str::from_utf8(&out[split + 1..])
            .ok()?
            .trim()
            .parse()
            .ok()?;
        let body = if split > MAX_ANSWER {
            Vec::new()
        } else {
            out[..split].to_vec()
        };
        Some((code, body))
    }

    fn config(&self, url: &str, extra: &str) -> String {
        format!(
            "url = \"{}\"\nheader = \"x-apikey: {}\"\nheader = \"accept: application/json\"\n{extra}",
            quote(url),
            self.key
        )
    }
}

impl Api for Curl {
    fn lookup(&self, sha256: &str) -> Answer {
        if !is_sha256(sha256) {
            return Answer::Failed;
        }
        self.call(&self.config(&format!("{API}/files/{sha256}"), ""), 60)
            .map_or(Answer::Failed, |(code, body)| lookup_answer(code, &body))
    }

    fn upload_url(&self) -> Answer {
        self.call(&self.config(&format!("{API}/files/upload_url"), ""), 60)
            .map_or(Answer::Failed, |(code, body)| address_answer(code, &body))
    }

    fn upload(&self, url: &str, path: &Path) -> Answer {
        let Some(path) = path.to_str().filter(|p| plain_path(p)) else {
            return Answer::Failed;
        };
        if url != format!("{API}/files") && !upload_address(url) {
            return Answer::Failed;
        }
        let extra = format!("form = \"file=@{path}\"\n");
        self.call(&self.config(url, &extra), 900)
            .map_or(Answer::Failed, |(code, _)| sent_answer(code))
    }
}

fn quote(text: &str) -> String {
    text.replace('\\', "\\\\").replace('"', "\\\"")
}

/// A kept file's path, safe inside curl's form syntax.
fn plain_path(path: &str) -> bool {
    path.starts_with('/')
        && path.len() <= 512
        && path
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'/' | b'.' | b'_' | b'-'))
}

/// The one-time address VirusTotal gives for a large file: HTTPS on its own
/// domain only.
pub fn upload_address(url: &str) -> bool {
    url.len() <= 2048
        && reqwest::Url::parse(url).is_ok_and(|u| {
            u.scheme() == "https"
                && u.port().is_none()
                && u.username().is_empty()
                && u.password().is_none()
                && u.host_str()
                    .is_some_and(|h| h == "virustotal.com" || h.ends_with(".virustotal.com"))
        })
}

/// A file report: its counts only (names of engines and threats are
/// someone else's words, never kept).
pub fn lookup_answer(code: u16, body: &[u8]) -> Answer {
    match code {
        200 => stats(body).map_or(Answer::Analysing, Answer::Found),
        404 => Answer::Unknown,
        429 => Answer::Quota,
        _ => Answer::Failed,
    }
}

fn stats(body: &[u8]) -> Option<Stats> {
    let value: serde_json::Value = serde_json::from_slice(body).ok()?;
    let attributes = value.pointer("/data/attributes")?;
    let counts = attributes.get("last_analysis_stats")?.as_object()?;
    let count = |key: &str| {
        counts
            .get(key)
            .map_or(Some(0), |n| n.as_i64().filter(|n| (0..=1000).contains(n)))
    };
    let stats = Stats {
        malicious: count("malicious")?,
        suspicious: count("suspicious")?,
        undetected: count("undetected")?,
        harmless: count("harmless")?,
        analysed: attributes
            .get("last_analysis_date")
            .and_then(serde_json::Value::as_i64)
            .filter(|t| (1..4_102_444_800).contains(t)),
    };
    (stats.total() > 0).then_some(stats)
}

pub fn address_answer(code: u16, body: &[u8]) -> Answer {
    match code {
        200 => serde_json::from_slice::<serde_json::Value>(body)
            .ok()
            .and_then(|v| v.get("data")?.as_str().map(str::to_owned))
            .filter(|url| upload_address(url))
            .map_or(Answer::Failed, Answer::Address),
        429 => Answer::Quota,
        _ => Answer::Failed,
    }
}

pub fn sent_answer(code: u16) -> Answer {
    match code {
        200 => Answer::Sent,
        429 => Answer::Quota,
        _ => Answer::Failed,
    }
}

/// The UTC day and month the quotas count in.
pub fn day_and_month(now: i64) -> (String, String) {
    let day = chrono::DateTime::from_timestamp(now, 0).unwrap_or_default();
    (
        day.format("%Y-%m-%d").to_string(),
        day.format("%Y-%m").to_string(),
    )
}

fn date(at: i64) -> String {
    chrono::DateTime::from_timestamp(at, 0)
        .map(|d| d.format("%Y-%m-%d").to_string())
        .unwrap_or_default()
}

/// What is known of a file at VirusTotal, as kept here.
#[derive(Debug, Clone, Default)]
pub struct Row {
    /// waiting, found, sent, unknown, too_big, error
    pub state: String,
    pub stats: Option<Stats>,
    /// When we last asked.
    pub checked: Option<i64>,
    /// When to ask next (`None`: only when the owner asks).
    pub next: Option<i64>,
    /// When we sent the file.
    pub sent: Option<i64>,
    /// Lookups since it was sent.
    pub waits: i64,
}

/// The line the owner's commands print for a file.
pub fn line(row: &Row, on: bool) -> String {
    let text = match (row.state.as_str(), &row.stats) {
        (_, Some(s)) if row.state == "found" || row.state == "error" => {
            let mut text = format!("{} / {}", s.malicious, s.total());
            if s.suspicious > 0 {
                text.push_str(&format!(", {} suspicious", s.suspicious));
            }
            if let Some(at) = s.analysed.or(row.checked) {
                text.push_str(&format!(" ({})", date(at)));
            }
            text
        }
        _ if !on => "off".to_owned(),
        ("sent", _) => "sent, analysing".to_owned(),
        ("unknown", _) => "unknown".to_owned(),
        ("too_big", _) => "too big".to_owned(),
        ("error", _) => match row.next {
            Some(at) => format!("error, again {}", date(at)),
            None => "error".to_owned(),
        },
        _ => "waiting".to_owned(),
    };
    format!("virustotal: {text}")
}

/// What one check came to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Checked {
    Done,
    /// The share of the quota is spent: nothing was asked, or the rest
    /// waits for another day.
    NoQuota,
}

/// Asks VirusTotal about one kept file (and sends it when it is unknown),
/// counting each request against `limits`, and keeps the answer.
pub fn check(
    store: &Store,
    blobs: &Blobs,
    api: &dyn Api,
    sha256: &str,
    limits: Limits,
    now: i64,
) -> Result<Checked, String> {
    let (day, month) = day_and_month(now);
    let take = || store.take_virustotal(&day, &month, limits.day, limits.month);
    let spent = || -> Result<Checked, String> {
        store.virustotal_spent(&day, QUOTA.day)?;
        Ok(Checked::NoQuota)
    };
    let row = store.virustotal(sha256)?.unwrap_or_default();
    // A failed request: asked again tomorrow, unless an answer is known
    // already (no re-checks for now).
    let later = |row: &Row| -> Result<(), String> {
        if row.state == "found" {
            store.set_virustotal(sha256, "found", None, None, row.sent, 0, now)
        } else {
            store.set_virustotal(
                sha256,
                "error",
                None,
                Some(now + DAY),
                row.sent,
                row.waits,
                now,
            )
        }
    };
    if !take()? {
        return Ok(Checked::NoQuota);
    }
    let waiting_after_send = row.sent.is_some_and(|t| now - t < SEND_AGAIN);
    match api.lookup(sha256) {
        Answer::Found(stats) => {
            store.set_virustotal(sha256, "found", Some(&stats), None, row.sent, 0, now)?;
        }
        Answer::Quota => return spent(),
        // Known but not analysed yet, or sent lately: asked again later, a
        // few times at most.
        Answer::Analysing => wait(store, sha256, &row, now)?,
        Answer::Unknown if waiting_after_send => wait(store, sha256, &row, now)?,
        Answer::Unknown => return send(store, blobs, api, sha256, &row, limits, now),
        _ => later(&row)?,
    }
    Ok(Checked::Done)
}

/// Asks about a file again later: after 2 hours, then daily, at most
/// `MAX_WAITS` times.
fn wait(store: &Store, sha256: &str, row: &Row, now: i64) -> Result<(), String> {
    let waits = row.waits + 1;
    if waits >= MAX_WAITS {
        return store.set_virustotal(sha256, "unknown", None, None, row.sent, waits, now);
    }
    let wait = WAITS[usize::try_from(row.waits).unwrap_or(0).min(WAITS.len() - 1)];
    store.set_virustotal(sha256, "sent", None, Some(now + wait), row.sent, waits, now)
}

/// Sends a file VirusTotal does not know: only one from a public site, and
/// small enough.
fn send(
    store: &Store,
    blobs: &Blobs,
    api: &dyn Api,
    sha256: &str,
    row: &Row,
    limits: Limits,
    now: i64,
) -> Result<Checked, String> {
    let (day, month) = day_and_month(now);
    let take = || store.take_virustotal(&day, &month, limits.day, limits.month);
    let spent = || -> Result<Checked, String> {
        store.virustotal_spent(&day, QUOTA.day)?;
        Ok(Checked::NoQuota)
    };
    let set = |state: &str, next: Option<i64>, sent: Option<i64>| {
        store.set_virustotal(sha256, state, None, next, sent, 0, now)
    };
    if !store.public_file(sha256)? {
        set("unknown", None, row.sent)?;
        return Ok(Checked::Done);
    }
    let size = store.blob_info(sha256)?.map_or(0, |b| b.size);
    if size > MAX_UPLOAD {
        set("too_big", None, row.sent)?;
        return Ok(Checked::Done);
    }
    let Some(path) = blobs.find(sha256) else {
        set("error", Some(now + DAY), row.sent)?;
        return Ok(Checked::Done);
    };
    let url = if size > DIRECT_UPLOAD {
        if !take()? {
            set("waiting", Some(now), row.sent)?;
            return Ok(Checked::NoQuota);
        }
        match api.upload_url() {
            Answer::Address(url) => url,
            Answer::Quota => return spent(),
            _ => {
                set("error", Some(now + DAY), row.sent)?;
                return Ok(Checked::Done);
            }
        }
    } else {
        format!("{API}/files")
    };
    if !take()? {
        set("waiting", Some(now + DAY), row.sent)?;
        return Ok(Checked::NoQuota);
    }
    match api.upload(&url, &path) {
        Answer::Sent => set("sent", Some(now + FIRST_WAIT), Some(now))?,
        Answer::Quota => return spent(),
        _ => set("error", Some(now + DAY), row.sent)?,
    }
    Ok(Checked::Done)
}

/// One step of the background: suspected files queued, the next one
/// checked. `Ok(false)` when there was nothing to do or no quota.
pub fn step(store: &Store, blobs: &Blobs, api: &dyn Api, now: i64) -> Result<bool, String> {
    store.queue_virustotal(now)?;
    let Some(sha256) = store.next_virustotal(now)? else {
        return Ok(false);
    };
    Ok(check(store, blobs, api, &sha256, BACKGROUND, now)? == Checked::Done)
}
