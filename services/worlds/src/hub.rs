//! Live connections: hints that something changed, so launchers ask again
//! instead of polling. Nothing private travels here, only room IDs.

use std::collections::HashMap;
use std::sync::Mutex;

use serde::Serialize;
use tokio::sync::mpsc;

/// Connections per account (several computers, a restart in between).
pub const PER_ACCOUNT: usize = 5;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    /// The list of rooms changed (joined, removed, deleted): ask for it.
    Rooms,
    /// A room changed (members, hosting, name): ask for it. A host renews
    /// its lease at once, so removals reach the game within seconds.
    Room { id: String },
}

/// Each account's open connections: number, session, events.
type Connections = HashMap<String, Vec<(u64, String, mpsc::Sender<Event>)>>;

#[derive(Default)]
pub struct Hub {
    next: Mutex<u64>,
    connections: Mutex<Connections>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl Hub {
    /// A new connection of `account` in session `sid`; none when it has too
    /// many.
    pub fn join(&self, account: &str, sid: &str) -> Option<(u64, mpsc::Receiver<Event>)> {
        let mut connections = lock(&self.connections);
        let list = connections.entry(account.to_owned()).or_default();
        list.retain(|(_, _, sender)| !sender.is_closed());
        if list.len() >= PER_ACCOUNT {
            return None;
        }
        let mut next = lock(&self.next);
        *next += 1;
        let (sender, receiver) = mpsc::channel(64);
        list.push((*next, sid.to_owned(), sender));
        Some((*next, receiver))
    }

    pub fn leave(&self, account: &str, id: u64) {
        let mut connections = lock(&self.connections);
        if let Some(list) = connections.get_mut(account) {
            list.retain(|(i, _, _)| *i != id);
            if list.is_empty() {
                connections.remove(account);
            }
        }
    }

    /// Tells `account` everywhere it is connected. A connection that cannot
    /// keep up misses the event; it asks again when it reconnects.
    pub fn tell(&self, account: &str, event: &Event) {
        let connections = lock(&self.connections);
        for (_, _, sender) in connections.get(account).into_iter().flatten() {
            let _ = sender.try_send(event.clone());
        }
    }

    pub fn tell_all(&self, accounts: &[String], event: &Event) {
        for account in accounts {
            self.tell(account, event);
        }
    }

    /// The sessions with an open connection.
    pub fn sessions(&self) -> Vec<String> {
        lock(&self.connections)
            .values()
            .flatten()
            .map(|(_, sid, _)| sid.clone())
            .collect()
    }

    /// Closes the connections of ended sessions: dropping their senders ends
    /// the socket.
    pub fn end(&self, sids: &[String]) {
        let mut connections = lock(&self.connections);
        for list in connections.values_mut() {
            list.retain(|(_, sid, _)| !sids.contains(sid));
        }
        connections.retain(|_, list| !list.is_empty());
    }

    /// Closes every connection of a deleted account.
    pub fn end_account(&self, account: &str) {
        lock(&self.connections).remove(account);
    }
}
