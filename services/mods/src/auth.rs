//! Who is asking: access tokens of oauth.lapi.sh (D-064) with `mods` among
//! their audiences, checked here with its published Ed25519 key. Only adding
//! to the catalog needs one; reading it is open.

use std::sync::RwLock;

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
