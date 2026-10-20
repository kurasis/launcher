//! The worlds.lapi.sh client (ADAPTATION A4, plan W7).
//!
//! Every call carries the launcher account's access token. Calls made for this
//! computer (hosting, tickets, admissions, the relay) also carry a DPoP proof
//! signed with the device key: an Ed25519 key made on first use, kept in the
//! system credential store and registered with the account as a device before
//! its first such call. A refused device (revoked, or its key held by another
//! account) gets a new key and is registered again, once.

use std::sync::{Arc, Mutex};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64;
use iroh::SecretKey;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::auth::vault::SecretStore;
use crate::error::{ErrorCode, LauncherError};
use crate::install::service::BoxFuture;
use crate::secrets::SecretString;
use crate::sync::lock;

const BASE: &str = "https://worlds.lapi.sh";
const MAX_RESPONSE_BYTES: usize = 512 * 1024;
/// The device key's entry in the credential store (hex of its 32 bytes).
pub const DEVICE_KEY: &str = "worlds-device-key";

/// worlds.lapi.sh, or the server a test build was made for
/// (`LAUNCHER_WORLDS_URL` at build time; release builds never set it).
pub fn base() -> &'static str {
    option_env!("LAUNCHER_WORLDS_URL")
        .filter(|u| !u.trim().is_empty())
        .unwrap_or(BASE)
}

/// What went wrong, for the interface to say in the player's language.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Problem {
    /// No launcher account, or its sign-in ended.
    SignedOut,
    Network,
    Server,
    TooMany,
    Invalid,
    NotFound,
    Forbidden,
    Banned,
    /// The account's nick does not fit a game name.
    NickUnfit,
    InviteInvalid,
    NotMember,
    /// Ten rooms, devices or invites already.
    Limit,
    WorldTaken,
    RevisionConflict,
    OwnerCannotLeave,
    NotHosting,
    /// The host's session ended or another start replaced it.
    SessionEnded,
    TicketInvalid,
    WrongSession,
    RoomFull,
    AlreadyConnected,
    /// This computer's device was refused.
    Device,
    /// Another refusal the service gave.
    Refused,
}

impl Problem {
    fn from_code(code: &str) -> Self {
        match code {
            "unauthorized" => Self::SignedOut,
            "bad_request" => Self::Invalid,
            "too_many" => Self::TooMany,
            "server_error" => Self::Server,
            "not_found" | "room_deleted" => Self::NotFound,
            "forbidden" => Self::Forbidden,
            "banned" => Self::Banned,
            "nick_unfit" => Self::NickUnfit,
            "invite_invalid" => Self::InviteInvalid,
            "not_member" | "removed" | "left" => Self::NotMember,
            "room_limit" | "device_limit" | "invite_limit" => Self::Limit,
            "world_taken" => Self::WorldTaken,
            "revision_conflict" => Self::RevisionConflict,
            "owner_cannot_leave" => Self::OwnerCannotLeave,
            "not_hosting" => Self::NotHosting,
            "session_ended" | "superseded" | "stopped" => Self::SessionEnded,
            "ticket_invalid" => Self::TicketInvalid,
            "wrong_session" => Self::WrongSession,
            "room_full" => Self::RoomFull,
            "already_connected" => Self::AlreadyConnected,
            "invalid_proof" | "device_unknown" | "device_revoked" | "device_key_taken" => {
                Self::Device
            }
            _ => Self::Refused,
        }
    }
}

impl Problem {
    /// The name the interface gets, as in the JSON.
    pub fn name(self) -> String {
        serde_json::to_value(self)
            .ok()
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_default()
    }
}

impl From<Problem> for LauncherError {
    fn from(problem: Problem) -> Self {
        let (code, message) = match problem {
            Problem::SignedOut => (
                ErrorCode::AuthReauthRequired,
                "Sign in to the launcher account again.",
            ),
            Problem::Network => (ErrorCode::NetworkError, "worlds.lapi.sh cannot be reached."),
            Problem::Server => (
                ErrorCode::ServiceUnavailable,
                "worlds.lapi.sh is not working right now.",
            ),
            Problem::TooMany => (ErrorCode::RateLimited, "Too many tries. Wait a minute."),
            _ => (ErrorCode::InvalidInput, "worlds.lapi.sh refused this."),
        };
        LauncherError::new(code, message).with_detail("worldsProblem", problem.name())
    }
}

/// The launcher account's access token, asked for before each call.
pub trait Token: Send + Sync + 'static {
    fn token(&self) -> BoxFuture<'_, Result<SecretString, Problem>>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Room {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub minecraft: String,
    pub revision: i64,
    pub owner: String,
    pub owner_nick: String,
    pub role: String,
    /// The world on the owner's computer, for the owner only.
    #[serde(default)]
    pub world: Option<String>,
    pub members: i64,
    pub hosting: bool,
    pub online: i64,
    pub max_players: i64,
    pub created: i64,
    pub updated: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Member {
    pub account: String,
    pub nick: String,
    pub role: String,
    pub state: String,
    pub joined: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Invite {
    pub id: String,
    pub created: i64,
    pub expires: i64,
    pub max_uses: i64,
    pub uses: i64,
}

/// A fresh invite with its code, shown once.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewInvite {
    pub invite: Invite,
    pub code: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    pub room: String,
    pub name: String,
    pub owner_nick: String,
    pub members: i64,
    pub minecraft: String,
}

/// A computer registered with the account.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Device {
    pub id: String,
    pub name: String,
    pub created: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub id: String,
    pub room: String,
    pub epoch: i64,
    pub endpoint: String,
    pub max_players: i64,
    pub lease_until: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Renewal {
    pub lease_until: i64,
    /// Nicks of the active members, for the whitelist.
    pub members: Vec<String>,
    /// Admissions to close now.
    pub revoked: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Admission {
    pub id: String,
    pub account: String,
    pub nick: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TicketGrant {
    pub ticket: String,
    pub session: String,
    pub host_epoch: String,
    pub host_endpoint: String,
    pub expires: i64,
}

#[derive(Debug, Deserialize)]
struct Enrolled {
    ttl: i64,
}

/// What a call is.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Proof {
    /// The account's token is enough.
    No,
    /// Made for this computer: carries a DPoP proof of the device key.
    Device,
}

pub struct WorldsClient {
    http: reqwest::Client,
    base: String,
    vault: Arc<dyn SecretStore>,
    token: Arc<dyn Token>,
    device_name: String,
    key: Mutex<Option<SecretKey>>,
    /// The access token's account was registered with this key in this run,
    /// and the device id the service gave it.
    registered: Mutex<Option<(String, String)>>,
}

fn proof_hash(token: &str) -> String {
    B64.encode(Sha256::digest(token.as_bytes()))
}

/// A DPoP proof (RFC 9449) for one request, signed with `key`.
pub fn dpop(key: &SecretKey, method: &str, url: &str, token: &str, now: i64) -> String {
    let header = json!({
        "typ": "dpop+jwt",
        "alg": "EdDSA",
        "jwk": {"kty": "OKP", "crv": "Ed25519", "x": B64.encode(key.public().as_bytes())},
    });
    let payload = json!({
        "jti": uuid::Uuid::new_v4().to_string(),
        "htm": method,
        "htu": url,
        "iat": now,
        "ath": proof_hash(token),
    });
    let head = B64.encode(header.to_string());
    let body = B64.encode(payload.to_string());
    let signature = key.sign(format!("{head}.{body}").as_bytes());
    format!("{head}.{body}.{}", B64.encode(signature.to_bytes()))
}

impl WorldsClient {
    pub fn new(
        app_version: &str,
        base: &str,
        vault: Arc<dyn SecretStore>,
        token: Arc<dyn Token>,
        device_name: &str,
    ) -> Result<Self, String> {
        let http = crate::launcher_account::our_client(app_version, base)?;
        // A name the service accepts: one short line.
        let device_name: String = device_name
            .chars()
            .filter(|c| !c.is_control())
            .take(48)
            .collect();
        Ok(Self {
            http,
            base: base.trim_end_matches('/').to_owned(),
            vault,
            token,
            device_name: if device_name.trim().is_empty() {
                "Launcher".to_owned()
            } else {
                device_name.trim().to_owned()
            },
            key: Mutex::new(None),
            registered: Mutex::new(None),
        })
    }

    /// The device key: from the credential store, or made now.
    fn device_key(&self) -> Result<SecretKey, Problem> {
        if let Some(key) = lock(&self.key).clone() {
            return Ok(key);
        }
        let stored = self
            .vault
            .get(DEVICE_KEY)
            .ok()
            .flatten()
            .and_then(|hex_key| {
                let bytes: [u8; 32] = hex::decode(hex_key.expose()).ok()?.try_into().ok()?;
                Some(SecretKey::from_bytes(&bytes))
            });
        let key = match stored {
            Some(key) => key,
            None => self.new_device_key()?,
        };
        *lock(&self.key) = Some(key.clone());
        Ok(key)
    }

    fn new_device_key(&self) -> Result<SecretKey, Problem> {
        let key = SecretKey::generate();
        let text = SecretString::new(hex::encode(key.to_bytes()));
        // Without the vault the key lives for this run: the device is then
        // registered again next time.
        let _ = self.vault.put(DEVICE_KEY, &text);
        *lock(&self.key) = Some(key.clone());
        *lock(&self.registered) = None;
        Ok(key)
    }

    async fn send(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<&Value>,
        proof: Proof,
        token: &SecretString,
    ) -> Result<Value, Problem> {
        let url = format!("{}{path}", self.base);
        let mut request = self
            .http
            .request(method.clone(), &url)
            .bearer_auth(token.expose())
            .header(reqwest::header::ACCEPT, "application/json");
        if proof == Proof::Device {
            let key = self.device_key()?;
            let now = chrono::Utc::now().timestamp();
            request = request.header(
                "dpop",
                dpop(&key, method.as_str(), &url, token.expose(), now),
            );
        }
        if let Some(body) = body {
            request = request.json(body);
        }
        let response = request.send().await.map_err(|_| Problem::Network)?;
        let status = response.status();
        let bytes = crate::launcher_account::body_within(response, MAX_RESPONSE_BYTES)
            .await
            .map_err(|e| match e {
                crate::launcher_account::BodyError::Network => Problem::Network,
                crate::launcher_account::BodyError::TooLarge => Problem::Server,
            })?;
        let value: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        if status.is_success() {
            return Ok(value);
        }
        if let Some(code) = value.get("error").and_then(Value::as_str) {
            return Err(Problem::from_code(code));
        }
        Err(match status.as_u16() {
            401 => Problem::SignedOut,
            429 => Problem::TooMany,
            404 => Problem::NotFound,
            _ => Problem::Server,
        })
    }

    /// Registers the device key with the token's account, once per run and
    /// account (a new sign-in may be another account).
    async fn register(&self, token: &SecretString) -> Result<(), Problem> {
        let marker = account_of(token.expose()).ok_or(Problem::SignedOut)?;
        if lock(&self.registered)
            .as_ref()
            .is_some_and(|(account, _)| *account == marker)
        {
            return Ok(());
        }
        let body = json!({ "name": self.device_name });
        let mut answer = self
            .send(
                reqwest::Method::POST,
                "/v1/devices",
                Some(&body),
                Proof::Device,
                token,
            )
            .await;
        if answer == Err(Problem::Device) {
            // Revoked, or the key belongs to another account: a new key.
            self.new_device_key()?;
            answer = self
                .send(
                    reqwest::Method::POST,
                    "/v1/devices",
                    Some(&body),
                    Proof::Device,
                    token,
                )
                .await;
        }
        let device: Device = serde_json::from_value(answer?).map_err(|_| Problem::Server)?;
        *lock(&self.registered) = Some((marker, device.id));
        Ok(())
    }

    async fn call<T: DeserializeOwned>(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<Value>,
        proof: Proof,
    ) -> Result<T, Problem> {
        let token = self.token.token().await?;
        if proof == Proof::Device {
            self.register(&token).await?;
        }
        let mut answer = self
            .send(method.clone(), path, body.as_ref(), proof, &token)
            .await;
        if proof == Proof::Device && answer == Err(Problem::Device) {
            *lock(&self.registered) = None;
            self.register(&token).await?;
            answer = self.send(method, path, body.as_ref(), proof, &token).await;
        }
        serde_json::from_value(answer?).map_err(|_| Problem::Server)
    }

    /// The account's computers, and which one is this.
    pub async fn devices(&self) -> Result<(Vec<Device>, Option<String>), Problem> {
        let token = self.token.token().await?;
        self.register(&token).await?;
        let current = lock(&self.registered).as_ref().map(|(_, id)| id.clone());
        let devices = self
            .call(reqwest::Method::GET, "/v1/devices", None, Proof::No)
            .await?;
        Ok((devices, current))
    }

    /// Signs another computer out of worlds: the worlds it hosts close.
    pub async fn revoke_device(&self, device: &str) -> Result<(), Problem> {
        let _: Value = self
            .call(
                reqwest::Method::DELETE,
                &format!("/v1/devices/{}", id(device)?),
                None,
                Proof::No,
            )
            .await?;
        Ok(())
    }

    pub async fn rooms(&self) -> Result<Vec<Room>, Problem> {
        self.call(reqwest::Method::GET, "/v1/rooms", None, Proof::No)
            .await
    }

    pub async fn room(&self, room: &str) -> Result<Room, Problem> {
        self.call(
            reqwest::Method::GET,
            &format!("/v1/rooms/{}", id(room)?),
            None,
            Proof::No,
        )
        .await
    }

    pub async fn create_room(
        &self,
        name: &str,
        description: &str,
        world: &str,
        minecraft: &str,
    ) -> Result<Room, Problem> {
        let body = json!({
            "name": name,
            "description": description,
            "world": world,
            "minecraft": minecraft,
        });
        self.call(reqwest::Method::POST, "/v1/rooms", Some(body), Proof::No)
            .await
    }

    pub async fn members(&self, room: &str) -> Result<Vec<Member>, Problem> {
        self.call(
            reqwest::Method::GET,
            &format!("/v1/rooms/{}/members", id(room)?),
            None,
            Proof::No,
        )
        .await
    }

    /// Removes, bans or unbans a member.
    pub async fn moderate(&self, room: &str, account: &str, action: &str) -> Result<(), Problem> {
        if !matches!(action, "remove" | "ban" | "unban") {
            return Err(Problem::Invalid);
        }
        let _: Value = self
            .call(
                reqwest::Method::POST,
                &format!("/v1/rooms/{}/members/{}/{action}", id(room)?, id(account)?),
                None,
                Proof::No,
            )
            .await?;
        Ok(())
    }

    pub async fn leave(&self, room: &str) -> Result<(), Problem> {
        let _: Value = self
            .call(
                reqwest::Method::POST,
                &format!("/v1/rooms/{}/leave", id(room)?),
                None,
                Proof::No,
            )
            .await?;
        Ok(())
    }

    pub async fn create_invite(
        &self,
        room: &str,
        hours: u32,
        uses: u32,
    ) -> Result<NewInvite, Problem> {
        self.call(
            reqwest::Method::POST,
            &format!("/v1/rooms/{}/invites", id(room)?),
            Some(json!({ "hours": hours, "uses": uses })),
            Proof::No,
        )
        .await
    }

    pub async fn preview(&self, code: &str) -> Result<Preview, Problem> {
        self.call(
            reqwest::Method::POST,
            "/v1/invites/preview",
            Some(json!({ "code": invite_code(code)? })),
            Proof::No,
        )
        .await
    }

    pub async fn redeem(&self, code: &str) -> Result<Room, Problem> {
        self.call(
            reqwest::Method::POST,
            "/v1/invites/redeem",
            Some(json!({ "code": invite_code(code)? })),
            Proof::No,
        )
        .await
    }

    pub async fn start_session(
        &self,
        room: &str,
        endpoint: &str,
        max_players: u32,
    ) -> Result<Session, Problem> {
        self.call(
            reqwest::Method::POST,
            &format!("/v1/rooms/{}/sessions", id(room)?),
            Some(json!({ "endpoint": endpoint, "maxPlayers": max_players })),
            Proof::Device,
        )
        .await
    }

    pub async fn renew(&self, session: &str) -> Result<Renewal, Problem> {
        self.call(
            reqwest::Method::POST,
            &format!("/v1/sessions/{}/renew", id(session)?),
            None,
            Proof::Device,
        )
        .await
    }

    pub async fn stop_session(&self, session: &str) -> Result<(), Problem> {
        let _: Value = self
            .call(
                reqwest::Method::POST,
                &format!("/v1/sessions/{}/stop", id(session)?),
                None,
                Proof::Device,
            )
            .await?;
        Ok(())
    }

    /// A ticket for one game connection of this computer's tunnel `endpoint`.
    pub async fn ticket(&self, room: &str, endpoint: &str) -> Result<TicketGrant, Problem> {
        self.call(
            reqwest::Method::POST,
            &format!("/v1/rooms/{}/tickets", id(room)?),
            Some(json!({ "endpoint": endpoint })),
            Proof::Device,
        )
        .await
    }

    /// The host consumes a guest's ticket; `peer` is the guest's tunnel key.
    pub async fn admit(
        &self,
        session: &str,
        ticket: &str,
        peer: &str,
    ) -> Result<Admission, Problem> {
        self.call(
            reqwest::Method::POST,
            &format!("/v1/sessions/{}/admissions", id(session)?),
            Some(json!({ "ticket": ticket, "peer": peer })),
            Proof::Device,
        )
        .await
    }

    pub async fn release(&self, session: &str, admission: &str) -> Result<(), Problem> {
        let _: Value = self
            .call(
                reqwest::Method::POST,
                &format!(
                    "/v1/sessions/{}/admissions/{}/release",
                    id(session)?,
                    id(admission)?
                ),
                None,
                Proof::Device,
            )
            .await?;
        Ok(())
    }

    /// Asks the relay to carry this guest's tunnel key; returns seconds left.
    pub async fn enroll(&self, room: &str, endpoint: &str) -> Result<i64, Problem> {
        let enrolled: Enrolled = self
            .call(
                reqwest::Method::POST,
                &format!("/v1/rooms/{}/relay", id(room)?),
                Some(json!({ "endpoint": endpoint })),
                Proof::Device,
            )
            .await?;
        Ok(enrolled.ttl)
    }
}

/// The account a token was issued to (`sub`), read without checking it: the
/// service checks the token, this only tells accounts apart.
fn account_of(token: &str) -> Option<String> {
    let payload = token.split('.').nth(1)?;
    let claims: Value = serde_json::from_slice(&B64.decode(payload).ok()?).ok()?;
    let sub = claims.get("sub")?.as_str()?;
    (!sub.is_empty() && sub.len() <= 128).then(|| sub.to_owned())
}

/// An ID that goes into a path: a canonical UUID, nothing else.
fn id(text: &str) -> Result<&str, Problem> {
    let canonical = uuid::Uuid::parse_str(text)
        .ok()
        .is_some_and(|u| u.hyphenated().to_string() == text);
    canonical.then_some(text).ok_or(Problem::Invalid)
}

/// An invite code as the service makes them: 22 base64url characters. A
/// pasted link or code with spaces around it is accepted.
pub fn invite_code(text: &str) -> Result<String, Problem> {
    let code = text.trim().rsplit(['/', '#', '=']).next().unwrap_or("");
    let shaped = code.len() == 22
        && code
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
    shaped
        .then(|| code.to_owned())
        .ok_or(Problem::InviteInvalid)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::vault::MemoryStore;
    use crate::worlds::fake_service::{self, ROOM};

    struct Fixed(String);

    impl Token for Fixed {
        fn token(&self) -> BoxFuture<'_, Result<SecretString, Problem>> {
            let token = self.0.clone();
            Box::pin(async move { Ok(SecretString::new(token)) })
        }
    }

    const OWNER: &str = "11111111-1111-4111-8111-111111111111";
    const OTHER: &str = "22222222-2222-4222-8222-222222222222";

    fn client(base: &str, vault: Arc<MemoryStore>, account: &str) -> WorldsClient {
        WorldsClient::new(
            "test",
            base,
            vault,
            Arc::new(Fixed(fake_service::token(account))),
            "Desk\u{7}top computer with a very long name that goes on and on",
        )
        .unwrap()
    }

    #[test]
    fn proofs_carry_the_key_the_request_and_the_token() {
        let key = SecretKey::from_bytes(&[9; 32]);
        let proof = dpop(&key, "POST", "https://worlds.lapi.sh/v1/x", "tok", 1000);
        let parts: Vec<&str> = proof.split('.').collect();
        assert_eq!(parts.len(), 3);
        let header: Value = serde_json::from_slice(&B64.decode(parts[0]).unwrap()).unwrap();
        let payload: Value = serde_json::from_slice(&B64.decode(parts[1]).unwrap()).unwrap();
        assert_eq!(header["typ"], "dpop+jwt");
        assert_eq!(header["alg"], "EdDSA");
        assert_eq!(header["jwk"]["x"], B64.encode(key.public().as_bytes()));
        assert_eq!(payload["htm"], "POST");
        assert_eq!(payload["htu"], "https://worlds.lapi.sh/v1/x");
        assert_eq!(payload["iat"], 1000);
        assert_eq!(payload["ath"], proof_hash("tok"));
        let signature: [u8; 64] = B64.decode(parts[2]).unwrap().try_into().unwrap();
        key.public()
            .verify(
                format!("{}.{}", parts[0], parts[1]).as_bytes(),
                &iroh::Signature::from_bytes(&signature),
            )
            .unwrap();
        let again = dpop(&key, "POST", "https://worlds.lapi.sh/v1/x", "tok", 1000);
        assert_ne!(proof, again, "a fresh jti each time");
    }

    #[test]
    fn paths_take_only_canonical_ids_and_codes_have_their_shape() {
        assert!(id(ROOM).is_ok());
        for junk in [
            "",
            "../v1/relay/allow",
            "6F1D3B8E-2C4A-4F7E-9B1A-0C2D3E4F5A6B",
            "x/y",
        ] {
            assert_eq!(id(junk), Err(Problem::Invalid), "{junk}");
        }
        let code = "AbCdEfGhIjKlMnOpQrSt_-";
        assert_eq!(invite_code(code).unwrap(), code);
        assert_eq!(invite_code(&format!("  {code}\n")).unwrap(), code);
        assert_eq!(
            invite_code(&format!("https://lapi.sh/join#{code}")).unwrap(),
            code
        );
        for junk in ["", "short", &format!("{code}x"), "AbCdEfGhIjKlMnOpQrSt%2"] {
            assert_eq!(invite_code(junk), Err(Problem::InviteInvalid), "{junk}");
        }
        assert_eq!(
            account_of(&fake_service::token(OWNER)).as_deref(),
            Some(OWNER)
        );
        assert_eq!(account_of("not a token"), None);
        assert_eq!(Problem::from_code("superseded"), Problem::SessionEnded);
        assert_eq!(Problem::from_code("something new"), Problem::Refused);
        assert_eq!(Problem::NotMember.name(), "not_member");
    }

    #[tokio::test]
    async fn the_device_is_registered_once_and_a_taken_key_is_replaced() {
        let fake = fake_service::start(&[(OWNER, "Owner", true), (OTHER, "Other", false)]).await;
        let vault = Arc::new(MemoryStore::default());
        let owner = client(&fake.base, Arc::clone(&vault), OWNER);
        let endpoint = "ab".repeat(32);
        owner.enroll(ROOM, &endpoint).await.unwrap();
        owner.enroll(ROOM, &endpoint).await.unwrap();
        let registrations = |fake: &fake_service::Fake| {
            lock(&fake.world)
                .calls
                .iter()
                .filter(|c| *c == "POST /v1/devices")
                .count()
        };
        assert_eq!(registrations(&fake), 1, "once per run and account");
        let kept = vault.get(DEVICE_KEY).unwrap().unwrap().expose().to_owned();

        // Another account on the same computer finds the stored key taken:
        // it makes a key of its own and registers that.
        let other = client(&fake.base, Arc::clone(&vault), OTHER);
        assert_eq!(other.enroll(ROOM, &endpoint).await.unwrap(), 30);
        assert_eq!(registrations(&fake), 3);
        let replaced = vault.get(DEVICE_KEY).unwrap().unwrap().expose().to_owned();
        assert_ne!(kept, replaced);
        assert_eq!(lock(&fake.world).devices.len(), 2);

        // The registered device's proofs are accepted; refusals come back
        // by name.
        assert_eq!(
            other.start_session(ROOM, &endpoint, 8).await.unwrap_err(),
            Problem::Forbidden
        );
        assert_eq!(
            other
                .room("7f1d3b8e-2c4a-4f7e-9b1a-0c2d3e4f5a6b")
                .await
                .unwrap_err(),
            Problem::NotFound
        );
        assert_eq!(other.room("../relay").await.unwrap_err(), Problem::Invalid);
    }

    #[tokio::test]
    async fn the_device_list_marks_this_computer_and_revokes_others() {
        let fake = fake_service::start(&[(OWNER, "Owner", true)]).await;
        let laptop = client(&fake.base, Arc::new(MemoryStore::default()), OWNER);
        let desktop = client(&fake.base, Arc::new(MemoryStore::default()), OWNER);
        let (_, laptop_id) = laptop.devices().await.unwrap();
        let (devices, current) = desktop.devices().await.unwrap();
        assert_eq!(devices.len(), 2);
        let current = current.unwrap();
        assert_ne!(Some(&current), laptop_id.as_ref());
        assert!(devices.iter().any(|d| d.id == current));

        desktop
            .revoke_device(laptop_id.as_deref().unwrap())
            .await
            .unwrap();
        let (devices, _) = desktop.devices().await.unwrap();
        assert_eq!(devices.len(), 1);
        assert_eq!(
            desktop.revoke_device("../rooms").await.unwrap_err(),
            Problem::Invalid
        );
        assert_eq!(
            desktop
                .revoke_device("7f1d3b8e-2c4a-4f7e-9b1a-0c2d3e4f5a6b")
                .await
                .unwrap_err(),
            Problem::NotFound
        );
    }
}
