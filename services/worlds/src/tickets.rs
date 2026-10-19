//! Join tickets (ADAPTATION A3): Ed25519 JWTs signed by this service, valid
//! for 60 seconds, for one member, room, session, device, guest endpoint and
//! host epoch. A host checks the signature with the published keys and then
//! consumes the ticket here, once.

use std::path::Path;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64;
use ed25519_dalek::{Signer, SigningKey, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const TTL: i64 = 60;
pub const AUDIENCE: &str = "worlds-host";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ticket {
    pub iss: String,
    pub aud: String,
    pub jti: String,
    pub iat: i64,
    pub nbf: i64,
    pub exp: i64,
    /// The member's account.
    pub sub: String,
    /// The member's device.
    pub dev: String,
    /// The guest's tunnel endpoint (iroh public key).
    pub epk: String,
    pub room: String,
    pub ses: String,
    /// Host epoch of the session.
    pub hep: i64,
    /// The service's authority epoch.
    pub aep: String,
    pub svc: String,
}

pub struct TicketKey {
    key: SigningKey,
    kid: String,
}

impl TicketKey {
    pub fn new(key: SigningKey) -> Self {
        let kid = hex(&Sha256::digest(key.verifying_key().as_bytes())[..8]);
        Self { key, kid }
    }

    /// The key in `path`, made on first start (readable by the service only).
    pub fn load_or_create(path: &Path) -> Result<Self, String> {
        if let Ok(text) = std::fs::read_to_string(path) {
            let bytes: [u8; 32] = from_hex(text.trim())
                .and_then(|b| b.try_into().ok())
                .ok_or("the ticket key file is damaged")?;
            return Ok(Self::new(SigningKey::from_bytes(&bytes)));
        }
        let mut seed = [0u8; 32];
        getrandom::fill(&mut seed).map_err(|e| e.to_string())?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::write(path, hex(&seed)).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
                .map_err(|e| e.to_string())?;
        }
        Ok(Self::new(SigningKey::from_bytes(&seed)))
    }

    pub fn sign(&self, ticket: &Ticket) -> String {
        let header = serde_json::json!({"alg": "EdDSA", "typ": "JWT", "kid": self.kid});
        let head = B64.encode(serde_json::to_vec(&header).unwrap_or_default());
        let body = B64.encode(serde_json::to_vec(ticket).unwrap_or_default());
        let signature = self.key.sign(format!("{head}.{body}").as_bytes());
        format!("{head}.{body}.{}", B64.encode(signature.to_bytes()))
    }

    pub fn public(&self) -> VerifyingKey {
        self.key.verifying_key()
    }

    pub fn jwks(&self) -> serde_json::Value {
        serde_json::json!({"keys": [{
            "kty": "OKP", "crv": "Ed25519", "use": "sig", "alg": "EdDSA",
            "kid": self.kid, "x": B64.encode(self.public().as_bytes()),
        }]})
    }

    /// The ticket, if this service signed it and it is valid at `now`.
    pub fn verify(&self, token: &str, issuer: &str, now: i64) -> Option<Ticket> {
        if token.len() > 4096 {
            return None;
        }
        let mut parts = token.split('.');
        let (head, body, signature) = (parts.next()?, parts.next()?, parts.next()?);
        if parts.next().is_some() {
            return None;
        }
        let header: serde_json::Value = serde_json::from_slice(&B64.decode(head).ok()?).ok()?;
        if header.get("alg")?.as_str()? != "EdDSA" || header.get("kid")?.as_str()? != self.kid {
            return None;
        }
        let signature = ed25519_dalek::Signature::from_slice(&B64.decode(signature).ok()?).ok()?;
        use ed25519_dalek::Verifier as _;
        self.public()
            .verify(format!("{head}.{body}").as_bytes(), &signature)
            .ok()?;
        let ticket: Ticket = serde_json::from_slice(&B64.decode(body).ok()?).ok()?;
        (ticket.iss == issuer
            && ticket.aud == AUDIENCE
            && ticket.svc == "gameplay"
            && ticket.nbf <= now
            && now < ticket.exp
            && ticket.exp - ticket.iat <= TTL)
            .then_some(ticket)
    }
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn from_hex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) || !text.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ticket(now: i64) -> Ticket {
        Ticket {
            iss: "https://worlds.lapi.sh".into(),
            aud: AUDIENCE.into(),
            jti: "t1".into(),
            iat: now,
            nbf: now,
            exp: now + TTL,
            sub: "a".into(),
            dev: "d".into(),
            epk: "e".into(),
            room: "r".into(),
            ses: "s".into(),
            hep: 1,
            aep: "x".into(),
            svc: "gameplay".into(),
        }
    }

    #[test]
    fn tickets_verify_only_while_valid_and_untouched() {
        let signer = TicketKey::new(SigningKey::from_bytes(&[3; 32]));
        let token = signer.sign(&ticket(1000));
        let iss = "https://worlds.lapi.sh";
        assert_eq!(signer.verify(&token, iss, 1000), Some(ticket(1000)));
        assert!(signer.verify(&token, iss, 1060).is_none(), "expired");
        assert!(signer.verify(&token, iss, 999).is_none(), "not yet");
        assert!(signer.verify(&token, "https://evil", 1000).is_none());
        let other = TicketKey::new(SigningKey::from_bytes(&[4; 32]));
        assert!(other.verify(&token, iss, 1000).is_none());
        let mut parts: Vec<&str> = token.split('.').collect();
        let mut changed = ticket(1000);
        changed.sub = "b".into();
        let body = B64.encode(serde_json::to_vec(&changed).unwrap());
        parts[1] = &body;
        assert!(signer.verify(&parts.join("."), iss, 1000).is_none());
        let mut long = ticket(1000);
        long.exp = 1000 + 3600;
        assert!(
            signer.verify(&signer.sign(&long), iss, 1000).is_none(),
            "too long"
        );
    }

    #[test]
    fn the_key_is_made_once() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ticket-key");
        let a = TicketKey::load_or_create(&path).unwrap();
        let b = TicketKey::load_or_create(&path).unwrap();
        assert_eq!(a.public(), b.public());
        std::fs::write(&path, "zz").unwrap();
        assert!(TicketKey::load_or_create(&path).is_err());
    }
}
