//! Request-ID idempotency for mutating commands (docs/spec/06 "Bootstrap and
//! ownership"): within one backend epoch a repeated request ID with an identical
//! payload returns the recorded result, and reuse with a different payload is
//! rejected with `REQUEST_ID_CONFLICT`.

use std::collections::{HashMap, VecDeque};

use serde_json::Value;

use crate::error::{ErrorCode, LauncherError, LauncherResult};

/// Maximum remembered requests. Older entries are forgotten first; a client that
/// retries after this many newer mutations must reconcile from the snapshot.
pub const LEDGER_CAPACITY: usize = 512;

const MAX_REQUEST_ID_LEN: usize = 64;

pub type RecordedResult = Result<Value, LauncherError>;

#[derive(Debug)]
pub enum Lookup {
    New,
    Replay(RecordedResult),
}

#[derive(Debug, Default)]
pub struct RequestLedger {
    entries: HashMap<String, (String, RecordedResult)>,
    order: VecDeque<String>,
}

/// Request IDs are client-generated opaque tokens (normally UUIDs).
pub fn validate_request_id(id: &str) -> LauncherResult<()> {
    let valid = !id.is_empty()
        && id.len() <= MAX_REQUEST_ID_LEN
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
    if valid {
        Ok(())
    } else {
        Err(LauncherError::invalid_input(
            "The request identifier is missing or malformed.",
        ))
    }
}

impl RequestLedger {
    pub fn lookup(&self, request_id: &str, fingerprint: &str) -> LauncherResult<Lookup> {
        match self.entries.get(request_id) {
            None => Ok(Lookup::New),
            Some((recorded, result)) if recorded == fingerprint => {
                Ok(Lookup::Replay(result.clone()))
            }
            Some(_) => Err(LauncherError::new(
                ErrorCode::RequestIdConflict,
                "This request identifier was already used for a different request.",
            )),
        }
    }

    pub fn record(&mut self, request_id: String, fingerprint: String, result: RecordedResult) {
        if self.entries.contains_key(&request_id) {
            return;
        }
        if self.order.len() == LEDGER_CAPACITY
            && let Some(oldest) = self.order.pop_front()
        {
            self.entries.remove(&oldest);
        }
        self.order.push_back(request_id.clone());
        self.entries.insert(request_id, (fingerprint, result));
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn identical_request_replays_and_conflict_is_rejected() {
        let mut ledger = RequestLedger::default();
        assert!(matches!(ledger.lookup("r1", "a").unwrap(), Lookup::New));
        ledger.record("r1".into(), "a".into(), Ok(json!({"id": 1})));
        match ledger.lookup("r1", "a").unwrap() {
            Lookup::Replay(Ok(value)) => assert_eq!(value, json!({"id": 1})),
            other => panic!("{other:?}"),
        }
        assert_eq!(
            ledger.lookup("r1", "b").unwrap_err().code,
            ErrorCode::RequestIdConflict
        );
    }

    #[test]
    fn errors_are_replayed_too() {
        let mut ledger = RequestLedger::default();
        let error = LauncherError::invalid_input("bad");
        ledger.record("r".into(), "f".into(), Err(error.clone()));
        match ledger.lookup("r", "f").unwrap() {
            Lookup::Replay(Err(replayed)) => assert_eq!(replayed, error),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn capacity_is_bounded() {
        let mut ledger = RequestLedger::default();
        for i in 0..=LEDGER_CAPACITY {
            ledger.record(format!("r{i}"), "f".into(), Ok(Value::Null));
        }
        assert!(matches!(ledger.lookup("r0", "f").unwrap(), Lookup::New));
        assert!(matches!(
            ledger.lookup(&format!("r{LEDGER_CAPACITY}"), "f").unwrap(),
            Lookup::Replay(_)
        ));
    }

    #[test]
    fn request_ids_are_validated() {
        assert!(validate_request_id("0f1e2d3c-4b5a-6978-8796-a5b4c3d2e1f0").is_ok());
        for bad in ["", "a b", "x/y", &"a".repeat(65), "ü"] {
            assert!(validate_request_id(bad).is_err(), "{bad}");
        }
    }
}
