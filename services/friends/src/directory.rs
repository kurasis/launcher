//! Nicks and the accounts they name, from the account service's internal API
//! (`127.0.0.1:8793`, never public). A nick can change at any time, so lists
//! show the current one; answers are kept a minute.

use std::collections::{BTreeMap, HashMap};
use std::future::Future;
use std::pin::Pin;
use std::sync::Mutex;
use std::time::{Duration, Instant};

const KEEP: Duration = Duration::from_secs(60);

type Boxed<'a, T> = Pin<Box<dyn Future<Output = Result<T, String>> + Send + 'a>>;

/// Where nicks come from (the account service, or a fake in tests).
pub trait Accounts: Send + Sync {
    /// The account with this nick, exactly (without regard to case).
    fn find<'a>(&'a self, nick: &'a str) -> Boxed<'a, Option<(String, String)>>;
    /// The current nicks of these accounts; missing ones are gone or switched off.
    fn nicks<'a>(&'a self, ids: &'a [String]) -> Boxed<'a, BTreeMap<String, String>>;
}

pub struct Internal {
    pub base: String,
    pub http: reqwest::Client,
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
                .map_err(|e| e.to_string())?;
            if response.status() == reqwest::StatusCode::NOT_FOUND {
                return Ok(None);
            }
            let body: serde_json::Value = response
                .error_for_status()
                .map_err(|e| e.to_string())?
                .json()
                .await
                .map_err(|e| e.to_string())?;
            Ok(Some((
                body["id"].as_str().unwrap_or_default().to_owned(),
                body["nick"].as_str().unwrap_or_default().to_owned(),
            ))
            .filter(|(id, _)| !id.is_empty()))
        })
    }

    fn nicks<'a>(&'a self, ids: &'a [String]) -> Boxed<'a, BTreeMap<String, String>> {
        Box::pin(async move {
            let body: serde_json::Value = self
                .http
                .post(format!("{}/internal/v1/nicks", self.base))
                .json(&serde_json::json!({ "ids": ids }))
                .send()
                .await
                .and_then(reqwest::Response::error_for_status)
                .map_err(|e| e.to_string())?
                .json()
                .await
                .map_err(|e| e.to_string())?;
            Ok(body["nicks"]
                .as_object()
                .into_iter()
                .flatten()
                .filter_map(|(id, nick)| Some((id.clone(), nick.as_str()?.to_owned())))
                .collect())
        })
    }
}

/// Answers of the account service, kept a minute.
pub struct Directory {
    source: Box<dyn Accounts>,
    cache: Mutex<HashMap<String, (Option<String>, Instant)>>,
}

impl Directory {
    pub fn new(source: Box<dyn Accounts>) -> Self {
        Self {
            source,
            cache: Mutex::new(HashMap::new()),
        }
    }

    pub async fn find(&self, nick: &str) -> Result<Option<(String, String)>, String> {
        self.source.find(nick).await
    }

    /// The nicks of `ids`; accounts that are gone have none.
    pub async fn nicks(&self, ids: &[String]) -> Result<BTreeMap<String, String>, String> {
        let now = Instant::now();
        let mut known = BTreeMap::new();
        let mut ask = Vec::new();
        {
            let cache = self
                .cache
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            for id in ids {
                match cache.get(id) {
                    Some((nick, at)) if now.duration_since(*at) < KEEP => {
                        if let Some(nick) = nick {
                            known.insert(id.clone(), nick.clone());
                        }
                    }
                    _ => {
                        if !ask.contains(id) {
                            ask.push(id.clone());
                        }
                    }
                }
            }
        }
        for chunk in ask.chunks(500) {
            let answered = self.source.nicks(chunk).await?;
            let mut cache = self
                .cache
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if cache.len() > 100_000 {
                cache.retain(|_, (_, at)| now.duration_since(*at) < KEEP);
            }
            for id in chunk {
                let nick = answered.get(id).cloned();
                if let Some(nick) = &nick {
                    known.insert(id.clone(), nick.clone());
                }
                cache.insert(id.clone(), (nick, now));
            }
        }
        Ok(known)
    }

    /// A nick that just changed (seen in the owner's own token).
    pub fn note(&self, id: &str, nick: &str) {
        self.cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(id.to_owned(), (Some(nick.to_owned()), Instant::now()));
    }
}
