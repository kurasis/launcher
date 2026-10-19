//! Live connections: what each signed-in launcher should hear now.

use std::collections::HashMap;
use std::sync::Mutex;

use serde::Serialize;
use tokio::sync::mpsc;

use crate::store::Message;

/// Connections per account (several computers, a restart in between).
pub const PER_ACCOUNT: usize = 5;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    /// Friends, requests or blocks changed: ask for them again.
    Friends,
    /// A room was made, changed or left: ask for the rooms again.
    Rooms,
    Message {
        message: Message,
    },
    /// The owner removed a message.
    Removed {
        room: String,
        message: i64,
    },
    /// A friend came online, went offline, or started or stopped playing.
    Presence {
        id: String,
        online: bool,
        playing: Option<String>,
    },
}

/// Each account's open connections: number, session, events.
type Connections = HashMap<String, Vec<(u64, String, mpsc::Sender<Event>)>>;

#[derive(Default)]
pub struct Hub {
    next: Mutex<u64>,
    connections: Mutex<Connections>,
    /// What each account plays, as its launcher said; gone when its last
    /// connection closes.
    playing: Mutex<HashMap<String, String>>,
}

impl Hub {
    /// A new connection of `account` in session `sid`; none when it has too
    /// many.
    pub fn join(&self, account: &str, sid: &str) -> Option<(u64, mpsc::Receiver<Event>)> {
        let mut connections = self
            .connections
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let list = connections.entry(account.to_owned()).or_default();
        list.retain(|(_, _, sender)| !sender.is_closed());
        if list.len() >= PER_ACCOUNT {
            return None;
        }
        let mut next = self
            .next
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *next += 1;
        let (sender, receiver) = mpsc::channel(64);
        list.push((*next, sid.to_owned(), sender));
        Some((*next, receiver))
    }

    /// Closes a connection; true when it was the account's last one.
    pub fn leave(&self, account: &str, id: u64) -> bool {
        let mut connections = self
            .connections
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(list) = connections.get_mut(account) else {
            return false;
        };
        list.retain(|(i, _, _)| *i != id);
        if !list.is_empty() {
            return false;
        }
        connections.remove(account);
        self.playing
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(account);
        true
    }

    /// Whether the account has a live connection.
    pub fn online(&self, account: &str) -> bool {
        self.connections
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(account)
            .is_some_and(|list| list.iter().any(|(_, _, sender)| !sender.is_closed()))
    }

    pub fn playing(&self, account: &str) -> Option<String> {
        self.playing
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(account)
            .cloned()
    }

    /// Sets or clears what the account plays; true when it changed.
    pub fn set_playing(&self, account: &str, game: Option<String>) -> bool {
        let mut playing = self
            .playing
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match game {
            Some(game) => playing.insert(account.to_owned(), game.clone()) != Some(game),
            None => playing.remove(account).is_some(),
        }
    }

    /// Closes every connection of an account (banned or deleted).
    pub fn end_account(&self, account: &str) {
        self.connections
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(account);
        self.playing
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(account);
    }

    /// Tells `account` everywhere it is connected. A connection that cannot
    /// keep up misses the event; it asks again when it reconnects.
    pub fn tell(&self, account: &str, event: &Event) {
        let connections = self
            .connections
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for (_, _, sender) in connections.get(account).into_iter().flatten() {
            let _ = sender.try_send(event.clone());
        }
    }

    /// Sessions open in live connections.
    pub fn sessions(&self) -> Vec<String> {
        self.connections
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .values()
            .flatten()
            .map(|(_, sid, _)| sid.clone())
            .collect()
    }

    /// Closes the connections of ended sessions: dropping their senders ends
    /// the socket.
    pub fn end(&self, sids: &[String]) {
        let mut connections = self
            .connections
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for list in connections.values_mut() {
            list.retain(|(_, sid, _)| !sids.contains(sid));
        }
        connections.retain(|_, list| !list.is_empty());
    }
}
