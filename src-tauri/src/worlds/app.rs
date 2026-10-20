//! Worlds with friends in the running launcher (plan W8). One [`Worlds`]
//! keeps what this computer does for a room: getting a world ready to host
//! (its server and Java), hosting it, or being a guest in someone's room. The
//! interface gets a [`WorldsView`] in the `worlds://state` event and asks for
//! rooms, members and invites with commands; the game is started into the
//! room with `worlds_play`.
//!
//! Only one room at a time: hosting and being a guest exclude each other, as
//! one hosting permit allows one world server.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use iroh::RelayMode;
use serde::Serialize;

use super::hosting::{self, Ending, RoomHost, Served, WorldServer};
use super::identity::Nick;
use super::joining::{self, RoomGuest, Route, Visit};
use super::properties::WorldSettings;
use super::service::{Problem, Room, Token, WorldsClient};
use super::{host, install, safety, tuple, world};
use crate::backup::store::{Reason, SnapshotInfo};
use crate::catalog::source::MetadataService;
use crate::download::{CancelToken, DownloadProgress, Executor, ExecutorConfig, ProgressSink};
use crate::error::{ErrorCode, LauncherError, LauncherResult};
use crate::install::service::{BoxFuture, LiveSource};
use crate::net::PublicClient;
use crate::runtime::RuntimeInstaller;
use crate::secrets::SecretString;
use crate::state::AppState;
use crate::storage::models::OperationKind;
use crate::storage::paths::{DataRoot, InstanceId, WorldId};
use crate::sync::lock;
use crate::tunnel::endpoint::{Reach, bind};
use crate::tunnel::frame::Refusal;

pub const STATE_EVENT: &str = "worlds://state";
/// Progress is told at most this often.
const PROGRESS_EVERY: Duration = Duration::from_millis(250);

/// What getting ready does now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    /// Downloading or checking the world's server and its Java.
    Server,
    /// The world server starts and loads the world.
    Starting,
    /// The room's session starts and the tunnel opens.
    Opening,
    /// A guest asks for a place and connects to the host.
    Joining,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preparing {
    pub room: String,
    pub stage: Stage,
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostingView {
    pub room: String,
    pub world: WorldId,
    pub phase: host::Phase,
    /// Players on the server now, the host included.
    pub players: Vec<String>,
    pub ended: Option<Ending>,
    /// Whether the last stop saved the world cleanly, once the server exited.
    pub clean: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GuestView {
    pub room: String,
    pub connected: bool,
    /// Why the host refused the last login, if it did.
    pub refused: Option<Refusal>,
    /// Straight to the host or through the relay, once chosen.
    pub path: Option<joining::PathKind>,
}

/// What the interface shows about this computer's room.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorldsView {
    pub preparing: Option<Preparing>,
    pub hosting: Option<HostingView>,
    pub guest: Option<GuestView>,
    /// Why the last start or join failed, as a problem name or error code.
    pub failed: Option<String>,
    /// A world is being backed up, restored, imported or exported.
    pub maintenance: bool,
}

/// A world kept on this computer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalWorld {
    pub id: WorldId,
    pub name: String,
    pub minecraft: String,
    pub eula_accepted: bool,
    pub server_installed: bool,
    pub settings: WorldSettings,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub last_run: Option<world::LastRun>,
    /// Whether the world has been played: only then is there a save to back
    /// up or export.
    pub played: bool,
}

impl From<world::WorldFile> for LocalWorld {
    fn from(world: world::WorldFile) -> Self {
        Self {
            id: world.id,
            name: world.name,
            minecraft: world.minecraft,
            eula_accepted: world.eula_accepted_at.is_some(),
            server_installed: world.server.is_some(),
            settings: world.settings,
            created_at: world.created_at,
            last_run: world.last_run,
            played: false,
        }
    }
}

struct Hosting {
    room: String,
    server: Arc<host::HostedServer>,
    host: RoomHost,
}

#[derive(Default)]
struct Inner {
    preparing: Option<Preparing>,
    cancel: Option<CancelToken>,
    hosting: Option<Arc<Hosting>>,
    guest: Option<Arc<RoomGuest>>,
    failed: Option<String>,
    told: Option<Instant>,
    /// A backup, restore, import or export runs ([`Worlds::claim`]).
    maintenance: bool,
}

type Emit = Box<dyn Fn(&WorldsView) + Send + Sync>;

pub struct Worlds {
    pub client: Arc<WorldsClient>,
    root: Option<DataRoot>,
    metadata: Option<Arc<MetadataService<PublicClient>>>,
    source: Option<Arc<LiveSource>>,
    runtimes: Option<RuntimeInstaller>,
    executor: Option<Executor>,
    inner: Mutex<Inner>,
    emit: Emit,
}

fn unavailable() -> LauncherError {
    LauncherError::new(
        ErrorCode::StorageUnavailable,
        "Launcher data is unavailable.",
    )
}

fn busy() -> LauncherError {
    LauncherError::new(
        ErrorCode::AppBusy,
        "This computer is already in a room. Leave it first.",
    )
}

fn changing() -> LauncherError {
    LauncherError::new(
        ErrorCode::AppBusy,
        "A world is being backed up or copied. Wait for it to finish.",
    )
}

fn hosted() -> LauncherError {
    LauncherError::new(
        ErrorCode::AppBusy,
        "This world is being hosted. Stop hosting it first.",
    )
}

/// Runs file work off the async threads.
async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> LauncherResult<T> + Send + 'static,
) -> LauncherResult<T> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|_| LauncherError::internal("The world change stopped."))?
}

/// The one backup, restore, import or export at a time; released on drop.
/// A quiet one (tidying up while listing) is not shown.
struct Claim<'a> {
    worlds: &'a Worlds,
    quiet: bool,
}

impl Drop for Claim<'_> {
    fn drop(&mut self) {
        lock(&self.worlds.inner).maintenance = false;
        if !self.quiet {
            self.worlds.tell();
        }
    }
}

/// The name a failure is shown by: the service's problem, else the error code.
fn failure_name(error: &LauncherError) -> String {
    let problem = error
        .safe_details
        .as_ref()
        .and_then(|d| d.get("worldsProblem"))
        .and_then(|p| match p {
            crate::error::SafeValue::Text(name) => Some(name.as_str()),
            _ => None,
        });
    problem.map_or_else(
        || {
            serde_json::to_value(error.code)
                .ok()
                .and_then(|v| v.as_str().map(str::to_owned))
                .unwrap_or_default()
        },
        str::to_owned,
    )
}

/// The relay both sides use when no direct path is found.
fn relay_reach() -> LauncherResult<Reach> {
    Ok(Reach::Network(RelayMode::custom([joining::relay_url()?])))
}

/// The game name of the signed-in launcher account.
pub fn nick_of(state: &AppState) -> Result<Nick, Problem> {
    let account = state
        .accounts_file()
        .launcher_account
        .ok_or(Problem::SignedOut)?;
    Nick::parse(&account.nick).map_err(|_| Problem::NickUnfit)
}

impl Worlds {
    pub fn new(
        app_version: &str,
        root: Option<DataRoot>,
        metadata: Option<Arc<MetadataService<PublicClient>>>,
        source: Option<Arc<LiveSource>>,
        token: Arc<dyn Token>,
        emit: Emit,
    ) -> Result<Self, String> {
        let client = WorldsClient::new(
            app_version,
            super::service::base(),
            Arc::new(crate::auth::vault::KeyringStore),
            token,
            &device_name(),
        )?;
        let (runtimes, executor) = match &root {
            Some(root) => {
                let transport = crate::download::transport::HttpTransport::public(app_version)
                    .map_err(|e| e.message)?;
                (
                    Some(RuntimeInstaller::new(
                        root.clone(),
                        transport.clone(),
                        ExecutorConfig::default(),
                    )),
                    Some(Executor::new(
                        transport,
                        root.clone(),
                        ExecutorConfig::default(),
                    )),
                )
            }
            None => (None, None),
        };
        Ok(Self {
            client: Arc::new(client),
            root,
            metadata,
            source,
            runtimes,
            executor,
            inner: Mutex::new(Inner::default()),
            emit,
        })
    }

    fn root(&self) -> LauncherResult<&DataRoot> {
        self.root.as_ref().ok_or_else(unavailable)
    }

    pub fn view(&self) -> WorldsView {
        view_of(&lock(&self.inner))
    }

    fn tell(&self) {
        let view = {
            let mut inner = lock(&self.inner);
            inner.told = Some(Instant::now());
            view_of(&inner)
        };
        (self.emit)(&view);
    }

    fn on_change(self: &Arc<Self>) -> Arc<dyn Fn() + Send + Sync> {
        let worlds = Arc::downgrade(self);
        Arc::new(move || {
            if let Some(worlds) = worlds.upgrade() {
                worlds.tell();
            }
        })
    }

    fn set_stage(&self, room: &str, stage: Stage) {
        lock(&self.inner).preparing = Some(Preparing {
            room: room.to_owned(),
            stage,
            downloaded_bytes: 0,
            total_bytes: 0,
        });
        self.tell();
    }

    /// Claims the one room this computer may be in, for getting ready.
    fn begin(&self, room: &str, stage: Stage) -> LauncherResult<CancelToken> {
        let cancel = CancelToken::new();
        {
            let mut inner = lock(&self.inner);
            if inner.preparing.is_some() || inner.hosting.is_some() || inner.guest.is_some() {
                return Err(busy());
            }
            if inner.maintenance {
                return Err(changing());
            }
            inner.preparing = Some(Preparing {
                room: room.to_owned(),
                stage,
                downloaded_bytes: 0,
                total_bytes: 0,
            });
            inner.cancel = Some(cancel.clone());
            inner.failed = None;
        }
        self.tell();
        Ok(cancel)
    }

    fn finish(&self, result: &LauncherResult<()>) {
        {
            let mut inner = lock(&self.inner);
            inner.preparing = None;
            inner.cancel = None;
            inner.failed = result.as_ref().err().map(failure_name);
        }
        self.tell();
    }

    /// Claims the one change of a world's files. Refused while a room gets
    /// ready (its world may be the one) and while `world` is hosted.
    fn claim(&self, world: Option<WorldId>) -> LauncherResult<Claim<'_>> {
        self.claim_as(world, false)
    }

    fn claim_as(&self, world: Option<WorldId>, quiet: bool) -> LauncherResult<Claim<'_>> {
        {
            let mut inner = lock(&self.inner);
            if inner.maintenance || inner.preparing.is_some() {
                return Err(changing());
            }
            if let (Some(world), Some(hosting)) = (world, inner.hosting.as_ref()) {
                let server = hosting.server.status();
                // `clean` is set once the exit is written down.
                if server.world_id == world && server.clean.is_none() {
                    return Err(hosted());
                }
            }
            inner.maintenance = true;
        }
        if !quiet {
            self.tell();
        }
        Ok(Claim {
            worlds: self,
            quiet,
        })
    }

    /// Stops getting ready (a download in progress ends at once).
    pub fn cancel(&self) {
        if let Some(cancel) = lock(&self.inner).cancel.as_ref() {
            cancel.cancel();
        }
    }

    /// The worlds kept on this computer, newest first.
    /// A backup, restore or import an exit cut short is finished first.
    pub fn local(&self) -> LauncherResult<Vec<LocalWorld>> {
        let root = self.root()?;
        let claim = self.claim_as(None, true).ok();
        let mut worlds: Vec<LocalWorld> = world::list(root)
            .into_iter()
            .filter_map(|mut found| {
                if claim.is_some()
                    && found.mutation.is_some()
                    && let Ok(false) = safety::recover(root, &mut found)
                {
                    return None;
                }
                let played = safety::save_dir(root, found.id).is_dir();
                Some(LocalWorld {
                    played,
                    ..LocalWorld::from(found)
                })
            })
            .collect();
        drop(claim);
        worlds.sort_by_key(|w| std::cmp::Reverse(w.created_at));
        Ok(worlds)
    }

    /// The world's backups, newest first.
    pub fn backups(&self, world: WorldId) -> LauncherResult<Vec<SnapshotInfo>> {
        let root = self.root()?;
        world::load(root, world)?;
        Ok(safety::list(root, world))
    }

    /// Backs up the stopped world now.
    pub async fn back_up(&self, world: WorldId) -> LauncherResult<SnapshotInfo> {
        let _claim = self.claim(Some(world))?;
        let root = self.root()?.clone();
        blocking(move || safety::backup(&root, world, Reason::Manual))
            .await?
            .ok_or_else(|| LauncherError::internal("The backup was not made."))
    }

    /// Puts a backup in place of the stopped world, backing it up first.
    pub async fn restore(&self, world: WorldId, backup: String) -> LauncherResult<()> {
        let _claim = self.claim(Some(world))?;
        let root = self.root()?.clone();
        blocking(move || safety::restore(&root, world, &backup)).await
    }

    /// Writes the stopped world as a zip; returns its size.
    pub async fn export(&self, world: WorldId, destination: PathBuf) -> LauncherResult<u64> {
        let _claim = self.claim(Some(world))?;
        let root = self.root()?.clone();
        blocking(move || safety::export(&root, world, &destination)).await
    }

    /// The installations whose single-player worlds can be imported.
    pub fn importable(&self, state: &AppState) -> LauncherResult<Vec<safety::Importable>> {
        let instances: Vec<_> = state
            .instance_ids()
            .into_iter()
            .filter_map(|id| state.instance(id))
            .collect();
        Ok(safety::importable(self.root()?, &instances))
    }

    /// Makes a world from a copy of a single-player world and its room. The
    /// installation must not be running; the original stays as it is. A room
    /// the service refuses leaves no world behind.
    pub async fn import(
        &self,
        state: &AppState,
        instance: InstanceId,
        save: String,
        name: String,
        description: &str,
    ) -> LauncherResult<Room> {
        check_description(description)?;
        let root = self.root()?.clone();
        let instance = state.instance(instance).ok_or_else(|| {
            LauncherError::new(
                ErrorCode::InstanceNotFound,
                "The installation no longer exists.",
            )
        })?;
        let created = {
            let _claim = self.claim(None)?;
            let permit = state.activity().try_acquire(
                OperationKind::Backup,
                Some(instance.id),
                uuid::Uuid::new_v4().to_string(),
            )?;
            let root = root.clone();
            let created = blocking(move || safety::import(&root, &instance, &save, &name)).await;
            drop(permit);
            created?
        };
        self.open_room(&root, &created, description).await
    }

    /// Creates a world on this computer and its room. A room the service
    /// refuses leaves no world behind.
    pub async fn create(
        &self,
        name: &str,
        description: &str,
        minecraft: &str,
        settings: WorldSettings,
    ) -> LauncherResult<Room> {
        let root = self.root()?.clone();
        check_description(description)?;
        let created = {
            let (root, name, minecraft) = (root.clone(), name.to_owned(), minecraft.to_owned());
            tokio::task::spawn_blocking(move || world::create(&root, &name, &minecraft, settings))
                .await
                .map_err(|_| LauncherError::internal("Creating the world stopped."))??
        };
        self.open_room(&root, &created, description).await
    }

    /// The room of a world just made here; refused, the world goes again.
    async fn open_room(
        &self,
        root: &DataRoot,
        created: &world::WorldFile,
        description: &str,
    ) -> LauncherResult<Room> {
        match self
            .client
            .create_room(
                &created.name,
                description.trim(),
                &created.id.to_string(),
                &created.minecraft,
            )
            .await
        {
            Ok(room) => Ok(room),
            Err(problem) => {
                let _ = std::fs::remove_dir_all(root.world_dir(created.id));
                Err(problem.into())
            }
        }
    }

    pub fn accept_eula(&self, world: WorldId) -> LauncherResult<LocalWorld> {
        world::accept_eula(self.root()?, world).map(LocalWorld::from)
    }

    /// Hosts `room` from its world on this computer: gets the server and Java
    /// ready, starts the server, then the room's session and the tunnel.
    pub async fn host(self: &Arc<Self>, state: &AppState, room: &str) -> LauncherResult<()> {
        let cancel = self.begin(room, Stage::Server)?;
        let result = self.host_inner(state, room, &cancel).await;
        self.finish(&result);
        result
    }

    async fn host_inner(
        self: &Arc<Self>,
        state: &AppState,
        room: &str,
        cancel: &CancelToken,
    ) -> LauncherResult<()> {
        let nick = nick_of(state)?;
        let room = self.client.room(room).await?;
        if room.role != "owner" {
            return Err(Problem::Forbidden.into());
        }
        let world_id = room
            .world
            .as_deref()
            .and_then(|w| w.parse::<WorldId>().ok())
            .ok_or_else(elsewhere)?;
        let root = self.root()?.clone();
        if world::load(&root, world_id).is_err() {
            return Err(elsewhere());
        }
        let (metadata, source, runtimes, executor) =
            match (&self.metadata, &self.source, &self.runtimes, &self.executor) {
                (Some(m), Some(s), Some(r), Some(e)) => (m, s, r, e),
                _ => return Err(unavailable()),
            };
        let sink = Progress {
            worlds: Arc::clone(self),
        };
        let (prepared, java) = install::prepare(
            &root,
            metadata,
            source.as_ref(),
            runtimes,
            executor,
            world_id,
            state.java_provider(),
            cancel,
            &sink,
        )
        .await?;

        self.set_stage(&room.id, Stage::Starting);
        let operation_id = uuid::Uuid::new_v4().to_string();
        let permit = state
            .activity()
            .try_acquire_hosting(world_id.uuid(), operation_id.clone())?;
        let physical = crate::validation::MemoryPolicy::detect()
            .map(|m| m.physical_mib * 1024 * 1024)
            .unwrap_or(0);
        let game_heap = state
            .instance_ids()
            .into_iter()
            .filter_map(|id| state.instance(id))
            .find(|i| i.world_room.as_deref() == Some(room.id.as_str()))
            .map_or(0, |i| i.max_heap_mib);
        let members = [nick];
        let launch = host::Launch {
            root: &root,
            world_id,
            java: &java,
            server_heap_mib: super::memory::DEFAULT_SERVER_HEAP_MIB,
            game_heap_mib: game_heap,
            physical_memory_bytes: physical,
            members: &members,
            operation_id,
        };
        let server = Arc::new(host::start(&launch, permit, self.on_change())?);
        let ready = {
            let server = Arc::clone(&server);
            tokio::task::spawn_blocking(move || server.wait_ready(host::STARTUP_DEADLINE))
                .await
                .map_err(|_| LauncherError::internal("Waiting for the world stopped."))?
        };
        if let Err(error) = ready {
            stop_server(Arc::clone(&server)).await;
            return Err(error);
        }

        self.set_stage(&room.id, Stage::Opening);
        let opened = async {
            let endpoint = bind(relay_reach()?, true).await?;
            let served = Arc::new(Served {
                server: Arc::clone(&server),
                root: root.clone(),
            }) as Arc<dyn WorldServer>;
            hosting::start(
                Arc::clone(&self.client),
                served,
                endpoint,
                &room.id,
                u32::from(prepared.settings.max_players),
                self.on_change(),
            )
            .await
        }
        .await;
        let room_host = match opened {
            Ok(room_host) => room_host,
            Err(error) => {
                stop_server(Arc::clone(&server)).await;
                return Err(error);
            }
        };
        let hosting = Arc::new(Hosting {
            room: room.id.clone(),
            server,
            host: room_host,
        });
        lock(&self.inner).hosting = Some(Arc::clone(&hosting));
        tokio::spawn(watch_hosting(Arc::downgrade(self), hosting));
        Ok(())
    }

    /// Stops hosting: the session ends, the tunnel closes and the server
    /// saves the world and exits; then the world is backed up.
    pub async fn stop_hosting(&self) {
        self.stop_hosting_and(true).await;
    }

    async fn stop_hosting_and(&self, back_up: bool) {
        let hosting = lock(&self.inner).hosting.clone();
        if let Some(hosting) = hosting {
            hosting.host.stop().await;
            stop_server(Arc::clone(&hosting.server)).await;
            {
                let mut inner = lock(&self.inner);
                if inner
                    .hosting
                    .as_ref()
                    .is_some_and(|h| Arc::ptr_eq(h, &hosting))
                {
                    inner.hosting = None;
                }
            }
            if back_up {
                self.back_up_after_play(hosting.server.status().world_id)
                    .await;
            }
        }
        self.tell();
    }

    /// The backup after a hosting session; an unchanged world makes none. A
    /// failure keeps the world as it is and the earlier backups.
    async fn back_up_after_play(&self, world: WorldId) {
        let (Ok(claim), Ok(root)) = (self.claim(None), self.root().cloned()) else {
            return;
        };
        let _ = blocking(move || safety::backup(&root, world, Reason::AfterPlay)).await;
        drop(claim);
    }

    /// Joins `room` as a guest: a place is asked for and the tunnel to the
    /// host opened; then the game can be started with [`Self::play_port`].
    pub async fn join(self: &Arc<Self>, state: &AppState, room: &str) -> LauncherResult<()> {
        let _cancel = self.begin(room, Stage::Joining)?;
        let result = self.join_inner(state, room).await;
        self.finish(&result);
        result
    }

    async fn join_inner(self: &Arc<Self>, state: &AppState, room: &str) -> LauncherResult<()> {
        let nick = nick_of(state)?;
        let room = self.client.room(room).await?;
        let tuple = tuple::find(&room.minecraft).ok_or_else(|| {
            LauncherError::invalid_input("Worlds are not available for this Minecraft version yet.")
        })?;
        if !room.hosting {
            return Err(Problem::NotHosting.into());
        }
        let endpoint = bind(relay_reach()?, false).await?;
        let guest = joining::join(
            Arc::clone(&self.client),
            endpoint,
            Visit {
                room: &room,
                nick,
                route: Route::Relay(joining::relay_url()?),
                protocol: tuple.protocol,
                version_name: tuple.minecraft,
            },
            self.on_change(),
        )
        .await?;
        lock(&self.inner).guest = Some(Arc::new(guest));
        Ok(())
    }

    /// Leaves the room this computer is a guest in.
    pub async fn disconnect(&self) {
        let guest = lock(&self.inner).guest.take();
        if let Some(guest) = guest {
            match Arc::try_unwrap(guest) {
                Ok(guest) => guest.leave().await,
                Err(guest) => drop(guest),
            }
        }
        self.tell();
    }

    /// The local port a game playing `room` connects to: the world server
    /// itself on the host, the tunnel's listener on a guest.
    pub fn play_port(&self, room: &str) -> Option<u16> {
        let inner = lock(&self.inner);
        if let Some(hosting) = inner.hosting.as_ref().filter(|h| h.room == room) {
            return (hosting.server.status().phase != host::Phase::Exited)
                .then(|| hosting.server.port());
        }
        inner
            .guest
            .as_ref()
            .filter(|g| g.room() == room && g.connected())
            .map(|g| g.port())
    }

    /// Ends everything at quit: hosting stops and saves the world.
    pub async fn shutdown(&self) {
        self.cancel();
        self.disconnect().await;
        // No backup on the way out: the next session's backup covers it.
        self.stop_hosting_and(false).await;
    }
}

fn check_description(description: &str) -> LauncherResult<()> {
    if description.chars().count() > 500 {
        return Err(LauncherError::invalid_input(
            "A room description has at most 500 characters.",
        ));
    }
    Ok(())
}

/// The room's world is not on this computer.
fn elsewhere() -> LauncherError {
    LauncherError::new(
        ErrorCode::InstanceNotFound,
        "This room's world is kept on another computer.",
    )
}

/// Asks the server to save and stop; it is killed if it does not in time.
async fn stop_server(server: Arc<host::HostedServer>) {
    let _ = tokio::task::spawn_blocking(move || server.stop(host::STOP_GRACE)).await;
}

/// Hosting that ends by itself (the server exited, the session was lost)
/// stops the rest.
async fn watch_hosting(worlds: std::sync::Weak<Worlds>, hosting: Arc<Hosting>) {
    loop {
        tokio::time::sleep(Duration::from_secs(1)).await;
        let Some(worlds) = worlds.upgrade() else {
            return;
        };
        let current = lock(&worlds.inner)
            .hosting
            .as_ref()
            .is_some_and(|h| Arc::ptr_eq(h, &hosting));
        if !current {
            return;
        }
        if let Some(ending) = hosting.host.ended() {
            if ending != Ending::Stopped {
                stop_server(Arc::clone(&hosting.server)).await;
                worlds
                    .back_up_after_play(hosting.server.status().world_id)
                    .await;
            }
            // Kept in the view until the player closes it or starts again.
            worlds.tell();
            return;
        }
    }
}

fn view_of(inner: &Inner) -> WorldsView {
    WorldsView {
        preparing: inner.preparing.clone(),
        hosting: inner.hosting.as_ref().map(|h| {
            let server = h.server.status();
            let status = h.host.status();
            HostingView {
                room: h.room.clone(),
                world: server.world_id,
                phase: server.phase,
                players: server.players,
                ended: status.ended,
                clean: server.clean,
            }
        }),
        guest: inner.guest.as_ref().map(|g| GuestView {
            room: g.room().to_owned(),
            connected: g.connected(),
            refused: g.refused().flatten(),
            path: g.path(),
        }),
        failed: inner.failed.clone(),
        maintenance: inner.maintenance,
    }
}

/// Download progress of the server and Java, told a few times a second.
struct Progress {
    worlds: Arc<Worlds>,
}

impl ProgressSink for Progress {
    fn progress(&self, progress: &DownloadProgress) {
        let due = {
            let mut inner = lock(&self.worlds.inner);
            let Some(preparing) = inner.preparing.as_mut() else {
                return;
            };
            preparing.downloaded_bytes = progress.downloaded_bytes;
            preparing.total_bytes = progress.total_download_bytes;
            inner.told.is_none_or(|t| t.elapsed() >= PROGRESS_EVERY)
        };
        if due {
            self.worlds.tell();
        }
    }
}

/// A computer's name in the device list: its system, never its host name.
fn device_name() -> String {
    let system = sysinfo::System::name().unwrap_or_else(|| std::env::consts::OS.to_owned());
    format!("{system} ({})", std::env::consts::ARCH)
}

/// The launcher account's access token, for worlds.lapi.sh.
pub struct AccountToken<R: tauri::Runtime>(pub tauri::AppHandle<R>);

impl<R: tauri::Runtime> Token for AccountToken<R> {
    fn token(&self) -> BoxFuture<'_, Result<SecretString, Problem>> {
        use crate::launcher_account::{AccountProblem, LauncherAccount};
        use tauri::Manager;
        Box::pin(async move {
            let account = Arc::clone(&self.0.state::<Arc<LauncherAccount>>());
            match account.access_token(&self.0.state::<AppState>()).await {
                Ok(token) => Ok(token),
                Err(AccountProblem::Network) => Err(Problem::Network),
                Err(AccountProblem::Server | AccountProblem::TooMany) => Err(Problem::Server),
                Err(_) => Err(Problem::SignedOut),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failures_are_named_by_problem_or_code() {
        let error: LauncherError = Problem::RoomFull.into();
        assert_eq!(failure_name(&error), "room_full");
        assert_eq!(failure_name(&elsewhere()), "INSTANCE_NOT_FOUND");
    }

    #[test]
    fn the_device_name_is_the_system_not_the_host() {
        let name = device_name();
        assert!(name.ends_with(&format!("({})", std::env::consts::ARCH)));
        if let Some(host) = sysinfo::System::host_name()
            && host.len() > 3
        {
            assert!(!name.contains(&host), "{name}");
        }
    }

    #[test]
    fn an_idle_view_is_empty() {
        let view = view_of(&Inner::default());
        assert_eq!(view, WorldsView::default());
        let json = serde_json::to_value(&view).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "preparing": null,
                "hosting": null,
                "guest": null,
                "failed": null,
                "maintenance": false
            })
        );
    }
}
