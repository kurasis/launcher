//! Hosting a room (plan W7): the world's server already runs on loopback; this
//! puts a tunnel endpoint in front of it, starts the room's session at
//! worlds.lapi.sh with that endpoint, renews the 30-second lease every 10
//! seconds and keeps the server in step with the room:
//!
//! - the whitelist follows the active members the renewal names;
//! - a guest's ticket is consumed by the service when its stream arrives
//!   ([`Admit`]); the admission names the account and its current nick, which
//!   the tunnel then requires in the login, and the account holds one place
//!   until its stream ends and the place is released;
//! - an admission the service revokes (the member left, was removed or
//!   banned) is kicked;
//! - an account seen before under another nick gets its player files moved
//!   to the new nick before it logs in ([`super::players`]).
//!
//! The session ends when the player stops hosting, the server exits, or the
//! service says the session is over (another start of the room, a restored
//! database). Then the tunnel stops accepting at once. A lease that lapsed
//! (the computer slept, the service could not be reached for 30 seconds)
//! cuts the guests off too, and while the server runs a new session is
//! opened on a fresh endpoint ([`Room`]).

use std::collections::{BTreeSet, HashMap};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use iroh::{Endpoint, EndpointId};
use serde::Serialize;
use tokio::sync::Notify;
use tokio::task::JoinHandle;

use super::host::HostedServer;
use super::identity::Nick;
use super::players;
use super::service::{Problem, Renewal, Session, WorldsClient};
use crate::error::{ErrorCode, LauncherError, LauncherResult};
use crate::install::service::BoxFuture;
use crate::storage::paths::DataRoot;
use crate::sync::lock;
use crate::tunnel::frame::{AdmissionRequest, Refusal};
use crate::tunnel::host::{Admit, Admitted, TunnelHost};

/// How often the lease is renewed.
pub const RENEW_EVERY: Duration = Duration::from_millis(if cfg!(test) { 300 } else { 10_000 });

/// How long a lease lasts at the service.
const LEASE: Duration = Duration::from_millis(if cfg!(test) { 900 } else { 30_000 });

/// A wall clock turned back by more than this ends the lease.
const CLOCK_SLACK: Duration = Duration::from_secs(2);

/// Waits between tries to open a new session after a lapsed lease.
const REOPEN_FIRST: Duration = Duration::from_millis(if cfg!(test) { 200 } else { 2_000 });
const REOPEN_MOST: Duration = Duration::from_millis(if cfg!(test) { 1_000 } else { 30_000 });

/// What the room needs from the running world server.
pub trait WorldServer: Send + Sync + 'static {
    fn port(&self) -> u16;
    fn protocol(&self) -> i32;
    fn running(&self) -> bool;
    /// Players on the server now, the host's own game included.
    fn players(&self) -> u32;
    /// Writes the whitelist and has the server read it again.
    fn set_members(&self, members: &[Nick]) -> LauncherResult<()>;
    fn kick(&self, nick: &Nick) -> LauncherResult<()>;
    /// `account` is about to log in as `nick`.
    fn admitted(&self, account: &str, nick: &Nick) -> LauncherResult<()>;
}

/// The launcher's own world server as a [`WorldServer`].
pub struct Served {
    pub server: Arc<HostedServer>,
    pub root: DataRoot,
}

impl WorldServer for Served {
    fn port(&self) -> u16 {
        self.server.port()
    }

    fn protocol(&self) -> i32 {
        self.server.protocol()
    }

    fn running(&self) -> bool {
        self.server.status().phase != super::host::Phase::Exited
    }

    fn players(&self) -> u32 {
        u32::try_from(self.server.status().players.len()).unwrap_or(u32::MAX)
    }

    fn set_members(&self, members: &[Nick]) -> LauncherResult<()> {
        super::world::write_whitelist(&self.root, self.server.world_id(), members)?;
        self.server.command("whitelist reload")
    }

    fn kick(&self, nick: &Nick) -> LauncherResult<()> {
        // A nick is letters, digits and underscores: a single safe word.
        self.server.command(&format!("kick {}", nick.as_str()))
    }

    fn admitted(&self, account: &str, nick: &Nick) -> LauncherResult<()> {
        let now = chrono::Utc::now().timestamp();
        players::admitted(&self.root, self.server.world_id(), account, nick, now).map(|_| ())
    }
}

/// Why hosting ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Ending {
    /// The player stopped it.
    Stopped,
    /// The world server exited.
    ServerExited,
    /// The service ended the session: another start, a lapsed lease or a
    /// restored database.
    SessionLost,
    /// The room is gone or this account no longer owns it.
    RoomGone,
}

/// What the hosting panel shows. Nicks stay in memory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostingStatus {
    pub room: String,
    pub session: String,
    pub players: Vec<String>,
    pub ended: Option<Ending>,
    /// The lease lapsed while the server runs; a new session is being opened.
    pub reopening: bool,
}

struct Shared {
    client: Arc<WorldsClient>,
    server: Arc<dyn WorldServer>,
    room: String,
    session: Session,
    /// Open admissions by ID, with their nick.
    admissions: Mutex<HashMap<String, Nick>>,
    members: Mutex<BTreeSet<Nick>>,
    ended: Mutex<Option<Ending>>,
    stop: Notify,
    on_change: Arc<dyn Fn() + Send + Sync>,
    endpoint: Endpoint,
    tunnel: Mutex<Option<TunnelHost>>,
}

impl Shared {
    /// Ends hosting once: no new streams, and the endpoint closes, which cuts
    /// off every guest in the tunnel.
    fn end(&self, ending: Ending) -> bool {
        let mut ended = lock(&self.ended);
        if ended.is_some() {
            return false;
        }
        *ended = Some(ending);
        drop(ended);
        lock(&self.tunnel).take();
        let endpoint = self.endpoint.clone();
        tokio::spawn(async move { endpoint.close().await });
        self.stop.notify_waiters();
        (self.on_change)();
        true
    }

    fn ended(&self) -> Option<Ending> {
        *lock(&self.ended)
    }

    /// Applies a renewal: the whitelist, then the revoked admissions.
    fn apply(&self, renewal: &Renewal) {
        let members: BTreeSet<Nick> = renewal
            .members
            .iter()
            .filter_map(|n| Nick::parse(n).ok())
            .collect();
        let changed = {
            let mut current = lock(&self.members);
            let changed = *current != members;
            *current = members.clone();
            changed
        };
        if changed {
            let list: Vec<Nick> = members.into_iter().collect();
            let _ = self.server.set_members(&list);
        }
        let revoked: Vec<Nick> = {
            let admissions = lock(&self.admissions);
            renewal
                .revoked
                .iter()
                .filter_map(|id| admissions.get(id).cloned())
                .collect()
        };
        for nick in &revoked {
            let _ = self.server.kick(nick);
        }
        if changed || !revoked.is_empty() {
            (self.on_change)();
        }
    }
}

fn refusal_of(problem: Problem) -> Refusal {
    match problem {
        Problem::TicketInvalid => Refusal::TicketInvalid,
        Problem::NotMember | Problem::Banned | Problem::Forbidden | Problem::NickUnfit => {
            Refusal::NotMember
        }
        Problem::AlreadyConnected => Refusal::AlreadyConnected,
        Problem::RoomFull => Refusal::RoomFull,
        Problem::WrongSession | Problem::SessionEnded => Refusal::WrongSession,
        _ => Refusal::Unavailable,
    }
}

/// The room's admission check for the tunnel host.
struct RoomAdmit(Arc<Shared>);

impl Admit for RoomAdmit {
    fn admit<'a>(
        &'a self,
        peer: EndpointId,
        request: &'a AdmissionRequest,
    ) -> BoxFuture<'a, Result<Admitted, Refusal>> {
        Box::pin(async move {
            let shared = &self.0;
            if shared.ended().is_some() {
                return Err(Refusal::ConnectionClosing);
            }
            if request.session_id != shared.session.id
                || request.expected_host_epoch != shared.session.epoch.to_string()
            {
                return Err(Refusal::WrongSession);
            }
            let admission = shared
                .client
                .admit(&shared.session.id, &request.ticket, &peer.to_string())
                .await
                .map_err(refusal_of)?;
            let release = || {
                let (client, session, id) = (
                    Arc::clone(&shared.client),
                    shared.session.id.clone(),
                    admission.id.clone(),
                );
                tokio::spawn(async move { client.release(&session, &id).await });
            };
            let Ok(nick) = Nick::parse(&admission.nick) else {
                release();
                return Err(Refusal::NotMember);
            };
            // A member who joined since the last renewal, or under a new nick.
            let added = lock(&shared.members).insert(nick.clone());
            if added {
                let list: Vec<Nick> = lock(&shared.members).iter().cloned().collect();
                let _ = shared.server.set_members(&list);
            }
            let (server, account, moved) = (
                Arc::clone(&shared.server),
                admission.account.clone(),
                nick.clone(),
            );
            let ready = tokio::task::spawn_blocking(move || server.admitted(&account, &moved))
                .await
                .ok()
                .and_then(Result::ok);
            if ready.is_none() {
                release();
                return Err(Refusal::HostBusy);
            }
            lock(&shared.admissions).insert(admission.id.clone(), nick.clone());
            (shared.on_change)();
            Ok(Admitted {
                connection_id: admission.id,
                nick,
            })
        })
    }

    fn released(&self, connection_id: &str) {
        let shared = &self.0;
        if lock(&shared.admissions).remove(connection_id).is_none() {
            return;
        }
        (shared.on_change)();
        let (client, session, id) = (
            Arc::clone(&shared.client),
            shared.session.id.clone(),
            connection_id.to_owned(),
        );
        tokio::spawn(async move {
            let _ = client.release(&session, &id).await;
        });
    }
}

/// Binds a fresh endpoint for a new session of the room.
pub type Rebind = Arc<dyn Fn() -> BoxFuture<'static, LauncherResult<Endpoint>> + Send + Sync>;

/// What every session of the room is opened with.
struct Opening {
    client: Arc<WorldsClient>,
    server: Arc<dyn WorldServer>,
    room: String,
    max_players: u32,
    on_change: Arc<dyn Fn() + Send + Sync>,
    rebind: Rebind,
}

/// When a lease was asked for: it lasts [`LEASE`] from the start of the
/// request that got it, so a late answer never adds time. Both clocks count:
/// the monotonic one may stand still while the computer sleeps, and a wall
/// clock turned back fails closed.
#[derive(Debug, Clone, Copy)]
struct Granted {
    at: Instant,
    wall: SystemTime,
}

impl Granted {
    fn now() -> Self {
        Self {
            at: Instant::now(),
            wall: SystemTime::now(),
        }
    }

    /// Time the lease has left; zero once it lapsed.
    fn left(&self) -> Duration {
        let wall = match self.wall.elapsed() {
            Ok(passed) => passed,
            Err(back) if back.duration() <= CLOCK_SLACK => Duration::ZERO,
            Err(_) => return Duration::ZERO,
        };
        LEASE.saturating_sub(self.at.elapsed().max(wall))
    }
}

/// The room across its sessions. A lease that lapsed while the server kept
/// running (the computer slept, the service could not be reached in time) is
/// never renewed: guests are cut off at once and a new session is opened on
/// a fresh endpoint, which the guests reconnect to.
struct Room {
    current: Mutex<Arc<Shared>>,
    reopening: Mutex<bool>,
    stopped: Mutex<bool>,
    /// Set when hosting ended between sessions.
    ended: Mutex<Option<Ending>>,
    stop: Notify,
}

impl Room {
    fn current(&self) -> Arc<Shared> {
        Arc::clone(&lock(&self.current))
    }

    fn finish(&self, ending: Ending, on_change: &(dyn Fn() + Send + Sync)) {
        lock(&self.ended).get_or_insert(ending);
        *lock(&self.reopening) = false;
        on_change();
    }
}

/// A hosted room; dropping it stops accepting guests (call [`RoomHost::stop`]
/// to end the session at the service too).
pub struct RoomHost {
    room: Arc<Room>,
    keeper: JoinHandle<()>,
}

impl Drop for RoomHost {
    fn drop(&mut self) {
        self.keeper.abort();
        lock(&self.room.current().tunnel).take();
    }
}

/// Starts hosting `room` on `endpoint` (bound with `accept`) in front of
/// `server`, which must be ready; `rebind` binds the endpoint of a later
/// session after a lapsed lease.
pub async fn start(
    client: Arc<WorldsClient>,
    server: Arc<dyn WorldServer>,
    endpoint: Endpoint,
    rebind: Rebind,
    room: &str,
    max_players: u32,
    on_change: Arc<dyn Fn() + Send + Sync>,
) -> LauncherResult<RoomHost> {
    if !server.running() {
        return Err(LauncherError::new(
            ErrorCode::GameExitedWithError,
            "The world server is not running.",
        ));
    }
    let opening = Opening {
        client,
        server,
        room: room.to_owned(),
        max_players: max_players.clamp(1, 20),
        on_change,
        rebind,
    };
    let (shared, granted) = open(&opening, endpoint).await?;
    let room = Arc::new(Room {
        current: Mutex::new(shared),
        reopening: Mutex::new(false),
        stopped: Mutex::new(false),
        ended: Mutex::new(None),
        stop: Notify::new(),
    });
    let keeper = tokio::spawn(run(Arc::clone(&room), opening, granted));
    Ok(RoomHost { room, keeper })
}

/// Opens a session on `endpoint`: the whitelist before the first guest (the
/// first renewal names the members), then the tunnel.
async fn open(opening: &Opening, endpoint: Endpoint) -> Result<(Arc<Shared>, Granted), Problem> {
    let session = match opening
        .client
        .start_session(
            &opening.room,
            &endpoint.id().to_string(),
            opening.max_players,
        )
        .await
    {
        Ok(session) => session,
        Err(problem) => {
            endpoint.close().await;
            return Err(problem);
        }
    };
    let shared = Arc::new(Shared {
        client: Arc::clone(&opening.client),
        server: Arc::clone(&opening.server),
        room: opening.room.clone(),
        session,
        admissions: Mutex::new(HashMap::new()),
        members: Mutex::new(BTreeSet::new()),
        ended: Mutex::new(None),
        stop: Notify::new(),
        on_change: Arc::clone(&opening.on_change),
        endpoint: endpoint.clone(),
        tunnel: Mutex::new(None),
    });
    let granted = Granted::now();
    match shared
        .client
        .renew(&shared.session.id, shared.server.players())
        .await
    {
        Ok(renewal) if granted.left() > Duration::ZERO => shared.apply(&renewal),
        failed => {
            let _ = shared.client.stop_session(&shared.session.id).await;
            endpoint.close().await;
            return Err(failed.err().unwrap_or(Problem::Network));
        }
    }
    let tunnel = crate::tunnel::host::serve(
        endpoint,
        Arc::new(RoomAdmit(Arc::clone(&shared))),
        opening.server.port(),
        opening.server.protocol(),
    );
    *lock(&shared.tunnel) = Some(tunnel);
    Ok((shared, granted))
}

/// Keeps the room's sessions: renews each lease, and opens a new session
/// after a lapsed one while the server runs.
async fn run(room: Arc<Room>, opening: Opening, mut granted: Granted) {
    loop {
        if !keep(&room, room.current(), granted).await {
            return;
        }
        let mut wait = REOPEN_FIRST;
        loop {
            if *lock(&room.stopped) {
                return room.finish(Ending::Stopped, opening.on_change.as_ref());
            }
            if !opening.server.running() {
                return room.finish(Ending::ServerExited, opening.on_change.as_ref());
            }
            let opened = match (opening.rebind)().await {
                Ok(endpoint) => open(&opening, endpoint).await,
                Err(_) => Err(Problem::Network),
            };
            match opened {
                Ok((fresh, at)) => {
                    *lock(&room.current) = Arc::clone(&fresh);
                    if *lock(&room.stopped) && fresh.end(Ending::Stopped) {
                        let _ = fresh.client.stop_session(&fresh.session.id).await;
                    }
                    *lock(&room.reopening) = false;
                    (opening.on_change)();
                    granted = at;
                    break;
                }
                Err(Problem::Network | Problem::Server | Problem::TooMany) => {
                    tokio::select! {
                        () = tokio::time::sleep(wait) => {}
                        () = room.stop.notified() => {}
                    }
                    wait = (wait * 2).min(REOPEN_MOST);
                }
                Err(Problem::NotFound | Problem::Forbidden) => {
                    return room.finish(Ending::RoomGone, opening.on_change.as_ref());
                }
                Err(_) => return room.finish(Ending::SessionLost, opening.on_change.as_ref()),
            }
        }
    }
}

/// Renews the session's lease until the session ends. True when it ended
/// because the lease lapsed while the server still runs; the room is then
/// marked reopening before guests are cut off.
async fn keep(room: &Room, shared: Arc<Shared>, mut granted: Granted) -> bool {
    let lapsed = |shared: &Shared| {
        if !shared.server.running() {
            shared.end(Ending::ServerExited);
            return false;
        }
        *lock(&room.reopening) = true;
        if !shared.end(Ending::SessionLost) {
            *lock(&room.reopening) = false;
            return false;
        }
        // The service ends it by itself; telling it frees the room sooner.
        let (client, session) = (Arc::clone(&shared.client), shared.session.id.clone());
        tokio::spawn(async move { client.stop_session(&session).await });
        true
    };
    loop {
        tokio::select! {
            () = tokio::time::sleep(RENEW_EVERY.min(granted.left())) => {}
            () = shared.stop.notified() => return false,
        }
        if shared.ended().is_some() {
            return false;
        }
        if !shared.server.running() {
            shared.end(Ending::ServerExited);
            let _ = shared.client.stop_session(&shared.session.id).await;
            return false;
        }
        if granted.left().is_zero() {
            return lapsed(&shared);
        }
        let asked = Granted::now();
        let renewed = tokio::time::timeout(
            granted.left(),
            shared
                .client
                .renew(&shared.session.id, shared.server.players()),
        )
        .await;
        match renewed {
            Ok(Ok(renewal)) => {
                granted = asked;
                shared.apply(&renewal);
            }
            Ok(Err(Problem::SessionEnded | Problem::WrongSession | Problem::Device)) => {
                // Ended at the service while this lease still ran: another
                // start of the room, or a restored database.
                shared.end(Ending::SessionLost);
                return false;
            }
            Ok(Err(Problem::NotFound | Problem::Forbidden | Problem::NotHosting)) => {
                shared.end(Ending::RoomGone);
                return false;
            }
            // Network trouble: the next renewal may still be in time.
            Ok(Err(_)) => {}
            // No answer before the lease ran out.
            Err(_) => return lapsed(&shared),
        }
    }
}

impl RoomHost {
    pub fn status(&self) -> HostingStatus {
        let shared = self.room.current();
        HostingStatus {
            room: shared.room.clone(),
            session: shared.session.id.clone(),
            players: lock(&shared.admissions)
                .values()
                .map(|n| n.as_str().to_owned())
                .collect(),
            ended: self.ended(),
            reopening: *lock(&self.room.reopening),
        }
    }

    /// Why hosting ended; none while a new session is being opened.
    pub fn ended(&self) -> Option<Ending> {
        if let Some(ending) = *lock(&self.room.ended) {
            return Some(ending);
        }
        if *lock(&self.room.reopening) {
            return None;
        }
        self.room.current().ended()
    }

    /// The current session's endpoint.
    pub fn endpoint(&self) -> Endpoint {
        self.room.current().endpoint.clone()
    }

    /// Ends the session: no new guests, everyone in the tunnel is cut off and
    /// the service is told. The world server is the caller's to stop.
    pub async fn stop(&self) {
        *lock(&self.room.stopped) = true;
        self.room.stop.notify_waiters();
        let shared = self.room.current();
        if shared.end(Ending::Stopped) {
            let _ = shared.client.stop_session(&shared.session.id).await;
        }
        if *lock(&self.room.reopening) {
            lock(&self.room.ended).get_or_insert(Ending::Stopped);
            *lock(&self.room.reopening) = false;
        }
    }
}

#[cfg(test)]
mod tests;
