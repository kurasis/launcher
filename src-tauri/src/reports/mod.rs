//! Crash reports the player chooses to send, and anonymous statistics they
//! agreed to (D-061 stages 2 and 3).
//!
//! A report is first a [`Preview`]: scrubbed files the player reads and picks
//! from. Sending puts it in `reports/outbox/` (gzip); the uploader posts it to
//! the crash service when the computer is online and tries again later after a
//! failure. Nothing is sent without the player's press, except the statistics
//! and feedback of a player who turned them on in Settings → Privacy.

pub mod scrub;

use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::error::{LauncherError, LauncherResult};
use crate::state::AppState;
use crate::storage::paths::DataRoot;

pub const REPORTS_URL: &str = "https://crash.lapi.sh/v1/reports";
pub const STATS_URL: &str = "https://crash.lapi.sh/v1/stats";
pub const FEEDBACK_URL: &str = "https://crash.lapi.sh/v1/feedback";
/// The newest part of one file kept in a report.
const MAX_FILE_BYTES: usize = 512 * 1024;
/// A sent report, compressed.
const MAX_GZIP_BYTES: usize = 1024 * 1024;
/// Lines of game output kept.
const OUTPUT_LINES: usize = 1500;
/// Unsent reports older than this are dropped.
const OUTBOX_DAYS: i64 = 30;
/// Statistics events kept while offline.
const MAX_QUEUED_EVENTS: usize = 500;

/// What a report is about.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Subject {
    /// A game run that crashed.
    #[serde(rename_all = "camelCase")]
    Game { run_id: String, instance_id: String },
    /// A failure of the launcher itself (journal.rs).
    #[serde(rename_all = "camelCase")]
    Launcher { failure_id: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GameInfo {
    pub version: String,
    pub loader: String,
    pub loader_version: String,
    pub java: String,
    pub mods: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct File {
    pub name: String,
    pub text: String,
}

/// What would be sent, for the player to read first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    pub id: String,
    /// `game` or `launcher`.
    pub source: String,
    pub kind: String,
    pub signature: String,
    pub game: Option<GameInfo>,
    pub files: Vec<File>,
    /// How much the scrubbing replaced.
    pub removed: scrub::Counts,
    #[serde(skip)]
    summary: Value,
    #[serde(skip)]
    analyzer_version: u32,
    #[serde(skip)]
    failure_id: Option<String>,
}

/// The result of pressing Send.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Delivery {
    /// The service has it.
    Sent,
    /// Offline or the service is unreachable: it goes later by itself.
    Queued,
}

/// One statistics event (D-061 stage 3); the service counts them per day.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Event {
    pub source: String,
    pub kind: String,
    pub signature: String,
    pub launcher_version: String,
    pub os: String,
    pub game_version: String,
    pub loader: String,
    pub loader_version: String,
    pub analyzer_version: u32,
    pub exit_code: Option<i64>,
}

/// Posts JSON (gzip) to the service; the real one checks the host and uses
/// HTTPS, tests record what would be sent.
pub trait Poster: Send + Sync {
    fn post<'a>(
        &'a self,
        url: &'a str,
        gzip_json: Vec<u8>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<u16, String>> + Send + 'a>>;
}

pub struct HttpPoster(crate::net::PublicClient);

impl HttpPoster {
    pub fn new(app_version: &str) -> LauncherResult<Self> {
        crate::net::PublicClient::new(crate::net::SourcePolicy::CrashReports, app_version).map(Self)
    }
}

impl Poster for HttpPoster {
    fn post<'a>(
        &'a self,
        url: &'a str,
        gzip_json: Vec<u8>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<u16, String>> + Send + 'a>> {
        Box::pin(async move {
            let url = url::Url::parse(url).map_err(|e| e.to_string())?;
            let request = self
                .0
                .post(&url)
                .map_err(|e| e.message)?
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .header(reqwest::header::CONTENT_ENCODING, "gzip")
                .timeout(Duration::from_secs(30))
                .body(gzip_json);
            let response = request.send().await.map_err(|e| {
                crate::net::map_transport_error(&e)
                    .safe_details
                    .and_then(|d| d.get("transport").map(|t| format!("{t:?}")))
                    .unwrap_or_else(|| "transport".to_owned())
            })?;
            Ok(response.status().as_u16())
        })
    }
}

fn gzip(bytes: &[u8]) -> Vec<u8> {
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    let _ = gz.write_all(bytes);
    gz.finish().unwrap_or_default()
}

/// The newest `max` bytes of a text, cut at a line start.
fn newest(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_owned();
    }
    let mut start = text.len() - max;
    while !text.is_char_boundary(start) {
        start += 1;
    }
    let rest = &text[start..];
    let rest = rest.find('\n').map_or(rest, |i| &rest[i + 1..]);
    format!("[… earlier lines left out]\n{rest}")
}

fn read_text(path: &Path) -> Option<String> {
    fs::read(path)
        .ok()
        .map(|b| String::from_utf8_lossy(&b).into_owned())
}

fn valid_uuid(id: &str) -> LauncherResult<uuid::Uuid> {
    uuid::Uuid::parse_str(id)
        .map_err(|_| LauncherError::invalid_input("This report does not exist."))
}

fn outbox(root: &DataRoot) -> PathBuf {
    root.path().join("reports").join("outbox")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Pending {
    created: DateTime<Utc>,
    attempts: u32,
    next: DateTime<Utc>,
    #[serde(default)]
    failure_id: Option<String>,
}

/// Previews waiting for the player's choice, and the uploader's wake-up.
pub struct Reports {
    previews: Mutex<HashMap<String, Preview>>,
    poster: Box<dyn Poster>,
    wake: tokio::sync::Notify,
}

impl Reports {
    pub fn new(poster: Box<dyn Poster>) -> Self {
        Self {
            previews: Mutex::new(HashMap::new()),
            poster,
            wake: tokio::sync::Notify::new(),
        }
    }

    fn context<'a>(
        state: &AppState,
        root: &'a DataRoot,
        home: Option<&'a Path>,
    ) -> scrub::Context<'a> {
        let accounts = state.accounts_file();
        let names = accounts
            .accounts
            .iter()
            .chain(accounts.active.iter())
            .map(|a| a.profile_name.clone())
            .chain(accounts.notoken_profiles.iter().map(|p| p.name.clone()))
            .collect();
        scrub::Context {
            names,
            home,
            data_root: root.path(),
        }
    }

    /// Builds what a report about `subject` would hold, scrubbed.
    pub fn preview(
        &self,
        state: &AppState,
        subject: &Subject,
        home: Option<&Path>,
    ) -> LauncherResult<Preview> {
        let root = state.data_root().ok_or_else(|| {
            LauncherError::new(
                crate::error::ErrorCode::StorageUnavailable,
                "Launcher data is unavailable.",
            )
        })?;
        let cx = Self::context(state, &root, home);
        let mut removed = scrub::Counts::default();
        let mut files = Vec::new();
        let mut add = |name: &str, text: &str| {
            let (text, counts) = scrub::text(&newest(text, MAX_FILE_BYTES), &cx);
            removed.add(counts);
            if !text.trim().is_empty() {
                files.push(File {
                    name: name.to_owned(),
                    text,
                });
            }
        };
        let preview = match subject {
            Subject::Game {
                run_id,
                instance_id,
            } => {
                let run = valid_uuid(run_id)?;
                let instance_id: crate::storage::paths::InstanceId = instance_id
                    .parse()
                    .map_err(|_| LauncherError::invalid_input("This report does not exist."))?;
                let logs = root.run_logs(run);
                if !logs.is_dir() {
                    return Err(LauncherError::invalid_input(
                        "The output of this game run is no longer kept.",
                    ));
                }
                let kept: Value = read_text(&logs.join(crate::launch::crash::DIAGNOSIS_FILE))
                    .and_then(|t| serde_json::from_str(&t).ok())
                    .unwrap_or(Value::Null);
                let diagnosis: Option<crate::launch::crash::CrashDiagnosis> =
                    serde_json::from_value(kept["diagnosis"].clone()).ok();
                if let Some(text) = read_text(&logs.join("output.log")) {
                    let lines: Vec<&str> = text.lines().collect();
                    let tail = lines[lines.len().saturating_sub(OUTPUT_LINES)..].join("\n");
                    add("output.log", &tail);
                }
                for name in [
                    crate::launch::crash::DIAGNOSIS_FILE,
                    crate::launch::crash::REPORT_COPY,
                    crate::launch::crash::JVM_ERROR_COPY,
                ] {
                    if let Some(text) = read_text(&logs.join(name)) {
                        add(name, &text);
                    }
                }
                let instance = state.instance(instance_id);
                let mods_dir = root.game_dir(instance_id).join("mods");
                let mut mods: Vec<String> = fs::read_dir(&mods_dir)
                    .map(|e| {
                        e.flatten()
                            .map(|e| e.file_name().to_string_lossy().into_owned())
                            .filter(|n| n.ends_with(".jar") || n.ends_with(".disabled"))
                            .collect()
                    })
                    .unwrap_or_default();
                mods.sort();
                if !mods.is_empty() {
                    add("mods.txt", &mods.join("\n"));
                }
                let game = instance.map(|i| GameInfo {
                    version: i.version_id.clone(),
                    loader: i
                        .loader
                        .as_ref()
                        .map(|l| crate::console::name(&l.kind))
                        .unwrap_or_default(),
                    loader_version: i
                        .loader
                        .as_ref()
                        .map(|l| l.version.clone())
                        .unwrap_or_default(),
                    java: String::new(),
                    mods: u32::try_from(mods.iter().filter(|m| m.ends_with(".jar")).count())
                        .unwrap_or(u32::MAX),
                });
                let summary = diagnosis
                    .as_ref()
                    .map(|d| {
                        let details = serde_json::to_string(&d.details).unwrap_or_default();
                        let (details, _) = scrub::text(&details, &cx);
                        json!({
                            "details": serde_json::from_str::<Value>(&details).unwrap_or(Value::Null),
                            "suspects": d.suspects,
                            "exitCode": d.details.get("exitCode"),
                        })
                    })
                    .unwrap_or(Value::Null);
                Preview {
                    id: String::new(),
                    source: "game".to_owned(),
                    kind: diagnosis
                        .as_ref()
                        .map_or_else(|| "unknown".to_owned(), |d| crate::console::name(&d.kind)),
                    signature: diagnosis
                        .as_ref()
                        .map(|d| d.compute_signature())
                        .unwrap_or_default(),
                    game,
                    files: Vec::new(),
                    removed: scrub::Counts::default(),
                    summary,
                    analyzer_version: kept["analyzerVersion"].as_u64().map_or(0, |v| v as u32),
                    failure_id: None,
                }
            }
            Subject::Launcher { failure_id } => {
                let logs = root.logs_dir();
                let failure = crate::journal::failures(&logs)
                    .into_iter()
                    .find(|f| f.id == *failure_id)
                    .ok_or_else(|| LauncherError::invalid_input("This report does not exist."))?;
                let mut record = failure.clone();
                record.log_tail = Vec::new();
                add(
                    "failure.json",
                    &serde_json::to_string_pretty(&record).unwrap_or_default(),
                );
                let log = if failure.log_tail.is_empty() {
                    crate::journal::tail(&logs.join(crate::journal::LOG_FILE), 300).join("\n")
                } else {
                    failure.log_tail.join("\n")
                };
                add("launcher.log", &log);
                Preview {
                    id: String::new(),
                    source: "launcher".to_owned(),
                    kind: crate::console::name(&failure.kind),
                    signature: failure.signature(),
                    game: None,
                    files: Vec::new(),
                    removed: scrub::Counts::default(),
                    summary: json!({ "version": failure.version }),
                    analyzer_version: 0,
                    failure_id: Some(failure.id.clone()),
                }
            }
        };
        let preview = Preview {
            id: uuid::Uuid::new_v4().to_string(),
            files,
            removed,
            ..preview
        };
        let mut previews = self.previews.lock().unwrap_or_else(PoisonError::into_inner);
        if previews.len() > 20 {
            previews.clear();
        }
        previews.insert(preview.id.clone(), preview.clone());
        Ok(preview)
    }

    /// Puts the chosen files of a preview in the outbox and tries to send it.
    pub async fn send(
        &self,
        state: &AppState,
        preview_id: &str,
        files: &[String],
        note: &str,
        home: Option<&Path>,
    ) -> LauncherResult<Delivery> {
        let preview = self
            .previews
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(preview_id)
            .ok_or_else(|| {
                LauncherError::invalid_input("This report preview has expired. Open it again.")
            })?;
        let root = state.data_root().ok_or_else(|| {
            LauncherError::new(
                crate::error::ErrorCode::StorageUnavailable,
                "Launcher data is unavailable.",
            )
        })?;
        let cx = Self::context(state, &root, home);
        let note: String = note.chars().take(2000).collect();
        let (note, _) = scrub::text(&note, &cx);
        let body = json!({
            "schema": 1,
            "installId": state.install_id()?,
            "launcher": {
                "version": state.app_version(),
                "os": std::env::consts::OS,
                "osVersion": "",
                "arch": std::env::consts::ARCH,
                "locale": state.settings().language.as_str(),
            },
            "source": preview.source,
            "kind": preview.kind,
            "signature": preview.signature,
            "analyzerVersion": preview.analyzer_version,
            "game": preview.game,
            "summary": preview.summary,
            "note": note,
            "files": preview.files.iter().filter(|f| files.contains(&f.name)).collect::<Vec<_>>(),
        });
        let compressed = gzip(&serde_json::to_vec(&body).unwrap_or_default());
        if compressed.len() > MAX_GZIP_BYTES {
            return Err(LauncherError::invalid_input(
                "This report is too large. Leave out the largest file and send it again.",
            ));
        }
        let dir = outbox(&root);
        fs::create_dir_all(&dir).map_err(|e| LauncherError::from_io(&e, "Saving the report"))?;
        let id = uuid::Uuid::new_v4().to_string();
        crate::storage::atomic::write(&dir.join(format!("{id}.json.gz")), &compressed)
            .map_err(|e| LauncherError::from_io(&e, "Saving the report"))?;
        let pending = Pending {
            created: Utc::now(),
            attempts: 0,
            next: Utc::now(),
            failure_id: preview.failure_id.clone(),
        };
        crate::storage::atomic::write(
            &dir.join(format!("{id}.meta.json")),
            &serde_json::to_vec(&pending).unwrap_or_default(),
        )
        .map_err(|e| LauncherError::from_io(&e, "Saving the report"))?;
        state.console().info(format!(
            "Crash report about {} ({}) saved for sending",
            preview.kind, preview.source
        ));
        if self.deliver(state, &root, &id).await {
            Ok(Delivery::Sent)
        } else {
            self.wake.notify_one();
            Ok(Delivery::Queued)
        }
    }

    /// Sends one outbox report now; true once the service has it or refused it
    /// for good.
    async fn deliver(&self, state: &AppState, root: &DataRoot, id: &str) -> bool {
        let dir = outbox(root);
        let meta_path = dir.join(format!("{id}.meta.json"));
        let body_path = dir.join(format!("{id}.json.gz"));
        let Some(mut meta) = fs::read(&meta_path)
            .ok()
            .and_then(|b| serde_json::from_slice::<Pending>(&b).ok())
        else {
            return false;
        };
        let Ok(body) = fs::read(&body_path) else {
            let _ = fs::remove_file(&meta_path);
            return false;
        };
        if state.network() == crate::state::dto::NetworkStatus::Offline {
            return false;
        }
        let result = self.poster.post(REPORTS_URL, body).await;
        let console = state.console();
        let done = match result {
            Ok(201) => {
                console.info("Crash report sent to the developers");
                if let Some(failure) = &meta.failure_id {
                    let _ = crate::journal::mark_reported(&root.logs_dir(), failure);
                }
                true
            }
            Ok(status @ (400 | 413 | 415 | 422)) => {
                console.warn(format!(
                    "The crash service refused a report (HTTP {status}); it was dropped"
                ));
                true
            }
            Ok(status) => {
                console.warn(format!(
                    "Sending a crash report failed (HTTP {status}); it is tried again later"
                ));
                false
            }
            Err(error) => {
                console.warn(format!(
                    "Sending a crash report failed ({error}); it is tried again later"
                ));
                false
            }
        };
        if done {
            let _ = fs::remove_file(&body_path);
            let _ = fs::remove_file(&meta_path);
        } else {
            meta.attempts += 1;
            let minutes = 2i64.saturating_pow(meta.attempts.min(9)).min(360);
            meta.next = Utc::now() + chrono::Duration::minutes(minutes);
            let _ = crate::storage::atomic::write(
                &meta_path,
                &serde_json::to_vec(&meta).unwrap_or_default(),
            );
        }
        done
    }

    /// One pass of the uploader: due reports, then queued statistics.
    pub async fn flush(&self, state: &AppState) {
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
                        let id = name.strip_suffix(".meta.json")?.to_owned();
                        let meta: Pending =
                            serde_json::from_slice(&fs::read(e.path()).ok()?).ok()?;
                        Some((id, meta))
                    })
                    .collect()
            })
            .unwrap_or_default();
        for (id, meta) in due {
            if Utc::now().signed_duration_since(meta.created) > chrono::Duration::days(OUTBOX_DAYS)
            {
                let _ = fs::remove_file(dir.join(format!("{id}.json.gz")));
                let _ = fs::remove_file(dir.join(format!("{id}.meta.json")));
                continue;
            }
            if meta.next <= Utc::now() {
                self.deliver(state, &root, &id).await;
            }
        }
        self.flush_stats(state, &root).await;
    }

    async fn flush_stats(&self, state: &AppState, root: &DataRoot) {
        if !state.settings().share_stats {
            return;
        }
        let path = stats_queue(root);
        let events: Vec<Event> = fs::read(&path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        if events.is_empty() {
            return;
        }
        let batch: Vec<&Event> = events.iter().take(50).collect();
        let body =
            gzip(&serde_json::to_vec(&json!({ "schema": 1, "events": batch })).unwrap_or_default());
        if let Ok(204 | 400 | 422) = self.poster.post(STATS_URL, body).await {
            let rest: Vec<&Event> = events.iter().skip(batch.len()).collect();
            let _ = crate::storage::atomic::write(
                &path,
                &serde_json::to_vec(&rest).unwrap_or_default(),
            );
        }
    }

    /// "Did this help?" under an explanation (stage 3): sent at once, only the
    /// cause and its signature.
    pub async fn feedback(
        &self,
        state: &AppState,
        kind: &str,
        signature: &str,
        helpful: bool,
    ) -> LauncherResult<()> {
        let body = json!({
            "schema": 1,
            "kind": kind,
            "signature": signature,
            "helpful": helpful,
            "analyzerVersion": crate::launch::crash::ANALYZER_VERSION,
            "launcherVersion": state.app_version(),
        });
        match self
            .poster
            .post(
                FEEDBACK_URL,
                gzip(&serde_json::to_vec(&body).unwrap_or_default()),
            )
            .await
        {
            Ok(204) => Ok(()),
            _ => Err(LauncherError::new(
                crate::error::ErrorCode::NetworkError,
                "The answer could not be sent. Try again when you are online.",
            )),
        }
    }

    /// Waits until a report is queued or `every` passes.
    pub async fn wait(&self, every: Duration) {
        let _ = tokio::time::timeout(every, self.wake.notified()).await;
    }
}

fn stats_queue(root: &DataRoot) -> PathBuf {
    root.path().join("reports").join("stats.json")
}

/// Counts an event for the anonymous statistics, when the player turned them
/// on; sent by the uploader.
pub fn count(state: &AppState, event: Event) {
    if !state.settings().share_stats {
        return;
    }
    let Some(root) = state.data_root() else {
        return;
    };
    let path = stats_queue(&root);
    let mut events: Vec<Event> = fs::read(&path)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default();
    if events.len() >= MAX_QUEUED_EVENTS {
        return;
    }
    events.push(event);
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = crate::storage::atomic::write(&path, &serde_json::to_vec(&events).unwrap_or_default());
}

#[cfg(test)]
mod tests;
