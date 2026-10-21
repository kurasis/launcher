//! Reviews the player sends to the developers (D-070): More → Leave a review.
//!
//! The form builds a [`Draft`]; sending scrubs its text files once more, puts
//! it in `reviews/outbox/` and tries to deliver it: a proof of work for the
//! service's challenge, then the review. Delivered reviews are kept in
//! `reviews/sent.json` with their number and key, so the player can read the
//! answers ([`Reviews::mine`]) and delete a review. Nothing leaves the computer
//! until the player presses Send.

use std::fs;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::time::Duration;

use base64::Engine as _;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::error::{LauncherError, LauncherResult};
use crate::reports::scrub;
use crate::state::AppState;
use crate::storage::paths::DataRoot;

pub const BASE: &str = "https://reviews.lapi.sh";
pub const MIN_MESSAGE_CHARS: usize = 20;
pub const MAX_MESSAGE_CHARS: usize = 5000;
const MAX_IMAGES: usize = 5;
const MAX_TEXTS: usize = 12;
/// The newest part of a text file that is sent.
pub const MAX_TEXT_BYTES: usize = 512 * 1024;
/// A picture as the form made it (scaled to 2560 pixels).
const MAX_IMAGE_BYTES: usize = 8 * 1024 * 1024;
/// The whole review; the service takes 16 MiB.
const MAX_BODY_BYTES: usize = 15 * 1024 * 1024;
/// Lines of a game log kept.
const LOG_LINES: usize = 1500;
/// Runs and logs older than this are not "the latest".
const RECENT_DAYS: i64 = 7;
/// Launcher failure records of these last days.
const FAILURE_DAYS: i64 = 3;
/// Unsent reviews older than this are dropped.
const OUTBOX_DAYS: i64 = 30;
/// Answers are asked for at most this often, unless the player asks.
const CHECK_EVERY_HOURS: i64 = 6;
/// The hardest challenge the launcher solves (the service's limit too).
const MAX_BITS: u8 = 28;
/// Reviews remembered in `sent.json`.
const MAX_SENT: usize = 200;
/// Extensions of text files a review takes (the service's list).
pub const TEXT_EXTENSIONS: [&str; 13] = [
    "log",
    "txt",
    "json",
    "json5",
    "toml",
    "cfg",
    "conf",
    "properties",
    "yml",
    "yaml",
    "xml",
    "csv",
    "ini",
];

fn unavailable() -> LauncherError {
    LauncherError::new(
        crate::error::ErrorCode::StorageUnavailable,
        "Launcher data is unavailable.",
    )
}

/// One file of a review: a scrubbed text, or a picture (PNG or JPEG, base64).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DraftFile {
    pub name: String,
    /// `text` or `image`.
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<String>,
}

/// What the form sends.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Draft {
    pub kind: String,
    pub category: String,
    #[serde(default)]
    pub subcategory: Option<String>,
    pub message: String,
    #[serde(default)]
    pub rating: Option<u8>,
    /// Ask for the answers with the launcher account (any computer).
    #[serde(default)]
    pub link_account: bool,
    #[serde(default)]
    pub diagnostic_id: Option<String>,
    #[serde(default)]
    pub ad: Option<i64>,
    #[serde(default)]
    pub files: Vec<DraftFile>,
}

/// Texts the form shows before sending, scrubbed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Found {
    pub files: Vec<DraftFile>,
    pub removed: scrub::Counts,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Delivery {
    Sent,
    /// Offline, or the service could not take it now: it goes later by itself.
    Queued,
}

/// A review this computer sent, with what the service last said of it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Sent {
    pub id: i64,
    /// Kept in `sent.json`; emptied before the interface gets the list.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub key: String,
    pub sent_at: DateTime<Utc>,
    pub kind: String,
    pub category: String,
    pub subcategory: String,
    /// The start of the message, to recognise it.
    pub excerpt: String,
    pub account: bool,
    /// `sent`, `read`, `answered`, `closed` or `gone`.
    pub status: String,
    #[serde(default)]
    pub reply: Option<String>,
    #[serde(default)]
    pub replied: Option<DateTime<Utc>>,
    /// The player has seen the current answer.
    #[serde(default)]
    pub reply_seen: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SentFile {
    #[serde(default)]
    checked: Option<DateTime<Utc>>,
    #[serde(default)]
    reviews: Vec<Sent>,
}

/// The player's reviews, newest first, and how many answers are unread.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Mine {
    pub reviews: Vec<Sent>,
    pub unseen: usize,
    /// Reviews still waiting in the outbox.
    pub waiting: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Pending {
    created: DateTime<Utc>,
    attempts: u32,
    next: DateTime<Utc>,
    link_account: bool,
    /// The review without its challenge.
    body: Value,
}

/// The launcher account's access token, when the player linked a review.
pub enum Token {
    Token(String),
    /// Not signed in (any more): the review goes without the account.
    SignedOut,
    /// The account service did not answer: try later.
    Later,
}

type Boxed<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub trait Accounts: Send + Sync {
    fn token<'a>(&'a self, state: &'a AppState) -> Boxed<'a, Token>;
}

impl Accounts for crate::launcher_account::LauncherAccount {
    fn token<'a>(&'a self, state: &'a AppState) -> Boxed<'a, Token> {
        Box::pin(async move {
            if state.accounts_file().launcher_account.is_none() {
                return Token::SignedOut;
            }
            match self.access_token(state).await {
                Ok(token) => Token::Token(token.expose().to_owned()),
                Err(crate::launcher_account::AccountProblem::SignInAgain) => Token::SignedOut,
                Err(_) => Token::Later,
            }
        })
    }
}

/// A request to the service: method, path, JSON body and headers; the answer's
/// status and body. Tests put a fake service here.
pub trait Transport: Send + Sync {
    fn call<'a>(
        &'a self,
        method: &'a str,
        path: &'a str,
        body: Option<Vec<u8>>,
        headers: Vec<(&'static str, String)>,
    ) -> Boxed<'a, Result<(u16, Vec<u8>), String>>;
}

pub struct Http(crate::net::PublicClient);

impl Http {
    pub fn new(app_version: &str) -> LauncherResult<Self> {
        crate::net::PublicClient::new(crate::net::SourcePolicy::Reviews, app_version).map(Self)
    }
}

impl Transport for Http {
    fn call<'a>(
        &'a self,
        method: &'a str,
        path: &'a str,
        body: Option<Vec<u8>>,
        headers: Vec<(&'static str, String)>,
    ) -> Boxed<'a, Result<(u16, Vec<u8>), String>> {
        Box::pin(async move {
            let url = url::Url::parse(&format!("{BASE}{path}")).map_err(|e| e.to_string())?;
            let request = match method {
                "GET" => self.0.get(&url),
                "DELETE" => self.0.delete(&url),
                _ => self.0.post(&url),
            }
            .map_err(|e| e.message)?;
            let mut request = request.timeout(Duration::from_secs(60));
            for (name, value) in headers {
                request = request.header(name, value);
            }
            if let Some(body) = body {
                request = request
                    .header(reqwest::header::CONTENT_TYPE, "application/json")
                    .body(body);
            }
            let (status, _, body) = self
                .0
                .get_bounded(request, 256 * 1024)
                .await
                .map_err(|e| e.message)?;
            Ok((status.as_u16(), body))
        })
    }
}

fn dir(root: &DataRoot) -> PathBuf {
    root.path().join("reviews")
}

fn outbox(root: &DataRoot) -> PathBuf {
    dir(root).join("outbox")
}

fn sent_path(root: &DataRoot) -> PathBuf {
    dir(root).join("sent.json")
}

fn read_sent(root: &DataRoot) -> SentFile {
    fs::read(sent_path(root))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

fn write_sent(root: &DataRoot, file: &SentFile) -> LauncherResult<()> {
    fs::create_dir_all(dir(root)).map_err(|e| LauncherError::from_io(&e, "Saving your reviews"))?;
    crate::storage::atomic::write(
        &sent_path(root),
        &serde_json::to_vec_pretty(file).unwrap_or_default(),
    )
    .map_err(|e| LauncherError::from_io(&e, "Saving your reviews"))
}

/// Leading zero bits of a hash.
fn zeros(hash: &[u8]) -> u32 {
    let mut count = 0;
    for byte in hash {
        if *byte == 0 {
            count += 8;
        } else {
            return count + byte.leading_zeros();
        }
    }
    count
}

/// The first number that, hashed with the challenge, starts with `bits` zero
/// bits (the service's proof of work).
pub fn solve(challenge: &str, bits: u8) -> String {
    let bits = u32::from(bits.min(MAX_BITS));
    let mut n: u64 = 0;
    loop {
        let nonce = n.to_string();
        if zeros(&Sha256::digest(format!("{challenge}:{nonce}").as_bytes())) >= bits {
            return nonce;
        }
        n += 1;
    }
}

/// A file name as the service keeps it, if it names a text file it takes.
pub fn text_name(name: &str) -> Option<String> {
    let base = name.rsplit(['/', '\\']).next().unwrap_or_default();
    let extension = base.rsplit_once('.')?.1.to_ascii_lowercase();
    (TEXT_EXTENSIONS.contains(&extension.as_str()) && !base.starts_with('.'))
        .then(|| base.chars().take(80).collect())
}

fn picture(data: &str) -> LauncherResult<()> {
    let too_big = || {
        LauncherError::invalid_input("A picture is too large. Leave it out or take a smaller one.")
    };
    if data.len() > MAX_IMAGE_BYTES / 3 * 4 + 4 {
        return Err(too_big());
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data.as_bytes())
        .map_err(|_| LauncherError::invalid_input("A picture could not be read. Add it again."))?;
    let png = bytes.starts_with(&[0x89, b'P', b'N', b'G']);
    let jpeg = bytes.starts_with(&[0xFF, 0xD8, 0xFF]);
    if !png && !jpeg {
        return Err(LauncherError::invalid_input(
            "A picture could not be read. Add it again.",
        ));
    }
    Ok(())
}

/// The newest `max` bytes of a text, cut at a line start.
fn newest(text: &str, max: usize) -> String {
    crate::reports::newest(text, max)
}

fn last_lines(text: &str, lines: usize) -> String {
    let all: Vec<&str> = text.lines().collect();
    all[all.len().saturating_sub(lines)..].join("\n")
}

fn recent(path: &Path) -> bool {
    fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .map(DateTime::<Utc>::from)
        .is_some_and(|t| Utc::now().signed_duration_since(t) < chrono::Duration::days(RECENT_DAYS))
}

fn read_text(path: &Path, max: u64) -> Option<String> {
    use std::io::Read;
    let file = fs::File::open(path).ok()?;
    let length = file.metadata().ok()?.len();
    let mut reader = std::io::BufReader::new(file);
    if length > max {
        use std::io::Seek;
        reader.seek(std::io::SeekFrom::Start(length - max)).ok()?;
    }
    let mut bytes = Vec::new();
    reader.take(max).read_to_end(&mut bytes).ok()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

pub struct Reviews {
    transport: Box<dyn Transport>,
    /// One delivery or check at a time.
    busy: tokio::sync::Mutex<()>,
    wake: tokio::sync::Notify,
}

impl Reviews {
    pub fn new(transport: Box<dyn Transport>) -> Self {
        Self {
            transport,
            busy: tokio::sync::Mutex::new(()),
            wake: tokio::sync::Notify::new(),
        }
    }

    /// The latest logs, scrubbed: the launcher log, the last run of the
    /// installation (or `run_id`'s), the installation's game log, the
    /// launcher's failures of the last days and a short `system.txt`.
    pub fn logs(
        &self,
        state: &AppState,
        instance_id: Option<&str>,
        run_id: Option<&str>,
        home: Option<&Path>,
    ) -> LauncherResult<Found> {
        let root = state.data_root().ok_or_else(unavailable)?;
        let instance = instance_id
            .map(|id| {
                id.parse::<crate::storage::paths::InstanceId>()
                    .map_err(|_| LauncherError::invalid_input("This installation does not exist."))
            })
            .transpose()?;
        let cx = crate::reports::scrub_context(state, &root, home);
        let mut found = Found {
            files: Vec::new(),
            removed: scrub::Counts::default(),
        };
        let mut add = |name: &str, text: &str| {
            let (text, counts) = scrub::text(&newest(text, MAX_TEXT_BYTES), &cx);
            found.removed.add(counts);
            if !text.trim().is_empty() {
                found.files.push(DraftFile {
                    name: name.to_owned(),
                    kind: "text".to_owned(),
                    text: Some(text),
                    data: None,
                });
            }
        };
        let logs = root.logs_dir();
        let launcher = crate::journal::tail(&logs.join(crate::journal::LOG_FILE), LOG_LINES);
        if !launcher.is_empty() {
            add("launcher.log", &launcher.join("\n"));
        }
        // The run: the one asked for, else the last one if it was this
        // installation's (or any, without one) and recent.
        let run = match run_id {
            Some(id) => Some(
                uuid::Uuid::parse_str(id)
                    .map_err(|_| LauncherError::invalid_input("This game run does not exist."))?,
            ),
            None => state.snapshot().last_run.and_then(|last| {
                let ours = instance.is_none_or(|id| last.instance_id == Some(id));
                let fresh = Utc::now().signed_duration_since(last.started_at)
                    < chrono::Duration::days(RECENT_DAYS);
                (ours && fresh)
                    .then(|| uuid::Uuid::parse_str(&last.run_id).ok())
                    .flatten()
            }),
        };
        if let Some(run) = run {
            let dir = root.run_logs(run);
            if let Some(text) = read_text(&dir.join("output.log"), 8 * 1024 * 1024) {
                add("output.log", &last_lines(&text, LOG_LINES));
            }
            for name in [
                crate::launch::crash::DIAGNOSIS_FILE,
                crate::launch::crash::REPORT_COPY,
                crate::launch::crash::JVM_ERROR_COPY,
            ] {
                if let Some(text) = read_text(&dir.join(name), 4 * 1024 * 1024) {
                    add(name, &text);
                }
            }
        }
        if let Some(id) = instance {
            let latest = root.game_dir(id).join("logs").join("latest.log");
            if recent(&latest)
                && let Some(text) = read_text(&latest, 8 * 1024 * 1024)
            {
                add("latest.log", &last_lines(&text, LOG_LINES));
            }
        }
        let failures: Vec<_> = crate::journal::failures(&logs)
            .into_iter()
            .filter(|f| {
                Utc::now().signed_duration_since(f.at) < chrono::Duration::days(FAILURE_DAYS)
            })
            .take(5)
            .map(|mut f| {
                f.log_tail = Vec::new();
                f
            })
            .collect();
        if !failures.is_empty() {
            add(
                "launcher-failures.json",
                &serde_json::to_string_pretty(&failures).unwrap_or_default(),
            );
        }
        let mut system = format!(
            "Launcher {}\nOS {} {}\nLanguage {}\n",
            state.app_version(),
            std::env::consts::OS,
            std::env::consts::ARCH,
            state.settings().language.as_str(),
        );
        if let Some(i) = instance.and_then(|id| state.instance(id)) {
            system.push_str(&format!("Minecraft {}\n", i.version_id));
            if let Some(loader) = &i.loader {
                system.push_str(&format!(
                    "Loader {} {}\n",
                    crate::console::name(&loader.kind),
                    loader.version
                ));
            }
            system.push_str(&format!("Memory for the game {} MiB\n", i.max_heap_mib));
            if let Some(java) = &i.java {
                system.push_str(&format!("Java {} ({})\n", java.major, java.release));
            }
            let mods = fs::read_dir(root.game_dir(i.id).join("mods"))
                .map(|e| {
                    e.flatten()
                        .filter(|e| e.file_name().to_string_lossy().ends_with(".jar"))
                        .count()
                })
                .unwrap_or(0);
            system.push_str(&format!("Mods {mods}\n"));
        }
        add("system.txt", &system);
        Ok(found)
    }

    /// A text file the player picked, scrubbed like the logs.
    pub fn text_file(
        &self,
        state: &AppState,
        name: &str,
        text: &str,
        home: Option<&Path>,
    ) -> LauncherResult<Found> {
        let name = text_name(name).ok_or_else(|| {
            LauncherError::invalid_input(
                "Only text files can be attached: logs, configs and other plain text.",
            )
        })?;
        if text.contains('\0') {
            return Err(LauncherError::invalid_input(
                "This file is not plain text. Only text files can be attached.",
            ));
        }
        let root = state.data_root().ok_or_else(unavailable)?;
        let cx = crate::reports::scrub_context(state, &root, home);
        let (text, removed) = scrub::text(&newest(text, MAX_TEXT_BYTES), &cx);
        Ok(Found {
            files: vec![DraftFile {
                name,
                kind: "text".to_owned(),
                text: Some(text),
                data: None,
            }],
            removed,
        })
    }

    /// Checks a draft and builds the review the service gets.
    fn body(&self, state: &AppState, draft: Draft, home: Option<&Path>) -> LauncherResult<Value> {
        let message = draft.message.trim();
        let chars = message.chars().count();
        if chars < MIN_MESSAGE_CHARS {
            return Err(LauncherError::invalid_input(
                "Write a little more: the message needs at least 20 characters.",
            ));
        }
        if chars > MAX_MESSAGE_CHARS {
            return Err(LauncherError::invalid_input(
                "The message is too long: 5000 characters at most.",
            ));
        }
        let plain = |s: &str, max: usize| {
            !s.is_empty()
                && s.len() <= max
                && s.bytes().all(|b| b.is_ascii_lowercase() || b == b'_')
        };
        if !plain(&draft.kind, 20)
            || !plain(&draft.category, 30)
            || draft.subcategory.as_deref().is_some_and(|s| !plain(s, 30))
        {
            return Err(LauncherError::invalid_input(
                "Choose what the review is about.",
            ));
        }
        let images = draft.files.iter().filter(|f| f.kind == "image").count();
        let texts = draft.files.iter().filter(|f| f.kind == "text").count();
        if images > MAX_IMAGES || texts > MAX_TEXTS {
            return Err(LauncherError::invalid_input(
                "Too many files: 5 pictures and 12 text files at most.",
            ));
        }
        let root = state.data_root().ok_or_else(unavailable)?;
        let cx = crate::reports::scrub_context(state, &root, home);
        let mut files = Vec::new();
        for file in draft.files {
            match (file.kind.as_str(), file.text, file.data) {
                ("text", Some(text), None) => {
                    let name = text_name(&file.name).ok_or_else(|| {
                        LauncherError::invalid_input(
                            "Only text files can be attached: logs, configs and other plain text.",
                        )
                    })?;
                    let (text, _) = scrub::text(&newest(&text, MAX_TEXT_BYTES), &cx);
                    files.push(json!({ "name": name, "kind": "text", "text": text }));
                }
                ("image", None, Some(data)) => {
                    picture(&data)?;
                    files.push(json!({ "name": "screenshot.png", "kind": "image", "data": data }));
                }
                _ => {
                    return Err(LauncherError::invalid_input(
                        "A file could not be read. Add it again.",
                    ));
                }
            }
        }
        let diagnostic = draft.diagnostic_id.filter(|d| {
            d.len() <= 64
                && d.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        });
        let body = json!({
            "schema": 1,
            "kind": draft.kind,
            "category": draft.category,
            "subcategory": draft.subcategory,
            "message": message,
            "rating": if draft.kind == "impression" { draft.rating.filter(|r| (1..=5).contains(r)) } else { None },
            "launcherVersion": state.app_version(),
            "os": format!("{} {}", std::env::consts::OS, std::env::consts::ARCH),
            "language": state.settings().language.as_str(),
            "installId": state.install_id()?,
            "diagnosticId": diagnostic,
            "ad": draft.ad.filter(|a| *a > 0),
            "files": files,
        });
        if serde_json::to_vec(&body).map_or(0, |b| b.len()) > MAX_BODY_BYTES {
            return Err(LauncherError::invalid_input(
                "The review is too large. Leave out a picture or a long file.",
            ));
        }
        Ok(body)
    }

    /// Puts the review in the outbox and tries to send it now.
    pub async fn send(
        &self,
        state: &AppState,
        accounts: &dyn Accounts,
        draft: Draft,
        home: Option<&Path>,
    ) -> LauncherResult<Delivery> {
        let link_account = draft.link_account;
        let body = self.body(state, draft, home)?;
        let root = state.data_root().ok_or_else(unavailable)?;
        let dir = outbox(&root);
        fs::create_dir_all(&dir).map_err(|e| LauncherError::from_io(&e, "Saving the review"))?;
        let id = uuid::Uuid::new_v4().to_string();
        let pending = Pending {
            created: Utc::now(),
            attempts: 0,
            next: Utc::now(),
            link_account,
            body,
        };
        crate::storage::atomic::write(
            &dir.join(format!("{id}.json")),
            &serde_json::to_vec(&pending).unwrap_or_default(),
        )
        .map_err(|e| LauncherError::from_io(&e, "Saving the review"))?;
        state.console().info("A review was saved for sending");
        if self.deliver(state, accounts, &root, &id).await {
            Ok(Delivery::Sent)
        } else {
            self.wake.notify_one();
            Ok(Delivery::Queued)
        }
    }

    /// Sends one review of the outbox; true once the service has it, or
    /// refused it for good.
    async fn deliver(
        &self,
        state: &AppState,
        accounts: &dyn Accounts,
        root: &DataRoot,
        id: &str,
    ) -> bool {
        let _busy = self.busy.lock().await;
        let path = outbox(root).join(format!("{id}.json"));
        let Some(mut pending) = fs::read(&path)
            .ok()
            .and_then(|b| serde_json::from_slice::<Pending>(&b).ok())
        else {
            return false;
        };
        if state.network() == crate::state::dto::NetworkStatus::Offline {
            return false;
        }
        let console = state.console();
        let outcome = self.attempt(state, accounts, &pending).await;
        let done = match outcome {
            Ok((id, key)) => {
                console.info("The review was sent to the developers");
                let body = &pending.body;
                let text = |k: &str| body[k].as_str().unwrap_or_default().to_owned();
                let mut file = read_sent(root);
                file.reviews.insert(
                    0,
                    Sent {
                        id,
                        key,
                        sent_at: Utc::now(),
                        kind: text("kind"),
                        category: text("category"),
                        subcategory: text("subcategory"),
                        excerpt: text("message").chars().take(120).collect(),
                        account: pending.link_account,
                        status: "sent".to_owned(),
                        reply: None,
                        replied: None,
                        reply_seen: false,
                    },
                );
                file.reviews.truncate(MAX_SENT);
                let _ = write_sent(root, &file);
                true
            }
            Err(Some(status)) => {
                console.warn(format!(
                    "The review service refused a review (HTTP {status}); it was dropped"
                ));
                true
            }
            Err(None) => false,
        };
        if done {
            let _ = fs::remove_file(&path);
        } else {
            pending.attempts += 1;
            let minutes = 2i64.saturating_pow(pending.attempts.min(9)).min(360);
            pending.next = Utc::now() + chrono::Duration::minutes(minutes);
            let _ = crate::storage::atomic::write(
                &path,
                &serde_json::to_vec(&pending).unwrap_or_default(),
            );
        }
        done
    }

    /// One try: a challenge, its proof of work, the review. Ok with the
    /// number and key; Err(Some(status)) when refused for good; Err(None)
    /// to try again later.
    async fn attempt(
        &self,
        state: &AppState,
        accounts: &dyn Accounts,
        pending: &Pending,
    ) -> Result<(i64, String), Option<u16>> {
        let console = state.console();
        let mut headers = Vec::new();
        if pending.link_account {
            match accounts.token(state).await {
                Token::Token(token) => headers.push(("authorization", format!("Bearer {token}"))),
                Token::SignedOut => {}
                Token::Later => return Err(None),
            }
        }
        let (status, answer) = self
            .transport
            .call("GET", "/v1/challenge", None, Vec::new())
            .await
            .map_err(|e| {
                console.warn(format!(
                    "Sending a review failed ({e}); it is tried again later"
                ));
                None
            })?;
        let answer: Value = serde_json::from_slice(&answer).unwrap_or(Value::Null);
        let (Some(challenge), Some(bits)) = (answer["challenge"].as_str(), answer["bits"].as_u64())
        else {
            console.warn(format!(
                "The review service is not taking reviews now (HTTP {status}); it is tried again later"
            ));
            return Err(None);
        };
        if challenge.len() > 100 {
            return Err(None);
        }
        let challenge = challenge.to_owned();
        let bits = u8::try_from(bits).unwrap_or(MAX_BITS).min(MAX_BITS);
        let solving = challenge.clone();
        let nonce = tokio::task::spawn_blocking(move || solve(&solving, bits))
            .await
            .map_err(|_| None)?;
        let mut body = pending.body.clone();
        body["challenge"] = json!(challenge);
        body["nonce"] = json!(nonce);
        let (status, answer) = self
            .transport
            .call(
                "POST",
                "/v1/reviews",
                Some(serde_json::to_vec(&body).unwrap_or_default()),
                headers,
            )
            .await
            .map_err(|e| {
                console.warn(format!(
                    "Sending a review failed ({e}); it is tried again later"
                ));
                None
            })?;
        match status {
            201 => {
                let answer: Value = serde_json::from_slice(&answer).unwrap_or(Value::Null);
                match (answer["id"].as_i64(), answer["key"].as_str()) {
                    (Some(id), Some(key))
                        if id > 0
                            && key.len() == 64
                            && key.bytes().all(|b| b.is_ascii_hexdigit()) =>
                    {
                        Ok((id, key.to_owned()))
                    }
                    _ => Err(Some(status)),
                }
            }
            400 | 413 | 415 | 422 => Err(Some(status)),
            _ => {
                console.warn(format!(
                    "Sending a review failed (HTTP {status}); it is tried again later"
                ));
                Err(None)
            }
        }
    }

    /// One pass of the uploader: reviews whose time has come.
    pub async fn flush(&self, state: &AppState, accounts: &dyn Accounts) {
        let Some(root) = state.data_root() else {
            return;
        };
        if state.network() == crate::state::dto::NetworkStatus::Offline {
            return;
        }
        let dir = outbox(&root);
        let due: Vec<(String, Pending)> = fs::read_dir(&dir)
            .map(|entries| {
                entries
                    .flatten()
                    .filter_map(|e| {
                        let name = e.file_name().to_string_lossy().into_owned();
                        let id = name.strip_suffix(".json")?.to_owned();
                        let pending: Pending =
                            serde_json::from_slice(&fs::read(e.path()).ok()?).ok()?;
                        Some((id, pending))
                    })
                    .collect()
            })
            .unwrap_or_default();
        for (id, pending) in due {
            if Utc::now().signed_duration_since(pending.created)
                > chrono::Duration::days(OUTBOX_DAYS)
            {
                let _ = fs::remove_file(dir.join(format!("{id}.json")));
                continue;
            }
            if pending.next <= Utc::now() {
                self.deliver(state, accounts, &root, &id).await;
            }
        }
    }

    /// Waits until a review waits for sending, or `every` passed.
    pub async fn wait(&self, every: Duration) {
        let _ = tokio::time::timeout(every, self.wake.notified()).await;
    }

    /// The player's reviews. Asks the service for answers when `refresh`, or
    /// when a review is open and the last check is 6 hours old.
    pub async fn mine(
        &self,
        state: &AppState,
        accounts: &dyn Accounts,
        refresh: bool,
    ) -> LauncherResult<Mine> {
        let root = state.data_root().ok_or_else(unavailable)?;
        let mut file = read_sent(&root);
        let open = file
            .reviews
            .iter()
            .any(|r| matches!(r.status.as_str(), "sent" | "read"));
        let due = file.checked.is_none_or(|at| {
            Utc::now().signed_duration_since(at) > chrono::Duration::hours(CHECK_EVERY_HOURS)
        });
        let linked = file.reviews.iter().any(|r| r.account);
        if (refresh && (!file.reviews.is_empty() || linked) || open && due)
            && state.network() != crate::state::dto::NetworkStatus::Offline
        {
            let _busy = self.busy.lock().await;
            if self.check(state, accounts, &mut file).await {
                file.checked = Some(Utc::now());
                write_sent(&root, &file)?;
            }
        }
        let waiting = fs::read_dir(outbox(&root))
            .map(|e| e.flatten().count())
            .unwrap_or(0);
        let unseen = file
            .reviews
            .iter()
            .filter(|r| r.reply.is_some() && !r.reply_seen)
            .count();
        let reviews = file
            .reviews
            .into_iter()
            .map(|r| Sent {
                key: String::new(),
                ..r
            })
            .collect();
        Ok(Mine {
            reviews,
            unseen,
            waiting,
        })
    }

    /// Asks the service for the reviews' states; true when it answered.
    async fn check(&self, state: &AppState, accounts: &dyn Accounts, file: &mut SentFile) -> bool {
        let mut headers = Vec::new();
        if file.reviews.iter().any(|r| r.account)
            && let Token::Token(token) = accounts.token(state).await
        {
            headers.push(("authorization", format!("Bearer {token}")));
        }
        let keys: Vec<Value> = file
            .reviews
            .iter()
            .filter(|r| r.status != "gone")
            .take(50)
            .map(|r| json!({ "id": r.id, "key": r.key }))
            .collect();
        let body = serde_json::to_vec(&json!({ "reviews": keys })).unwrap_or_default();
        let Ok((200, answer)) = self
            .transport
            .call("POST", "/v1/status", Some(body), headers)
            .await
        else {
            return false;
        };
        let answer: Value = serde_json::from_slice(&answer).unwrap_or(Value::Null);
        for item in answer["reviews"].as_array().into_iter().flatten() {
            let Some(id) = item["id"].as_i64() else {
                continue;
            };
            let Some(review) = file.reviews.iter_mut().find(|r| r.id == id) else {
                // A review linked to the account, sent from another computer:
                // its key is not here, so only its state is shown.
                continue;
            };
            let status = item["status"].as_str().unwrap_or("read");
            if !matches!(status, "sent" | "read" | "answered" | "closed" | "gone") {
                continue;
            }
            review.status = status.to_owned();
            let reply: Option<String> = item["reply"].as_str().map(|r| {
                r.chars()
                    .filter(|c| !c.is_control() || *c == '\n')
                    .take(5000)
                    .collect()
            });
            if reply.is_some() && reply != review.reply {
                review.reply = reply;
                review.reply_seen = false;
                review.replied = item["replied"]
                    .as_i64()
                    .and_then(|t| DateTime::from_timestamp(t, 0));
            }
        }
        true
    }

    /// The player has read the answers.
    pub fn seen(&self, state: &AppState) -> LauncherResult<()> {
        let root = state.data_root().ok_or_else(unavailable)?;
        let mut file = read_sent(&root);
        for review in &mut file.reviews {
            review.reply_seen = true;
        }
        write_sent(&root, &file)
    }

    /// Deletes a review on the service and here.
    pub async fn delete(&self, state: &AppState, id: i64) -> LauncherResult<()> {
        let root = state.data_root().ok_or_else(unavailable)?;
        let file = read_sent(&root);
        let review =
            file.reviews.iter().find(|r| r.id == id).ok_or_else(|| {
                LauncherError::invalid_input("This review is not on this computer.")
            })?;
        let key = review.key.clone();
        let _busy = self.busy.lock().await;
        let result = self
            .transport
            .call(
                "DELETE",
                &format!("/v1/reviews/{id}"),
                None,
                vec![("x-review-key", key)],
            )
            .await;
        match result {
            Ok((204 | 404, _)) => {
                let mut file = read_sent(&root);
                file.reviews.retain(|r| r.id != id);
                write_sent(&root, &file)
            }
            _ => Err(LauncherError::new(
                crate::error::ErrorCode::NetworkError,
                "The review service could not be reached. Try again later.",
            )),
        }
    }
}

#[cfg(test)]
mod tests;
