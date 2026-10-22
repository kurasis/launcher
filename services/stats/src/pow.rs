//! The work a request carries (D-071): a nonce such that the SHA-256 of
//! `stats.lapi.sh|<batch>|<day>|<nonce>` starts with `bits` zero bits. A
//! launcher finds one in well under a second once an hour; thousands of
//! made-up batches cost as many seconds. The batch is random and used once,
//! so a nonce cannot be reused, and no challenge has to be fetched first.

use sha2::{Digest, Sha256};

/// Bits asked for unless `LAPI_STATS_WORK_BITS` says otherwise.
pub const DEFAULT_BITS: u32 = 20;
/// What the setting may say.
pub const MIN_BITS: u32 = 8;
pub const MAX_BITS: u32 = 28;

/// Leading zero bits of a digest.
pub fn zeros(digest: &[u8]) -> u32 {
    let mut bits = 0;
    for byte in digest {
        if *byte == 0 {
            bits += 8;
        } else {
            return bits + byte.leading_zeros();
        }
    }
    bits
}

pub fn digest(batch: &str, day: &str, nonce: &str) -> [u8; 32] {
    Sha256::digest(format!("stats.lapi.sh|{batch}|{day}|{nonce}").as_bytes()).into()
}

/// Whether `nonce` (decimal digits, at most 20) does the work.
pub fn done(batch: &str, day: &str, nonce: &str, bits: u32) -> bool {
    !nonce.is_empty()
        && nonce.len() <= 20
        && nonce.bytes().all(|b| b.is_ascii_digit())
        && zeros(&digest(batch, day, nonce)) >= bits
}

/// Finds a nonce; for tests and the documentation's example.
#[cfg(test)]
pub fn solve(batch: &str, day: &str, bits: u32) -> String {
    (0u64..)
        .map(|n| n.to_string())
        .find(|nonce| done(batch, day, nonce, bits))
        .unwrap_or_default()
}
