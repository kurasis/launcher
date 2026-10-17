//! The console journal (D-037): what the launcher did and what the game printed,
//! for the console window.
//!
//! Entries live only in memory, in a ring of [`CAPACITY`] entries; the oldest are
//! dropped first. Game lines arrive already redacted (launch/redact.rs) and
//! launcher entries are written from values that never hold a token, an
//! authorization code or a password. The console window polls [`Console::read`]
//! with the last sequence number it has seen.

use std::collections::VecDeque;
use std::sync::{Mutex, PoisonError};

use chrono::{DateTime, Utc};
use serde::Serialize;

/// Entries kept in memory.
pub const CAPACITY: usize = 10_000;
/// Longer lines are cut (with a marker) so one runaway line cannot fill memory.
pub const MAX_LINE_BYTES: usize = 64 * 1024;
/// Entries returned by one read.
pub const PAGE: usize = 2_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    Launcher,
    Game,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    Info,
    Warn,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    /// Increases by one per entry for the whole session, starting at 1.
    pub seq: u64,
    pub at: DateTime<Utc>,
    pub source: Source,
    pub level: Level,
    /// Game lines from standard error.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub stderr: bool,
    pub text: String,
}

/// Entries after a sequence number.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Page {
    pub entries: Vec<Entry>,
    /// Pass back as `after` to continue.
    pub last_seq: u64,
    /// Entries after the requested point that were already dropped from memory.
    pub missed: u64,
}

#[derive(Default)]
struct Ring {
    entries: VecDeque<Entry>,
    last_seq: u64,
}

#[derive(Default)]
pub struct Console {
    ring: Mutex<Ring>,
}

/// Cuts `text` to at most `max` bytes on a character boundary.
fn bounded(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_owned();
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{} [… {} bytes cut]", &text[..end], text.len() - end)
}

/// Level of a game line from the logger's usual markers (`[main/WARN]`,
/// `[Render thread/ERROR]`, `WARN:`); everything else is information.
pub fn game_level(text: &str) -> Level {
    let head = &text[..text.len().min(160)];
    if head.contains("/ERROR]") || head.contains("/FATAL]") || head.contains("[SEVERE]") {
        Level::Error
    } else if head.contains("/WARN]") || head.contains("[WARNING]") {
        Level::Warn
    } else {
        Level::Info
    }
}

impl Console {
    fn push(&self, source: Source, level: Level, stderr: bool, text: &str) {
        let mut ring = self.ring.lock().unwrap_or_else(PoisonError::into_inner);
        ring.last_seq += 1;
        let entry = Entry {
            seq: ring.last_seq,
            at: Utc::now(),
            source,
            level,
            stderr,
            text: bounded(text, MAX_LINE_BYTES),
        };
        if ring.entries.len() == CAPACITY {
            ring.entries.pop_front();
        }
        ring.entries.push_back(entry);
    }

    pub fn info(&self, text: impl AsRef<str>) {
        self.push(Source::Launcher, Level::Info, false, text.as_ref());
    }

    pub fn warn(&self, text: impl AsRef<str>) {
        self.push(Source::Launcher, Level::Warn, false, text.as_ref());
    }

    pub fn error(&self, text: impl AsRef<str>) {
        self.push(Source::Launcher, Level::Error, false, text.as_ref());
    }

    /// One redacted line of game output.
    pub fn game(&self, stderr: bool, text: &str) {
        self.push(Source::Game, game_level(text), stderr, text);
    }

    /// A game entry whose level is already known (a folded log4j event).
    pub fn game_event(&self, level: Level, text: &str) {
        self.push(Source::Game, level, false, text);
    }

    /// Up to [`PAGE`] entries after `after`.
    pub fn read(&self, after: u64) -> Page {
        let ring = self.ring.lock().unwrap_or_else(PoisonError::into_inner);
        let first = ring.entries.front().map_or(ring.last_seq + 1, |e| e.seq);
        let missed = first.saturating_sub(after.saturating_add(1));
        let entries: Vec<Entry> = ring
            .entries
            .iter()
            .skip_while(|e| e.seq <= after)
            .take(PAGE)
            .cloned()
            .collect();
        Page {
            last_seq: entries.last().map_or(after.max(first - 1), |e| e.seq),
            entries,
            missed,
        }
    }
}

/// Folds the game's log4j XML events (what Minecraft prints when started with
/// its launcher logging configuration) into one readable entry each:
/// `[thread/LEVEL] logger: message`, plus a stack trace when there is one. Any
/// other line passes through unchanged.
#[derive(Default)]
pub struct GameLines {
    pending: Option<String>,
}

const EVENT_START: &str = "<log4j:Event ";
const EVENT_END: &str = "</log4j:Event>";

impl GameLines {
    /// Feeds one standard-output line; returns what the console shows, if
    /// anything yet.
    pub fn feed(&mut self, line: &str) -> Option<(Level, String)> {
        let trimmed = line.trim();
        if let Some(pending) = &mut self.pending {
            pending.push_str(line);
            pending.push('\n');
            if trimmed.ends_with(EVENT_END) || pending.len() > MAX_LINE_BYTES {
                let event = self.pending.take().unwrap_or_default();
                return Some(fold(&event));
            }
            return None;
        }
        if trimmed.starts_with(EVENT_START) {
            if trimmed.ends_with(EVENT_END) {
                return Some(fold(trimmed));
            }
            self.pending = Some(format!("{line}\n"));
            return None;
        }
        Some((game_level(line), line.to_owned()))
    }
}

fn attribute<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let start = tag.find(&format!(" {name}=\""))? + name.len() + 3;
    let len = tag[start..].find('"')?;
    Some(&tag[start..start + len])
}

fn cdata<'a>(event: &'a str, element: &str) -> Option<&'a str> {
    let open = format!("<log4j:{element}><![CDATA[");
    let start = event.find(&open)? + open.len();
    let len = event[start..].find("]]>")?;
    Some(&event[start..start + len])
}

fn unescape(text: &str) -> String {
    text.replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

/// One log4j event as a console line; unreadable events stay as they were.
fn fold(event: &str) -> (Level, String) {
    let tag = event.split('>').next().unwrap_or(event);
    let (Some(level), Some(message)) = (attribute(tag, "level"), cdata(event, "Message")) else {
        return (Level::Info, event.trim_end().to_owned());
    };
    let thread = attribute(tag, "thread").map(unescape).unwrap_or_default();
    let logger = attribute(tag, "logger").map(unescape).unwrap_or_default();
    let mut text = format!("[{thread}/{level}] {logger}: {message}");
    if let Some(trace) = cdata(event, "Throwable") {
        text.push('\n');
        text.push_str(trace.trim_end());
    }
    let level = match level {
        "ERROR" | "FATAL" => Level::Error,
        "WARN" => Level::Warn,
        _ => Level::Info,
    };
    (level, text)
}

/// A value's serialized (snake_case) name, for journal text.
pub fn name<T: Serialize>(value: &T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_continue_where_they_stopped() {
        let console = Console::default();
        console.info("started");
        console.game(false, "[12:00:00] [main/WARN]: low memory");
        console.game(true, "[12:00:01] [Render thread/ERROR]: crash");
        let page = console.read(0);
        assert_eq!(page.entries.len(), 3);
        assert_eq!(page.missed, 0);
        assert_eq!(page.entries[1].level, Level::Warn);
        assert_eq!(page.entries[2].level, Level::Error);
        assert!(page.entries[2].stderr);
        assert_eq!(page.entries[0].source, Source::Launcher);
        let next = console.read(page.last_seq);
        assert!(next.entries.is_empty());
        assert_eq!(next.last_seq, 3);
        console.error("failed");
        let next = console.read(page.last_seq);
        assert_eq!(next.entries.len(), 1);
        assert_eq!(next.entries[0].seq, 4);
    }

    #[test]
    fn the_oldest_entries_are_dropped_and_reported() {
        let console = Console::default();
        for i in 0..CAPACITY + 5 {
            console.info(format!("line {i}"));
        }
        let page = console.read(0);
        assert_eq!(page.missed, 5);
        assert_eq!(page.entries.len(), PAGE);
        assert_eq!(page.entries[0].text, "line 5");
        // A reader that is up to date misses nothing.
        let last = (CAPACITY + 5) as u64;
        assert_eq!(console.read(last).missed, 0);
        assert_eq!(console.read(last).last_seq, last);
    }

    #[test]
    fn long_lines_are_cut_on_a_character_boundary() {
        let console = Console::default();
        let long = "é".repeat(MAX_LINE_BYTES);
        console.game(false, &long);
        let text = &console.read(0).entries[0].text;
        assert!(text.len() < MAX_LINE_BYTES + 64);
        assert!(text.ends_with("bytes cut]"));
    }

    #[test]
    fn log4j_events_fold_into_one_entry() {
        let mut lines = GameLines::default();
        let event = [
            r#"<log4j:Event logger="chm" timestamp="1792253042855" level="WARN" thread="Client thread">"#,
            "  <log4j:Message><![CDATA[Author: Paul Lamb, <www.paulscode.com>]]></log4j:Message>",
            "</log4j:Event>",
        ];
        assert_eq!(lines.feed(event[0]), None);
        assert_eq!(lines.feed(event[1]), None);
        assert_eq!(
            lines.feed(event[2]),
            Some((
                Level::Warn,
                "[Client thread/WARN] chm: Author: Paul Lamb, <www.paulscode.com>".to_owned()
            ))
        );
        // With a stack trace, and on one line.
        let one = concat!(
            r#"<log4j:Event logger="a &amp; b" timestamp="1" level="ERROR" thread="Render thread">"#,
            "<log4j:Message><![CDATA[Crash]]></log4j:Message>",
            "<log4j:Throwable><![CDATA[java.lang.Error: x\n\tat A.b(A.java:1)\n]]></log4j:Throwable>",
            "</log4j:Event>"
        );
        let (level, text) = lines.feed(one).unwrap();
        assert_eq!(level, Level::Error);
        assert_eq!(
            text,
            "[Render thread/ERROR] a & b: Crash\njava.lang.Error: x\n\tat A.b(A.java:1)"
        );
        // Plain lines (old versions) pass through.
        assert_eq!(
            lines.feed("27 achievements"),
            Some((Level::Info, "27 achievements".to_owned()))
        );
        // An unreadable event is shown as it was.
        assert_eq!(lines.feed("<log4j:Event broken>"), None);
        let (_, raw) = lines.feed("</log4j:Event>").unwrap();
        assert!(raw.starts_with("<log4j:Event broken>"));
    }

    #[test]
    fn names_are_snake_case() {
        assert_eq!(name(&Level::Warn), "warn");
        assert_eq!(name(&Source::Game), "game");
    }
}
