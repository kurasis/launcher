//! Proof of work: before a review the launcher asks for a challenge and finds
//! a number that, hashed with it, starts with enough zero bits. A second on a
//! player's computer; a thousand a second for a spammer. A challenge is signed
//! by the service, lives ten minutes and is taken once.

use std::collections::HashMap;
use std::sync::Mutex;

use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};

/// How long a challenge may be used.
pub const LIFETIME: i64 = 600;
pub const DEFAULT_BITS: u8 = 20;
pub const MAX_BITS: u8 = 28;

pub struct Challenges {
    secret: [u8; 32],
    used: Mutex<HashMap<String, i64>>,
}

fn mac(secret: &[u8; 32], text: &str) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret).expect("any key length");
    mac.update(b"pow|");
    mac.update(text.as_bytes());
    hex::encode(&mac.finalize().into_bytes()[..16])
}

/// Leading zero bits of a hash.
fn zeros(hash: &[u8]) -> u32 {
    let mut count = 0;
    for byte in hash {
        if *byte == 0 {
            count += 8;
        } else {
            return count + byte.leading_zeros();
        }
    }
    count
}

/// The hash a solution is judged by: SHA-256 of `challenge:nonce`.
pub fn hash(challenge: &str, nonce: &str) -> [u8; 32] {
    Sha256::digest(format!("{challenge}:{nonce}").as_bytes()).into()
}

/// The first nonce that solves a challenge (the launcher does this; here for
/// the tests).
#[cfg(test)]
pub fn solve(challenge: &str, bits: u8) -> String {
    (0u64..)
        .map(|n| n.to_string())
        .find(|n| zeros(&hash(challenge, n)) >= u32::from(bits))
        .unwrap_or_default()
}

impl Challenges {
    pub fn new(secret: [u8; 32]) -> Self {
        Self {
            secret,
            used: Mutex::new(HashMap::new()),
        }
    }

    /// `bits.expires.random.signature`.
    pub fn issue(&self, bits: u8, now: i64) -> String {
        let mut random = [0u8; 12];
        getrandom::fill(&mut random).expect("system randomness");
        let body = format!("{bits}.{}.{}", now + LIFETIME, hex::encode(random));
        let signature = mac(&self.secret, &body);
        format!("{body}.{signature}")
    }

    /// True once per signed, unexpired challenge whose nonce is good enough.
    pub fn take(&self, challenge: &str, nonce: &str, now: i64) -> bool {
        if challenge.len() > 100 || nonce.is_empty() || nonce.len() > 20 {
            return false;
        }
        if !nonce.bytes().all(|b| b.is_ascii_digit()) {
            return false;
        }
        let Some((body, signature)) = challenge.rsplit_once('.') else {
            return false;
        };
        if mac(&self.secret, body) != signature {
            return false;
        }
        let mut parts = body.split('.');
        let (Some(bits), Some(expires)) = (
            parts.next().and_then(|b| b.parse::<u8>().ok()),
            parts.next().and_then(|e| e.parse::<i64>().ok()),
        ) else {
            return false;
        };
        if expires <= now || zeros(&hash(challenge, nonce)) < u32::from(bits) {
            return false;
        }
        let mut used = self
            .used
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        used.retain(|_, until| *until > now);
        used.insert(challenge.to_owned(), expires).is_none()
    }
}
