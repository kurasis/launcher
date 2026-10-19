//! Secrets, hashes and signed tokens.
//!
//! - Passwords: Argon2id (19 MiB, 2 passes, OWASP's minimum), PHC strings.
//! - Codes, refresh tokens, email links: 32 random bytes, base64url; only
//!   their SHA-256 is stored, so a copy of the database gives no working token.
//!   Letters waiting to be sent hold their link sealed with `mail.key`, which
//!   is not in the database, and sent letters keep no body.
//! - Access tokens: JWT signed with Ed25519 (`EdDSA`). The key is made on the
//!   server at the first start and never leaves it; other services check
//!   tokens with the public key from `/.well-known/jwks.json`.

use std::io::Write;
use std::path::Path;

use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::{Algorithm, Argon2, Params, Version};
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64;
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

/// 32 random bytes as base64url (43 characters).
pub fn token() -> String {
    let mut bytes = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    B64.encode(bytes)
}

/// What is stored for a token.
pub fn digest(token: &str) -> String {
    B64.encode(Sha256::digest(token.as_bytes()))
}

/// Equal without telling, by its timing, how much is equal.
pub fn same(a: &str, b: &str) -> bool {
    a.len() == b.len() && bool::from(a.as_bytes().ct_eq(b.as_bytes()))
}

/// PKCE S256: the challenge is base64url(SHA-256(verifier)); the verifier is
/// 43–128 characters of `A-Z a-z 0-9 - . _ ~` (RFC 7636).
pub fn pkce_matches(verifier: &str, challenge: &str) -> bool {
    (43..=128).contains(&verifier.len())
        && verifier
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-._~".contains(&b))
        && same(&B64.encode(Sha256::digest(verifier.as_bytes())), challenge)
}

fn argon() -> Argon2<'static> {
    let params = Params::new(19 * 1024, 2, 1, None).expect("valid Argon2 parameters");
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
}

pub fn hash_password(password: &str) -> String {
    let salt = SaltString::generate(&mut rand::rngs::OsRng);
    argon()
        .hash_password(password.as_bytes(), &salt)
        .expect("hashing with valid parameters")
        .to_string()
}

pub fn password_matches(password: &str, stored: &str) -> bool {
    PasswordHash::new(stored)
        .map(|hash| argon().verify_password(password.as_bytes(), &hash).is_ok())
        .unwrap_or(false)
}

/// A hash to check against when there is no account, so a missing account
/// takes as long as a wrong password.
pub fn dummy_hash() -> &'static str {
    static DUMMY: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    DUMMY.get_or_init(|| hash_password("no account has this password"))
}

/// The claims of an access token.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Claims {
    pub iss: String,
    /// The account ID.
    pub sub: String,
    pub aud: Vec<String>,
    pub iat: i64,
    pub exp: i64,
    /// The session the token belongs to.
    pub sid: String,
    pub nick: String,
    /// The email address is confirmed.
    pub ev: bool,
}

/// 32 secret bytes from `path`, or new ones written there (readable by the
/// service alone) on the first start.
pub fn secret_file(path: &Path) -> Result<[u8; 32], String> {
    match std::fs::read(path) {
        Ok(bytes) => bytes
            .try_into()
            .map_err(|_| format!("{} is not a key", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let mut seed = [0u8; 32];
            rand::rngs::OsRng.fill_bytes(&mut seed);
            let mut options = std::fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
            let mut file = options
                .open(path)
                .map_err(|e| format!("creating {}: {e}", path.display()))?;
            file.write_all(&seed)
                .and_then(|()| file.sync_all())
                .map_err(|e| format!("writing {}: {e}", path.display()))?;
            Ok(seed)
        }
        Err(e) => Err(format!("reading {}: {e}", path.display())),
    }
}

pub struct Keys {
    signing: SigningKey,
    pub kid: String,
}

impl Keys {
    /// Reads the key from `path`, or makes one there (readable by the service
    /// alone) on the first start.
    pub fn load_or_create(path: &Path) -> Result<Self, String> {
        Ok(Self::from_seed(secret_file(path)?))
    }

    pub fn from_seed(seed: [u8; 32]) -> Self {
        let signing = SigningKey::from_bytes(&seed);
        let kid = B64.encode(&Sha256::digest(signing.verifying_key().as_bytes())[..8]);
        Self { signing, kid }
    }

    pub fn public(&self) -> VerifyingKey {
        self.signing.verifying_key()
    }

    /// The public key as a JWK set.
    pub fn jwks(&self) -> serde_json::Value {
        serde_json::json!({
            "keys": [{
                "kty": "OKP",
                "crv": "Ed25519",
                "alg": "EdDSA",
                "use": "sig",
                "kid": self.kid,
                "x": B64.encode(self.public().as_bytes()),
            }]
        })
    }

    pub fn sign(&self, claims: &Claims) -> String {
        let header = serde_json::json!({ "alg": "EdDSA", "typ": "JWT", "kid": self.kid });
        let input = format!(
            "{}.{}",
            B64.encode(header.to_string()),
            B64.encode(serde_json::to_vec(claims).unwrap_or_default())
        );
        let signature = self.signing.sign(input.as_bytes());
        format!("{input}.{}", B64.encode(signature.to_bytes()))
    }
}

/// Checks a token's signature, issuer, audience and time; the claims if all
/// hold.
pub fn verify(
    key: &VerifyingKey,
    token: &str,
    issuer: &str,
    audience: &str,
    now: i64,
) -> Option<Claims> {
    if token.len() > 4096 {
        return None;
    }
    let mut parts = token.split('.');
    let (header, payload, signature) = (parts.next()?, parts.next()?, parts.next()?);
    if parts.next().is_some() {
        return None;
    }
    let header: serde_json::Value = serde_json::from_slice(&B64.decode(header).ok()?).ok()?;
    // Only our algorithm: never "none", never a key named by the token.
    if header.get("alg")?.as_str()? != "EdDSA" {
        return None;
    }
    let signature = Signature::from_slice(&B64.decode(signature).ok()?).ok()?;
    key.verify(
        format!("{}.{payload}", token.split('.').next()?).as_bytes(),
        &signature,
    )
    .ok()?;
    let claims: Claims = serde_json::from_slice(&B64.decode(payload).ok()?).ok()?;
    (claims.iss == issuer
        && claims.aud.iter().any(|a| a == audience)
        && claims.exp > now
        && claims.iat <= now + 60)
        .then_some(claims)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn claims(now: i64) -> Claims {
        Claims {
            iss: "https://oauth.example".into(),
            sub: "id".into(),
            aud: vec![
                "account".into(),
                "friends".into(),
                "skins".into(),
                "mods".into(),
            ],
            iat: now,
            exp: now + 600,
            sid: "s".into(),
            nick: "Steve".into(),
            ev: false,
        }
    }

    #[test]
    fn tokens_verify_only_as_signed() {
        let keys = Keys::from_seed([7; 32]);
        let now = 1_760_000_000;
        let token = keys.sign(&claims(now));
        let key = keys.public();
        let iss = "https://oauth.example";
        assert_eq!(verify(&key, &token, iss, "friends", now), Some(claims(now)));
        assert_eq!(verify(&key, &token, iss, "skins", now), Some(claims(now)));
        assert!(
            verify(&key, &token, iss, "other", now).is_none(),
            "audience"
        );
        assert!(verify(&key, &token, "https://evil", "friends", now).is_none());
        assert!(
            verify(&key, &token, iss, "friends", now + 601).is_none(),
            "expired"
        );
        // Another key, a changed payload, "alg: none" and junk are refused.
        let other = Keys::from_seed([8; 32]).sign(&claims(now));
        assert!(verify(&key, &other, iss, "friends", now).is_none());
        let mut forged = claims(now);
        forged.nick = "Admin".into();
        let parts: Vec<&str> = token.split('.').collect();
        let swapped = format!(
            "{}.{}.{}",
            parts[0],
            B64.encode(serde_json::to_vec(&forged).unwrap()),
            parts[2]
        );
        assert!(verify(&key, &swapped, iss, "friends", now).is_none());
        let none = format!(
            "{}.{}.",
            B64.encode(r#"{"alg":"none","typ":"JWT"}"#),
            parts[1]
        );
        assert!(verify(&key, &none, iss, "friends", now).is_none());
        for junk in ["", "a.b", "a.b.c.d", "...", &"x".repeat(5000)] {
            assert!(verify(&key, junk, iss, "friends", now).is_none());
        }
    }

    #[test]
    fn passwords_pkce_and_digests() {
        let stored = hash_password("correct horse battery");
        assert!(stored.starts_with("$argon2id$"));
        assert!(password_matches("correct horse battery", &stored));
        assert!(!password_matches("correct horse batterY", &stored));
        assert!(!password_matches("x", "not a hash"));
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        // RFC 7636, appendix B.
        assert!(pkce_matches(
            verifier,
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        ));
        assert!(!pkce_matches(
            "short",
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        ));
        assert!(!pkce_matches(
            verifier,
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cN"
        ));
        assert_ne!(token(), token());
        assert_eq!(token().len(), 43);
        assert_eq!(digest("a"), digest("a"));
        assert!(same("abc", "abc") && !same("abc", "abd") && !same("abc", "ab"));
    }

    #[test]
    fn the_key_is_made_once_and_private() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("signing.key");
        let first = Keys::load_or_create(&path).unwrap();
        let second = Keys::load_or_create(&path).unwrap();
        assert_eq!(first.kid, second.kid);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        std::fs::write(&path, b"short").unwrap();
        assert!(Keys::load_or_create(&path).is_err());
    }
}
