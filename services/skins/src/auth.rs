//! Who is asking: access tokens of oauth.lapi.sh (D-064), checked here with its
//! published Ed25519 key, without asking it each time. As for friends, but the
//! token must name `skins` among its audiences (D-065).

use std::sync::RwLock;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::Deserialize;

/// The audience a token must name to be taken here.
pub const AUDIENCE: &str = "skins";

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

    /// The claims of a token signed by the account service for skins, if it
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
            && claims.aud.iter().any(|a| a == AUDIENCE)
            && claims.exp > now
            && claims.iat <= now + 60
            && crate::directory::account_id(&claims.sub))
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

/// Sessions seen in tokens, and those the account service says ended. A
/// token is refused once its session ended, though its signature still holds
/// until it runs out.
#[derive(Default)]
pub struct Sessions {
    seen: std::sync::Mutex<std::collections::HashMap<String, std::time::Instant>>,
    ended: std::sync::Mutex<std::collections::HashMap<String, std::time::Instant>>,
    deleted: std::sync::Mutex<std::collections::HashMap<String, std::time::Instant>>,
}

/// Longer than any token lives (10 minutes).
const REMEMBER: std::time::Duration = std::time::Duration::from_secs(15 * 60);

impl Sessions {
    pub fn ended(&self, sid: &str) -> bool {
        self.ended
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .contains_key(sid)
    }

    pub fn seen(&self, sid: &str) {
        let mut seen = self
            .seen
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if seen.len() > 200_000 {
            let now = std::time::Instant::now();
            seen.retain(|_, at| now.duration_since(*at) < REMEMBER);
        }
        seen.insert(sid.to_owned(), std::time::Instant::now());
    }

    /// Sessions seen in a token that may still be valid.
    pub fn recent(&self) -> Vec<String> {
        let now = std::time::Instant::now();
        let mut seen = self
            .seen
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        seen.retain(|_, at| now.duration_since(*at) < REMEMBER);
        seen.keys().cloned().collect()
    }

    pub fn end(&self, sids: &[String]) {
        let now = std::time::Instant::now();
        let mut ended = self
            .ended
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        ended.retain(|_, at| now.duration_since(*at) < REMEMBER);
        for sid in sids {
            ended.insert(sid.clone(), now);
        }
    }

    /// Whether the account was deleted lately.
    pub fn gone(&self, account: &str) -> bool {
        self.deleted
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .contains_key(account)
    }

    /// Accounts deleted lately: their tokens are refused until they run out,
    /// even from a session never seen here, so nothing new is kept for them
    /// after they were cleaned out.
    pub fn forget(&self, accounts: &[String]) {
        let now = std::time::Instant::now();
        let mut deleted = self
            .deleted
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        deleted.retain(|_, at| now.duration_since(*at) < REMEMBER);
        for account in accounts {
            deleted.insert(account.clone(), now);
        }
    }
}
