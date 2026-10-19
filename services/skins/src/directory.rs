//! Nicks and the accounts they name, from the account service's internal API
//! (`127.0.0.1:8793`, never public). A nick can change at any time, so a skin
//! is found by the account behind the nick; answers are kept a minute, those
//! that name nobody too, so a flood of lookups reaches the account service at
//! most once a minute per nick.
//!
//! Errors never carry the address asked, since it holds the nick.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Mutex;
use std::time::{Duration, Instant};

const KEEP: Duration = Duration::from_secs(60);
/// Nicks kept at most; beyond, old answers go first, then all.
const KEEP_MAX: usize = 100_000;

type Boxed<'a, T> = Pin<Box<dyn Future<Output = Result<T, String>> + Send + 'a>>;

/// A nick as the account service has them: 3–16 of `A-Z a-z 0-9 _`, as it
/// is (nothing trimmed, nothing that looks like a letter).
pub fn nick(text: &str) -> bool {
    (3..=16).contains(&text.len()) && text.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

/// An account ID as the account service writes them (a hyphenated UUID).
pub fn account_id(text: &str) -> bool {
    uuid::Uuid::parse_str(text).is_ok_and(|u| u.hyphenated().to_string() == text)
}

/// Where nicks come from (the account service, or a fake in tests).
pub trait Accounts: Send + Sync {
    /// The account with this nick, exactly (without regard to case): ID and
    /// the nick as written.
    fn find<'a>(&'a self, nick: &'a str) -> Boxed<'a, Option<(String, String)>>;
    /// Of these sessions, those that ended (signed out, or the account is
    /// switched off).
    fn ended<'a>(&'a self, sessions: &'a [String]) -> Boxed<'a, Vec<String>>;
    /// Accounts deleted since `since` (Unix time): ID and when.
    fn deleted(&self, since: i64) -> Boxed<'_, Vec<(String, i64)>>;
}

pub struct Internal {
    pub base: String,
    pub http: reqwest::Client,
}

/// A request's error, without the address it went to.
fn error(e: reqwest::Error) -> String {
    e.without_url().to_string()
}

impl Accounts for Internal {
    fn find<'a>(&'a self, nick: &'a str) -> Boxed<'a, Option<(String, String)>> {
        Box::pin(async move {
            let response = self
                .http
                .get(format!("{}/internal/v1/find", self.base))
                .query(&[("nick", nick)])
                .send()
                .await
                .map_err(error)?;
            if response.status() == reqwest::StatusCode::NOT_FOUND {
                return Ok(None);
            }
            let body: serde_json::Value = response
                .error_for_status()
                .map_err(error)?
                .json()
                .await
                .map_err(error)?;
            Ok(Some((
                body["id"].as_str().unwrap_or_default().to_owned(),
                body["nick"].as_str().unwrap_or_default().to_owned(),
            ))
            .filter(|(id, found)| account_id(id) && self::nick(found)))
        })
    }

    fn ended<'a>(&'a self, sessions: &'a [String]) -> Boxed<'a, Vec<String>> {
        Box::pin(async move {
            let body: serde_json::Value = self
                .http
                .post(format!("{}/internal/v1/ended", self.base))
                .json(&serde_json::json!({ "sessions": sessions }))
                .send()
                .await
                .and_then(reqwest::Response::error_for_status)
                .map_err(error)?
                .json()
                .await
                .map_err(error)?;
            Ok(body["ended"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|s| Some(s.as_str()?.to_owned()))
                .collect())
        })
    }

    fn deleted(&self, since: i64) -> Boxed<'_, Vec<(String, i64)>> {
        Box::pin(async move {
            let body: serde_json::Value = self
                .http
                .get(format!("{}/internal/v1/deleted", self.base))
                .query(&[("since", since)])
                .send()
                .await
                .and_then(reqwest::Response::error_for_status)
                .map_err(error)?
                .json()
                .await
                .map_err(error)?;
            Ok(body["deleted"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|d| Some((d["id"].as_str()?.to_owned(), d["at"].as_i64()?)))
                .collect())
        })
    }
}

type Found = Option<(String, String)>;

/// Answers of the account service, kept a minute.
pub struct Directory {
    source: Box<dyn Accounts>,
    /// By nick in lowercase.
    cache: Mutex<HashMap<String, (Found, Instant)>>,
}

impl Directory {
    pub fn new(source: Box<dyn Accounts>) -> Self {
        Self {
            source,
            cache: Mutex::new(HashMap::new()),
        }
    }

    /// The account with this nick and the nick as written, if there is one.
    /// The caller has checked the nick ([`nick`]).
    pub async fn find(&self, nick: &str) -> Result<Found, String> {
        let key = nick.to_ascii_lowercase();
        let now = Instant::now();
        if let Some((found, at)) = self
            .cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(&key)
            && now.duration_since(*at) < KEEP
        {
            return Ok(found.clone());
        }
        let found = self.source.find(nick).await?;
        let mut cache = self
            .cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if cache.len() >= KEEP_MAX {
            cache.retain(|_, (_, at)| now.duration_since(*at) < KEEP);
            if cache.len() >= KEEP_MAX {
                cache.clear();
            }
        }
        cache.insert(key, (found.clone(), now));
        Ok(found)
    }

    /// Forgets what is kept about an account (deleted, or its nick
    /// changed).
    pub fn forget(&self, account: &str) {
        self.cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .retain(|_, (found, _)| found.as_ref().is_none_or(|(id, _)| id != account));
    }

    pub async fn ended(&self, sessions: &[String]) -> Result<Vec<String>, String> {
        self.source.ended(sessions).await
    }

    pub async fn deleted(&self, since: i64) -> Result<Vec<(String, i64)>, String> {
        self.source.deleted(since).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nicks_and_ids() {
        for good in ["Steve", "a_b", "x".repeat(16).as_str(), "A1_"] {
            assert!(nick(good), "{good}");
        }
        for bad in [
            "",
            "ab",
            &"x".repeat(17),
            " Steve",
            "Steve ",
            "../x",
            "a\0b",
            "Stеve", // Cyrillic е
            "Ｓteve",
            "a' OR 1=1",
            "%00",
        ] {
            assert!(!nick(bad), "{bad:?}");
        }
        assert!(account_id("00000000-0000-4000-8000-000000000001"));
        assert!(!account_id("00000000-0000-4000-8000-00000000000Z"));
        assert!(!account_id("00000000-0000-4000-8000-00000000000A"));
        assert!(!account_id("../../etc"));
        assert!(!account_id("00000000000040008000000000000001"));
    }
}
