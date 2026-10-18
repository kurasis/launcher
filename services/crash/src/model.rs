//! What the launcher sends (D-061) and how it is checked before it is stored.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Files in one report, and their total size.
pub const MAX_FILES: usize = 12;
pub const MAX_TEXT_BYTES: usize = 8 * 1024 * 1024;
const MAX_SUMMARY_BYTES: usize = 16 * 1024;
const MAX_NOTE_CHARS: usize = 2000;
/// Events in one statistics batch.
pub const MAX_EVENTS: usize = 50;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Launcher {
    pub version: String,
    pub os: String,
    #[serde(default)]
    pub os_version: String,
    pub arch: String,
    #[serde(default)]
    pub locale: String,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Game {
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub loader: String,
    #[serde(default)]
    pub loader_version: String,
    #[serde(default)]
    pub java: String,
    #[serde(default)]
    pub mods: u32,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct File {
    pub name: String,
    pub text: String,
}

/// One report, sent only when the player chooses to.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub schema: u32,
    pub install_id: String,
    pub launcher: Launcher,
    /// `game` or `launcher`.
    pub source: String,
    /// The analyser's cause (`missing_dependency`), or `panic`, `frontend`,
    /// `unclean_exit` for the launcher.
    pub kind: String,
    /// Groups reports of the same failure: normalized top stack frames.
    #[serde(default)]
    pub signature: String,
    #[serde(default)]
    pub analyzer_version: u32,
    #[serde(default)]
    pub game: Option<Game>,
    #[serde(default)]
    pub summary: Value,
    #[serde(default)]
    pub note: String,
    #[serde(default)]
    pub files: Vec<File>,
}

/// One counted event of the anonymous statistics (stage 3): no install ID,
/// no files.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase")]
pub struct Event {
    pub source: String,
    pub kind: String,
    #[serde(default)]
    pub signature: String,
    pub launcher_version: String,
    pub os: String,
    #[serde(default)]
    pub game_version: String,
    #[serde(default)]
    pub loader: String,
    #[serde(default)]
    pub loader_version: String,
    #[serde(default)]
    pub analyzer_version: u32,
    #[serde(default)]
    pub exit_code: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Stats {
    pub schema: u32,
    pub events: Vec<Event>,
}

/// "Did this explanation help?"
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Feedback {
    pub schema: u32,
    pub kind: String,
    #[serde(default)]
    pub signature: String,
    pub helpful: bool,
    #[serde(default)]
    pub analyzer_version: u32,
    pub launcher_version: String,
}

fn word(value: &str, max: usize) -> bool {
    !value.is_empty()
        && value.len() <= max
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.' | b'+'))
}

fn text(value: &str, max: usize) -> bool {
    value.len() <= max && !value.chars().any(|c| c.is_control())
}

fn source(value: &str) -> bool {
    matches!(value, "game" | "launcher")
}

impl Report {
    /// Why the report is refused, if it is.
    pub fn problem(&self) -> Option<&'static str> {
        if self.schema != 1 {
            return Some("unknown schema");
        }
        if uuid::Uuid::parse_str(&self.install_id).is_err() {
            return Some("install id");
        }
        let l = &self.launcher;
        if !word(&l.version, 40) || !word(&l.os, 20) || !word(&l.arch, 20) {
            return Some("launcher");
        }
        if !text(&l.os_version, 80) || !text(&l.locale, 20) {
            return Some("launcher");
        }
        if !source(&self.source) || !word(&self.kind, 40) {
            return Some("source or kind");
        }
        if !text(&self.signature, 300) {
            return Some("signature");
        }
        if let Some(g) = &self.game
            && (!text(&g.version, 60)
                || !text(&g.loader, 20)
                || !text(&g.loader_version, 60)
                || !text(&g.java, 60))
        {
            return Some("game");
        }
        if serde_json::to_vec(&self.summary).map_or(true, |s| s.len() > MAX_SUMMARY_BYTES) {
            return Some("summary too large");
        }
        if self.note.chars().count() > MAX_NOTE_CHARS {
            return Some("note too long");
        }
        if self.files.len() > MAX_FILES {
            return Some("too many files");
        }
        if self.files.iter().any(|f| !word(&f.name, 80)) {
            return Some("file name");
        }
        let total: usize = self.files.iter().map(|f| f.text.len()).sum();
        if total > MAX_TEXT_BYTES {
            return Some("files too large");
        }
        None
    }
}

impl Event {
    pub fn problem(&self) -> Option<&'static str> {
        let ok = source(&self.source)
            && word(&self.kind, 40)
            && text(&self.signature, 300)
            && word(&self.launcher_version, 40)
            && word(&self.os, 20)
            && text(&self.game_version, 60)
            && text(&self.loader, 20)
            && text(&self.loader_version, 60);
        (!ok).then_some("event")
    }
}

impl Feedback {
    pub fn problem(&self) -> Option<&'static str> {
        let ok = self.schema == 1
            && word(&self.kind, 40)
            && text(&self.signature, 300)
            && word(&self.launcher_version, 40);
        (!ok).then_some("feedback")
    }
}
