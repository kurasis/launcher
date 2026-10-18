//! The launcher's own record of what went wrong (D-061 stage 1), kept in the
//! data folder's `logs` and never sent anywhere by itself.
//!
//! - `launcher.log`: the console's launcher entries, rotated at
//!   [`MAX_LOG_BYTES`] with [`MAX_LOG_FILES`] files.
//! - `launcher-crashes/*.json`: one [`Failure`] per panic, error in the window,
//!   or start after an exit that was not clean; the newest [`MAX_FAILURES`] stay.
//! - `session.json`: exists while the launcher runs. Finding one at start means
//!   the last session was killed, hung or lost power.
//!
//! Texts here are what the launcher itself wrote: console entries never hold a
//! token (console.rs), and the export and reports sanitize again on the way out.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock, PoisonError};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub const LOG_FILE: &str = "launcher.log";
pub const MAX_LOG_BYTES: u64 = 2 * 1024 * 1024;
pub const MAX_LOG_FILES: usize = 5;
pub const FAILURES_DIR: &str = "launcher-crashes";
pub const MAX_FAILURES: usize = 30;
const SESSION_FILE: &str = "session.json";
/// Lines of `launcher.log` kept with a failure.
const TAIL_LINES: usize = 200;
/// Errors from the window recorded per session; a page in a loop stays bounded.
const MAX_FRONTEND_PER_SESSION: usize = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureKind {
    /// A Rust panic.
    Panic,
    /// An error in the window's code.
    Frontend,
    /// The last session ended without closing normally.
    UncleanExit,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Failure {
    pub id: String,
    pub kind: FailureKind,
    pub at: DateTime<Utc>,
    pub version: String,
    pub os: String,
    pub arch: String,
    pub message: String,
    /// Source place of a panic, `file:line`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub place: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread: Option<String>,
    /// A panic's backtrace or a window error's stack.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stack: Option<String>,
    /// The last lines of `launcher.log` when it happened.
    #[serde(default)]
    pub log_tail: Vec<String>,
    /// Set once the player sent it or chose not to (D-061 stage 2).
    #[serde(default)]
    pub reported: bool,
    /// Set once counted in the anonymous statistics (stage 3).
    #[serde(default)]
    pub counted: bool,
}

impl Failure {
    pub fn new(kind: FailureKind, version: &str, message: impl Into<String>) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            kind,
            at: Utc::now(),
            version: version.to_owned(),
            os: std::env::consts::OS.to_owned(),
            arch: std::env::consts::ARCH.to_owned(),
            message: message.into(),
            place: None,
            thread: None,
            stack: None,
            log_tail: Vec::new(),
            reported: false,
            counted: false,
        }
    }

    /// Groups failures of the same cause: the kind and the place, or the
    /// message's first line without numbers.
    pub fn signature(&self) -> String {
        let what = self.place.clone().unwrap_or_else(|| {
            self.message
                .lines()
                .next()
                .unwrap_or_default()
                .chars()
                .filter(|c| !c.is_ascii_digit())
                .take(120)
                .collect()
        });
        format!("{}:{what}", crate::console::name(&self.kind))
    }
}

/// The launcher log file, rotated by size.
pub struct LogFile {
    dir: PathBuf,
    file: Option<File>,
    written: u64,
}

fn log_name(index: usize) -> String {
    if index == 0 {
        LOG_FILE.to_owned()
    } else {
        format!("launcher.{index}.log")
    }
}

impl LogFile {
    pub fn open(logs_dir: &Path) -> Self {
        let _ = fs::create_dir_all(logs_dir);
        let path = logs_dir.join(LOG_FILE);
        let written = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .ok();
        Self {
            dir: logs_dir.to_path_buf(),
            file,
            written,
        }
    }

    fn rotate(&mut self) {
        self.file = None;
        let _ = fs::remove_file(self.dir.join(log_name(MAX_LOG_FILES - 1)));
        for index in (0..MAX_LOG_FILES - 1).rev() {
            let _ = fs::rename(
                self.dir.join(log_name(index)),
                self.dir.join(log_name(index + 1)),
            );
        }
        self.file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.dir.join(LOG_FILE))
            .ok();
        self.written = 0;
    }

    /// Appends one line; a failed write is dropped (the console still has it).
    pub fn write_line(&mut self, line: &str) {
        if self.written >= MAX_LOG_BYTES {
            self.rotate();
        }
        if let Some(file) = &mut self.file
            && writeln!(file, "{line}").is_ok()
        {
            self.written += line.len() as u64 + 1;
        }
    }
}

/// The last `lines` lines of a file, reading at most its last 256 KiB.
pub fn tail(path: &Path, lines: usize) -> Vec<String> {
    let Ok(mut file) = File::open(path) else {
        return Vec::new();
    };
    let length = file.metadata().map(|m| m.len()).unwrap_or(0);
    let _ = file.seek(SeekFrom::Start(length.saturating_sub(256 * 1024)));
    let mut bytes = Vec::new();
    let _ = file.read_to_end(&mut bytes);
    let text = String::from_utf8_lossy(&bytes);
    let all: Vec<&str> = text.lines().collect();
    all[all.len().saturating_sub(lines)..]
        .iter()
        .map(|l| (*l).to_owned())
        .collect()
}

/// Writes a failure record and keeps the newest [`MAX_FAILURES`].
pub fn record(logs_dir: &Path, failure: &Failure) -> io::Result<PathBuf> {
    let dir = logs_dir.join(FAILURES_DIR);
    fs::create_dir_all(&dir)?;
    let name = format!(
        "{}-{}-{}.json",
        failure.at.format("%Y%m%dT%H%M%S"),
        crate::console::name(&failure.kind),
        &failure.id[..8]
    );
    let path = dir.join(name);
    let json = serde_json::to_vec_pretty(failure).map_err(io::Error::other)?;
    crate::storage::atomic::write(&path, &json)?;
    let mut files: Vec<_> = fs::read_dir(&dir)?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    files.sort();
    for old in files.iter().take(files.len().saturating_sub(MAX_FAILURES)) {
        let _ = fs::remove_file(old);
    }
    Ok(path)
}

/// Failure records, newest first.
pub fn failures(logs_dir: &Path) -> Vec<Failure> {
    let Ok(entries) = fs::read_dir(logs_dir.join(FAILURES_DIR)) else {
        return Vec::new();
    };
    let mut list: Vec<Failure> = entries
        .flatten()
        .filter_map(|e| fs::read(e.path()).ok())
        .filter_map(|bytes| serde_json::from_slice(&bytes).ok())
        .collect();
    list.sort_by_key(|f| std::cmp::Reverse(f.at));
    list
}

/// Changes one failure record.
pub fn update(logs_dir: &Path, id: &str, change: impl FnOnce(&mut Failure)) -> io::Result<()> {
    for entry in fs::read_dir(logs_dir.join(FAILURES_DIR))?.flatten() {
        let Ok(bytes) = fs::read(entry.path()) else {
            continue;
        };
        if let Ok(mut failure) = serde_json::from_slice::<Failure>(&bytes)
            && failure.id == id
        {
            change(&mut failure);
            let json = serde_json::to_vec_pretty(&failure).map_err(io::Error::other)?;
            return crate::storage::atomic::write(&entry.path(), &json);
        }
    }
    Err(io::ErrorKind::NotFound.into())
}

/// Marks a failure as sent or declined, so the launcher stops offering it.
pub fn mark_reported(logs_dir: &Path, id: &str) -> io::Result<()> {
    update(logs_dir, id, |f| f.reported = true)
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Session {
    pid: u32,
    started_at: DateTime<Utc>,
    version: String,
}

/// Where failures go once the data folder is known; the panic hook reads it.
struct Journal {
    logs_dir: PathBuf,
    version: String,
    frontend: Mutex<Vec<String>>,
}

static JOURNAL: OnceLock<Journal> = OnceLock::new();

/// Starts the session in `logs_dir`: records an unclean end of the last one,
/// marks this one as running and lets the panic hook write there.
pub fn begin(logs_dir: &Path, version: &str) -> Option<Failure> {
    let unclean = begin_session(logs_dir, version);
    let _ = JOURNAL.set(Journal {
        logs_dir: logs_dir.to_path_buf(),
        version: version.to_owned(),
        frontend: Mutex::new(Vec::new()),
    });
    unclean
}

fn begin_session(logs_dir: &Path, version: &str) -> Option<Failure> {
    let marker = logs_dir.join(SESSION_FILE);
    let previous: Option<Session> = fs::read(&marker)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok());
    let unclean = previous.map(|last| {
        let mut failure = Failure::new(
            FailureKind::UncleanExit,
            &last.version,
            format!(
                "The launcher started at {} did not close normally: it was ended, froze, or the computer lost power.",
                last.started_at.format("%Y-%m-%d %H:%M:%S UTC")
            ),
        );
        failure.log_tail = tail(&logs_dir.join(LOG_FILE), TAIL_LINES);
        let _ = record(logs_dir, &failure);
        failure
    });
    let session = Session {
        pid: std::process::id(),
        started_at: Utc::now(),
        version: version.to_owned(),
    };
    if let Ok(json) = serde_json::to_vec(&session) {
        let _ = fs::create_dir_all(logs_dir);
        let _ = crate::storage::atomic::write(&marker, &json);
    }
    unclean
}

/// A clean exit: the next start finds no marker.
pub fn end(logs_dir: &Path) {
    let _ = fs::remove_file(logs_dir.join(SESSION_FILE));
}

/// Records panics, then lets the default hook print them. Installed before
/// anything else runs; it writes only once [`begin`] knows the folder.
pub fn install_panic_hook() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if let Some(journal) = JOURNAL.get() {
            let message = info
                .payload()
                .downcast_ref::<&str>()
                .map(|s| (*s).to_owned())
                .or_else(|| info.payload().downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "a panic without a message".to_owned());
            let mut failure = Failure::new(FailureKind::Panic, &journal.version, message);
            failure.place = info
                .location()
                .map(|l| format!("{}:{}", l.file(), l.line()));
            failure.thread = std::thread::current().name().map(str::to_owned);
            failure.stack = Some(std::backtrace::Backtrace::force_capture().to_string());
            failure.log_tail = tail(&journal.logs_dir.join(LOG_FILE), TAIL_LINES);
            let _ = record(&journal.logs_dir, &failure);
        }
        default(info);
    }));
}

/// An error in the window. Repeats of one message and more than
/// [`MAX_FRONTEND_PER_SESSION`] per session are not recorded.
pub fn frontend_error(message: &str, stack: Option<&str>) -> bool {
    let Some(journal) = JOURNAL.get() else {
        return false;
    };
    let message: String = message.chars().take(2000).collect();
    {
        let mut seen = journal
            .frontend
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if seen.len() >= MAX_FRONTEND_PER_SESSION || seen.contains(&message) {
            return false;
        }
        seen.push(message.clone());
    }
    let mut failure = Failure::new(FailureKind::Frontend, &journal.version, message);
    failure.stack = stack.map(|s| s.chars().take(8000).collect());
    failure.log_tail = tail(&journal.logs_dir.join(LOG_FILE), TAIL_LINES);
    record(&journal.logs_dir, &failure).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_log_rotates_and_keeps_five_files() {
        let dir = tempfile::tempdir().unwrap();
        let mut log = LogFile::open(dir.path());
        let line = "x".repeat(1024);
        for _ in 0..(MAX_LOG_BYTES / 1025 + 1) * 7 {
            log.write_line(&line);
        }
        let names: Vec<String> = fs::read_dir(dir.path())
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names.len(), MAX_LOG_FILES, "{names:?}");
        assert!(fs::metadata(dir.path().join(LOG_FILE)).unwrap().len() <= MAX_LOG_BYTES + 1025);
        assert_eq!(
            tail(&dir.path().join(LOG_FILE), 3),
            vec![line.clone(), line.clone(), line]
        );
    }

    #[test]
    fn a_session_left_open_is_found_at_the_next_start() {
        let dir = tempfile::tempdir().unwrap();
        let mut log = LogFile::open(dir.path());
        log.write_line("last words");
        assert_eq!(begin_session(dir.path(), "0.1.0"), None);
        // Killed: no end().
        let unclean = begin_session(dir.path(), "0.1.1").expect("unclean exit");
        assert_eq!(unclean.kind, FailureKind::UncleanExit);
        assert_eq!(unclean.version, "0.1.0", "the version that did not close");
        assert_eq!(unclean.log_tail, vec!["last words"]);
        assert_eq!(failures(dir.path()).len(), 1);
        end(dir.path());
        assert_eq!(begin_session(dir.path(), "0.1.1"), None);
    }

    #[test]
    fn records_are_bounded_listed_and_marked_sent() {
        let dir = tempfile::tempdir().unwrap();
        for i in 0..MAX_FAILURES + 3 {
            let mut f = Failure::new(FailureKind::Frontend, "0.1.0", format!("error {i}"));
            f.at = Utc::now() + chrono::Duration::seconds(i as i64);
            record(dir.path(), &f).unwrap();
        }
        let list = failures(dir.path());
        assert_eq!(list.len(), MAX_FAILURES);
        assert_eq!(list[0].message, format!("error {}", MAX_FAILURES + 2));
        mark_reported(dir.path(), &list[0].id).unwrap();
        assert!(failures(dir.path())[0].reported);
        assert_eq!(list[0].signature(), "frontend:error ");
    }
}
