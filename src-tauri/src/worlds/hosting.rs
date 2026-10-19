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
//! service says the session is over (another start of the room, a lapsed
//! lease, a restored database). Then the tunnel stops accepting at once.

use std::collections::{BTreeSet, HashMap};
use std::sync::{Arc, Mutex};
use std::time::Duration;

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

/// How often the lease is renewed; it lasts 30 seconds.
pub const RENEW_EVERY: Duration = Duration::from_millis(if cfg!(test) { 300 } else { 10_000 });

/// What the room needs from the running world server.
pub trait WorldServer: Send + Sync + 'static {
    fn port(&self) -> u16;
    fn protocol(&self) -> i32;
    fn running(&self) -> bool;
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

/// A hosted room; dropping it stops accepting guests (call [`RoomHost::stop`]
/// to end the session at the service too).
pub struct RoomHost {
    shared: Arc<Shared>,
    keeper: JoinHandle<()>,
}

impl Drop for RoomHost {
    fn drop(&mut self) {
        self.keeper.abort();
        lock(&self.shared.tunnel).take();
    }
}

/// Starts hosting `room` on `endpoint` (bound with `accept`) in front of
/// `server`, which must be ready. `members` are the room's members now.
pub async fn start(
    client: Arc<WorldsClient>,
    server: Arc<dyn WorldServer>,
    endpoint: Endpoint,
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
    let session = client
        .start_session(room, &endpoint.id().to_string(), max_players.clamp(1, 20))
        .await?;
    let shared = Arc::new(Shared {
        client,
        server: Arc::clone(&server),
        room: room.to_owned(),
        session,
        admissions: Mutex::new(HashMap::new()),
        members: Mutex::new(BTreeSet::new()),
        ended: Mutex::new(None),
        stop: Notify::new(),
        on_change,
        endpoint: endpoint.clone(),
        tunnel: Mutex::new(None),
    });
    // The whitelist before the first guest: the first renewal names the
    // members.
    match shared.client.renew(&shared.session.id).await {
        Ok(renewal) => shared.apply(&renewal),
        Err(problem) => {
            let _ = shared.client.stop_session(&shared.session.id).await;
            return Err(problem.into());
        }
    }
    let tunnel = crate::tunnel::host::serve(
        endpoint,
        Arc::new(RoomAdmit(Arc::clone(&shared))),
        server.port(),
        server.protocol(),
    );
    *lock(&shared.tunnel) = Some(tunnel);
    let keeper = tokio::spawn(keep(Arc::clone(&shared)));
    Ok(RoomHost { shared, keeper })
}

/// Renews the lease until the session ends.
async fn keep(shared: Arc<Shared>) {
    loop {
        tokio::select! {
            () = tokio::time::sleep(RENEW_EVERY) => {}
            () = shared.stop.notified() => return,
        }
        if shared.ended().is_some() {
            return;
        }
        if !shared.server.running() {
            shared.end(Ending::ServerExited);
            let _ = shared.client.stop_session(&shared.session.id).await;
            return;
        }
        match shared.client.renew(&shared.session.id).await {
            Ok(renewal) => shared.apply(&renewal),
            Err(Problem::SessionEnded | Problem::WrongSession | Problem::Device) => {
                shared.end(Ending::SessionLost);
                return;
            }
            Err(Problem::NotFound | Problem::Forbidden | Problem::NotHosting) => {
                shared.end(Ending::RoomGone);
                return;
            }
            // Network trouble: the next renewal may still be in time; if not,
            // the service ends the session and says so then.
            Err(_) => {}
        }
    }
}

impl RoomHost {
    pub fn status(&self) -> HostingStatus {
        HostingStatus {
            room: self.shared.room.clone(),
            session: self.shared.session.id.clone(),
            players: lock(&self.shared.admissions)
                .values()
                .map(|n| n.as_str().to_owned())
                .collect(),
            ended: self.shared.ended(),
        }
    }

    pub fn ended(&self) -> Option<Ending> {
        self.shared.ended()
    }

    pub fn endpoint(&self) -> &Endpoint {
        &self.shared.endpoint
    }

    /// Ends the session: no new guests, everyone in the tunnel is cut off and
    /// the service is told. The world server is the caller's to stop.
    pub async fn stop(&self) {
        if self.shared.end(Ending::Stopped) {
            let _ = self
                .shared
                .client
                .stop_session(&self.shared.session.id)
                .await;
        }
    }
}

#[cfg(test)]
mod tests;
