//! Friends and chats of the launcher account at friends.lapi.sh (D-064).
//!
//! The core keeps one live connection (a WebSocket that lasts as long as the
//! account's access token) and the lists the interface shows: friends,
//! requests both ways, blocked people and rooms with their unread counts. The
//! interface gets them as a [`FriendsView`] in the `friends://state` event
//! and new messages in `friends://message`; it asks for history with
//! commands. Everything people wrote is shown as plain text.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tauri::{AppHandle, Emitter, Manager, Runtime};

use crate::launcher_account::{AccountProblem, LauncherAccount};
use crate::state::AppState;
use crate::sync::lock;

const BASE: &str = "https://friends.lapi.sh";
pub const STATE_EVENT: &str = "friends://state";
pub const MESSAGE_EVENT: &str = "friends://message";
/// The owner removed a message: `{ room, message }`.
pub const REMOVED_EVENT: &str = "friends://removed";
const MAX_RESPONSE_BYTES: usize = 2 * 1024 * 1024;
/// An account's export: every message it wrote.
const MAX_EXPORT_BYTES: usize = 64 * 1024 * 1024;

/// friends.lapi.sh, or the server a test build was made for
/// (`LAUNCHER_FRIENDS_URL` at build time; release builds never set it).
fn base() -> &'static str {
    option_env!("LAUNCHER_FRIENDS_URL")
        .filter(|u| !u.trim().is_empty())
        .unwrap_or(BASE)
}

/// TLS for the live connection, with the system's certificate checks as the
/// HTTPS client has them (none for a plain `ws://` test server).
fn tls() -> Result<Option<Arc<rustls::ClientConfig>>, FriendsProblem> {
    use rustls_platform_verifier::BuilderVerifierExt;
    if !base().starts_with("https://") {
        return Ok(None);
    }
    let config = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::aws_lc_rs::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .map_err(|_| FriendsProblem::Server)?
    .with_platform_verifier()
    .map_err(|_| FriendsProblem::Server)?
    .with_no_client_auth();
    Ok(Some(Arc::new(config)))
}

/// What went wrong, for the interface to say in the player's language.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FriendsProblem {
    /// No launcher account, or its sign-in ended.
    SignedOut,
    Network,
    NoSuchNick,
    Yourself,
    /// You blocked them: unblock first.
    YouBlocked,
    /// Too many friends, requests or rooms.
    Limit,
    TooMany,
    NotFriends,
    NotOwner,
    /// The owner muted the account for a while: it reads but writes nothing.
    Muted,
    /// The owner banned the account from friends.
    Banned,
    /// Friends asks for a confirmed email first.
    EmailNotConfirmed,
    Invalid,
    Server,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Person {
    pub id: String,
    pub nick: String,
    /// Friends since, or when the request or block was made.
    #[serde(alias = "since", default)]
    pub at: i64,
    /// A friend with the launcher open (unless they hide it).
    #[serde(default)]
    pub online: bool,
    /// What the friend plays now, as their launcher names it.
    #[serde(default)]
    pub playing: Option<String>,
}

/// A mute or ban the owner put on the account.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Sanction {
    /// `mute` or `ban`.
    pub kind: String,
    /// Unix time it ends; 0 for good.
    pub until: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Member {
    pub id: String,
    pub nick: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessage {
    pub id: i64,
    pub room: String,
    pub sender: String,
    pub body: String,
    pub created: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Room {
    pub id: String,
    /// `direct` (two friends) or `group`.
    pub kind: String,
    pub name: String,
    pub owner: String,
    pub members: Vec<Member>,
    pub last: Option<ChatMessage>,
    pub unread: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FriendsView {
    /// A launcher account is signed in.
    pub signed_in: bool,
    /// The live connection is open.
    pub connected: bool,
    pub me: Option<Member>,
    pub friends: Vec<Person>,
    pub incoming: Vec<Person>,
    pub outgoing: Vec<Person>,
    pub blocked: Vec<Person>,
    pub rooms: Vec<Room>,
    /// Unread messages in all rooms.
    pub unread: i64,
    /// Friends see that this account is online.
    pub visible: bool,
    pub sanction: Option<Sanction>,
    pub problem: Option<FriendsProblem>,
}

fn shown() -> bool {
    true
}

#[derive(Deserialize)]
struct MeAnswer {
    id: String,
    nick: String,
    #[serde(default = "shown")]
    visible: bool,
    #[serde(default)]
    sanction: Option<Sanction>,
}

#[derive(Deserialize)]
struct StateAnswer {
    me: MeAnswer,
    friends: Vec<Person>,
    incoming: Vec<Person>,
    outgoing: Vec<Person>,
    blocked: Vec<Person>,
}

fn problem_of(status: u16, body: &Value) -> FriendsProblem {
    match (status, body["error"].as_str().unwrap_or_default()) {
        (401, _) => FriendsProblem::SignedOut,
        (404, "no_such_nick") => FriendsProblem::NoSuchNick,
        (422, "yourself") => FriendsProblem::Yourself,
        (409, "you_blocked") => FriendsProblem::YouBlocked,
        (429, "limit") => FriendsProblem::Limit,
        (429, _) => FriendsProblem::TooMany,
        (403, "not_friends") => FriendsProblem::NotFriends,
        (403, "not_owner") => FriendsProblem::NotOwner,
        (403, "mute") => FriendsProblem::Muted,
        (403, "ban") => FriendsProblem::Banned,
        (403, "email_not_confirmed") => FriendsProblem::EmailNotConfirmed,
        (422 | 404, _) => FriendsProblem::Invalid,
        _ => FriendsProblem::Server,
    }
}

pub struct Friends {
    http: reqwest::Client,
    view: Mutex<FriendsView>,
    /// What the player plays now; told again after each reconnect.
    playing: Mutex<Option<String>>,
    /// Wakes the live loop at once (sign-in, sign-out, back online).
    wake: tokio::sync::Notify,
    /// When each room (or request) last made a system notification: one per
    /// few seconds, however fast messages come.
    notified: Mutex<std::collections::HashMap<String, std::time::Instant>>,
}

impl Friends {
    pub fn new(app_version: &str) -> Result<Self, String> {
        let http = crate::launcher_account::our_client(app_version, base())?;
        Ok(Self {
            http,
            view: Mutex::new(FriendsView::default()),
            playing: Mutex::new(None),
            wake: tokio::sync::Notify::new(),
            notified: Mutex::new(std::collections::HashMap::new()),
        })
    }

    pub fn view(&self) -> FriendsView {
        lock(&self.view).clone()
    }

    fn set<R: Runtime>(&self, app: &AppHandle<R>, edit: impl FnOnce(&mut FriendsView)) {
        let view = {
            let mut view = lock(&self.view);
            let before = view.clone();
            edit(&mut view);
            view.unread = view.rooms.iter().map(|r| r.unread).sum();
            if *view == before {
                return;
            }
            view.clone()
        };
        let _ = app.emit(STATE_EVENT, view);
    }

    pub fn wake(&self) {
        self.wake.notify_one();
    }

    async fn token<R: Runtime>(&self, app: &AppHandle<R>) -> Result<String, FriendsProblem> {
        let account = Arc::clone(&app.state::<Arc<LauncherAccount>>());
        match account.access_token(&app.state::<AppState>()).await {
            Ok(token) => Ok(token.expose().to_owned()),
            Err(AccountProblem::Network) => Err(FriendsProblem::Network),
            // The account service failing or busy is not a sign-out: try again.
            Err(AccountProblem::Server | AccountProblem::TooMany) => Err(FriendsProblem::Server),
            Err(_) => Err(FriendsProblem::SignedOut),
        }
    }

    /// Calls the API with the account's token.
    async fn call<R: Runtime>(
        &self,
        app: &AppHandle<R>,
        method: reqwest::Method,
        path: &str,
        body: Option<Value>,
    ) -> Result<Value, FriendsProblem> {
        self.call_within(app, method, path, body, MAX_RESPONSE_BYTES)
            .await
    }

    async fn call_within<R: Runtime>(
        &self,
        app: &AppHandle<R>,
        method: reqwest::Method,
        path: &str,
        body: Option<Value>,
        max: usize,
    ) -> Result<Value, FriendsProblem> {
        let token = self.token(app).await?;
        let mut request = self
            .http
            .request(method, format!("{}{path}", base()))
            .bearer_auth(token)
            .header(reqwest::header::ACCEPT, "application/json");
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request.send().await.map_err(|_| FriendsProblem::Network)?;
        let status = response.status().as_u16();
        let bytes = crate::launcher_account::body_within(response, max)
            .await
            .map_err(|e| match e {
                crate::launcher_account::BodyError::Network => FriendsProblem::Network,
                crate::launcher_account::BodyError::TooLarge => FriendsProblem::Server,
            })?;
        let value: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        if (200..300).contains(&status) {
            Ok(value)
        } else {
            Err(problem_of(status, &value))
        }
    }

    /// Reads the lists and rooms again.
    pub async fn refresh<R: Runtime>(&self, app: &AppHandle<R>) -> Result<(), FriendsProblem> {
        let state = self
            .call(app, reqwest::Method::GET, "/v1/state", None)
            .await;
        let rooms = self
            .call(app, reqwest::Method::GET, "/v1/rooms", None)
            .await;
        match (state, rooms) {
            (Ok(state), Ok(rooms)) => {
                let state: StateAnswer =
                    serde_json::from_value(state).map_err(|_| FriendsProblem::Server)?;
                let rooms: Vec<Room> = serde_json::from_value(rooms["rooms"].clone())
                    .map_err(|_| FriendsProblem::Server)?;
                self.set(app, |view| {
                    view.signed_in = true;
                    view.visible = state.me.visible;
                    view.sanction = state.me.sanction;
                    view.me = Some(Member {
                        id: state.me.id,
                        nick: state.me.nick,
                    });
                    view.friends = state.friends;
                    view.incoming = state.incoming;
                    view.outgoing = state.outgoing;
                    view.blocked = state.blocked;
                    view.rooms = rooms;
                    view.problem = None;
                });
                Ok(())
            }
            (Err(problem), _) | (_, Err(problem)) => {
                self.set(app, |view| {
                    view.problem = Some(problem);
                    // Banned, or the email not confirmed yet: the account is
                    // signed in, with nothing to show until that changes.
                    if matches!(
                        problem,
                        FriendsProblem::Banned | FriendsProblem::EmailNotConfirmed
                    ) {
                        *view = FriendsView {
                            signed_in: true,
                            problem: Some(problem),
                            ..FriendsView::default()
                        };
                    }
                });
                Err(problem)
            }
        }
    }

    /// The live loop: keeps the connection while an account is signed in and
    /// the internet is there, and reconnects with a fresh token.
    pub async fn run<R: Runtime>(self: Arc<Self>, app: AppHandle<R>) {
        let mut failures: u32 = 0;
        loop {
            let state = app.state::<AppState>();
            let signed_in = state.accounts_file().launcher_account.is_some();
            let offline = state.network() == crate::state::dto::NetworkStatus::Offline;
            if !signed_in {
                self.set(&app, |view| *view = FriendsView::default());
            }
            if !signed_in || offline {
                self.set(&app, |view| view.connected = false);
                tokio::select! {
                    () = self.wake.notified() => {}
                    () = tokio::time::sleep(Duration::from_secs(30)) => {}
                }
                continue;
            }
            let outcome = match self.refresh(&app).await {
                Ok(()) => self.live(&app).await,
                Err(problem) => Err(problem),
            };
            self.set(&app, |view| view.connected = false);
            failures = match outcome {
                Ok(()) => 0,
                Err(
                    problem @ (FriendsProblem::SignedOut
                    | FriendsProblem::Banned
                    | FriendsProblem::EmailNotConfirmed),
                ) => {
                    // The problem stays as it is: the panel says what to do.
                    self.set(&app, |view| view.problem = Some(problem));
                    // Until the account signs in again; a refusal the friends
                    // server had no reason for (its keys not read yet after a
                    // restart) passes, so try again now and then too.
                    tokio::select! {
                        () = self.wake.notified() => {}
                        () = tokio::time::sleep(Duration::from_secs(300)) => {}
                    }
                    0
                }
                Err(_) => failures.saturating_add(1),
            };
            let wait = Duration::from_secs(match failures {
                0 => 1,
                n => (2u64 << n.min(5)).min(60),
            });
            tokio::select! {
                () = self.wake.notified() => {}
                () = tokio::time::sleep(wait) => {}
            }
        }
    }

    /// One live connection, until the server ends it (its token expired).
    async fn live<R: Runtime>(&self, app: &AppHandle<R>) -> Result<(), FriendsProblem> {
        use tokio_tungstenite::tungstenite::Message as Ws;
        use tokio_tungstenite::tungstenite::client::IntoClientRequest;
        let token = self.token(app).await?;
        let url = format!("{}/v1/live", base().replacen("http", "ws", 1));
        let mut request = url
            .into_client_request()
            .map_err(|_| FriendsProblem::Server)?;
        request.headers_mut().insert(
            "authorization",
            format!("Bearer {token}")
                .parse()
                .map_err(|_| FriendsProblem::Server)?,
        );
        let config = tokio_tungstenite::tungstenite::protocol::WebSocketConfig::default()
            .max_message_size(Some(256 * 1024))
            .max_frame_size(Some(256 * 1024));
        let (socket, _) = tokio_tungstenite::connect_async_tls_with_config(
            request,
            Some(config),
            false,
            tls()?.map(tokio_tungstenite::Connector::Rustls),
        )
        .await
        .map_err(|_| FriendsProblem::Network)?;
        self.set(app, |view| view.connected = true);
        // The server forgets what we play when the connection drops.
        let playing = lock(&self.playing).clone();
        if playing.is_some() {
            let _ = self.tell_playing(app, playing).await;
        }
        let (mut out, mut incoming) = socket.split();
        loop {
            tokio::select! {
                () = self.wake.notified() => {
                    // Signed out or something changed: start over.
                    let _ = out.send(Ws::Close(None)).await;
                    return Ok(());
                }
                frame = tokio::time::timeout(Duration::from_secs(90), incoming.next()) => {
                    let Ok(frame) = frame else {
                        return Err(FriendsProblem::Network);
                    };
                    match frame {
                        Some(Ok(Ws::Text(text))) => self.event(app, &text).await,
                        Some(Ok(Ws::Ping(data))) => {
                            let _ = out.send(Ws::Pong(data)).await;
                        }
                        Some(Ok(Ws::Close(_))) | None => return Ok(()),
                        Some(Ok(_)) => {}
                        Some(Err(_)) => return Err(FriendsProblem::Network),
                    }
                }
            }
        }
    }

    async fn event<R: Runtime>(&self, app: &AppHandle<R>, text: &str) {
        let Ok(event) = serde_json::from_str::<Value>(text) else {
            return;
        };
        match event["type"].as_str() {
            Some("friends" | "rooms") => {
                let before: Vec<String> = lock(&self.view)
                    .incoming
                    .iter()
                    .map(|p| p.id.clone())
                    .collect();
                let _ = self.refresh(app).await;
                let new: Vec<String> = lock(&self.view)
                    .incoming
                    .iter()
                    .filter(|p| !before.contains(&p.id))
                    .map(|p| p.nick.clone())
                    .collect();
                for nick in new {
                    let words = request_words(app.state::<AppState>().settings().language.as_str());
                    self.notify(app, &format!("request:{nick}"), &nick, words);
                }
            }
            Some("message") => {
                let Ok(message) = serde_json::from_value::<ChatMessage>(event["message"].clone())
                else {
                    return;
                };
                let mine = lock(&self.view)
                    .me
                    .as_ref()
                    .is_some_and(|me| me.id == message.sender);
                let known = lock(&self.view).rooms.iter().any(|r| r.id == message.room);
                if !known {
                    let _ = self.refresh(app).await;
                } else {
                    self.set(app, |view| {
                        if let Some(room) = view.rooms.iter_mut().find(|r| r.id == message.room) {
                            room.last = Some(message.clone());
                            if !mine {
                                room.unread += 1;
                            }
                        }
                        // The room with the newest message first.
                        view.rooms.sort_by_key(|r| {
                            std::cmp::Reverse(r.last.as_ref().map_or(0, |m| m.created))
                        });
                    });
                }
                if !mine {
                    let title = {
                        let view = lock(&self.view);
                        view.rooms
                            .iter()
                            .find(|r| r.id == message.room)
                            .map(|room| {
                                let sender = room
                                    .members
                                    .iter()
                                    .find(|m| m.id == message.sender)
                                    .map_or("—", |m| m.nick.as_str());
                                if room.kind == "group" && !room.name.is_empty() {
                                    format!("{sender} · {}", room.name)
                                } else {
                                    sender.to_owned()
                                }
                            })
                    };
                    if let Some(title) = title {
                        self.notify(app, &message.room, &title, &message.body);
                    }
                }
                let _ = app.emit(MESSAGE_EVENT, message);
            }
            Some("removed") => {
                let (Some(room), Some(message)) =
                    (event["room"].as_str(), event["message"].as_i64())
                else {
                    return;
                };
                let _ = app.emit(REMOVED_EVENT, json!({ "room": room, "message": message }));
            }
            Some("presence") => {
                let Some(id) = event["id"].as_str() else {
                    return;
                };
                let online = event["online"].as_bool().unwrap_or(false);
                let playing = event["playing"]
                    .as_str()
                    .map(|p| p.chars().take(60).collect::<String>());
                self.set(app, |view| {
                    if let Some(friend) = view.friends.iter_mut().find(|f| f.id == id) {
                        friend.online = online;
                        friend.playing = playing.filter(|_| online);
                    }
                });
            }
            _ => {}
        }
    }

    /// A system notification for a message or request, when the player wants
    /// them and the launcher is not in front (in front, the interface plays
    /// its sound instead).
    fn notify<R: Runtime>(&self, app: &AppHandle<R>, key: &str, title: &str, body: &str) {
        use tauri_plugin_notification::NotificationExt as _;
        if !app.state::<AppState>().settings().chat_notifications {
            return;
        }
        let in_front = app
            .get_webview_window(crate::lifecycle::MAIN_WINDOW)
            .is_some_and(|w| w.is_focused().unwrap_or(false) && !w.is_minimized().unwrap_or(true));
        if in_front {
            return;
        }
        {
            let now = std::time::Instant::now();
            let mut notified = lock(&self.notified);
            notified.retain(|_, at| now.duration_since(*at) < Duration::from_secs(60));
            if notified
                .get(key)
                .is_some_and(|at| now.duration_since(*at) < Duration::from_secs(5))
            {
                return;
            }
            notified.insert(key.to_owned(), now);
        }
        let _ = app
            .notification()
            .builder()
            .title(notification_text(title, 64))
            .body(notification_text(body, 160))
            .show();
    }

    async fn tell_playing<R: Runtime>(
        &self,
        app: &AppHandle<R>,
        playing: Option<String>,
    ) -> Result<(), FriendsProblem> {
        self.call(
            app,
            reqwest::Method::POST,
            "/v1/presence/playing",
            Some(json!({ "playing": playing })),
        )
        .await
        .map(drop)
    }

    /// The game started (`Some`, "Minecraft 1.21.1 · Fabric") or ended
    /// (`None`): friends see it while the live connection is open.
    pub async fn set_playing<R: Runtime>(&self, app: &AppHandle<R>, playing: Option<String>) {
        *lock(&self.playing) = playing.clone();
        if lock(&self.view).connected {
            let _ = self.tell_playing(app, playing).await;
        }
    }

    /// Declines every request waiting for the account.
    pub async fn decline_all<R: Runtime>(&self, app: &AppHandle<R>) -> Result<(), FriendsProblem> {
        self.call(app, reqwest::Method::POST, "/v1/requests/decline-all", None)
            .await?;
        let _ = self.refresh(app).await;
        Ok(())
    }

    /// Reports a waiting request as spam; it goes.
    pub async fn report_request<R: Runtime>(
        &self,
        app: &AppHandle<R>,
        id: &str,
    ) -> Result<(), FriendsProblem> {
        if uuid::Uuid::parse_str(id).is_err() {
            return Err(FriendsProblem::Invalid);
        }
        self.call(
            app,
            reqwest::Method::POST,
            "/v1/requests/report",
            Some(json!({ "id": id })),
        )
        .await?;
        let _ = self.refresh(app).await;
        Ok(())
    }

    /// Shows or hides the account's online status from its friends.
    pub async fn set_visible<R: Runtime>(
        &self,
        app: &AppHandle<R>,
        visible: bool,
    ) -> Result<(), FriendsProblem> {
        self.call(
            app,
            reqwest::Method::POST,
            "/v1/presence/visible",
            Some(json!({ "visible": visible })),
        )
        .await?;
        self.set(app, |view| view.visible = visible);
        Ok(())
    }

    /// What friends keeps about the account (for its data export).
    pub async fn export<R: Runtime>(&self, app: &AppHandle<R>) -> Result<Value, FriendsProblem> {
        self.call_within(
            app,
            reqwest::Method::GET,
            "/v1/export",
            None,
            MAX_EXPORT_BYTES,
        )
        .await
    }

    // What the interface asks for.

    pub async fn request<R: Runtime>(
        &self,
        app: &AppHandle<R>,
        nick: &str,
    ) -> Result<String, FriendsProblem> {
        let answer = self
            .call(
                app,
                reqwest::Method::POST,
                "/v1/requests",
                Some(json!({ "nick": nick })),
            )
            .await?;
        let _ = self.refresh(app).await;
        Ok(answer["result"].as_str().unwrap_or("sent").to_owned())
    }

    /// accept, decline, cancel, remove, block or unblock someone.
    pub async fn act<R: Runtime>(
        &self,
        app: &AppHandle<R>,
        action: &str,
        id: &str,
    ) -> Result<(), FriendsProblem> {
        if !matches!(
            action,
            "accept" | "decline" | "cancel" | "remove" | "block" | "unblock"
        ) || uuid::Uuid::parse_str(id).is_err()
        {
            return Err(FriendsProblem::Invalid);
        }
        self.call(
            app,
            reqwest::Method::POST,
            &format!("/v1/people/{action}"),
            Some(json!({ "id": id })),
        )
        .await?;
        let _ = self.refresh(app).await;
        Ok(())
    }

    pub async fn open_direct<R: Runtime>(
        &self,
        app: &AppHandle<R>,
        friend: &str,
    ) -> Result<String, FriendsProblem> {
        let answer = self
            .call(
                app,
                reqwest::Method::POST,
                "/v1/rooms/direct",
                Some(json!({ "id": friend })),
            )
            .await?;
        let _ = self.refresh(app).await;
        answer["room"]
            .as_str()
            .map(str::to_owned)
            .ok_or(FriendsProblem::Server)
    }

    pub async fn create_group<R: Runtime>(
        &self,
        app: &AppHandle<R>,
        name: &str,
        members: &[String],
    ) -> Result<String, FriendsProblem> {
        let answer = self
            .call(
                app,
                reqwest::Method::POST,
                "/v1/rooms/group",
                Some(json!({ "name": name, "members": members })),
            )
            .await?;
        let _ = self.refresh(app).await;
        answer["room"]
            .as_str()
            .map(str::to_owned)
            .ok_or(FriendsProblem::Server)
    }

    /// add or remove someone in a group, or rename it.
    pub async fn change_room<R: Runtime>(
        &self,
        app: &AppHandle<R>,
        room: &str,
        action: &str,
        value: &str,
    ) -> Result<(), FriendsProblem> {
        if uuid::Uuid::parse_str(room).is_err() {
            return Err(FriendsProblem::Invalid);
        }
        let body = match action {
            "add" | "remove" => json!({ "id": value }),
            "name" => json!({ "name": value }),
            _ => return Err(FriendsProblem::Invalid),
        };
        self.call(
            app,
            reqwest::Method::POST,
            &format!("/v1/rooms/{room}/{action}"),
            Some(body),
        )
        .await?;
        let _ = self.refresh(app).await;
        Ok(())
    }

    /// Up to 50 messages before `before`, oldest first.
    pub async fn messages<R: Runtime>(
        &self,
        app: &AppHandle<R>,
        room: &str,
        before: Option<i64>,
    ) -> Result<Vec<ChatMessage>, FriendsProblem> {
        if uuid::Uuid::parse_str(room).is_err() {
            return Err(FriendsProblem::Invalid);
        }
        let query = before.map_or_else(String::new, |b| format!("?before={b}"));
        let answer = self
            .call(
                app,
                reqwest::Method::GET,
                &format!("/v1/rooms/{room}/messages{query}"),
                None,
            )
            .await?;
        let mut list: Vec<ChatMessage> = serde_json::from_value(answer["messages"].clone())
            .map_err(|_| FriendsProblem::Server)?;
        list.reverse();
        Ok(list)
    }

    pub async fn send<R: Runtime>(
        &self,
        app: &AppHandle<R>,
        room: &str,
        body: &str,
    ) -> Result<ChatMessage, FriendsProblem> {
        if uuid::Uuid::parse_str(room).is_err()
            || body.trim().is_empty()
            || body.chars().count() > 2000
        {
            return Err(FriendsProblem::Invalid);
        }
        let answer = self
            .call(
                app,
                reqwest::Method::POST,
                &format!("/v1/rooms/{room}/messages"),
                Some(json!({ "body": body })),
            )
            .await?;
        serde_json::from_value(answer["message"].clone()).map_err(|_| FriendsProblem::Server)
    }

    /// The room was read up to `upto`.
    pub async fn read<R: Runtime>(
        &self,
        app: &AppHandle<R>,
        room: &str,
        upto: i64,
    ) -> Result<(), FriendsProblem> {
        if uuid::Uuid::parse_str(room).is_err() {
            return Err(FriendsProblem::Invalid);
        }
        self.call(
            app,
            reqwest::Method::POST,
            &format!("/v1/rooms/{room}/read"),
            Some(json!({ "upto": upto })),
        )
        .await?;
        self.set(app, |view| {
            if let Some(room) = view.rooms.iter_mut().find(|r| r.id == room) {
                room.unread = 0;
            }
        });
        Ok(())
    }

    pub async fn report<R: Runtime>(
        &self,
        app: &AppHandle<R>,
        message: i64,
        reason: &str,
    ) -> Result<(), FriendsProblem> {
        self.call(
            app,
            reqwest::Method::POST,
            &format!("/v1/messages/{message}/report"),
            Some(json!({ "reason": reason.chars().take(500).collect::<String>() })),
        )
        .await
        .map(drop)
    }
}

/// Text from another player made fit for a system notification: one line of
/// at most `max` characters, no control or direction characters, and on Linux
/// no markup (its notification servers read a little HTML in the body).
fn notification_text(text: &str, max: usize) -> String {
    let clean: String = text
        .chars()
        .map(|c| if c == '\n' || c == '\t' { ' ' } else { c })
        .filter(|c| {
            !c.is_control()
                && !matches!(c, '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
        })
        .collect();
    let clean = clean.trim();
    let mut short: String = clean.chars().take(max).collect();
    if clean.chars().count() > max {
        short.push('…');
    }
    if cfg!(target_os = "linux") {
        short = short
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;");
    }
    short
}

/// "wants to be your friend" in the launcher's language (the notification is
/// made here, without the interface).
fn request_words(language: &str) -> &'static str {
    match language.get(..2).unwrap_or_default() {
        "ru" => "хочет добавить вас в друзья",
        "ar" => "يريد إضافتك صديقًا",
        _ => "wants to be your friend",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notification_text_is_one_short_clean_line() {
        assert_eq!(
            notification_text("hi\nthere\u{202E}!\u{7}", 40),
            "hi there!"
        );
        let long = "a".repeat(200);
        let short = notification_text(&long, 160);
        assert_eq!(short.chars().count(), 161);
        assert!(short.ends_with('…'));
        let markup = notification_text("<a href=\"x\">&</a>", 40);
        if cfg!(target_os = "linux") {
            assert_eq!(markup, "&lt;a href=\"x\"&gt;&amp;&lt;/a&gt;");
        } else {
            assert_eq!(markup, "<a href=\"x\">&</a>");
        }
        assert_eq!(request_words("ru-RU"), "хочет добавить вас в друзья");
        assert_eq!(request_words("pt-BR"), "wants to be your friend");
    }

    #[test]
    fn answers_become_problems() {
        let body = |e: &str| json!({ "error": e });
        assert_eq!(
            problem_of(401, &body("unauthorized")),
            FriendsProblem::SignedOut
        );
        assert_eq!(
            problem_of(404, &body("no_such_nick")),
            FriendsProblem::NoSuchNick
        );
        assert_eq!(
            problem_of(409, &body("you_blocked")),
            FriendsProblem::YouBlocked
        );
        assert_eq!(problem_of(429, &body("limit")), FriendsProblem::Limit);
        assert_eq!(problem_of(429, &body("too_many")), FriendsProblem::TooMany);
        assert_eq!(
            problem_of(403, &body("not_owner")),
            FriendsProblem::NotOwner
        );
        assert_eq!(problem_of(403, &body("mute")), FriendsProblem::Muted);
        assert_eq!(problem_of(403, &body("ban")), FriendsProblem::Banned);
        assert_eq!(
            problem_of(403, &body("email_not_confirmed")),
            FriendsProblem::EmailNotConfirmed
        );
        assert_eq!(problem_of(500, &Value::Null), FriendsProblem::Server);
    }

    #[test]
    fn presence_and_sanctions_read_into_the_view() {
        let state: StateAnswer = serde_json::from_value(json!({
            "me": { "id": "a", "nick": "Alice", "visible": false,
                    "sanction": { "kind": "mute", "until": 99, "reason": "x" } },
            "friends": [
                { "id": "b", "nick": "Bob", "since": 5, "online": true, "playing": "Minecraft 1.21.1" },
                { "id": "c", "nick": "Carol", "since": 6 }
            ],
            "incoming": [], "outgoing": [], "blocked": [],
        }))
        .unwrap();
        assert!(!state.me.visible);
        assert_eq!(
            state.me.sanction,
            Some(Sanction {
                kind: "mute".into(),
                until: 99
            })
        );
        assert!(state.friends[0].online);
        assert_eq!(
            state.friends[0].playing.as_deref(),
            Some("Minecraft 1.21.1")
        );
        assert!(!state.friends[1].online && state.friends[1].playing.is_none());
        // An older server says nothing of it: shown, no sanction.
        let old: StateAnswer = serde_json::from_value(json!({
            "me": { "id": "a", "nick": "Alice" },
            "friends": [], "incoming": [], "outgoing": [], "blocked": [],
        }))
        .unwrap();
        assert!(old.me.visible && old.me.sanction.is_none());
    }

    #[test]
    fn server_lists_read_into_the_view() {
        let state: StateAnswer = serde_json::from_value(json!({
            "me": { "id": "a", "nick": "Alice" },
            "friends": [{ "id": "b", "nick": "Bob", "since": 5 }],
            "incoming": [{ "id": "c", "nick": "Carol", "at": 6 }],
            "outgoing": [],
            "blocked": [],
        }))
        .unwrap();
        assert_eq!(state.friends[0].at, 5);
        assert_eq!(state.incoming[0].nick, "Carol");
    }
}
