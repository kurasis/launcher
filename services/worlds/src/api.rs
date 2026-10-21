//! The worlds API (ADAPTATION A4). Every call carries an oauth.lapi.sh access
//! token for the audience `worlds`; the account is its `sub` and the player
//! name its `nick`, never anything the body says. Calls a device makes for
//! itself (hosting, tickets, admissions) also carry a DPoP proof of that
//! device's key.

use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::body::Bytes;
use axum::extract::ws::{Message as WsMessage, WebSocket, WebSocketUpgrade};
use axum::extract::{DefaultBodyLimit, Path, State};
use axum::http::{HeaderMap, Method, StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64;
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::auth::{Claims, Replays, Sessions, Tokens, verify_proof};
use crate::hub::{Event, Hub};
use crate::store::{self, Moderation, Role, Store};
use crate::text;
use crate::tickets::{self, Ticket, TicketKey};

/// Invite lifetimes in hours and uses (spec 04: a day and one use by
/// default, a week at most).
pub const INVITE_HOURS: (i64, i64, i64) = (1, 24, 168);
pub const INVITE_USES: (i64, i64, i64) = (1, 1, 10);
pub const MAX_PLAYERS: i64 = 20;
/// An invite code: 16 random bytes in unpadded base64url.
const CODE_LENGTH: usize = 22;

/// Unix seconds, moved forward by tests.
#[derive(Default)]
pub struct Clock {
    offset: AtomicI64,
}

impl Clock {
    pub fn now(&self) -> i64 {
        chrono::Utc::now().timestamp() + self.offset.load(Ordering::Relaxed)
    }

    #[cfg(test)]
    pub fn advance(&self, seconds: i64) {
        self.offset.fetch_add(seconds, Ordering::Relaxed);
    }
}

/// Actions counted since the start of their window, per account and kind.
type Windows = HashMap<(String, &'static str), (Instant, u32)>;

#[derive(Default)]
pub struct Limiter {
    windows: Mutex<Windows>,
}

impl Limiter {
    pub fn allow(&self, account: &str, bucket: &'static str, limit: u32, window: Duration) -> bool {
        let mut windows = self
            .windows
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let now = Instant::now();
        if windows.len() > 200_000 {
            windows.retain(|_, (start, _)| now.duration_since(*start) < Duration::from_secs(3600));
        }
        let entry = windows
            .entry((account.to_owned(), bucket))
            .or_insert((now, 0));
        if now.duration_since(entry.0) >= window {
            *entry = (now, 0);
        }
        entry.1 = entry.1.saturating_add(1);
        entry.1 <= limit
    }
}

pub struct App {
    pub store: Store,
    pub tokens: Tokens,
    /// Sign-outs and deleted accounts, from the account service.
    pub sessions: Sessions,
    pub tickets: TicketKey,
    pub replays: Replays,
    pub hub: Hub,
    pub limiter: Limiter,
    pub clock: Clock,
    /// `https://worlds.lapi.sh`: the tickets' issuer and the origin DPoP
    /// proofs name.
    pub base: String,
    /// The relay's bearer token for the allow list; without one the list is
    /// not served.
    pub relay_token: Option<String>,
}

fn status_of(code: &str) -> StatusCode {
    match code {
        "unauthorized" | "invalid_proof" => StatusCode::UNAUTHORIZED,
        "forbidden" | "banned" | "nick_unfit" | "device_unknown" | "ticket_invalid"
        | "not_member" => StatusCode::FORBIDDEN,
        "not_found" | "invite_invalid" => StatusCode::NOT_FOUND,
        "bad_request" => StatusCode::BAD_REQUEST,
        "too_many" => StatusCode::TOO_MANY_REQUESTS,
        "server_error" => StatusCode::INTERNAL_SERVER_ERROR,
        _ => StatusCode::CONFLICT,
    }
}

fn refuse(code: &str) -> Response {
    (status_of(code), Json(json!({ "error": code }))).into_response()
}

fn failed(e: &str) -> Response {
    // Database text names no player: IDs and SQL only.
    eprintln!("worlds: {e}");
    refuse("server_error")
}

fn answer<T: serde::Serialize>(result: store::Result<T>) -> Response {
    match result {
        Ok(value) => Json(value).into_response(),
        Err(store::Error::No(code)) => refuse(code),
        Err(store::Error::Db(e)) => failed(&e),
    }
}

/// A JSON body of exactly this shape.
fn parse<T: DeserializeOwned>(body: &Bytes) -> Result<T, Box<Response>> {
    serde_json::from_slice(body).map_err(|_| Box::new(refuse("bad_request")))
}

/// The caller and the token they came with.
pub struct Caller {
    pub claims: Claims,
    token: String,
}

fn who(app: &App, headers: &HeaderMap) -> Result<Caller, Box<Response>> {
    let token = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::trim)
        .ok_or_else(|| Box::new(refuse("unauthorized")))?;
    let claims = app
        .tokens
        .verify(token, app.clock.now())
        .ok_or_else(|| Box::new(refuse("unauthorized")))?;
    // Signed out, switched off or deleted.
    if !app.sessions.admit(&claims.sid, &claims.sub) {
        return Err(Box::new(refuse("unauthorized")));
    }
    // The nick is the player's name in the world; one the game would refuse
    // cannot be let in.
    if !text::nick(&claims.nick) {
        return Err(Box::new(refuse("nick_unfit")));
    }
    if let Err(e) = app.store.note_nick(&claims.sub, &claims.nick) {
        return Err(Box::new(answer::<()>(Err(e))));
    }
    Ok(Caller {
        claims,
        token: token.to_owned(),
    })
}

/// The device key a DPoP proof for this request was made with.
fn proof_key(
    app: &App,
    headers: &HeaderMap,
    caller: &Caller,
    method: &Method,
    uri: &Uri,
) -> Result<String, Box<Response>> {
    let invalid = || Box::new(refuse("invalid_proof"));
    let proof = headers
        .get("dpop")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(invalid)?;
    let url = format!("{}{}", app.base, uri.path());
    verify_proof(
        proof,
        method.as_str(),
        &url,
        &caller.token,
        app.clock.now(),
        &app.replays,
    )
    .ok_or_else(invalid)
}

/// The caller's registered device that made this request.
fn device_of(
    app: &App,
    headers: &HeaderMap,
    caller: &Caller,
    method: &Method,
    uri: &Uri,
) -> Result<String, Box<Response>> {
    let key = proof_key(app, headers, caller, method, uri)?;
    match app.store.device_of(&caller.claims.sub, &key) {
        Ok(Some(device)) => Ok(device),
        Ok(None) => Err(Box::new(refuse("device_unknown"))),
        Err(e) => Err(Box::new(answer::<()>(Err(e)))),
    }
}

macro_rules! me {
    ($app:expr, $headers:expr) => {
        match who(&$app, &$headers) {
            Ok(caller) => caller,
            Err(response) => return *response,
        }
    };
}

macro_rules! device {
    ($app:expr, $headers:expr, $me:expr, $method:expr, $uri:expr) => {
        match device_of(&$app, &$headers, &$me, &$method, &$uri) {
            Ok(device) => device,
            Err(response) => return *response,
        }
    };
}

macro_rules! body {
    ($bytes:expr) => {
        match parse(&$bytes) {
            Ok(value) => value,
            Err(response) => return *response,
        }
    };
}

macro_rules! ids {
    ($($id:expr),+) => {
        if !($(text::id(&$id))&&+) {
            return refuse("not_found");
        }
    };
}

macro_rules! limit {
    ($app:expr, $me:expr, $bucket:literal, $count:expr, $seconds:expr) => {
        if !$app.limiter.allow(
            &$me.claims.sub,
            $bucket,
            $count,
            Duration::from_secs($seconds),
        ) {
            return refuse("too_many");
        }
    };
}

/// Tells everyone in a room that it changed.
fn changed(app: &App, room: &str) {
    if let Ok(accounts) = app.store.members_of(room) {
        app.hub.tell_all(
            &accounts,
            &Event::Room {
                id: room.to_owned(),
            },
        );
    }
}

async fn health(State(app): State<Arc<App>>) -> Json<Value> {
    Json(json!({
        "ok": true,
        "service": "lapi-worlds",
        "version": env!("CARGO_PKG_VERSION"),
        "keys": app.tokens.has_keys(),
    }))
}

async fn jwks(State(app): State<Arc<App>>) -> Json<Value> {
    Json(app.tickets.jwks())
}

// Devices.

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NewDevice {
    name: String,
}

async fn register_device(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    method: Method,
    uri: Uri,
    bytes: Bytes,
) -> Response {
    let me = me!(app, headers);
    let key = match proof_key(&app, &headers, &me, &method, &uri) {
        Ok(key) => key,
        Err(response) => return *response,
    };
    let request: NewDevice = body!(bytes);
    let Some(name) = text::line(&request.name, text::DEVICE_NAME_MAX) else {
        return refuse("bad_request");
    };
    limit!(app, me, "device", 20, 86_400);
    answer(
        app.store
            .register_device(&me.claims.sub, &key, &name, app.clock.now()),
    )
}

async fn devices(State(app): State<Arc<App>>, headers: HeaderMap) -> Response {
    let me = me!(app, headers);
    answer(app.store.devices(&me.claims.sub))
}

async fn revoke_device(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Path(device): Path<String>,
) -> Response {
    let me = me!(app, headers);
    ids!(device);
    match app
        .store
        .revoke_device(&me.claims.sub, &device, app.clock.now())
    {
        Ok(rooms) => {
            for room in &rooms {
                changed(&app, room);
            }
            Json(json!({ "ok": true })).into_response()
        }
        Err(e) => answer::<()>(Err(e)),
    }
}

// Rooms.

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NewRoom {
    name: String,
    #[serde(default)]
    description: String,
    world: String,
    minecraft: String,
}

async fn create_room(State(app): State<Arc<App>>, headers: HeaderMap, bytes: Bytes) -> Response {
    let me = me!(app, headers);
    let request: NewRoom = body!(bytes);
    let (Some(name), Some(description)) = (
        text::line(&request.name, text::NAME_MAX),
        text::description(&request.description),
    ) else {
        return refuse("bad_request");
    };
    if !text::id(&request.world) || !text::minecraft(&request.minecraft) {
        return refuse("bad_request");
    }
    limit!(app, me, "room", 10, 3600);
    let made = app.store.create_room(
        &me.claims.sub,
        &me.claims.nick,
        &store::RoomDraft {
            name: &name,
            description: &description,
            world: &request.world,
            minecraft: &request.minecraft,
        },
        app.clock.now(),
    );
    if made.is_ok() {
        app.hub.tell(&me.claims.sub, &Event::Rooms);
    }
    answer(made)
}

async fn rooms(State(app): State<Arc<App>>, headers: HeaderMap) -> Response {
    let me = me!(app, headers);
    answer(app.store.rooms(&me.claims.sub, app.clock.now()))
}

async fn room(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Path(room): Path<String>,
) -> Response {
    let me = me!(app, headers);
    ids!(room);
    answer(app.store.room(&me.claims.sub, &room, app.clock.now()))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RoomChange {
    revision: i64,
    name: Option<String>,
    description: Option<String>,
}

async fn update_room(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Path(room): Path<String>,
    bytes: Bytes,
) -> Response {
    let me = me!(app, headers);
    ids!(room);
    let request: RoomChange = body!(bytes);
    let name = match request
        .name
        .as_deref()
        .map(|n| text::line(n, text::NAME_MAX))
    {
        Some(None) => return refuse("bad_request"),
        other => other.flatten(),
    };
    let description = match request.description.as_deref().map(text::description) {
        Some(None) => return refuse("bad_request"),
        other => other.flatten(),
    };
    let updated = app.store.update_room(
        &me.claims.sub,
        &room,
        request.revision,
        name.as_deref(),
        description.as_deref(),
        app.clock.now(),
    );
    if updated.is_ok() {
        changed(&app, &room);
    }
    answer(updated)
}

async fn delete_room(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Path(room): Path<String>,
) -> Response {
    let me = me!(app, headers);
    ids!(room);
    match app
        .store
        .delete_room(&me.claims.sub, &room, app.clock.now())
    {
        Ok(accounts) => {
            app.hub.tell_all(&accounts, &Event::Rooms);
            Json(json!({ "ok": true })).into_response()
        }
        Err(e) => answer::<()>(Err(e)),
    }
}

// Members.

async fn members(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Path(room): Path<String>,
) -> Response {
    let me = me!(app, headers);
    ids!(room);
    answer(app.store.members(&me.claims.sub, &room))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RoleChange {
    role: String,
}

async fn act_on_member(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Path((room, account, action)): Path<(String, String, String)>,
    bytes: Bytes,
) -> Response {
    let me = me!(app, headers);
    ids!(room, account);
    let now = app.clock.now();
    let done = match action.as_str() {
        "remove" => app
            .store
            .moderate(&me.claims.sub, &room, &account, Moderation::Remove, now),
        "ban" => app
            .store
            .moderate(&me.claims.sub, &room, &account, Moderation::Ban, now),
        "unban" => app
            .store
            .moderate(&me.claims.sub, &room, &account, Moderation::Unban, now),
        "role" => {
            let request: RoleChange = body!(bytes);
            let Some(role) = Role::parse(&request.role) else {
                return refuse("bad_request");
            };
            app.store
                .set_role(&me.claims.sub, &room, &account, role, now)
        }
        _ => return refuse("not_found"),
    };
    if done.is_ok() {
        app.hub.tell(&account, &Event::Rooms);
        changed(&app, &room);
    }
    answer(done.map(|()| json!({ "ok": true })))
}

async fn leave(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Path(room): Path<String>,
) -> Response {
    let me = me!(app, headers);
    ids!(room);
    let left = app.store.leave(&me.claims.sub, &room, app.clock.now());
    if left.is_ok() {
        app.hub.tell(&me.claims.sub, &Event::Rooms);
        changed(&app, &room);
    }
    answer(left.map(|()| json!({ "ok": true })))
}

// Invites.

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NewInvite {
    hours: Option<i64>,
    uses: Option<i64>,
}

fn digest(code: &str) -> String {
    tickets::hex(&Sha256::digest(code.as_bytes()))
}

fn bounded(value: Option<i64>, (min, default, max): (i64, i64, i64)) -> Option<i64> {
    let value = value.unwrap_or(default);
    (min..=max).contains(&value).then_some(value)
}

async fn create_invite(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Path(room): Path<String>,
    bytes: Bytes,
) -> Response {
    let me = me!(app, headers);
    ids!(room);
    let request: NewInvite = body!(bytes);
    let (Some(hours), Some(uses)) = (
        bounded(request.hours, INVITE_HOURS),
        bounded(request.uses, INVITE_USES),
    ) else {
        return refuse("bad_request");
    };
    limit!(app, me, "invite", 30, 3600);
    let mut bytes = [0u8; 16];
    if getrandom::fill(&mut bytes).is_err() {
        return failed("no random bytes");
    }
    let code = B64.encode(bytes);
    match app.store.create_invite(
        &me.claims.sub,
        &room,
        &digest(&code),
        hours,
        uses,
        app.clock.now(),
    ) {
        Ok(invite) => Json(json!({ "invite": invite, "code": code })).into_response(),
        Err(e) => answer::<()>(Err(e)),
    }
}

async fn invites(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Path(room): Path<String>,
) -> Response {
    let me = me!(app, headers);
    ids!(room);
    answer(app.store.invites(&me.claims.sub, &room, app.clock.now()))
}

async fn revoke_invite(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Path((room, invite)): Path<(String, String)>,
) -> Response {
    let me = me!(app, headers);
    ids!(room, invite);
    answer(
        app.store
            .revoke_invite(&me.claims.sub, &room, &invite, app.clock.now())
            .map(|()| json!({ "ok": true })),
    )
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Code {
    code: String,
}

/// The digest of a code that has the right shape.
fn code_digest(bytes: &Bytes) -> Result<String, Box<Response>> {
    let request: Code = parse(bytes)?;
    let code = request.code.trim();
    let shaped = code.len() == CODE_LENGTH
        && code
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
    if !shaped {
        return Err(Box::new(refuse("invite_invalid")));
    }
    Ok(digest(code))
}

async fn preview_invite(State(app): State<Arc<App>>, headers: HeaderMap, bytes: Bytes) -> Response {
    let me = me!(app, headers);
    limit!(app, me, "invite_try", 30, 600);
    let digest = match code_digest(&bytes) {
        Ok(digest) => digest,
        Err(response) => return *response,
    };
    answer(app.store.preview(&digest, app.clock.now()))
}

async fn redeem_invite(State(app): State<Arc<App>>, headers: HeaderMap, bytes: Bytes) -> Response {
    let me = me!(app, headers);
    limit!(app, me, "invite_try", 30, 600);
    let digest = match code_digest(&bytes) {
        Ok(digest) => digest,
        Err(response) => return *response,
    };
    let now = app.clock.now();
    match app
        .store
        .redeem(&me.claims.sub, &me.claims.nick, &digest, now)
    {
        Ok(room) => {
            app.hub.tell(&me.claims.sub, &Event::Rooms);
            changed(&app, &room);
            answer(app.store.room(&me.claims.sub, &room, now))
        }
        Err(e) => answer::<()>(Err(e)),
    }
}

// Hosting.

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct NewSession {
    endpoint: String,
    max_players: i64,
    /// The world's room pack, for a world with mods.
    #[serde(default)]
    pack: Option<NewPack>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NewPack {
    version: String,
    sha256: String,
}

async fn start_session(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    method: Method,
    uri: Uri,
    Path(room): Path<String>,
    bytes: Bytes,
) -> Response {
    let me = me!(app, headers);
    ids!(room);
    let device = device!(app, headers, me, method, uri);
    let request: NewSession = body!(bytes);
    if !text::endpoint(&request.endpoint) || !(1..=MAX_PLAYERS).contains(&request.max_players) {
        return refuse("bad_request");
    }
    let pack = match request.pack {
        Some(p) if text::id(&p.version) && text::sha256(&p.sha256) => Some(store::Pack {
            version: p.version,
            sha256: p.sha256,
        }),
        Some(_) => return refuse("bad_request"),
        None => None,
    };
    limit!(app, me, "host", 60, 3600);
    let started = app.store.start_session(
        &me.claims.sub,
        &room,
        &device,
        &request.endpoint,
        request.max_players,
        pack.as_ref(),
        app.clock.now(),
    );
    if started.is_ok() {
        changed(&app, &room);
    }
    answer(started)
}

/// What the host tells with a renewal. Older launchers send no body.
#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct RenewBody {
    /// Players on the world server now, the host's own game included.
    players: Option<u32>,
}

/// More than any world server holds; a larger count is refused.
const MOST_PLAYERS: u32 = 1000;

async fn renew(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    method: Method,
    uri: Uri,
    Path(session): Path<String>,
    bytes: Bytes,
) -> Response {
    let me = me!(app, headers);
    ids!(session);
    let device = device!(app, headers, me, method, uri);
    let told: RenewBody = if bytes.is_empty() {
        RenewBody::default()
    } else {
        body!(bytes)
    };
    if told.players.is_some_and(|n| n > MOST_PLAYERS) {
        return refuse("bad_request");
    }
    answer(
        app.store
            .renew(&device, &session, told.players, app.clock.now()),
    )
}

async fn stop_session(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    method: Method,
    uri: Uri,
    Path(session): Path<String>,
) -> Response {
    let me = me!(app, headers);
    ids!(session);
    let device = device!(app, headers, me, method, uri);
    match app.store.stop_session(&device, &session, app.clock.now()) {
        Ok(room) => {
            changed(&app, &room);
            Json(json!({ "ok": true })).into_response()
        }
        Err(e) => answer::<()>(Err(e)),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NewTicket {
    endpoint: String,
}

async fn ticket(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    method: Method,
    uri: Uri,
    Path(room): Path<String>,
    bytes: Bytes,
) -> Response {
    let me = me!(app, headers);
    ids!(room);
    let device = device!(app, headers, me, method, uri);
    let request: NewTicket = body!(bytes);
    if !text::endpoint(&request.endpoint) {
        return refuse("bad_request");
    }
    limit!(app, me, "ticket", 30, 60);
    let now = app.clock.now();
    let jti = uuid::Uuid::new_v4().to_string();
    let expires = now + tickets::TTL;
    let session = match app
        .store
        .issue_ticket(&me.claims.sub, &room, &jti, expires, now)
    {
        Ok(session) => session,
        Err(e) => return answer::<()>(Err(e)),
    };
    let token = app.tickets.sign(&Ticket {
        iss: app.base.clone(),
        aud: tickets::AUDIENCE.to_owned(),
        jti,
        iat: now,
        nbf: now,
        exp: expires,
        sub: me.claims.sub.clone(),
        dev: device,
        epk: request.endpoint,
        room: room.clone(),
        ses: session.id.clone(),
        hep: session.epoch,
        aep: app.store.authority().to_owned(),
        svc: "gameplay".to_owned(),
    });
    Json(json!({
        "ticket": token,
        "session": session.id,
        "hostEpoch": session.epoch.to_string(),
        "hostEndpoint": session.endpoint,
        "expires": expires,
        "pack": session.pack,
    }))
    .into_response()
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Consume {
    ticket: String,
    peer: String,
}

async fn admit(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    method: Method,
    uri: Uri,
    Path(session): Path<String>,
    bytes: Bytes,
) -> Response {
    let me = me!(app, headers);
    ids!(session);
    let device = device!(app, headers, me, method, uri);
    let request: Consume = body!(bytes);
    if !text::endpoint(&request.peer) {
        return refuse("bad_request");
    }
    let now = app.clock.now();
    let Some(ticket) = app.tickets.verify(&request.ticket, &app.base, now) else {
        return refuse("ticket_invalid");
    };
    let admitted = app
        .store
        .admit(&device, &session, &ticket, &request.peer, now);
    if admitted.is_ok() {
        changed(&app, &ticket.room);
    }
    answer(admitted)
}

async fn release(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    method: Method,
    uri: Uri,
    Path((session, admission)): Path<(String, String)>,
) -> Response {
    let me = me!(app, headers);
    ids!(session, admission);
    let device = device!(app, headers, me, method, uri);
    let now = app.clock.now();
    match app.store.release(&device, &session, &admission, now) {
        Ok(room) => {
            changed(&app, &room);
            Json(json!({ "ok": true })).into_response()
        }
        Err(e) => answer::<()>(Err(e)),
    }
}

// The relay (relay.lapi.sh, its own server).

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Enrollment {
    endpoint: String,
}

/// A guest asks the relay to carry its tunnel key; it repeats this every 10
/// seconds while it plays.
async fn enroll(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    method: Method,
    uri: Uri,
    Path(room): Path<String>,
    bytes: Bytes,
) -> Response {
    let me = me!(app, headers);
    ids!(room);
    let device = device!(app, headers, me, method, uri);
    let request: Enrollment = body!(bytes);
    if !text::endpoint(&request.endpoint) {
        return refuse("bad_request");
    }
    limit!(app, me, "relay", 30, 60);
    let now = app.clock.now();
    answer(
        app.store
            .enroll(&me.claims.sub, &device, &room, &request.endpoint, now)
            .map(|until| json!({ "ttl": until - now })),
    )
}

/// The relay's group for an account: enough to count connections per
/// account, without telling the relay who it is.
fn relay_group(account: &str) -> String {
    tickets::hex(&Sha256::digest(format!("relay-group:{account}").as_bytes())[..12])
}

/// Every tunnel key the relay may carry, with seconds left. Asked by the
/// relay every 10 seconds.
async fn relay_allow(State(app): State<Arc<App>>, headers: HeaderMap) -> Response {
    let Some(expected) = app.relay_token.as_deref() else {
        return refuse("not_found");
    };
    let given = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or_default();
    // Compared as digests, so the time taken tells nothing about the token.
    if Sha256::digest(given.as_bytes()) != Sha256::digest(expected.as_bytes()) {
        return refuse("unauthorized");
    }
    let now = app.clock.now();
    match app.store.relay_allow(now) {
        Ok(grants) => Json(json!({
            "allow": grants
                .iter()
                .map(|g| json!({
                    "endpoint": g.endpoint,
                    "group": relay_group(&g.account),
                    "ttl": g.until - now,
                }))
                .collect::<Vec<_>>(),
        }))
        .into_response(),
        Err(e) => answer::<()>(Err(e)),
    }
}

// Live hints.

async fn live(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    upgrade: WebSocketUpgrade,
) -> Response {
    let me = me!(app, headers);
    let Some((id, events)) = app.hub.join(&me.claims.sub) else {
        return refuse("too_many");
    };
    let lasts =
        Duration::from_secs(u64::try_from((me.claims.exp - app.clock.now()).max(1)).unwrap_or(1));
    upgrade
        .max_message_size(4 * 1024)
        .on_upgrade(move |socket| async move {
            serve_socket(socket, events, lasts).await;
            app.hub.leave(&me.claims.sub, id);
        })
}

async fn serve_socket(
    socket: WebSocket,
    mut events: tokio::sync::mpsc::Receiver<Event>,
    lasts: Duration,
) {
    let (mut out, mut incoming) = socket.split();
    let end = tokio::time::sleep(lasts);
    tokio::pin!(end);
    let mut ping = tokio::time::interval(Duration::from_secs(25));
    loop {
        tokio::select! {
            () = &mut end => break,
            _ = ping.tick() => {
                if out.send(WsMessage::Ping(Vec::new().into())).await.is_err() {
                    break;
                }
            }
            event = events.recv() => {
                let Some(event) = event else { break };
                let text = serde_json::to_string(&event).unwrap_or_default();
                if out.send(WsMessage::Text(text.into())).await.is_err() {
                    break;
                }
            }
            frame = incoming.next() => {
                // The launcher sends nothing but pongs and the close.
                match frame {
                    Some(Ok(WsMessage::Pong(_) | WsMessage::Ping(_))) => {}
                    _ => break,
                }
            }
        }
    }
    let _ = out.send(WsMessage::Close(None)).await;
}

// The account service.

/// Asks the account service which recently seen sessions ended and refuses
/// their tokens from now on. When it cannot be asked, tokens hold until they
/// run out.
pub async fn check_sessions(app: &App, http: &reqwest::Client, accounts: &str) {
    let sessions = app.sessions.recent();
    for chunk in sessions.chunks(500) {
        match crate::auth::ended(http, accounts, chunk).await {
            Ok(ended) => app.sessions.end(&ended),
            Err(e) => {
                eprintln!("asking which sessions ended failed: {e}");
                return;
            }
        }
    }
}

/// Cleans out accounts the account service says were deleted after
/// `since`; returns the time to ask from next.
pub async fn purge_deleted(app: &App, http: &reqwest::Client, accounts: &str, since: i64) -> i64 {
    let deleted = match crate::auth::deleted(http, accounts, since).await {
        Ok(deleted) => deleted,
        Err(e) => {
            eprintln!("asking which accounts were deleted failed: {e}");
            return since;
        }
    };
    purge(app, &deleted, since)
}

/// Refuses the tokens of deleted accounts, then cleans them out.
pub fn purge(app: &App, deleted: &[(String, i64)], since: i64) -> i64 {
    let now = app.clock.now();
    let lately: Vec<String> = deleted
        .iter()
        .filter(|(_, at)| now - at < crate::auth::REMEMBER.as_secs().cast_signed())
        .map(|(account, _)| account.clone())
        .collect();
    app.sessions.forget(&lately);
    let mut newest = since;
    for (account, at) in deleted {
        match app.store.purge(account, now) {
            Ok(rooms) => {
                for (room, accounts) in rooms {
                    app.hub.tell_all(&accounts, &Event::Rooms);
                    changed(app, &room);
                }
            }
            Err(e) => {
                eprintln!("cleaning out a deleted account failed: {e:?}");
                return newest;
            }
        }
        newest = newest.max(*at);
    }
    newest
}

pub fn router(app: Arc<App>) -> Router {
    Router::new()
        .route("/v1/health", get(health))
        .route("/.well-known/jwks.json", get(jwks))
        .route("/v1/devices", get(devices).post(register_device))
        .route("/v1/devices/{device}", delete(revoke_device))
        .route("/v1/rooms", get(rooms).post(create_room))
        .route(
            "/v1/rooms/{room}",
            get(room).patch(update_room).delete(delete_room),
        )
        .route("/v1/rooms/{room}/members", get(members))
        .route(
            "/v1/rooms/{room}/members/{account}/{action}",
            post(act_on_member),
        )
        .route("/v1/rooms/{room}/leave", post(leave))
        .route("/v1/rooms/{room}/invites", get(invites).post(create_invite))
        .route("/v1/rooms/{room}/invites/{invite}", delete(revoke_invite))
        .route("/v1/invites/preview", post(preview_invite))
        .route("/v1/invites/redeem", post(redeem_invite))
        .route("/v1/rooms/{room}/sessions", post(start_session))
        .route("/v1/rooms/{room}/tickets", post(ticket))
        .route("/v1/rooms/{room}/relay", post(enroll))
        .route("/v1/relay/allow", get(relay_allow))
        .route("/v1/sessions/{session}/renew", post(renew))
        .route("/v1/sessions/{session}/stop", post(stop_session))
        .route("/v1/sessions/{session}/admissions", post(admit))
        .route(
            "/v1/sessions/{session}/admissions/{admission}/release",
            post(release),
        )
        .route("/v1/live", get(live))
        .layer(DefaultBodyLimit::max(16 * 1024))
        .with_state(app)
}
