//! Who the relay carries (ADAPTATION A5, plan W6). worlds.lapi.sh lists the
//! tunnel keys of live hosts and of the members they admit, each with its
//! account's group and seconds left (at most 30, renewed every 10). A key not
//! on the list is refused at connect; a connected key whose record runs out,
//! or that drops off the list, is closed. A group (one account) holds at most
//! a few connections at once.

use std::collections::HashMap;
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use iroh_base::EndpointId;
use iroh_relay::server::{Access, AccessControl, ClientRequest, ConnectionId};
use serde::Deserialize;

/// Records last no longer than this, whatever the list says.
pub const MAX_TTL: Duration = Duration::from_secs(60);
/// Entries read from one list.
pub const MAX_ENTRIES: usize = 100_000;

/// One list entry as worlds.lapi.sh sends it.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    endpoint: String,
    group: String,
    ttl: i64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct List {
    allow: Vec<Entry>,
}

/// The list's entries that have a valid shape: key, group, time left.
pub fn parse(body: &[u8]) -> Result<Vec<(EndpointId, String, Duration)>, String> {
    let list: List = serde_json::from_slice(body).map_err(|_| "the allow list is not valid")?;
    if list.allow.len() > MAX_ENTRIES {
        return Err("the allow list is too long".to_owned());
    }
    Ok(list
        .allow
        .into_iter()
        .filter_map(|entry| {
            let hex = entry.endpoint.len() == 64
                && entry
                    .endpoint
                    .bytes()
                    .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
            let group = (1..=64).contains(&entry.group.len())
                && entry.group.bytes().all(|b| b.is_ascii_alphanumeric());
            if !hex || !group || entry.ttl <= 0 {
                return None;
            }
            let key = EndpointId::from_str(&entry.endpoint).ok()?;
            let ttl = Duration::from_secs(u64::try_from(entry.ttl).ok()?).min(MAX_TTL);
            Some((key, entry.group, ttl))
        })
        .collect())
}

#[derive(Debug, Default)]
struct State {
    /// Keys allowed, their group, until when.
    allowed: HashMap<EndpointId, (String, Instant)>,
    /// Connected keys: their group when they connected, and the connections.
    connected: HashMap<EndpointId, (String, Vec<ConnectionId>)>,
    /// Connections per group.
    groups: HashMap<String, usize>,
}

#[derive(Debug)]
pub struct Gate {
    state: Mutex<State>,
    per_group: usize,
}

impl Gate {
    pub fn new(per_group: usize) -> Self {
        Self {
            state: Mutex::new(State::default()),
            per_group,
        }
    }

    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Takes a fresh list in place of the one before.
    pub fn replace(&self, list: Vec<(EndpointId, String, Duration)>, now: Instant) {
        let allowed = list
            .into_iter()
            .map(|(key, group, ttl)| (key, (group, now + ttl)))
            .collect();
        self.state().allowed = allowed;
    }

    /// Admits a connection of `key` if its record holds and its group has
    /// room; counts it then.
    pub fn admit(&self, key: EndpointId, connection: ConnectionId, now: Instant) -> bool {
        let mut state = self.state();
        let Some((group, until)) = state.allowed.get(&key).cloned() else {
            return false;
        };
        if until <= now {
            return false;
        }
        let used = state.groups.get(&group).copied().unwrap_or(0);
        if used >= self.per_group {
            return false;
        }
        state.groups.insert(group.clone(), used + 1);
        state
            .connected
            .entry(key)
            .or_insert_with(|| (group, Vec::new()))
            .1
            .push(connection);
        true
    }

    /// A connection ended.
    pub fn left(&self, key: EndpointId, connection: ConnectionId) {
        let mut state = self.state();
        let Some((group, connections)) = state.connected.get_mut(&key) else {
            return;
        };
        let before = connections.len();
        connections.retain(|c| *c != connection);
        if connections.len() == before {
            return;
        }
        let group = group.clone();
        if connections.is_empty() {
            state.connected.remove(&key);
        }
        if let Some(count) = state.groups.get_mut(&group) {
            *count = count.saturating_sub(1);
            if *count == 0 {
                state.groups.remove(&group);
            }
        }
    }

    /// Connected keys whose record ran out or is gone.
    pub fn outlawed(&self, now: Instant) -> Vec<EndpointId> {
        let state = self.state();
        state
            .connected
            .keys()
            .filter(|key| {
                !state
                    .allowed
                    .get(*key)
                    .is_some_and(|(_, until)| *until > now)
            })
            .copied()
            .collect()
    }

    /// Connections open now (for the counters).
    pub fn connections(&self) -> usize {
        self.state().groups.values().sum()
    }
}

/// The relay server's access control, backed by the gate.
#[derive(Debug, Clone)]
pub struct Guard(pub Arc<Gate>);

impl AccessControl for Guard {
    async fn on_connect(&self, request: &ClientRequest) -> Access {
        if self.0.admit(
            request.endpoint_id(),
            request.connection_id(),
            Instant::now(),
        ) {
            Access::Allow
        } else {
            Access::Deny { reason: None }
        }
    }

    fn on_disconnect(&self, endpoint_id: EndpointId, connection_id: ConnectionId) {
        self.0.left(endpoint_id, connection_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iroh_base::SecretKey;

    fn key(n: u8) -> EndpointId {
        SecretKey::from_bytes(&[n; 32]).public()
    }

    fn id() -> ConnectionId {
        // Connection IDs come from the relay's own counter.
        let parts = http::Request::new(()).into_parts().0;
        ClientRequest::new(key(0), Default::default(), parts).connection_id()
    }

    #[test]
    fn lists_keep_only_well_formed_entries() {
        let good = key(1).to_string();
        let body = serde_json::json!({"allow": [
            {"endpoint": good, "group": "a1", "ttl": 30},
            {"endpoint": good.to_uppercase(), "group": "a1", "ttl": 30},
            {"endpoint": "zz".repeat(32), "group": "a1", "ttl": 30},
            {"endpoint": good, "group": "../x", "ttl": 30},
            {"endpoint": good, "group": "a1", "ttl": 0},
            {"endpoint": good, "group": "a1", "ttl": 99999},
        ]});
        let list = parse(body.to_string().as_bytes()).unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[1].2, MAX_TTL, "capped");
        assert!(parse(b"{").is_err());
        assert!(parse(br#"{"allow": [], "more": 1}"#).is_err());
        assert!(parse(br#"{"allow": "all"}"#).is_err());
    }

    #[test]
    fn only_listed_keys_connect_and_drop_off_when_their_record_ends() {
        let gate = Gate::new(2);
        let now = Instant::now();
        assert!(!gate.admit(key(1), id(), now), "nothing listed");
        gate.replace(
            vec![
                (key(1), "g1".into(), Duration::from_secs(30)),
                (key(2), "g1".into(), Duration::from_secs(10)),
                (key(3), "g1".into(), Duration::from_secs(30)),
                (key(4), "g2".into(), Duration::from_secs(30)),
            ],
            now,
        );
        let first = id();
        assert!(gate.admit(key(1), first, now));
        assert!(gate.admit(key(2), id(), now));
        assert!(!gate.admit(key(3), id(), now), "the group is full");
        assert!(gate.admit(key(4), id(), now), "another group");
        assert!(!gate.admit(key(5), id(), now), "not listed");
        assert_eq!(gate.connections(), 3);
        assert!(gate.outlawed(now).is_empty());

        // Key 2's record runs out; key 4 drops off a fresh list.
        let later = now + Duration::from_secs(11);
        assert_eq!(gate.outlawed(later), [key(2)]);
        assert!(!gate.admit(key(2), id(), later));
        gate.replace(vec![(key(1), "g1".into(), Duration::from_secs(30))], later);
        let mut outlawed = gate.outlawed(later);
        outlawed.sort();
        let mut expected = vec![key(2), key(4)];
        expected.sort();
        assert_eq!(outlawed, expected);

        // A closed connection frees its group's place, once.
        gate.left(key(1), first);
        gate.left(key(1), first);
        assert_eq!(gate.connections(), 2);
        assert!(gate.outlawed(later).len() == 2);

        // No list for 30 seconds: everyone drops off.
        assert_eq!(gate.outlawed(later + Duration::from_secs(31)).len(), 2);
    }
}
