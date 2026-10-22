//! What a game started from a server ad did (D-071): whether it got onto the
//! server, why the server turned it away, and how long it played there. Read
//! from the game's output by a few fixed words. None of the output leaves
//! the computer: only the counts, and one word for a refusal.
//!
//! The game connects (`Connecting to host, port`); it is on the server once
//! the chat shows something, or after two minutes without a refusal. A
//! refusal before that is counted with its reason; one after it, or a
//! connection to another server, ends the time on the server.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::{Change, Stats};
use crate::state::AppState;
use crate::sync::lock;

/// Two minutes after connecting without a refusal count as being in.
pub const JOINED_AFTER: Duration = Duration::from_secs(120);
/// Only the start of a long line is read.
const READ_CHARS: usize = 1000;

/// What one line of the game's output says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Seen {
    Connecting,
    /// The chat showed something: the game is on the server.
    Inside,
    /// The server turned the game away, or the connection ended: why.
    Refused(&'static str),
    Nothing,
}

/// Words of a connection that failed or ended. The game writes the first
/// one for every failed connection; the exception lines after it are not
/// read, as other parts of the game (Realms, telemetry) fail with the same.
const ENDED: [&str; 4] = [
    "couldn't connect to server",
    "client disconnected with reason",
    "disconnected from server",
    "lost connection",
];

pub fn read(line: &str) -> Seen {
    if crate::launch::server::connecting_to(line).is_some() {
        return Seen::Connecting;
    }
    let start: String = line.chars().take(READ_CHARS).collect();
    let text = start.to_lowercase();
    if text.contains("[chat]") {
        return Seen::Inside;
    }
    if ENDED.iter().any(|w| text.contains(w)) {
        return Seen::Refused(reason(&text));
    }
    Seen::Nothing
}

/// The one word the service takes for a refusal (`REFUSALS` there).
fn reason(text: &str) -> &'static str {
    let has = |words: &[&str]| words.iter().any(|w| text.contains(w));
    if has(&["outdated", "incompatible", "please use", "version"]) {
        "version"
    } else if has(&["whitelist", "white-list", "white list"]) {
        "whitelist"
    } else if has(&["banned"]) {
        "ban"
    } else if has(&["server is full", "full server", "is full"]) {
        "full"
    } else if has(&["couldn't connect", "timed out", "timeout"]) {
        "noanswer"
    } else {
        "other"
    }
}

#[derive(Debug, Default)]
struct Track {
    connected: Option<Instant>,
    joined: bool,
    refused: bool,
    left: Option<Instant>,
    ended: bool,
}

/// One run of the game started from an ad.
pub struct AdRun {
    stats: Arc<Stats>,
    ad: String,
    track: Mutex<Track>,
}

impl AdRun {
    pub(super) fn new(stats: Arc<Stats>, ad: String) -> Self {
        Self {
            stats,
            ad,
            track: Mutex::new(Track::default()),
        }
    }

    pub fn ad(&self) -> &str {
        &self.ad
    }

    /// The game started, with this version.
    pub fn started(&self, state: &AppState, version: &str) {
        self.stats.count(state, &self.ad, &Change::Started(version));
    }

    pub fn line(&self, state: &AppState, text: &str) {
        self.line_at(state, text, Instant::now());
    }

    pub fn line_at(&self, state: &AppState, text: &str, now: Instant) {
        let seen = read(text);
        let joined = {
            let mut track = lock(&self.track);
            if track.ended {
                return;
            }
            let before = track.joined || track.refused;
            match (seen, track.connected) {
                (Seen::Connecting, None) => track.connected = Some(now),
                // Another server: the time on this one ends.
                (Seen::Connecting, Some(_)) if track.joined && track.left.is_none() => {
                    track.left = Some(now);
                }
                (Seen::Inside, Some(_)) if !before => track.joined = true,
                (Seen::Refused(reason), Some(_)) if !before => {
                    track.refused = true;
                    drop(track);
                    self.stats.count(state, &self.ad, &Change::Refused(reason));
                    return;
                }
                (Seen::Refused(_), Some(_)) if track.joined && track.left.is_none() => {
                    track.left = Some(now);
                }
                _ => {}
            }
            if !(track.joined || track.refused)
                && track
                    .connected
                    .is_some_and(|at| now.duration_since(at) >= JOINED_AFTER)
            {
                track.joined = true;
            }
            track.joined && !before
        };
        if joined {
            self.stats.count(state, &self.ad, &Change::Joined);
        }
    }

    pub fn ended(&self, state: &AppState) {
        self.ended_at(state, Instant::now());
    }

    /// The game exited: its minutes on the server count.
    pub fn ended_at(&self, state: &AppState, now: Instant) {
        let (joined_now, minutes) = {
            let mut track = lock(&self.track);
            if track.ended {
                return;
            }
            track.ended = true;
            let Some(at) = track.connected else {
                return;
            };
            let joined_now =
                !track.joined && !track.refused && now.duration_since(at) >= JOINED_AFTER;
            track.joined |= joined_now;
            if !track.joined {
                return;
            }
            let until = track.left.unwrap_or(now);
            let minutes =
                u32::try_from(until.duration_since(at).as_secs() / 60).unwrap_or(u32::MAX);
            (joined_now, minutes)
        };
        if joined_now {
            self.stats.count(state, &self.ad, &Change::Joined);
        }
        self.stats.count(state, &self.ad, &Change::Played(minutes));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_are_read_by_fixed_words() {
        assert_eq!(
            read("[12:00:01] [Render thread/INFO]: Connecting to play.example.net, 25565"),
            Seen::Connecting
        );
        assert_eq!(
            read("[12:00:03] [Render thread/INFO]: [System] [CHAT] Welcome to the server!"),
            Seen::Inside
        );
        for (line, why) in [
            (
                "[Render thread/WARN]: Client disconnected with reason: Outdated client! Please use 1.21.4",
                "version",
            ),
            (
                "[Render thread/WARN]: Client disconnected with reason: Incompatible client!",
                "version",
            ),
            (
                "[Render thread/WARN]: Client disconnected with reason: You are not whitelisted on this server!",
                "whitelist",
            ),
            (
                "[Render thread/WARN]: Client disconnected with reason: You are banned from this server.",
                "ban",
            ),
            (
                "[Render thread/WARN]: Client disconnected with reason: The server is full!",
                "full",
            ),
            (
                "[Server Connector #1/ERROR]: Couldn't connect to server",
                "noanswer",
            ),
            (
                "[Render thread/WARN]: Client disconnected with reason: Timed out",
                "noanswer",
            ),
            (
                "[Render thread/WARN]: Client disconnected with reason: Flying is not enabled",
                "other",
            ),
        ] {
            assert_eq!(read(line), Seen::Refused(why), "{line}");
        }
        for line in [
            "[Render thread/INFO]: Loaded 1234 recipes",
            "java.net.UnknownHostException: play.example.net",
            "[Realms Notification Availability checker #1/ERROR]: Connection refused",
        ] {
            assert_eq!(read(line), Seen::Nothing, "{line}");
        }
        // A long line is read from its start only.
        assert_eq!(
            read(&format!("{} [CHAT] hi", "x".repeat(5000))),
            Seen::Nothing
        );
    }
}
