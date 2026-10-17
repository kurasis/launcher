//! PKCE (RFC 7636, S256) and OAuth `state` values.
//!
//! Every login attempt gets a fresh verifier and state from the operating system's
//! cryptographic random source. Both stay in Rust memory as secrets.

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use sha2::{Digest, Sha256};

use crate::error::{LauncherError, LauncherResult};
use crate::secrets::SecretString;

/// 32 random bytes encode to a 43-character verifier, the RFC 7636 minimum length.
const RANDOM_BYTES: usize = 32;

fn random_token() -> LauncherResult<String> {
    let mut bytes = [0u8; RANDOM_BYTES];
    getrandom::fill(&mut bytes)
        .map_err(|_| LauncherError::internal("The system random number generator failed."))?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

/// A PKCE verifier with its S256 challenge.
pub struct Pkce {
    pub verifier: SecretString,
    pub challenge: String,
}

impl Pkce {
    pub fn generate() -> LauncherResult<Self> {
        let verifier = random_token()?;
        let challenge = challenge_for(&verifier);
        Ok(Self {
            verifier: SecretString::new(verifier),
            challenge,
        })
    }
}

/// `BASE64URL(SHA256(ASCII(verifier)))` without padding.
pub fn challenge_for(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

/// A fresh, unguessable OAuth `state` value.
pub fn new_state() -> LauncherResult<SecretString> {
    random_token().map(SecretString::new)
}

/// Constant-time comparison for secrets such as `state`.
pub fn secrets_equal(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc7636_appendix_b_vector() {
        assert_eq!(
            challenge_for("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn verifiers_are_fresh_and_well_formed() {
        let a = Pkce::generate().unwrap();
        let b = Pkce::generate().unwrap();
        assert_ne!(a.verifier.expose(), b.verifier.expose());
        assert_eq!(a.verifier.expose().len(), 43);
        assert!(
            a.verifier
                .expose()
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
        );
        assert_eq!(a.challenge, challenge_for(a.verifier.expose()));
        assert_ne!(new_state().unwrap().expose(), new_state().unwrap().expose());
    }

    #[test]
    fn secret_comparison() {
        assert!(secrets_equal("abc", "abc"));
        assert!(!secrets_equal("abc", "abd"));
        assert!(!secrets_equal("abc", "abcd"));
    }
}
