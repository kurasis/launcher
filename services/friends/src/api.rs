//! The friends API (D-064). Every call carries the account service's access
//! token; the account is its `sub`, never anything the body says.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::extract::ws::{Message as WsMessage, WebSocket, WebSocketUpgrade};
use axum::extract::{DefaultBodyLimit, Path, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::auth::{Claims, Tokens};
use crate::directory::Directory;
use crate::hub::{Event, Hub};
use crate::store::{Message, Requested, Store};
use crate::text;

pub const MAX_FRIENDS: usize = 300;
pub const MAX_OUTGOING: usize = 100;
pub const MAX_GROUPS: i64 = 50;
/// People in a group, its owner included.
pub const MAX_MEMBERS: usize = 20;

/// Actions counted since the start of their window, per account and kind.
type Windows = HashMap<(String, &'static str), (Instant, u32)>;

/// Actions counted per account in a window.
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
    pub directory: Directory,
    pub hub: Hub,
    pub limiter: Limiter,
    /// Only accounts with a confirmed email may use friends.
    pub require_email: bool,
}

pub fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

fn refuse(status: StatusCode, code: &str) -> Response {
    (status, Json(json!({ "error": code }))).into_response()
}

fn failed(e: &str) -> Response {
    eprintln!("friends: {e}");
    refuse(StatusCode::INTERNAL_SERVER_ERROR, "server_error")
}

/// The account behind a request's bearer token.
fn who(app: &App, headers: &HeaderMap) -> Result<Claims, Box<Response>> {
    let unauthorized = || Box::new(refuse(StatusCode::UNAUTHORIZED, "unauthorized"));
    let token = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .ok_or_else(unauthorized)?;
    let claims = app
        .tokens
        .verify(token.trim(), now())
        .ok_or_else(unauthorized)?;
    if app.require_email && !claims.ev {
        return Err(Box::new(refuse(
            StatusCode::FORBIDDEN,
            "email_not_confirmed",
        )));
    }
    app.directory.note(&claims.sub, &claims.nick);
    Ok(claims)
}

macro_rules! me {
    ($app:expr, $headers:expr) => {
        match who(&$app, &$headers) {
            Ok(claims) => claims,
            Err(response) => return *response,
        }
    };
}

/// People as the launcher shows them: ID, current nick, a time.
async fn people(app: &App, list: Vec<(String, i64)>, key: &str) -> Result<Vec<Value>, String> {
    let ids: Vec<String> = list.iter().map(|(id, _)| id.clone()).collect();
    let nicks = app.directory.nicks(&ids).await?;
    Ok(list
        .into_iter()
        .filter_map(|(id, at)| {
            let nick = nicks.get(&id)?;
            Some(json!({ "id": id, "nick": nick, key: at }))
        })
        .collect())
}

async fn state(State(app): State<Arc<App>>, headers: HeaderMap) -> Response {
    let me = me!(app, headers);
    let lists = (|| -> Result<_, String> {
        Ok((
            app.store.friends(&me.sub)?,
            app.store.incoming(&me.sub)?,
            app.store.outgoing(&me.sub)?,
            app.store.blocked(&me.sub)?,
        ))
    })();
    let Ok((friends, incoming, outgoing, blocked)) = lists else {
        return failed("reading the lists");
    };
    let built = async {
        Ok::<_, String>(json!({
            "me": { "id": me.sub, "nick": me.nick },
            "friends": people(&app, friends, "since").await?,
            "incoming": people(&app, incoming, "at").await?,
            "outgoing": people(&app, outgoing, "at").await?,
            "blocked": people(&app, blocked, "at").await?,
        }))
    }
    .await;
    match built {
        Ok(body) => Json(body).into_response(),
        Err(e) => failed(&e),
    }
}

#[derive(Deserialize)]
struct NickBody {
    nick: String,
}

/// A friend request by exact nick.
async fn request(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Json(body): Json<NickBody>,
) -> Response {
    let me = me!(app, headers);
    let Some(nick) = text::nick(&body.nick) else {
        return refuse(StatusCode::UNPROCESSABLE_ENTITY, "nick_invalid");
    };
    if !app
        .limiter
        .allow(&me.sub, "request", 20, Duration::from_secs(3600))
    {
        return refuse(StatusCode::TOO_MANY_REQUESTS, "too_many");
    }
    let target = match app.directory.find(nick).await {
        Ok(Some((id, _))) => id,
        Ok(None) => return refuse(StatusCode::NOT_FOUND, "no_such_nick"),
        Err(e) => return failed(&e),
    };
    if target == me.sub {
        return refuse(StatusCode::UNPROCESSABLE_ENTITY, "yourself");
    }
    match app.store.blocks(&me.sub, &target) {
        Ok(true) => return refuse(StatusCode::CONFLICT, "you_blocked"),
        Ok(false) => {}
        Err(e) => return failed(&e),
    }
    // Someone who blocked us sees nothing; we see a request that waits.
    if app.store.blocks(&target, &me.sub).unwrap_or(true) {
        return Json(json!({ "result": "sent" })).into_response();
    }
    let counts = (
        app.store.friends(&me.sub).map(|l| l.len()),
        app.store.outgoing(&me.sub).map(|l| l.len()),
        app.store.friends(&target).map(|l| l.len()),
    );
    match counts {
        (Ok(mine), Ok(out), Ok(theirs)) => {
            if mine >= MAX_FRIENDS || out >= MAX_OUTGOING || theirs >= MAX_FRIENDS {
                return refuse(StatusCode::TOO_MANY_REQUESTS, "limit");
            }
        }
        _ => return failed("counting friends"),
    }
    match app.store.request(&me.sub, &target, now()) {
        Ok(result) => {
            if matches!(result, Requested::Sent | Requested::Friends) {
                app.hub.tell(&target, &Event::Friends);
                app.hub.tell(&me.sub, &Event::Friends);
            }
            let result = match result {
                Requested::Sent | Requested::AlreadySent => "sent",
                Requested::Friends | Requested::AlreadyFriends => "friends",
            };
            Json(json!({ "result": result })).into_response()
        }
        Err(e) => failed(&e),
    }
}

#[derive(Deserialize)]
struct IdBody {
    id: String,
}

fn valid_id(body: &IdBody) -> Option<&str> {
    text::account_id(&body.id).then_some(body.id.as_str())
}

/// accept, decline (theirs), cancel (ours), remove (a friend), block, unblock.
async fn act(
    State(app): State<Arc<App>>,
    Path(action): Path<String>,
    headers: HeaderMap,
    Json(body): Json<IdBody>,
) -> Response {
    let me = me!(app, headers);
    let Some(other) = valid_id(&body) else {
        return refuse(StatusCode::UNPROCESSABLE_ENTITY, "id");
    };
    if other == me.sub {
        return refuse(StatusCode::UNPROCESSABLE_ENTITY, "yourself");
    }
    if !app
        .limiter
        .allow(&me.sub, "act", 120, Duration::from_secs(3600))
    {
        return refuse(StatusCode::TOO_MANY_REQUESTS, "too_many");
    }
    let now = now();
    let (result, tell_other) = match action.as_str() {
        "accept" => (app.store.accept(&me.sub, other, now), true),
        // Declining is not told: the request just stops waiting for them.
        "decline" => (app.store.drop_request(other, &me.sub), false),
        "cancel" => (app.store.drop_request(&me.sub, other), true),
        "remove" => (app.store.unfriend(&me.sub, other), true),
        // Blocking is not told either.
        "block" => (app.store.block(&me.sub, other, now).map(|()| true), false),
        "unblock" => (app.store.unblock(&me.sub, other), false),
        _ => return refuse(StatusCode::NOT_FOUND, "not_found"),
    };
    match result {
        Ok(true) => {
            app.hub.tell(&me.sub, &Event::Friends);
            if tell_other {
                app.hub.tell(other, &Event::Friends);
            }
            StatusCode::NO_CONTENT.into_response()
        }
        Ok(false) => refuse(StatusCode::NOT_FOUND, "not_found"),
        Err(e) => failed(&e),
    }
}

// Rooms and messages.

fn message_json(message: &Message) -> Value {
    serde_json::to_value(message).unwrap_or(Value::Null)
}

async fn rooms(State(app): State<Arc<App>>, headers: HeaderMap) -> Response {
    let me = me!(app, headers);
    let list = match app.store.rooms_of(&me.sub) {
        Ok(list) => list,
        Err(e) => return failed(&e),
    };
    let mut ids: Vec<String> = Vec::new();
    let mut members: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (room, ..) in &list {
        match app.store.members(&room.id) {
            Ok(m) => {
                ids.extend(m.iter().cloned());
                members.insert(room.id.clone(), m);
            }
            Err(e) => return failed(&e),
        }
    }
    ids.sort();
    ids.dedup();
    let nicks = match app.directory.nicks(&ids).await {
        Ok(nicks) => nicks,
        Err(e) => return failed(&e),
    };
    let rooms: Vec<Value> = list
        .iter()
        .map(|(room, last, unread)| {
            let people: Vec<Value> = members[&room.id]
                .iter()
                .filter_map(|id| Some(json!({ "id": id, "nick": nicks.get(id)? })))
                .collect();
            json!({
                "id": room.id,
                "kind": room.kind,
                "name": room.name,
                "owner": room.owner,
                "members": people,
                "last": last.as_ref().map(message_json),
                "unread": unread,
            })
        })
        .collect();
    Json(json!({ "rooms": rooms })).into_response()
}

/// Opens the room of two friends.
async fn direct(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Json(body): Json<IdBody>,
) -> Response {
    let me = me!(app, headers);
    let Some(friend) = valid_id(&body) else {
        return refuse(StatusCode::UNPROCESSABLE_ENTITY, "id");
    };
    match app.store.are_friends(&me.sub, friend) {
        Ok(true) => {}
        Ok(false) => return refuse(StatusCode::FORBIDDEN, "not_friends"),
        Err(e) => return failed(&e),
    }
    match app.store.direct_room(&me.sub, friend, now()) {
        Ok(room) => Json(json!({ "room": room })).into_response(),
        Err(e) => failed(&e),
    }
}

#[derive(Deserialize)]
struct GroupBody {
    name: String,
    members: Vec<String>,
}

/// Makes a room of the owner and 2 or more friends.
async fn group(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Json(body): Json<GroupBody>,
) -> Response {
    let me = me!(app, headers);
    let Some(name) = text::line(&body.name, text::NAME_MAX) else {
        return refuse(StatusCode::UNPROCESSABLE_ENTITY, "name");
    };
    let mut others: Vec<String> = body
        .members
        .iter()
        .filter(|id| **id != me.sub)
        .cloned()
        .collect();
    others.sort();
    others.dedup();
    if others.len() < 2 || others.len() + 1 > MAX_MEMBERS {
        return refuse(StatusCode::UNPROCESSABLE_ENTITY, "members");
    }
    for other in &others {
        if !text::account_id(other) || !app.store.are_friends(&me.sub, other).unwrap_or(false) {
            return refuse(StatusCode::FORBIDDEN, "not_friends");
        }
    }
    if app.store.owned_groups(&me.sub).unwrap_or(i64::MAX) >= MAX_GROUPS
        || !app
            .limiter
            .allow(&me.sub, "group", 10, Duration::from_secs(3600))
    {
        return refuse(StatusCode::TOO_MANY_REQUESTS, "limit");
    }
    match app.store.create_group(&me.sub, &name, &others, now()) {
        Ok(room) => {
            for member in std::iter::once(&me.sub).chain(others.iter()) {
                app.hub.tell(member, &Event::Rooms);
            }
            Json(json!({ "room": room })).into_response()
        }
        Err(e) => failed(&e),
    }
}

/// The room, if `me` is in it.
fn room_of(app: &App, room: &str, me: &str) -> Result<crate::store::Room, Box<Response>> {
    if !text::account_id(room) {
        return Err(Box::new(refuse(StatusCode::NOT_FOUND, "not_found")));
    }
    match (app.store.room(room), app.store.is_member(room, me)) {
        (Ok(Some(found)), Ok(true)) => Ok(found),
        (Ok(_), Ok(_)) => Err(Box::new(refuse(StatusCode::NOT_FOUND, "not_found"))),
        _ => Err(Box::new(failed("reading a room"))),
    }
}

fn tell_members(app: &App, room: &str, event: &Event) {
    for member in app.store.members(room).unwrap_or_default() {
        app.hub.tell(&member, event);
    }
}

/// Adds a friend to a group (its owner only).
async fn add_member(
    State(app): State<Arc<App>>,
    Path(room): Path<String>,
    headers: HeaderMap,
    Json(body): Json<IdBody>,
) -> Response {
    let me = me!(app, headers);
    let found = match room_of(&app, &room, &me.sub) {
        Ok(found) => found,
        Err(response) => return *response,
    };
    let Some(other) = valid_id(&body) else {
        return refuse(StatusCode::UNPROCESSABLE_ENTITY, "id");
    };
    if found.kind != "group" || found.owner != me.sub {
        return refuse(StatusCode::FORBIDDEN, "not_owner");
    }
    if !app.store.are_friends(&me.sub, other).unwrap_or(false) {
        return refuse(StatusCode::FORBIDDEN, "not_friends");
    }
    if app
        .store
        .members(&room)
        .map(|m| m.len())
        .unwrap_or(usize::MAX)
        >= MAX_MEMBERS
    {
        return refuse(StatusCode::UNPROCESSABLE_ENTITY, "members");
    }
    match app.store.add_member(&room, other, now()) {
        Ok(_) => {
            tell_members(&app, &room, &Event::Rooms);
            StatusCode::NO_CONTENT.into_response()
        }
        Err(e) => failed(&e),
    }
}

/// Takes someone out of a group: its owner anyone, anyone themself.
async fn remove_member(
    State(app): State<Arc<App>>,
    Path(room): Path<String>,
    headers: HeaderMap,
    Json(body): Json<IdBody>,
) -> Response {
    let me = me!(app, headers);
    let found = match room_of(&app, &room, &me.sub) {
        Ok(found) => found,
        Err(response) => return *response,
    };
    let Some(other) = valid_id(&body) else {
        return refuse(StatusCode::UNPROCESSABLE_ENTITY, "id");
    };
    if found.kind != "group" || (other != me.sub && found.owner != me.sub) {
        return refuse(StatusCode::FORBIDDEN, "not_owner");
    }
    let before = app.store.members(&room).unwrap_or_default();
    match app.store.remove_member(&room, other) {
        Ok(true) => {
            // An owner who leaves passes the room on to who joined first.
            if other == found.owner
                && let Some(next) = app
                    .store
                    .members(&room)
                    .ok()
                    .and_then(|m| m.into_iter().next())
            {
                let _ = app.store.set_owner(&room, &next);
            }
            for member in before {
                app.hub.tell(&member, &Event::Rooms);
            }
            StatusCode::NO_CONTENT.into_response()
        }
        Ok(false) => refuse(StatusCode::NOT_FOUND, "not_found"),
        Err(e) => failed(&e),
    }
}

#[derive(Deserialize)]
struct NameBody {
    name: String,
}

async fn rename(
    State(app): State<Arc<App>>,
    Path(room): Path<String>,
    headers: HeaderMap,
    Json(body): Json<NameBody>,
) -> Response {
    let me = me!(app, headers);
    let found = match room_of(&app, &room, &me.sub) {
        Ok(found) => found,
        Err(response) => return *response,
    };
    if found.kind != "group" || found.owner != me.sub {
        return refuse(StatusCode::FORBIDDEN, "not_owner");
    }
    let Some(name) = text::line(&body.name, text::NAME_MAX) else {
        return refuse(StatusCode::UNPROCESSABLE_ENTITY, "name");
    };
    match app.store.rename(&room, &name) {
        Ok(()) => {
            tell_members(&app, &room, &Event::Rooms);
            StatusCode::NO_CONTENT.into_response()
        }
        Err(e) => failed(&e),
    }
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Page {
    before: Option<i64>,
    limit: Option<i64>,
}

async fn history(
    State(app): State<Arc<App>>,
    Path(room): Path<String>,
    headers: HeaderMap,
    Query(page): Query<Page>,
) -> Response {
    let me = me!(app, headers);
    if let Err(response) = room_of(&app, &room, &me.sub) {
        return *response;
    }
    let limit = page.limit.unwrap_or(50).clamp(1, 100);
    match app.store.messages(&room, &me.sub, page.before, limit) {
        Ok(list) => Json(json!({ "messages": list })).into_response(),
        Err(e) => failed(&e),
    }
}

#[derive(Deserialize)]
struct BodyText {
    body: String,
}

async fn send(
    State(app): State<Arc<App>>,
    Path(room): Path<String>,
    headers: HeaderMap,
    Json(body): Json<BodyText>,
) -> Response {
    let me = me!(app, headers);
    let found = match room_of(&app, &room, &me.sub) {
        Ok(found) => found,
        Err(response) => return *response,
    };
    let Some(text) = text::message(&body.body) else {
        return refuse(StatusCode::UNPROCESSABLE_ENTITY, "message");
    };
    if !app
        .limiter
        .allow(&me.sub, "burst", 5, Duration::from_secs(10))
        || !app
            .limiter
            .allow(&me.sub, "hour", 600, Duration::from_secs(3600))
    {
        return refuse(StatusCode::TOO_MANY_REQUESTS, "too_many");
    }
    let members = app.store.members(&room).unwrap_or_default();
    if found.kind == "direct" {
        // Two people who are no longer friends, or one blocked the other,
        // keep their history but write no more.
        let Some(other) = members.iter().find(|m| **m != me.sub) else {
            return refuse(StatusCode::FORBIDDEN, "not_friends");
        };
        if !app.store.are_friends(&me.sub, other).unwrap_or(false) {
            return refuse(StatusCode::FORBIDDEN, "not_friends");
        }
    }
    let message = match app.store.post(&room, &me.sub, &text, now()) {
        Ok(message) => message,
        Err(e) => return failed(&e),
    };
    let event = Event::Message {
        message: message.clone(),
    };
    for member in members {
        if member == me.sub || !app.store.blocks(&member, &me.sub).unwrap_or(true) {
            app.hub.tell(&member, &event);
        }
    }
    Json(json!({ "message": message })).into_response()
}

#[derive(Deserialize)]
struct ReadBody {
    upto: i64,
}

async fn read(
    State(app): State<Arc<App>>,
    Path(room): Path<String>,
    headers: HeaderMap,
    Json(body): Json<ReadBody>,
) -> Response {
    let me = me!(app, headers);
    if let Err(response) = room_of(&app, &room, &me.sub) {
        return *response;
    }
    match app.store.mark_read(&room, &me.sub, body.upto) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => failed(&e),
    }
}

#[derive(Deserialize)]
struct ReportBody {
    reason: String,
}

/// Reports a message to the owner; only someone who can see it may.
async fn report(
    State(app): State<Arc<App>>,
    Path(message): Path<i64>,
    headers: HeaderMap,
    Json(body): Json<ReportBody>,
) -> Response {
    let me = me!(app, headers);
    let reason = text::line(&body.reason, text::REASON_MAX).unwrap_or_default();
    if !app
        .limiter
        .allow(&me.sub, "report", 20, Duration::from_secs(3600))
    {
        return refuse(StatusCode::TOO_MANY_REQUESTS, "too_many");
    }
    let Ok(Some((room, _sender))) = app.store.message_place(message) else {
        return refuse(StatusCode::NOT_FOUND, "not_found");
    };
    if !app.store.is_member(&room, &me.sub).unwrap_or(false) {
        return refuse(StatusCode::NOT_FOUND, "not_found");
    }
    match app.store.report(message, &me.sub, &reason, now()) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => failed(&e),
    }
}

/// Live events. The connection lasts as long as its token: the launcher
/// comes back with a fresh one.
async fn live(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    upgrade: WebSocketUpgrade,
) -> Response {
    let me = me!(app, headers);
    let Some((id, events)) = app.hub.join(&me.sub) else {
        return refuse(StatusCode::TOO_MANY_REQUESTS, "too_many_connections");
    };
    let lasts = Duration::from_secs(u64::try_from((me.exp - now()).max(1)).unwrap_or(1));
    upgrade
        .max_message_size(4 * 1024)
        .on_upgrade(move |socket| async move {
            serve_socket(socket, events, lasts).await;
            app.hub.leave(&me.sub, id);
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

async fn health(State(app): State<Arc<App>>) -> Json<Value> {
    Json(json!({
        "ok": true,
        "service": "lapi-friends",
        "version": env!("CARGO_PKG_VERSION"),
        "keys": app.tokens.has_keys(),
    }))
}

pub fn router(app: Arc<App>) -> Router {
    Router::new()
        .route("/v1/health", get(health))
        .route("/v1/state", get(state))
        .route("/v1/requests", post(request))
        .route("/v1/people/{action}", post(act))
        .route("/v1/rooms", get(rooms))
        .route("/v1/rooms/direct", post(direct))
        .route("/v1/rooms/group", post(group))
        .route("/v1/rooms/{room}/add", post(add_member))
        .route("/v1/rooms/{room}/remove", post(remove_member))
        .route("/v1/rooms/{room}/name", post(rename))
        .route("/v1/rooms/{room}/messages", get(history).post(send))
        .route("/v1/rooms/{room}/read", post(read))
        .route("/v1/messages/{message}/report", post(report))
        .route("/v1/live", get(live))
        .layer(DefaultBodyLimit::max(16 * 1024))
        .with_state(app)
}
