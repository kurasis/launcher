//! Who is asking, and from which device (ADAPTATION A4).
//!
//! Access tokens of oauth.lapi.sh are checked here with its published Ed25519
//! keys, for the audience `worlds`. Calls a device makes for itself (hosting,
//! tickets, admissions) also carry a DPoP proof (RFC 9449) signed with that
//! device's Ed25519 key: bound to the method, the URL and this access token,
//! fresh, and used once.

use std::collections::HashMap;
use std::sync::{Mutex, RwLock};
use std::time::{Duration, Instant};

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::Deserialize;
use sha2::{Digest, Sha256};

/// What a valid access token says.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Claims {
    pub iss: String,
    pub sub: String,
    pub aud: Vec<String>,
    pub iat: i64,
    pub exp: i64,
    pub sid: String,
    pub nick: String,
    #[serde(default)]
    pub ev: bool,
}

pub const MAX_TOKEN: usize = 4096;

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Splits a compact JWS and checks its signature with `key`; returns the
/// header and payload JSON.
fn verify_jws(token: &str, key: &VerifyingKey) -> Option<(serde_json::Value, serde_json::Value)> {
    let (header, payload) = parts(token)?;
    let mut split = token.split('.');
    let signed = format!("{}.{}", split.next()?, split.next()?);
    let signature = Signature::from_slice(&B64.decode(split.next()?).ok()?).ok()?;
    key.verify(signed.as_bytes(), &signature).ok()?;
    Some((header, payload))
}

/// Header and payload of a compact JWS without checking it (to find the key).
fn parts(token: &str) -> Option<(serde_json::Value, serde_json::Value)> {
    if token.len() > MAX_TOKEN {
        return None;
    }
    let mut split = token.split('.');
    let (header, payload, _signature) = (split.next()?, split.next()?, split.next()?);
    if split.next().is_some() {
        return None;
    }
    let header: serde_json::Value = serde_json::from_slice(&B64.decode(header).ok()?).ok()?;
    let payload: serde_json::Value = serde_json::from_slice(&B64.decode(payload).ok()?).ok()?;
    // Only EdDSA; a token never picks its own algorithm.
    (header.get("alg")?.as_str()? == "EdDSA").then_some((header, payload))
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
                Some((
                    k["kid"].as_str().unwrap_or_default().to_owned(),
                    okp_key(k)?,
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

    /// The claims of a valid token for this service.
    pub fn verify(&self, token: &str, now: i64) -> Option<Claims> {
        let (header, _) = parts(token)?;
        let kid = header
            .get("kid")
            .and_then(|k| k.as_str())
            .unwrap_or_default();
        let keys = self
            .keys
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let payload = keys
            .iter()
            .filter(|(id, _)| kid.is_empty() || id == kid)
            .find_map(|(_, key)| verify_jws(token, key))?
            .1;
        let claims: Claims = serde_json::from_value(payload).ok()?;
        (claims.iss == self.issuer
            && claims.aud.iter().any(|a| a == "worlds")
            && claims.exp > now
            && claims.iat <= now + 60
            && uuid::Uuid::parse_str(&claims.sub).is_ok())
        .then_some(claims)
    }
}

pub async fn fetch_jwks(http: &reqwest::Client, url: &str) -> Result<serde_json::Value, String> {
    let response = http.get(url).send().await.map_err(|e| e.to_string())?;
    if !response.status().is_success() {
        return Err(format!("{url} answered {}", response.status()));
    }
    response.json().await.map_err(|e| e.to_string())
}

/// The Ed25519 key of an `OKP` JWK.
pub fn okp_key(jwk: &serde_json::Value) -> Option<VerifyingKey> {
    if jwk["kty"] != "OKP" || jwk["crv"] != "Ed25519" {
        return None;
    }
    let x: [u8; 32] = B64.decode(jwk["x"].as_str()?).ok()?.try_into().ok()?;
    VerifyingKey::from_bytes(&x).ok()
}

/// A device public key as stored: unpadded base64url of its 32 bytes.
pub fn key_text(key: &VerifyingKey) -> String {
    B64.encode(key.as_bytes())
}

/// `ath`: the hash of the access token a proof is bound to.
pub fn token_hash(token: &str) -> String {
    B64.encode(Sha256::digest(token.as_bytes()))
}

/// Proofs older or newer than this are refused.
pub const PROOF_WINDOW: i64 = 60;

/// Proof IDs seen in the window, so a captured proof cannot be replayed.
#[derive(Default)]
pub struct Replays {
    seen: Mutex<HashMap<String, Instant>>,
}

impl Replays {
    /// True the first time `jti` is seen.
    pub fn first(&self, jti: &str) -> bool {
        let mut seen = lock(&self.seen);
        let now = Instant::now();
        if seen.len() > 100_000 {
            seen.retain(|_, at| now.duration_since(*at) < Duration::from_secs(300));
        }
        if seen.contains_key(jti) {
            return false;
        }
        seen.insert(jti.to_owned(), now);
        true
    }
}

/// Checks a DPoP proof for `method` and `url` bound to `access_token`.
/// Returns the device key it was made with.
pub fn verify_proof(
    proof: &str,
    method: &str,
    url: &str,
    access_token: &str,
    now: i64,
    replays: &Replays,
) -> Option<String> {
    let (header, _) = parts(proof)?;
    if header.get("typ")?.as_str()? != "dpop+jwt" {
        return None;
    }
    let key = okp_key(header.get("jwk")?)?;
    let (_, payload) = verify_jws(proof, &key)?;
    let jti = payload.get("jti")?.as_str()?;
    let iat = payload.get("iat")?.as_i64()?;
    let fresh = (now - iat).abs() <= PROOF_WINDOW;
    let bound = payload.get("htm")?.as_str()? == method
        && payload.get("htu")?.as_str()? == url
        && payload.get("ath")?.as_str()? == token_hash(access_token);
    (fresh && bound && (16..=128).contains(&jti.len()) && replays.first(jti))
        .then(|| key_text(&key))
}

#[cfg(test)]
pub mod testing {
    //! Signing helpers for tests: an account service and a device.

    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

    pub fn sign(
        key: &SigningKey,
        header: &serde_json::Value,
        payload: &serde_json::Value,
    ) -> String {
        let head = B64.encode(serde_json::to_vec(header).unwrap());
        let body = B64.encode(serde_json::to_vec(payload).unwrap());
        let signature = key.sign(format!("{head}.{body}").as_bytes());
        format!("{head}.{body}.{}", B64.encode(signature.to_bytes()))
    }

    pub fn jwk(key: &SigningKey) -> serde_json::Value {
        serde_json::json!({"kty": "OKP", "crv": "Ed25519", "x": B64.encode(key.verifying_key().as_bytes())})
    }

    pub fn proof(key: &SigningKey, method: &str, url: &str, token: &str, iat: i64) -> String {
        sign(
            key,
            &serde_json::json!({"typ": "dpop+jwt", "alg": "EdDSA", "jwk": jwk(key)}),
            &serde_json::json!({
                "jti": uuid::Uuid::new_v4().to_string(),
                "htm": method,
                "htu": url,
                "iat": iat,
                "ath": token_hash(token),
            }),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::testing::*;
    use super::*;
    use ed25519_dalek::SigningKey;

    #[test]
    fn proofs_are_bound_fresh_and_single_use() {
        let device = SigningKey::from_bytes(&[7; 32]);
        let replays = Replays::default();
        let url = "https://worlds.lapi.sh/v1/rooms/r/tickets";
        let now = 1_000_000;
        let good = proof(&device, "POST", url, "tok", now);
        assert_eq!(
            verify_proof(&good, "POST", url, "tok", now, &replays),
            Some(key_text(&device.verifying_key()))
        );
        assert_eq!(
            verify_proof(&good, "POST", url, "tok", now, &replays),
            None,
            "replay"
        );
        for (method, u, token, iat) in [
            ("GET", url, "tok", now),
            ("POST", "https://worlds.lapi.sh/v1/other", "tok", now),
            ("POST", url, "other", now),
            ("POST", url, "tok", now - 61),
            ("POST", url, "tok", now + 61),
        ] {
            let p = proof(&device, "POST", url, "tok", iat);
            assert_eq!(verify_proof(&p, method, u, token, now, &replays), None);
        }
        // Signed by another key than the one in its header.
        let other = SigningKey::from_bytes(&[8; 32]);
        let forged = sign(
            &other,
            &serde_json::json!({"typ": "dpop+jwt", "alg": "EdDSA", "jwk": jwk(&device)}),
            &serde_json::json!({"jti": "0123456789abcdef", "htm": "POST", "htu": url, "iat": now, "ath": token_hash("tok")}),
        );
        assert_eq!(
            verify_proof(&forged, "POST", url, "tok", now, &replays),
            None
        );
        let none_alg = good.replacen(
            &good[..good.find('.').unwrap()],
            &B64.encode(br#"{"alg":"none","typ":"dpop+jwt"}"#),
            1,
        );
        assert_eq!(
            verify_proof(&none_alg, "POST", url, "tok", now, &replays),
            None
        );
    }

    #[test]
    fn tokens_need_the_worlds_audience() {
        let issuer = SigningKey::from_bytes(&[1; 32]);
        let tokens = Tokens::new("https://oauth.lapi.sh");
        tokens.set_jwks(&serde_json::json!({"keys": [jwk(&issuer)]}));
        let make = |aud: &str| {
            sign(
                &issuer,
                &serde_json::json!({"alg": "EdDSA", "typ": "JWT"}),
                &serde_json::json!({
                    "iss": "https://oauth.lapi.sh", "sub": uuid::Uuid::new_v4().to_string(),
                    "aud": [aud], "iat": 100, "exp": 700, "sid": "s", "nick": "Notch", "ev": true
                }),
            )
        };
        assert!(tokens.verify(&make("worlds"), 200).is_some());
        assert!(tokens.verify(&make("friends"), 200).is_none());
        assert!(tokens.verify(&make("worlds"), 701).is_none());
    }
}
