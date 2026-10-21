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
use super::{host, install, pack, safety, tuple, update, world};
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
    /// The world is backed up before its mods are updated.
    BackingUp,
    /// The world's new mods start on a copy of that backup.
    Trying,
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
    /// The lease lapsed (sleep, the service out of reach) while the server
    /// runs; a new session is being opened and guests will reconnect.
    pub reopening: bool,
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
    /// The tunnel was lost and is being opened again.
    pub reconnecting: bool,
    /// The room no longer lets this computer in (closed, not a member).
    pub gave_up: bool,
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
    /// The mod loader of a world with mods, and how many mods it has.
    pub loader: Option<crate::loader::LoaderChoice>,
    pub mods: u32,
    /// Its generation of mods, and the installation they came from.
    pub generation: Option<u32>,
    pub source: Option<InstanceId>,
    /// Its mods were updated and it has not started with them yet.
    pub update_pending: bool,
}

impl From<world::WorldFile> for LocalWorld {
    fn from(world: world::WorldFile) -> Self {
        let update_pending = update::pending(&world);
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
            loader: world.pack.as_ref().map(|p| p.loader.clone()),
            mods: world
                .pack
                .as_ref()
                .map_or(0, |p| u32::try_from(p.mods().count()).unwrap_or(u32::MAX)),
            generation: world.pack.as_ref().map(|p| p.generation),
            source: world.pack.as_ref().and_then(|p| p.source),
            update_pending,
        }
    }
}

/// An installation a world with mods can be made from (W12).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModSource {
    pub instance_id: InstanceId,
    pub name: String,
    pub minecraft: String,
    pub loader: crate::loader::LoaderChoice,
}

/// The player's installations with a mod loader and release worlds with
/// mods run, by name; room installations are not theirs to pick.
pub fn mod_sources(state: &AppState) -> Vec<ModSource> {
    let mut sources: Vec<ModSource> = state
        .instance_ids()
        .into_iter()
        .filter_map(|id| state.instance(id))
        .filter(|i| i.world_room.is_none())
        .filter_map(|i| {
            let loader = i.loader.clone()?;
            pack::tested(loader.kind, &i.version_id).then(|| ModSource {
                instance_id: i.id,
                name: i.display_name.clone(),
                minecraft: i.version_id.clone(),
                loader,
            })
        })
        .collect();
    sources.sort_by_key(|s| s.name.to_lowercase());
    sources
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
/// Our relay; `relay_only` keeps every path on it, so the other side never
/// learns this computer's addresses (A13).
fn relay_reach(relay_only: bool) -> LauncherResult<Reach> {
    let relays = RelayMode::custom([joining::relay_url()?]);
    Ok(if relay_only {
        Reach::RelayOnly(relays)
    } else {
        Reach::Network(relays)
    })
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

    /// The Minecraft versions a world can be made for, newest first: the
    /// catalog's releases from 1.7.6 on, and its snapshots from 14w17a when
    /// `snapshots`. Empty without a catalog.
    pub async fn versions(&self, snapshots: bool) -> Vec<String> {
        let Some(metadata) = &self.metadata else {
            return Vec::new();
        };
        let (catalog, ..) = metadata.catalog(false).await;
        catalog
            .map(|c| c.versions)
            .unwrap_or_default()
            .into_iter()
            .filter(|e| match e.version_type.as_str() {
                "release" => true,
                "snapshot" => snapshots,
                _ => false,
            })
            .filter(|e| tuple::version(&e.id).is_some())
            .map(|e| e.id)
            .collect()
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
        if claim.is_some() {
            // Copies an update was tried on when the launcher exited.
            for trial in world::trials(root) {
                let _ = std::fs::remove_dir_all(root.world_dir(trial.id));
            }
        }
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
    /// installation must not be running; the original stays as it is. A world
    /// of an installation with a mod loader takes its mods (W12). A room the
    /// service refuses leaves no world behind.
    pub async fn import(
        &self,
        state: &AppState,
        mods: &crate::mods::Mods,
        instance: InstanceId,
        save: String,
        name: String,
        description: &str,
    ) -> LauncherResult<Room> {
        check_description(description)?;
        let root = self.root()?.clone();
        let instance = state.instance(instance).ok_or_else(instance_gone)?;
        let created = {
            let _claim = self.claim(None)?;
            let permit = state.activity().try_acquire(
                OperationKind::Backup,
                Some(instance.id),
                uuid::Uuid::new_v4().to_string(),
            )?;
            let built = match &instance.loader {
                Some(loader) => Some(build_pack(state, mods, &root, &instance, loader).await?),
                None => None,
            };
            let root = root.clone();
            let created = blocking(move || {
                let created = safety::import(&root, &instance, &save, &name)?;
                with_pack(&root, created, built)
            })
            .await;
            drop(permit);
            created?
        };
        self.open_room(&root, &created, description).await
    }

    /// Creates a world on this computer and its room; with `from`, a world
    /// with the mods of that installation, on its Minecraft version (W12). A
    /// room the service refuses leaves no world behind.
    #[allow(clippy::too_many_arguments)]
    pub async fn create(
        &self,
        state: &AppState,
        mods: &crate::mods::Mods,
        name: &str,
        description: &str,
        minecraft: &str,
        settings: WorldSettings,
        from: Option<InstanceId>,
    ) -> LauncherResult<Room> {
        let root = self.root()?.clone();
        check_description(description)?;
        let created = match from {
            None => {
                let (root, name, minecraft) = (root.clone(), name.to_owned(), minecraft.to_owned());
                blocking(move || world::create(&root, &name, &minecraft, settings)).await?
            }
            Some(id) => {
                let instance = state.instance(id).ok_or_else(instance_gone)?;
                let loader = instance.loader.clone().ok_or_else(|| {
                    LauncherError::invalid_input("This installation has no mod loader.")
                })?;
                // Its files are read while the world is made: not while it runs.
                let permit = state.activity().try_acquire(
                    OperationKind::Backup,
                    Some(instance.id),
                    uuid::Uuid::new_v4().to_string(),
                )?;
                let built = build_pack(state, mods, &root, &instance, &loader).await?;
                let (root, name) = (root.clone(), name.to_owned());
                let created = blocking(move || {
                    let created = world::create(&root, &name, &instance.version_id, settings)?;
                    with_pack(&root, created, Some(built))
                })
                .await;
                drop(permit);
                created?
            }
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

    /// The world of a room this computer keeps, if any.
    pub fn local_world_of(&self, room: &Room) -> Option<world::WorldFile> {
        let id = room.world.as_deref()?.parse::<WorldId>().ok()?;
        world::load(self.root().ok()?, id).ok()
    }

    /// Puts a world's client mods into the host's room installation.
    pub async fn place_client(
        &self,
        world: &world::WorldFile,
        instance: InstanceId,
    ) -> LauncherResult<()> {
        let root = self.root()?.clone();
        let world = world.clone();
        blocking(move || pack::place_client(&root, &world, &root.game_dir(instance))).await
    }

    pub fn accept_eula(&self, world: WorldId) -> LauncherResult<LocalWorld> {
        world::accept_eula(self.root()?, world).map(LocalWorld::from)
    }

    /// The installation and the world a mods update of `world` would use:
    /// the world must have mods and be kept here; the installation must be
    /// the player's, on the world's Minecraft version.
    fn update_source(
        &self,
        state: &AppState,
        world: WorldId,
        instance: InstanceId,
    ) -> LauncherResult<(world::WorldFile, crate::storage::models::InstanceFile)> {
        let found = world::load(self.root()?, world)?;
        if found.pack.is_none() {
            return Err(LauncherError::invalid_input(
                "This world has no mods. Make a new world from the installation instead.",
            ));
        }
        let instance = state.instance(instance).ok_or_else(instance_gone)?;
        if instance.world_room.is_some() || instance.loader.is_none() {
            return Err(LauncherError::invalid_input(
                "This installation has no mod loader.",
            ));
        }
        if instance.version_id != found.minecraft {
            return Err(LauncherError::invalid_input(
                "The installation plays another Minecraft version than the world.",
            ));
        }
        Ok((found, instance))
    }

    /// What updating `world`'s mods from the installation `instance` would
    /// change (W12). Nothing changes yet.
    pub async fn update_preview(
        &self,
        state: &AppState,
        mods: &crate::mods::Mods,
        world: WorldId,
        instance: InstanceId,
    ) -> LauncherResult<update::Preview> {
        let (found, instance) = self.update_source(state, world, instance)?;
        let root = self.root()?.clone();
        let loader = instance.loader.clone().ok_or_else(instance_gone)?;
        let permit = state.activity().try_acquire(
            OperationKind::Backup,
            Some(instance.id),
            uuid::Uuid::new_v4().to_string(),
        )?;
        let built = build_pack(state, mods, &root, &instance, &loader).await?;
        let preview = blocking(move || update::preview(&root, &found, &built)).await;
        drop(permit);
        preview
    }

    /// Updates the mods of `room`'s stopped world from the installation
    /// `instance`, when it is still the change `expected` names (W12): the
    /// world is backed up, the new generation tried on a copy of that backup,
    /// then kept with the way back. `choices` answers each changed setting.
    /// The next start confirms it, or puts the earlier mods and the backup
    /// back.
    #[allow(clippy::too_many_arguments)]
    pub async fn update(
        self: &Arc<Self>,
        state: &AppState,
        mods: &crate::mods::Mods,
        room: &str,
        world: WorldId,
        instance: InstanceId,
        expected: &str,
        choices: std::collections::HashMap<String, update::Keep>,
    ) -> LauncherResult<()> {
        let cancel = self.begin(room, Stage::Server)?;
        let result = self
            .update_inner(
                state, mods, room, world, instance, expected, choices, &cancel,
            )
            .await;
        self.finish(&result);
        result
    }

    #[allow(clippy::too_many_arguments)]
    async fn update_inner(
        self: &Arc<Self>,
        state: &AppState,
        mods: &crate::mods::Mods,
        room: &str,
        world_id: WorldId,
        instance: InstanceId,
        expected: &str,
        choices: std::collections::HashMap<String, update::Keep>,
        cancel: &CancelToken,
    ) -> LauncherResult<()> {
        let found = self.client.room(room).await?;
        if found.role != "owner" {
            return Err(Problem::Forbidden.into());
        }
        if found.world.as_deref() != Some(world_id.to_string().as_str()) {
            return Err(elsewhere());
        }
        let (current, instance) = self.update_source(state, world_id, instance)?;
        let root = self.root()?.clone();
        let (metadata, source, runtimes, executor) =
            match (&self.metadata, &self.source, &self.runtimes, &self.executor) {
                (Some(m), Some(s), Some(r), Some(e)) => (m, s, r, e),
                _ => return Err(unavailable()),
            };
        let loader = instance.loader.clone().ok_or_else(instance_gone)?;
        let staged = {
            let permit = state.activity().try_acquire(
                OperationKind::Backup,
                Some(instance.id),
                uuid::Uuid::new_v4().to_string(),
            )?;
            let built = build_pack(state, mods, &root, &instance, &loader).await?;
            let (root, expected) = (root.clone(), expected.to_owned());
            let staged =
                blocking(move || update::stage(&root, &current, built, &expected, &choices)).await;
            drop(permit);
            staged?
        };

        self.set_stage(room, Stage::BackingUp);
        let backup = if safety::save_dir(&root, world_id).is_dir() {
            let to = root.clone();
            let made = blocking(move || safety::backup(&to, world_id, Reason::BeforeUpdate)).await;
            match made {
                Ok(made) => made.map(|b| b.id),
                Err(error) => {
                    let root = root.clone();
                    let _ = blocking(move || update::abandon(&root, world_id)).await;
                    return Err(error);
                }
            }
        } else {
            None
        };
        {
            let root = root.clone();
            blocking(move || update::begin(&root, world_id)).await?;
        }

        self.set_stage(room, Stage::Server);
        let sink = Progress {
            worlds: Arc::clone(self),
        };
        let starting = {
            let (worlds, room) = (Arc::downgrade(self), room.to_owned());
            move || {
                if let Some(worlds) = worlds.upgrade() {
                    worlds.set_stage(&room, Stage::Trying);
                }
            }
        };
        let trial = update::Trial {
            root: &root,
            metadata,
            source: source.as_ref(),
            runtimes,
            executor,
            activity: state.activity(),
            java: state.java_provider(),
            game_heap_mib: self.room_game_heap(state, room),
            physical_memory_bytes: physical_memory(),
            cancel,
            sink: &sink,
            starting: &starting,
        };
        let tried = update::try_on_copy(&trial, world_id, &staged, backup.as_deref()).await;
        let manifest = match tried {
            Ok(manifest) => manifest,
            Err(error) => {
                let root = root.clone();
                let _ = blocking(move || update::abandon(&root, world_id)).await;
                // Stopped by the player, the copy said nothing about the mods.
                if cancel.is_cancelled() {
                    return Err(error);
                }
                return Err(error.with_detail("worldsTrialFailed", true));
            }
        };
        let root = root.clone();
        blocking(move || update::commit(&root, world_id, staged, manifest, backup).map(|_| ()))
            .await
    }

    /// A start that failed: a world waiting for its first start after an
    /// update of its mods gets its earlier mods and its backup back, and the
    /// error says so. A failure the mods cannot cause changes nothing.
    async fn after_failed_start(
        &self,
        root: &DataRoot,
        world: WorldId,
        error: LauncherError,
    ) -> LauncherError {
        let by_mods = matches!(
            error.code,
            ErrorCode::GameExitedWithError | ErrorCode::OperationInterrupted
        );
        let waiting = world::load(root, world).is_ok_and(|w| update::pending(&w));
        if !by_mods || !waiting {
            return error;
        }
        let root = root.clone();
        match blocking(move || update::roll_back(&root, world)).await {
            Ok(true) => error.with_detail("worldsRolledBack", true),
            Ok(false) => error,
            Err(rolled) => rolled,
        }
    }

    /// The heap of the host's own game in the room's installation.
    fn room_game_heap(&self, state: &AppState, room: &str) -> u32 {
        state
            .instance_ids()
            .into_iter()
            .filter_map(|id| state.instance(id))
            .find(|i| i.world_room.as_deref() == Some(room))
            .map_or(0, |i| i.max_heap_mib)
    }

    /// Hosts `room` from its world on this computer: gets the server and Java
    /// ready, starts the server, then the room's session and the tunnel. A
    /// world with mods publishes its room pack with `publish` first, once per
    /// generation.
    pub async fn host(
        self: &Arc<Self>,
        state: &AppState,
        room: &str,
        publish: pack::PublishRoom,
    ) -> LauncherResult<()> {
        let cancel = self.begin(room, Stage::Server)?;
        let result = self.host_inner(state, room, publish, &cancel).await;
        self.finish(&result);
        result
    }

    async fn host_inner(
        self: &Arc<Self>,
        state: &AppState,
        room: &str,
        publish: pack::PublishRoom,
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
        let room_pack = pack::publish_room(&root, world_id, publish).await?;

        self.set_stage(&room.id, Stage::Starting);
        let operation_id = uuid::Uuid::new_v4().to_string();
        let permit = state
            .activity()
            .try_acquire_hosting(world_id.uuid(), operation_id.clone())?;
        let physical = physical_memory();
        let game_heap = self.room_game_heap(state, &room.id);
        let members = [nick];
        let launch = host::Launch {
            root: &root,
            world_id,
            java: &java,
            server_heap_mib: if prepared.pack.is_some() {
                super::memory::DEFAULT_MODDED_SERVER_HEAP_MIB
            } else {
                super::memory::DEFAULT_SERVER_HEAP_MIB
            },
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
            return Err(self.after_failed_start(&root, world_id, error).await);
        }
        if update::pending(&prepared) {
            let root = root.clone();
            blocking(move || update::confirm(&root, world_id)).await?;
        }

        self.set_stage(&room.id, Stage::Opening);
        let relay_only = state.settings().worlds_relay_only;
        let opened = async {
            let endpoint = bind(relay_reach(relay_only)?, true).await?;
            let served = Arc::new(Served {
                server: Arc::clone(&server),
                root: root.clone(),
            }) as Arc<dyn WorldServer>;
            let rebind: hosting::Rebind = Arc::new(move || {
                Box::pin(async move { bind(relay_reach(relay_only)?, true).await })
            });
            hosting::start(
                Arc::clone(&self.client),
                served,
                endpoint,
                rebind,
                &room.id,
                u32::from(prepared.settings.max_players),
                room_pack,
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
        if tuple::version(&room.minecraft).is_none() {
            return Err(LauncherError::invalid_input(
                "Worlds are not available for this Minecraft version yet.",
            ));
        }
        if !room.hosting {
            return Err(Problem::NotHosting.into());
        }
        let endpoint = bind(relay_reach(state.settings().worlds_relay_only)?, false).await?;
        let guest = joining::join(
            Arc::clone(&self.client),
            endpoint,
            Visit {
                room: &room,
                nick,
                route: Route::Relay(joining::relay_url()?),
                version_name: room.minecraft.clone(),
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

fn instance_gone() -> LauncherError {
    LauncherError::new(
        ErrorCode::InstanceNotFound,
        "The installation no longer exists.",
    )
}

/// A generation of mods from an installation with a mod loader.
async fn build_pack(
    state: &AppState,
    mods: &crate::mods::Mods,
    root: &DataRoot,
    instance: &crate::storage::models::InstanceFile,
    loader: &crate::loader::LoaderChoice,
) -> LauncherResult<pack::Built> {
    pack::check_supported(loader, &instance.version_id)?;
    mods.content.ensure_idle(instance.id)?;
    crate::mods::online(state)?;
    let harmful = mods.harmful().await?;
    let mut built =
        pack::build(&mods.client, &harmful, &root.game_dir(instance.id), loader).await?;
    built.pack.source = Some(instance.id);
    Ok(built)
}

/// Keeps a generation in a world just made (blocking); a failure takes the
/// world away again.
fn with_pack(
    root: &DataRoot,
    mut created: world::WorldFile,
    built: Option<pack::Built>,
) -> LauncherResult<world::WorldFile> {
    let Some(built) = built else {
        return Ok(created);
    };
    let kept = pack::store(root, &created, built).and_then(|generation| {
        created.pack = Some(generation);
        world::save(root, &created)
    });
    match kept {
        Ok(()) => Ok(created),
        Err(error) => {
            let _ = std::fs::remove_dir_all(root.world_dir(created.id));
            Err(error)
        }
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

/// The computer's memory in bytes, zero when unknown.
fn physical_memory() -> u64 {
    crate::validation::MemoryPolicy::detect()
        .map(|m| m.physical_mib * 1024 * 1024)
        .unwrap_or(0)
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
                reopening: status.reopening,
                clean: server.clean,
            }
        }),
        guest: inner.guest.as_ref().map(|g| GuestView {
            room: g.room().to_owned(),
            connected: g.connected(),
            refused: g.refused().flatten(),
            path: g.path(),
            reconnecting: g.reconnecting(),
            gave_up: g.gave_up(),
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
