//! Joining a room as a guest (plan W7). The guest gets a fresh tunnel key,
//! asks the relay to carry it (renewed every 10 seconds while it plays),
//! learns the host's tunnel key from a first ticket and connects, directly
//! when the networks allow and through relay.lapi.sh otherwise. Then a
//! listener on 127.0.0.1 waits for the game, which is started straight into
//! it; every login of the game gets a ticket of its own at that moment (the
//! first one is used if it is still fresh).

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use iroh::endpoint::Connection;
#[cfg(test)]
use iroh::endpoint::VarInt;
use iroh::{Endpoint, EndpointAddr, EndpointId, RelayUrl};
use tokio::task::JoinHandle;

use super::identity::Nick;
use super::service::{Problem, Room, TicketGrant, WorldsClient};
use crate::error::{ErrorCode, LauncherError, LauncherResult};
use crate::install::service::BoxFuture;
use crate::sync::lock;
use crate::tunnel::frame::{ALPN, Refusal};
use crate::tunnel::guest::{
    self, Grant, GuestListener, RoomStatus, SharedConnection, StatusSnapshot, Target, Tickets,
};

/// Relay enrollment and the room's status are renewed this often.
pub const RENEW_EVERY: Duration = Duration::from_secs(10);
/// A ticket lasts 60 seconds; an older first ticket is not handed out.
const TICKET_FRESH: Duration = Duration::from_secs(40);
/// The relay reads its allow list every 10 seconds, so a new key may wait
/// that long before it is carried.
const CONNECT_FOR: Duration = Duration::from_secs(30);

/// How the guest reaches the host.
#[derive(Debug, Clone)]
pub enum Route {
    /// Through relay.lapi.sh, and directly once a path is found.
    Relay(RelayUrl),
    /// A known address (tests on loopback).
    Known(EndpointAddr),
}

/// The relay of worlds with friends, or the one a test build was made for
/// (`LAUNCHER_RELAY_URL` at build time; release builds never set it).
pub fn relay_url() -> LauncherResult<RelayUrl> {
    option_env!("LAUNCHER_RELAY_URL")
        .filter(|u| !u.trim().is_empty())
        .unwrap_or("https://relay.lapi.sh")
        .parse()
        .map_err(|_| LauncherError::internal("The relay address is not valid."))
}

/// Which way the tunnel's game data goes now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PathKind {
    /// Straight between the two computers.
    Direct,
    /// Through relay.lapi.sh.
    Relay,
}

/// The path the connection sends on, once one is chosen.
pub fn path_of(connection: &Connection) -> Option<PathKind> {
    connection
        .paths()
        .iter()
        .find(|p| p.is_selected())
        .map(|p| {
            if p.is_ip() {
                PathKind::Direct
            } else {
                PathKind::Relay
            }
        })
}

/// How often the chosen path and the connection are looked at.
const PATH_EVERY: Duration = Duration::from_secs(2);
/// A lost connection is tried again after this, doubling up to the most.
const RECONNECT_FIRST: Duration = Duration::from_secs(2);
const RECONNECT_MOST: Duration = Duration::from_secs(30);
/// A host whose lease lapsed (it slept, or lost the service for a moment)
/// opens a new session within this; until then a room that is not hosted is
/// tried again.
const HOST_GRACE: Duration = Duration::from_secs(if cfg!(test) { 1 } else { 120 });

struct Shared {
    client: Arc<WorldsClient>,
    room: String,
    endpoint_key: String,
    first: Mutex<Option<(TicketGrant, Instant)>>,
    status: Mutex<Option<StatusSnapshot>>,
    refused: Mutex<Option<Option<Refusal>>>,
    path: Mutex<Option<PathKind>>,
    /// The tunnel was lost and is being opened again.
    reconnecting: Mutex<bool>,
    /// The room no longer lets this computer in: it stopped trying.
    gave_up: Mutex<bool>,
    on_change: Arc<dyn Fn() + Send + Sync>,
}

fn grant(ticket: TicketGrant) -> Grant {
    Grant {
        ticket: ticket.ticket,
        session_id: ticket.session,
        host_epoch: ticket.host_epoch,
    }
}

struct RoomTickets(Arc<Shared>);

impl Tickets for RoomTickets {
    fn ticket(&self) -> BoxFuture<'_, Result<Grant, LauncherError>> {
        Box::pin(async move {
            let first = lock(&self.0.first)
                .take()
                .filter(|(_, at)| at.elapsed() < TICKET_FRESH);
            if let Some((ticket, _)) = first {
                return Ok(grant(ticket));
            }
            let ticket = self
                .0
                .client
                .ticket(&self.0.room, &self.0.endpoint_key)
                .await?;
            Ok(grant(ticket))
        })
    }
}

struct Status(Arc<Shared>);

impl RoomStatus for Status {
    fn snapshot(&self) -> Option<StatusSnapshot> {
        lock(&self.0.status).clone()
    }

    fn refused(&self, refusal: Option<Refusal>) {
        *lock(&self.0.refused) = Some(refusal);
        (self.0.on_change)();
    }
}

fn snapshot(room: &Room) -> StatusSnapshot {
    StatusSnapshot {
        observed_at: Instant::now(),
        name: room.name.clone(),
        online: u32::try_from(room.online.max(0)).unwrap_or(0),
        max: u32::try_from(room.max_players.max(0)).unwrap_or(0),
    }
}

/// A joined room: the game connects to [`RoomGuest::port`].
pub struct RoomGuest {
    shared: Arc<Shared>,
    listener: GuestListener,
    connection: SharedConnection,
    endpoint: Endpoint,
    keeper: JoinHandle<()>,
    watcher: JoinHandle<()>,
}

impl Drop for RoomGuest {
    fn drop(&mut self) {
        self.keeper.abort();
        self.watcher.abort();
    }
}

async fn connect(
    endpoint: &Endpoint,
    host: EndpointId,
    route: &Route,
) -> LauncherResult<Connection> {
    let address = match route {
        Route::Relay(url) => EndpointAddr::new(host).with_relay_url(url.clone()),
        Route::Known(address) => address.clone(),
    };
    if address.id != host {
        return Err(LauncherError::invalid_input(
            "The host's address does not match.",
        ));
    }
    let until = Instant::now() + CONNECT_FOR;
    loop {
        match endpoint.connect(address.clone(), ALPN).await {
            Ok(connection) => return Ok(connection),
            Err(_) if Instant::now() < until => {
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
            Err(_) => {
                return Err(LauncherError::new(
                    ErrorCode::NetworkError,
                    "The host's computer cannot be reached.",
                ));
            }
        }
    }
}

/// Who joins which room, and how.
pub struct Visit<'a> {
    pub room: &'a Room,
    pub nick: Nick,
    pub route: Route,
    /// The game protocol and version name of the room's Minecraft version.
    pub protocol: i32,
    pub version_name: &'static str,
}

/// Joins a room on `endpoint` (bound without `accept`).
pub async fn join(
    client: Arc<WorldsClient>,
    endpoint: Endpoint,
    visit: Visit<'_>,
    on_change: Arc<dyn Fn() + Send + Sync>,
) -> LauncherResult<RoomGuest> {
    let Visit {
        room,
        nick,
        route,
        protocol,
        version_name,
    } = visit;
    let endpoint_key = endpoint.id().to_string();
    if matches!(route, Route::Relay(_)) {
        client.enroll(&room.id, &endpoint_key).await?;
    }
    let first = client.ticket(&room.id, &endpoint_key).await?;
    let host: EndpointId = first
        .host_endpoint
        .parse()
        .map_err(|_| LauncherError::from(Problem::Server))?;
    let connection = connect(&endpoint, host, &route).await?;
    let shared = Arc::new(Shared {
        client,
        room: room.id.clone(),
        endpoint_key,
        first: Mutex::new(Some((first, Instant::now()))),
        status: Mutex::new(Some(snapshot(room))),
        refused: Mutex::new(None),
        path: Mutex::new(path_of(&connection)),
        reconnecting: Mutex::new(false),
        gave_up: Mutex::new(false),
        on_change,
    });
    let connection: SharedConnection = Arc::new(Mutex::new(connection));
    let listener = guest::listen(Target {
        connection: Arc::clone(&connection),
        nick,
        protocol,
        version_name,
        tickets: Arc::new(RoomTickets(Arc::clone(&shared))),
        status: Arc::new(Status(Arc::clone(&shared))),
    })
    .await
    .map_err(|e| LauncherError::from_io(&e, "Opening the world for the game"))?;
    let relayed = matches!(route, Route::Relay(_));
    let keeper = tokio::spawn(keep(Arc::clone(&shared), relayed));
    let watcher = tokio::spawn(watch(
        Arc::clone(&shared),
        Arc::clone(&connection),
        endpoint.clone(),
        route,
    ));
    Ok(RoomGuest {
        shared,
        listener,
        connection,
        endpoint,
        keeper,
        watcher,
    })
}

/// Tells when the connection moves between the relay and a direct path,
/// and opens a lost connection again while the room still lets this
/// computer in (the host's network dropped, the relay restarted).
async fn watch(
    shared: Arc<Shared>,
    connection: SharedConnection,
    endpoint: Endpoint,
    route: Route,
) {
    let mut wait = RECONNECT_FIRST;
    let mut lost: Option<Instant> = None;
    loop {
        tokio::time::sleep(PATH_EVERY).await;
        let current = lock(&connection).clone();
        if current.close_reason().is_none() {
            wait = RECONNECT_FIRST;
            lost = None;
            let now = path_of(&current);
            let changed = std::mem::replace(&mut *lock(&shared.path), now) != now;
            if changed {
                (shared.on_change)();
            }
            continue;
        }
        if !std::mem::replace(&mut *lock(&shared.reconnecting), true) {
            *lock(&shared.path) = None;
            (shared.on_change)();
        }
        let since = *lost.get_or_insert_with(Instant::now);
        match reconnect(&shared, &endpoint, &route).await {
            Ok(fresh) => {
                *lock(&shared.path) = path_of(&fresh);
                *lock(&connection) = fresh;
                *lock(&shared.reconnecting) = false;
                (shared.on_change)();
            }
            Err(Some(Problem::NotHosting)) if since.elapsed() < HOST_GRACE => {
                tokio::time::sleep(wait).await;
                wait = (wait * 2).min(RECONNECT_MOST);
            }
            Err(Some(_)) => {
                *lock(&shared.reconnecting) = false;
                *lock(&shared.gave_up) = true;
                (shared.on_change)();
                return;
            }
            Err(None) => {
                tokio::time::sleep(wait).await;
                wait = (wait * 2).min(RECONNECT_MOST);
            }
        }
    }
}

/// A new connection to the room's current host. `Err(Some)` when the room
/// refuses this computer (closed, not a member): trying again will not help.
async fn reconnect(
    shared: &Shared,
    endpoint: &Endpoint,
    route: &Route,
) -> Result<Connection, Option<Problem>> {
    if matches!(route, Route::Relay(_)) {
        enroll_or_stop(shared).await?;
    }
    let ticket = match shared
        .client
        .ticket(&shared.room, &shared.endpoint_key)
        .await
    {
        Ok(ticket) => ticket,
        Err(problem) => return Err(final_problem(problem)),
    };
    let host: EndpointId = ticket.host_endpoint.parse().map_err(|_| None)?;
    // Known addresses (tests) name the host they were made for.
    let route = match route {
        Route::Known(address) if address.id != host => return Err(None),
        other => other,
    };
    let fresh = connect(endpoint, host, route).await.map_err(|_| None)?;
    *lock(&shared.first) = Some((ticket, Instant::now()));
    Ok(fresh)
}

async fn enroll_or_stop(shared: &Shared) -> Result<(), Option<Problem>> {
    match shared
        .client
        .enroll(&shared.room, &shared.endpoint_key)
        .await
    {
        Ok(_) => Ok(()),
        Err(problem) => Err(final_problem(problem)),
    }
}

/// Problems that end the trying; network trouble and busy servers do not.
fn final_problem(problem: Problem) -> Option<Problem> {
    match problem {
        Problem::Network | Problem::Server | Problem::TooMany => None,
        other => Some(other),
    }
}

async fn keep(shared: Arc<Shared>, relayed: bool) {
    loop {
        tokio::time::sleep(RENEW_EVERY).await;
        if relayed {
            let _ = shared
                .client
                .enroll(&shared.room, &shared.endpoint_key)
                .await;
        }
        if let Ok(room) = shared.client.room(&shared.room).await {
            *lock(&shared.status) = Some(snapshot(&room));
        }
    }
}

impl RoomGuest {
    /// The local port the game connects to.
    pub fn port(&self) -> u16 {
        self.listener.port()
    }

    pub fn room(&self) -> &str {
        &self.shared.room
    }

    /// Why the host last refused a login, if it did.
    pub fn refused(&self) -> Option<Option<Refusal>> {
        *lock(&self.shared.refused)
    }

    /// Which way the game data goes now.
    pub fn path(&self) -> Option<PathKind> {
        *lock(&self.shared.path)
    }

    /// Whether the tunnel to the host still stands.
    pub fn connected(&self) -> bool {
        lock(&self.connection).close_reason().is_none()
    }

    /// Closes the tunnel as a lost network would.
    #[cfg(test)]
    pub fn cut(&self) {
        lock(&self.connection).close(VarInt::from_u32(9), b"cut");
    }

    /// The tunnel was lost and is being opened again.
    pub fn reconnecting(&self) -> bool {
        *lock(&self.shared.reconnecting)
    }

    /// The room no longer lets this computer in; leave and join again.
    pub fn gave_up(&self) -> bool {
        *lock(&self.shared.gave_up)
    }

    pub async fn leave(self) {
        self.keeper.abort();
        self.watcher.abort();
        self.endpoint.close().await;
    }
}
