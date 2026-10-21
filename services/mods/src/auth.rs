//! Who is asking: access tokens of oauth.lapi.sh (D-064) with `mods` among
//! their audiences, checked here with its published Ed25519 key. Only adding
//! to the catalog needs one; reading it is open.

use std::collections::HashMap;
use std::sync::{Mutex, RwLock};
use std::time::{Duration, Instant};

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::Deserialize;

/// What a valid token says.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Claims {
    pub iss: String,
    pub sub: String,
    pub aud: Vec<String>,
    pub iat: i64,
    pub exp: i64,
    pub sid: String,
    pub nick: String,
    pub ev: bool,
}

pub struct Tokens {
    issuer: String,
    keys: RwLock<Vec<(String, VerifyingKey)>>,
}

impl Tokens {
    pub fn new(issuer: &str) -> Self {
        Self {
            issuer: issuer.trim_end_matches('/').to_owned(),
            keys: RwLock::new(Vec::new()),
        }
    }

    /// Replaces the keys with those of a JWK set (Ed25519 only).
    pub fn set_jwks(&self, jwks: &serde_json::Value) -> usize {
        let keys: Vec<(String, VerifyingKey)> = jwks["keys"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|k| k["kty"] == "OKP" && k["crv"] == "Ed25519")
            .filter_map(|k| {
                let x: [u8; 32] = B64.decode(k["x"].as_str()?).ok()?.try_into().ok()?;
                Some((
                    k["kid"].as_str().unwrap_or_default().to_owned(),
                    VerifyingKey::from_bytes(&x).ok()?,
                ))
            })
            .collect();
        let count = keys.len();
        if count > 0 {
            *self
                .keys
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = keys;
        }
        count
    }

    pub fn has_keys(&self) -> bool {
        !self
            .keys
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .is_empty()
    }

    /// The claims of a token signed by the account service for the catalog, if it
    /// is still valid.
    pub fn verify(&self, token: &str, now: i64) -> Option<Claims> {
        if token.len() > 4096 {
            return None;
        }
        let mut parts = token.split('.');
        let (header, payload, signature) = (parts.next()?, parts.next()?, parts.next()?);
        if parts.next().is_some() {
            return None;
        }
        let header: serde_json::Value = serde_json::from_slice(&B64.decode(header).ok()?).ok()?;
        // Only EdDSA; the token never chooses the algorithm or brings a key.
        if header.get("alg")?.as_str()? != "EdDSA" {
            return None;
        }
        let kid = header
            .get("kid")
            .and_then(|k| k.as_str())
            .unwrap_or_default();
        let signature = Signature::from_slice(&B64.decode(signature).ok()?).ok()?;
        let signed = format!("{}.{payload}", token.split('.').next()?);
        let keys = self
            .keys
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let ok = keys
            .iter()
            .filter(|(id, _)| kid.is_empty() || id == kid)
            .any(|(_, key)| key.verify(signed.as_bytes(), &signature).is_ok());
        if !ok {
            return None;
        }
        let claims: Claims = serde_json::from_slice(&B64.decode(payload).ok()?).ok()?;
        (claims.iss == self.issuer
            && claims.aud.iter().any(|a| a == "mods")
            && claims.exp > now
            && claims.iat <= now + 60
            && uuid::Uuid::parse_str(&claims.sub).is_ok())
        .then_some(claims)
    }
}

/// Fetches the account service's keys (`/.well-known/jwks.json`).
pub async fn fetch_jwks(http: &reqwest::Client, url: &str) -> Result<serde_json::Value, String> {
    let response = http.get(url).send().await.map_err(|e| e.to_string())?;
    if !response.status().is_success() {
        return Err(format!("{url} answered {}", response.status()));
    }
    response.json().await.map_err(|e| e.to_string())
}

/// Sessions seen in tokens, those the account service says ended, and
/// accounts it says were deleted. A token is refused once its session ended
/// or its account is gone, though its signature still holds until it runs
/// out.
#[derive(Default)]
pub struct Sessions {
    seen: Mutex<HashMap<String, Instant>>,
    ended: Mutex<HashMap<String, Instant>>,
    deleted: Mutex<HashMap<String, Instant>>,
}

/// Longer than any token lives (10 minutes).
pub const REMEMBER: Duration = Duration::from_secs(15 * 60);

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl Sessions {
    /// False for a token of a session that ended or of a deleted account;
    /// otherwise its session is noted for the next check.
    pub fn admit(&self, sid: &str, account: &str) -> bool {
        if lock(&self.ended).contains_key(sid) || lock(&self.deleted).contains_key(account) {
            return false;
        }
        let mut seen = lock(&self.seen);
        let now = Instant::now();
        if seen.len() > 200_000 {
            seen.retain(|_, at| now.duration_since(*at) < REMEMBER);
        }
        seen.insert(sid.to_owned(), now);
        true
    }

    /// Sessions seen in a token that may still be valid.
    pub fn recent(&self) -> Vec<String> {
        let now = Instant::now();
        let mut seen = lock(&self.seen);
        seen.retain(|_, at| now.duration_since(*at) < REMEMBER);
        seen.keys().cloned().collect()
    }

    pub fn end(&self, sids: &[String]) {
        remember(&self.ended, sids);
    }

    /// Accounts deleted lately: their tokens are refused until they run out.
    pub fn forget(&self, accounts: &[String]) {
        remember(&self.deleted, accounts);
    }
}

fn remember(map: &Mutex<HashMap<String, Instant>>, keys: &[String]) {
    let now = Instant::now();
    let mut map = lock(map);
    map.retain(|_, at| now.duration_since(*at) < REMEMBER);
    for key in keys {
        map.insert(key.clone(), now);
    }
}

/// Of these sessions, those that ended (signed out, or the account is
/// switched off or deleted), from the account service's internal API.
pub async fn ended(
    http: &reqwest::Client,
    base: &str,
    sessions: &[String],
) -> Result<Vec<String>, String> {
    let body: serde_json::Value = http
        .post(format!("{base}/internal/v1/ended"))
        .json(&serde_json::json!({ "sessions": sessions }))
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(|e| e.without_url().to_string())?
        .json()
        .await
        .map_err(|e| e.without_url().to_string())?;
    Ok(body["ended"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|s| Some(s.as_str()?.to_owned()))
        .collect())
}

/// Accounts deleted after `since` (Unix time): ID and when.
pub async fn deleted(
    http: &reqwest::Client,
    base: &str,
    since: i64,
) -> Result<Vec<(String, i64)>, String> {
    let body: serde_json::Value = http
        .get(format!("{base}/internal/v1/deleted"))
        .query(&[("since", since)])
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(|e| e.without_url().to_string())?
        .json()
        .await
        .map_err(|e| e.without_url().to_string())?;
    Ok(body["deleted"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|d| Some((d["id"].as_str()?.to_owned(), d["at"].as_i64()?)))
        .filter(|(id, _)| {
            uuid::Uuid::parse_str(id).is_ok_and(|u| u.hyphenated().to_string() == *id)
        })
        .collect())
}
