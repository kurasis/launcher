//! `launch_game`: preflight, compile, spawn and supervise one Minecraft run
//! (docs/spec/05 "Launch preflight", "Process supervision", "Readiness,
//! cancellation and path boundaries").
//!
//! Preflight holds the global activity permit and checks the install lock, the
//! locked files (existence and size), the runtime tree and the locked metadata,
//! projects the selected asset index and then asks the account service for fresh
//! access. Any failure ends the run before a process exists; files that need
//! repair mark the installation `repair_required`. Cancellation is honoured up to
//! the spawn; afterwards only an explicit Stop ends the game. The permit is
//! released only after the game exited, output was drained and the run folder
//! was cleaned up.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::process::Command;
use std::sync::{Arc, Mutex, PoisonError};

use chrono::Utc;
use uuid::Uuid;

use super::argfile;
use super::compile::{self, LaunchIdentity, LaunchInputs};
use super::logs::{self, RunLog};
use super::redact::Redactor;
use crate::download::CancelToken;
use crate::error::{ErrorCode, LauncherError, LauncherResult};
use crate::install::lock;
use crate::install::service::{BoxFuture, PlanSource, StateAccess};
use crate::lifecycle::child::{self, OutputLine, OutputTail, Stream, Supervised};
use crate::runtime::{self, Verification};
use crate::state::dto::{RunKind, RunState, RunSummary};
use crate::storage::models::{
    InstallLock, InstallationHealth, LastResult, OperationKind, Outcome, ProfileRef,
};
use crate::storage::paths::{DataRoot, InstanceId, SafeName};
use crate::storage::reconcile::{self, LockState};

/// Everything preflight produced for the spawn.
struct Prepared {
    command: Command,
    secrets: Vec<String>,
    run_dir: PathBuf,
    java: PathBuf,
}

/// Lines of output kept in the run summary.
const SUMMARY_TAIL_LINES: usize = 40;

/// The main class of every version whose client accepts `--demo` (1.6 and later).
pub const NOTOKEN_MAIN_CLASS: &str = "net.minecraft.client.main.Main";
/// Main class of mod loaders started through Mojang's launchwrapper (D-039).
pub const LAUNCHWRAPPER_MAIN_CLASS: &str = "net.minecraft.launchwrapper.Launch";

/// Who a launch plays as.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaunchMode {
    /// A signed-in Microsoft account (its Minecraft profile ID), after a fresh
    /// access check.
    Account(String),
    /// A no-token profile: Minecraft's demo without an account (D-033, D-045).
    NoToken(super::notoken::NoTokenProfile),
}

/// Per-run inputs taken from the installation and the request.
struct RunSettings {
    max_heap_mib: u32,
    resolution: crate::storage::models::Resolution,
    mode: LaunchMode,
    /// The user's own Java instead of the private runtime (D-044).
    custom_java: Option<PathBuf>,
    /// The user's JVM options, after the version's own.
    jvm_arguments: Vec<String>,
    /// The user's Minecraft arguments and fullscreen choice (D-056).
    game_arguments: Vec<String>,
    fullscreen: bool,
}

/// Fresh account verification for a launch.
pub trait Authorizer: Send + Sync {
    fn authorize<'a>(
        &'a self,
        profile_id: &'a str,
    ) -> BoxFuture<'a, LauncherResult<LaunchIdentity>>;
}

impl Authorizer for Arc<crate::auth::service::AuthService> {
    fn authorize<'a>(
        &'a self,
        profile_id: &'a str,
    ) -> BoxFuture<'a, LauncherResult<LaunchIdentity>> {
        Box::pin(async move {
            let auth = self.verify_for_launch(profile_id).await?;
            Ok(LaunchIdentity {
                player_name: auth.profile.name,
                uuid: auth.profile.id,
                access_token: auth.access_token,
                xuid: auth.xuid,
                client_id: auth.client_id,
                notoken: false,
                offline: auth.offline,
            })
        })
    }
}

/// Window and process-handle hooks; the Tauri implementation drives the main
/// window exactly like the P03 lifecycle harness.
pub trait LaunchHost: StateAccess {
    fn store_handle(&self, run_id: &str, supervised: Supervised);
    fn clear_handle(&self, run_id: &str);
    fn kill(&self, run_id: &str) -> bool;
    /// The process is running; schedule the window unload if configured.
    fn run_started(&self, run_id: &str);
    /// The process exited; a pending unload must not happen.
    fn process_exited(&self, run_id: &str);
    /// Output drained and cleanup done; make sure one window shows the result.
    fn run_finished(&self, run_id: &str);
}

pub struct LaunchService {
    host: Arc<dyn LaunchHost>,
    source: Arc<dyn PlanSource>,
    auth: Arc<dyn Authorizer>,
    launcher_version: String,
    pending: Mutex<Option<(String, CancelToken)>>,
    requests: Mutex<Requests>,
    /// Runs the user asked to stop; their end is "stopped", not a game error.
    stop_requested: Arc<Mutex<HashSet<String>>>,
    force_argfile: bool,
}

/// Accepted request IDs (oldest first) and the instance and run they started.
type Requests = (VecDeque<String>, HashMap<String, (InstanceId, String)>);

fn lock_or<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn repair_needed(what: &str) -> LauncherError {
    LauncherError::new(
        ErrorCode::RepairRequired,
        "Some game files are missing or damaged. Repair the installation, then play.",
    )
    .with_detail("check", what.to_owned())
}

/// Files a repair restored minutes ago are gone again: antivirus software
/// removing them (often Java, or a mod it misjudges) is the usual cause, and
/// another repair alone would only go round in a circle.
fn damaged_again(error: LauncherError, last: Option<&LastResult>) -> LauncherError {
    let repaired_recently = last.is_some_and(|r| {
        r.kind == OperationKind::Repair
            && r.outcome == Outcome::Succeeded
            && Utc::now().signed_duration_since(r.finished_at) < chrono::Duration::minutes(30)
    });
    if error.code != ErrorCode::RepairRequired || !repaired_recently {
        return error;
    }
    let mut again = LauncherError::new(
        ErrorCode::RepairRequired,
        "Files the last repair restored are missing or damaged again. Antivirus software may be removing them: allow the launcher's data folder in it, then repair the installation.",
    );
    again.safe_details = error.safe_details;
    again.with_detail("afterRepair", "yes")
}

impl LaunchService {
    pub fn new(
        host: Arc<dyn LaunchHost>,
        source: Arc<dyn PlanSource>,
        auth: Arc<dyn Authorizer>,
        launcher_version: &str,
    ) -> Self {
        Self {
            host,
            source,
            auth,
            launcher_version: launcher_version.to_owned(),
            pending: Mutex::default(),
            requests: Mutex::default(),
            stop_requested: Arc::default(),
            force_argfile: false,
        }
    }

    /// Always pass JVM arguments through an argfile (tests and diagnostics).
    pub fn with_argfile(mut self) -> Self {
        self.force_argfile = true;
        self
    }

    fn known_run(
        &self,
        request_id: &str,
        instance_id: InstanceId,
    ) -> LauncherResult<Option<RunSummary>> {
        let requests = lock_or(&self.requests);
        let Some((instance, run_id)) = requests.1.get(request_id) else {
            return Ok(None);
        };
        if *instance != instance_id {
            return Err(LauncherError::new(
                ErrorCode::RequestIdConflict,
                "This request ID was already used for another request.",
            ));
        }
        let state = self.host.app_state();
        Ok(state
            .current_run()
            .filter(|r| r.run_id == *run_id)
            .or_else(|| state.snapshot().last_run.filter(|r| r.run_id == *run_id)))
    }

    fn remember(&self, request_id: &str, instance_id: InstanceId, run_id: &str) {
        let mut requests = lock_or(&self.requests);
        requests.0.push_back(request_id.to_owned());
        requests
            .1
            .insert(request_id.to_owned(), (instance_id, run_id.to_owned()));
        while requests.0.len() > 64 {
            if let Some(old) = requests.0.pop_front() {
                requests.1.remove(&old);
            }
        }
    }

    /// Cancels a launch that has not spawned Java yet. Returns `None` if
    /// `run_id` is not a pending launch.
    pub fn cancel_pending(&self, run_id: &str) -> Option<()> {
        let pending = lock_or(&self.pending);
        match pending.as_ref() {
            Some((id, token)) if id == run_id => {
                token.cancel();
                Some(())
            }
            _ => None,
        }
    }

    /// Requests termination of the owned game process of `run_id`, or of a game
    /// that survived a launcher restart (after re-checking its identity).
    pub fn stop(&self, run_id: &str) -> bool {
        lock_or(&self.stop_requested).insert(run_id.to_owned());
        if self.host.kill(run_id) {
            return true;
        }
        lock_or(&self.stop_requested).remove(run_id);
        let Some(root) = self.host.app_state().data_root() else {
            return false;
        };
        super::orphan::read(&root)
            .filter(|r| r.run_id == run_id)
            .is_some_and(|r| super::orphan::stop(&r))
    }

    /// At startup: a game that outlived the previous supervisor holds the activity
    /// permit until it exits, so nothing can launch or change shared files.
    pub fn watch_survivor(&self) -> Option<String> {
        let state = self.host.app_state();
        let root = state.data_root()?;
        let record = super::orphan::survivor(&root)?;
        let permit = state
            .activity()
            .try_acquire(
                OperationKind::Launch,
                Some(record.instance_id),
                record.run_id.clone(),
            )
            .ok()?;
        state.begin_run(RunSummary {
            run_id: record.run_id.clone(),
            kind: RunKind::Game,
            instance_id: Some(record.instance_id),
            state: RunState::Running,
            started_at: record.started_at,
            ended_at: None,
            exit_code: None,
            duration_ms: None,
            drain_timed_out: false,
            output_tail: Vec::new(),
            output_lines_dropped: 0,
            error: Some(super::orphan::interrupted_error()),
            logging_degraded: false,
            signal: None,
            diagnosis: None,
        });
        let host = Arc::clone(&self.host);
        let run_id = record.run_id.clone();
        std::thread::Builder::new()
            .name("survivor-watch".into())
            .spawn(move || {
                while super::orphan::is_alive(&record) {
                    std::thread::sleep(super::orphan::WATCH_INTERVAL);
                }
                let state = host.app_state();
                super::orphan::clear(&root, &record.run_id);
                state.finish_run(&record.run_id, |run| {
                    run.state = RunState::Exited;
                    run.ended_at = Some(Utc::now());
                    run.error = Some(super::orphan::interrupted_error());
                });
                drop(permit);
                state.announce_runs();
                host.run_finished(&record.run_id);
            })
            .ok()?;
        Some(run_id)
    }

    /// Who plays `instance_id`: its own profile, else the default one, else the
    /// built-in demo player (D-045).
    pub fn mode_for(&self, instance_id: InstanceId) -> LaunchMode {
        let state = self.host.app_state();
        match state.profile_for(instance_id) {
            Some(ProfileRef::Microsoft { id }) => LaunchMode::Account(id),
            Some(ProfileRef::NoToken { id }) => state
                .notoken_name(&id)
                .and_then(|name| super::notoken::NoTokenProfile::new(&name).ok())
                .map(LaunchMode::NoToken)
                .unwrap_or_else(|| LaunchMode::NoToken(Default::default())),
            None => LaunchMode::NoToken(Default::default()),
        }
    }

    /// Launches as the installation's profile.
    pub async fn launch(
        &self,
        request_id: &str,
        instance_id: InstanceId,
    ) -> LauncherResult<RunSummary> {
        self.launch_as(request_id, instance_id, self.mode_for(instance_id))
            .await
    }

    pub async fn launch_as(
        &self,
        request_id: &str,
        instance_id: InstanceId,
        mode: LaunchMode,
    ) -> LauncherResult<RunSummary> {
        crate::state::ledger::validate_request_id(request_id)?;
        if let Some(run) = self.known_run(request_id, instance_id)? {
            return Ok(run);
        }
        let state = self.host.app_state();
        let instance = state.instance(instance_id).ok_or_else(|| {
            LauncherError::new(
                ErrorCode::InstanceNotFound,
                "The installation no longer exists.",
            )
        })?;
        match instance.health {
            InstallationHealth::Ready => {}
            InstallationHealth::NotInstalled => {
                return Err(LauncherError::new(
                    ErrorCode::RepairRequired,
                    "This installation is not installed yet. Install it, then play.",
                ));
            }
            InstallationHealth::Unsupported => {
                return Err(LauncherError::new(
                    ErrorCode::UnsupportedSchema,
                    "This installation was created by a newer launcher.",
                ));
            }
            InstallationHealth::NeedsVerification | InstallationHealth::RepairRequired => {
                return Err(repair_needed("health"));
            }
        }
        let jvm_arguments = runtime::custom::parse_jvm_arguments(&instance.jvm_arguments)?;
        let game_arguments = super::arguments::parse_game_arguments(&instance.game_arguments)?;
        let root = state.data_root().ok_or_else(|| {
            LauncherError::new(
                ErrorCode::StorageUnavailable,
                "Launcher data is unavailable.",
            )
        })?;
        let run_uuid = Uuid::new_v4();
        let run_id = run_uuid.to_string();
        let permit = state.activity().try_acquire(
            OperationKind::Launch,
            Some(instance_id),
            run_id.clone(),
        )?;
        self.remember(request_id, instance_id, &run_id);
        let cancel = CancelToken::new();
        *lock_or(&self.pending) = Some((run_id.clone(), cancel.clone()));
        state.begin_run(RunSummary {
            run_id: run_id.clone(),
            kind: RunKind::Game,
            instance_id: Some(instance_id),
            state: RunState::Preflight,
            started_at: Utc::now(),
            ended_at: None,
            exit_code: None,
            duration_ms: None,
            drain_timed_out: false,
            output_tail: Vec::new(),
            output_lines_dropped: 0,
            error: None,
            logging_degraded: false,
            signal: None,
            diagnosis: None,
        });
        state.console().info(match &mode {
            LaunchMode::NoToken(profile) => format!(
                "Launch without a token (Minecraft demo) as {}: checking the installation",
                profile.name()
            ),
            LaunchMode::Account(_) => {
                "Launch with a Microsoft account: checking the installation and access".to_owned()
            }
        });

        let prepared = self
            .preflight(
                &root,
                instance_id,
                run_uuid,
                RunSettings {
                    max_heap_mib: instance.max_heap_mib,
                    resolution: instance.resolution,
                    mode,
                    custom_java: instance.custom_java.as_ref().map(|j| j.path.clone()),
                    jvm_arguments,
                    game_arguments,
                    fullscreen: instance.fullscreen,
                },
                &cancel,
            )
            .await;
        let Prepared {
            command,
            secrets,
            run_dir,
            java,
        } = match prepared {
            Ok(prepared) => prepared,
            Err(error) => {
                *lock_or(&self.pending) = None;
                let _ = std::fs::remove_dir_all(root.run_dir(instance_id, run_uuid));
                let error = damaged_again(error, instance.last_result.as_ref());
                if error.code == ErrorCode::RepairRequired {
                    let _ = state.update_instance(instance_id, |i| {
                        i.health = InstallationHealth::RepairRequired;
                    });
                }
                state.finish_run(&run_id, |run| {
                    run.state = if error.code == ErrorCode::OperationCancelled {
                        RunState::Cancelled
                    } else {
                        RunState::Failed
                    };
                    run.ended_at = Some(Utc::now());
                    run.error = Some(error.clone());
                });
                drop(permit);
                state.announce_runs();
                return Err(error);
            }
        };

        // Spawn boundary: serialized with cancellation.
        let spawned = {
            let mut pending = lock_or(&self.pending);
            if cancel.is_cancelled() {
                Err(LauncherError::new(
                    ErrorCode::OperationCancelled,
                    "The launch was cancelled.",
                ))
            } else {
                *pending = None;
                self.spawn(
                    &root,
                    instance_id,
                    &run_id,
                    run_dir.clone(),
                    (command, &java),
                    &secrets,
                    permit,
                )
            }
        };
        match spawned {
            Ok(()) => {}
            Err(error) => {
                *lock_or(&self.pending) = None;
                let _ = std::fs::remove_dir_all(&run_dir);
                state.finish_run(&run_id, |run| {
                    run.state = if error.code == ErrorCode::OperationCancelled {
                        RunState::Cancelled
                    } else {
                        RunState::Failed
                    };
                    run.ended_at = Some(Utc::now());
                    run.error = Some(error.clone());
                });
                state.announce_runs();
                return Err(error);
            }
        }
        state
            .current_run()
            .filter(|r| r.run_id == run_id)
            .or_else(|| state.snapshot().last_run.filter(|r| r.run_id == run_id))
            .ok_or_else(|| LauncherError::internal("The run disappeared."))
    }

    async fn preflight(
        &self,
        root: &DataRoot,
        instance_id: InstanceId,
        run: Uuid,
        settings: RunSettings,
        cancel: &CancelToken,
    ) -> LauncherResult<Prepared> {
        let check_cancel = || {
            if cancel.is_cancelled() {
                Err(LauncherError::new(
                    ErrorCode::OperationCancelled,
                    "The launch was cancelled.",
                ))
            } else {
                Ok(())
            }
        };
        let lock = match reconcile::read_lock(&root.install_lock(instance_id))
            .map_err(|e| LauncherError::from_io(&e, "Reading the installation record"))?
        {
            LockState::Complete(lock) if lock.instance_id == instance_id => *lock,
            _ => return Err(repair_needed("lock")),
        };
        // Locked files, and what a loader installer produced: existence and size
        // (full hashes are Repair's job).
        let mut files = lock.artifacts.clone();
        if let Some(loader) = &lock.loader {
            files.extend(loader.generated.iter().cloned());
        }
        let files_root = root.clone();
        let missing = tokio::task::spawn_blocking(move || {
            files.iter().find_map(|a| {
                let mut path = files_root.path().to_path_buf();
                path.extend(a.path.split('/'));
                match std::fs::metadata(&path) {
                    Ok(m) if m.is_file() && m.len() == a.size => None,
                    _ => Some(a.path.clone()),
                }
            })
        })
        .await
        .map_err(|_| LauncherError::internal("A file check stopped unexpectedly."))?;
        if missing.is_some() {
            return Err(repair_needed("files"));
        }
        check_cancel()?;
        let RunSettings {
            max_heap_mib,
            resolution,
            mode,
            custom_java,
            jvm_arguments,
            game_arguments,
            fullscreen,
        } = settings;
        let java = match custom_java {
            Some(path) => self.custom_java(path, &lock).await?,
            None => self.private_java(root, &lock).await?,
        };
        check_cancel()?;
        let plan = self.locked_plan(&lock).await?;
        lock::project_asset_index(
            root,
            &SafeName::parse(&lock.asset_index.id)?,
            &lock.asset_index.sha1,
        )?;
        check_cancel()?;

        // Fresh access check (or the no-token profile); without it Java is never started.
        let identity: BoxFuture<'_, LauncherResult<LaunchIdentity>> = match &mode {
            LaunchMode::Account(profile_id) => self.auth.authorize(profile_id),
            LaunchMode::NoToken(profile) => {
                let identity = profile.identity();
                Box::pin(async move { Ok(identity) })
            }
        };
        let identity = tokio::select! {
            identity = identity => identity?,
            () = cancel.cancelled() => {
                return Err(LauncherError::new(ErrorCode::OperationCancelled, "The launch was cancelled."));
            }
        };
        check_cancel()?;
        if identity.offline {
            self.host.app_state().console().warn(format!(
                "Offline: {} starts without the online account check; multiplayer and \
                 Realms need the connection",
                identity.player_name
            ));
        }
        // Only versions started through Minecraft's own main class know `--demo`;
        // older ones would run the full game, so a no-token identity cannot start them.
        // A mod loader hands the arguments on to that same class.
        if identity.notoken && plan.main_class == LAUNCHWRAPPER_MAIN_CLASS {
            // Forge's launchwrapper tweaker (1.12.2) drops `--demo` and would
            // start the full game.
            return Err(LauncherError::new(
                ErrorCode::NoTokenUnavailable,
                "This mod loader version cannot run the demo. Sign in with an account \
                 that owns Minecraft to play it.",
            ));
        }
        if identity.notoken && plan.game_main_class() != NOTOKEN_MAIN_CLASS {
            return Err(LauncherError::new(
                ErrorCode::NoTokenUnavailable,
                "This Minecraft version has no demo mode. Sign in with an account that \
                 owns Minecraft to play it.",
            ));
        }

        let run_dir = root.run_dir(instance_id, run);
        let natives = run_dir.join("natives");
        let game_dir = root.game_dir(instance_id);
        let (assets_root, assets_plan, assets_game_dir) =
            (root.clone(), plan.clone(), game_dir.clone());
        let game_assets = tokio::task::spawn_blocking(move || {
            super::legacy::prepare_assets(&assets_root, &assets_plan, &assets_game_dir)
        })
        .await
        .map_err(|_| LauncherError::internal("Preparing assets stopped unexpectedly."))??;
        check_cancel()?;
        let compiled = compile::compile(&LaunchInputs {
            plan: &plan,
            root,
            game_dir: &game_dir,
            game_assets: &game_assets,
            natives_dir: &natives,
            max_heap_mib,
            resolution,
            identity: &identity,
            launcher_version: &self.launcher_version,
            extra_jvm_args: &jvm_arguments,
            extra_game_args: &game_arguments,
            fullscreen,
        })?;
        let storage = |e: std::io::Error| LauncherError::from_io(&e, "Preparing the launch");
        std::fs::create_dir_all(&game_dir).map_err(storage)?;
        std::fs::create_dir_all(&natives).map_err(storage)?;
        for dir in &compiled.native_dirs {
            std::fs::create_dir_all(dir).map_err(storage)?;
        }
        if !plan.natives.is_empty() {
            let (native_root, jars, dest) = (root.clone(), plan.natives.clone(), natives.clone());
            tokio::task::spawn_blocking(move || {
                super::legacy::extract_natives(&native_root, &jars, &dest)
            })
            .await
            .map_err(|_| {
                LauncherError::internal("Preparing native libraries stopped unexpectedly.")
            })??;
        }
        // Non-ASCII arguments must survive Java's native decoding.
        let check_java = java.clone();
        let args: Vec<String> = compiled
            .jvm_args
            .iter()
            .chain(compiled.game_args.iter())
            .filter(|a| !a.is_ascii())
            .filter(|a| !compiled.secrets().iter().any(|s| a.contains(s.as_str())))
            .cloned()
            .collect();
        tokio::task::spawn_blocking(move || {
            let refs: Vec<&str> = args.iter().map(String::as_str).collect();
            super::encoding::check(&check_java, &refs)
        })
        .await
        .map_err(|_| LauncherError::internal("An encoding check stopped unexpectedly."))??;
        check_cancel()?;
        let (command, _) = argfile::build_command(
            &java,
            &compiled,
            &run_dir.join("jvm.args"),
            self.force_argfile,
        )?;
        Ok(Prepared {
            command,
            secrets: compiled.secrets().to_vec(),
            run_dir,
            java,
        })
    }

    /// The installation's private runtime, after checking it is intact.
    async fn private_java(&self, root: &DataRoot, lock: &InstallLock) -> LauncherResult<PathBuf> {
        let package = lock::runtime_from_lock(lock)?;
        let verify_root = root.clone();
        let installed = match tokio::task::spawn_blocking(move || {
            runtime::verify_installed(&verify_root, &package)
        })
        .await
        .map_err(|_| LauncherError::internal("A runtime check stopped unexpectedly."))??
        {
            Verification::Valid(installed) => *installed,
            _ => return Err(repair_needed("runtime")),
        };
        if installed.receipt.tree_receipt_sha256 != lock.runtime.tree_receipt_sha256 {
            return Err(repair_needed("runtime receipt"));
        }
        Ok(installed.java)
    }

    /// The user's own Java (D-044): it must still start. Compatibility was
    /// accepted when it was chosen; a change since then is only reported.
    async fn custom_java(&self, path: PathBuf, lock: &InstallLock) -> LauncherResult<PathBuf> {
        let (major, arch) = (lock.java_major, lock.arch);
        let checked = tokio::task::spawn_blocking(move || {
            runtime::custom::check(&path, major, arch)
        })
        .await
        .map_err(|_| LauncherError::internal("A runtime check stopped unexpectedly."))?
        .map_err(|e| {
            if e.code == ErrorCode::PermissionDenied {
                return e;
            }
            let mut error = LauncherError::new(
                ErrorCode::RuntimeUnavailable,
                "The Java chosen for this installation could not be started. Choose another \
                 one in the installation settings.",
            );
            error.safe_details = e.safe_details;
            error
        })?;
        let console = self.host.app_state().console();
        console.info(format!(
            "Your own Java {} ({}, {}) for a game built for Java {}",
            checked.java_version,
            checked.vendor.as_deref().unwrap_or("unknown vendor"),
            checked.arch,
            checked.required_major
        ));
        if !checked.issues.is_empty() {
            console.warn(format!(
                "This Java may not suit the game: {}",
                checked
                    .issues
                    .iter()
                    .map(|i| match i {
                        crate::storage::models::JavaIssue::Older => "older than required",
                        crate::storage::models::JavaIssue::Newer =>
                            "newer than the game was built for",
                        crate::storage::models::JavaIssue::Architecture =>
                            "another processor architecture",
                    })
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        Ok(checked.path)
    }

    /// The plan from the locked descriptor; must match the lock exactly.
    async fn locked_plan(
        &self,
        lock: &InstallLock,
    ) -> LauncherResult<crate::install::plan::GamePlan> {
        let plan = self.source.locked(lock).await?;
        let derived = lock::build_lock(
            &lock::FinalInstallPlan {
                instance_id: lock.instance_id,
                game: plan.clone(),
                runtime: lock::runtime_from_lock(lock)?,
            },
            lock.runtime.clone(),
        );
        if derived.artifacts != lock.artifacts || derived.classpath != lock.classpath {
            return Err(repair_needed("metadata"));
        }
        Ok(plan)
    }

    #[allow(clippy::too_many_arguments)]
    fn spawn(
        &self,
        root: &DataRoot,
        instance_id: InstanceId,
        run_id: &str,
        run_dir: PathBuf,
        (command, java): (Command, &std::path::Path),
        secrets: &[String],
        permit: crate::state::activity::ActivityPermit,
    ) -> LauncherResult<()> {
        let state = self.host.app_state();
        let logs_dir = root.run_logs(Uuid::parse_str(run_id).unwrap_or_default());
        logs::prune(
            &root.path().join("logs").join("runs"),
            Some(run_id),
            logs::MAX_RUNS,
            logs::MAX_TOTAL_BYTES,
        );
        let log = Arc::new(Mutex::new(RunLog::open(&logs_dir)));
        let tail = Arc::new(Mutex::new(OutputTail::default()));
        let redactor = Redactor::new(secrets);
        // The command as the console shows it: secrets replaced, one argument per
        // line (the classpath alone can be several kilobytes).
        let console = state.console();
        console.info(format!("Java: {}", java.display()));
        let args: Vec<String> = command
            .get_args()
            .map(|a| redactor.redact(&a.to_string_lossy()))
            .collect();
        console.info(format!("Arguments ({}):", args.len()));
        // An option and its value on one line: `--username Player`.
        let mut rest = args.iter().peekable();
        while let Some(arg) = rest.next() {
            match rest.peek() {
                Some(value) if arg.starts_with('-') && !value.starts_with('-') => {
                    console.info(format!("  {arg} {value}"));
                    rest.next();
                }
                _ => console.info(format!("  {arg}")),
            }
        }
        let (sink_log, sink_tail) = (Arc::clone(&log), Arc::clone(&tail));
        let console_host = Arc::clone(&self.host);
        let mut events = crate::console::GameLines::default();
        // A question on the console repeats forever on the game's empty input;
        // after a few the launcher ends the game and explains it (D-059).
        let asked = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (sink_asked, question_id) = (Arc::clone(&asked), run_id.to_owned());
        let (mut questions, mut ended) = (0u32, false);
        let sink = Box::new(move |line: OutputLine| {
            if super::crash::is_console_question(&line.text) {
                questions += 1;
                if questions >= super::crash::CONSOLE_QUESTION_LIMIT && !ended {
                    sink_asked.store(true, std::sync::atomic::Ordering::SeqCst);
                    // Before its handle is stored the game cannot be ended yet;
                    // the next question tries again.
                    ended = console_host.kill(&question_id);
                }
                if questions > super::crash::CONSOLE_QUESTION_LIMIT {
                    return;
                }
            }
            let text = redactor.redact(&line.text);
            let console = console_host.app_state().console();
            if line.stream == Stream::Stderr {
                console.game(true, &text);
            } else if let Some((level, folded)) = events.feed(&text) {
                console.game_event(level, &folded);
            }
            lock_or(&sink_log).write_line(&text);
            lock_or(&sink_tail).push(OutputLine {
                stream: line.stream,
                text,
            });
        });
        let exited_host = Arc::clone(&self.host);
        let exited_id = run_id.to_owned();
        let finish_host = Arc::clone(&self.host);
        let finish_id = run_id.to_owned();
        let finish_root = root.clone();
        let stop_requested = Arc::clone(&self.stop_requested);
        let crash_redactor = Redactor::new(secrets);
        let crash_logs = logs_dir.clone();
        let game_dir = root.game_dir(instance_id);
        let spawned_at = std::time::SystemTime::now();
        let spawned = child::spawn(
            command,
            sink,
            move || exited_host.process_exited(&exited_id),
            move |outcome| {
                let state = finish_host.app_state();
                let lines: Vec<String> = lock_or(&tail).lines().map(|l| l.text.clone()).collect();
                let dropped = lock_or(&tail).dropped();
                let start = lines.len().saturating_sub(SUMMARY_TAIL_LINES);
                let degraded = lock_or(&log).degraded();
                let stopped = lock_or(&stop_requested).remove(&finish_id);
                let exit = outcome.status.as_ref().ok().map(super::crash::ExitInfo::of);
                // The player's own Stop needs no explanation.
                let asked = asked.load(std::sync::atomic::Ordering::SeqCst);
                let diagnosis = exit.filter(|_| !stopped).and_then(|exit| {
                    explain(
                        state,
                        instance_id,
                        (exit, asked),
                        &lines,
                        (&game_dir, &crash_logs),
                        spawned_at,
                        &crash_redactor,
                    )
                });
                let (run_state, exit_code, error) = match &outcome.status {
                    Ok(status) if stopped => (RunState::Stopped, status.code(), None),
                    // A clean exit after a crash report: Forge's mod error screen.
                    Ok(status) if status.success() && diagnosis.is_none() => {
                        (RunState::Exited, status.code(), None)
                    }
                    Ok(status) => (
                        RunState::Failed,
                        status.code(),
                        Some(LauncherError::new(
                            ErrorCode::GameExitedWithError,
                            "The game exited with an error. The output below may explain why.",
                        )),
                    ),
                    Err(kind) => (
                        RunState::Failed,
                        None,
                        Some(
                            LauncherError::new(
                                ErrorCode::GameExitedWithError,
                                "Waiting for the game failed.",
                            )
                            .with_detail("ioKind", kind.clone()),
                        ),
                    ),
                };
                let _ = std::fs::remove_dir_all(&run_dir);
                let finished_at = Utc::now();
                let _ = state.update_instance(instance_id, |i| {
                    i.last_result = Some(LastResult {
                        kind: OperationKind::Launch,
                        outcome: match run_state {
                            RunState::Exited => Outcome::Succeeded,
                            RunState::Stopped => Outcome::Cancelled,
                            _ => Outcome::Failed,
                        },
                        finished_at,
                        error_code: error.as_ref().map(|e| e.code),
                        exit_code,
                    });
                });
                // Cleanup happens before the result is published, so whoever sees
                // the finished run also sees no handle or active-run record.
                finish_host.clear_handle(&finish_id);
                super::orphan::clear(&finish_root, &finish_id);
                state.finish_run(&finish_id, |run| {
                    run.state = run_state;
                    run.ended_at = Some(finished_at);
                    run.exit_code = exit_code;
                    run.signal = exit.and_then(|e| e.signal);
                    run.diagnosis = diagnosis;
                    run.duration_ms = u64::try_from(outcome.duration.as_millis()).ok();
                    run.drain_timed_out = outcome.drain_timed_out;
                    run.output_tail = lines[start..].to_vec();
                    run.output_lines_dropped = dropped + start as u64;
                    run.logging_degraded = degraded;
                    run.error = error;
                });
                // Released only after exit, bounded drain and cleanup.
                drop(permit);
                state.announce_runs();
                finish_host.run_finished(&finish_id);
            },
        );
        let supervised = spawned.map_err(|error| {
            crate::error::java_blocked(&error).unwrap_or_else(|| {
                LauncherError::new(ErrorCode::GameSpawnFailed, "Java could not be started.")
                    .with_detail("ioKind", format!("{:?}", error.kind()))
            })
        })?;
        // Lets a restarted launcher recognize this game if the supervisor dies.
        super::orphan::record(root, run_id, instance_id, supervised.pid(), java);
        self.host.store_handle(run_id, supervised);
        let _ = state.update_instance(instance_id, |i| i.last_played_at = Some(Utc::now()));
        state.update_run(run_id, |run| {
            if run.state == RunState::Preflight {
                run.state = RunState::Running;
            }
        });
        self.host.run_started(run_id);
        Ok(())
    }
}

/// Why the run ended, from its output and the files it left (D-059).
#[allow(clippy::too_many_arguments)]
fn explain(
    state: &crate::state::AppState,
    instance_id: InstanceId,
    (exit, console_question): (super::crash::ExitInfo, bool),
    output: &[String],
    (game_dir, logs_dir): (&std::path::Path, &std::path::Path),
    spawned_at: std::time::SystemTime,
    redactor: &Redactor,
) -> Option<super::crash::CrashDiagnosis> {
    use super::crash::{self, CrashInput, RunFile};
    let files = crash::files::collect(game_dir, spawned_at, output, redactor);
    fn as_file(file: &Option<(String, String)>) -> Option<RunFile<'_>> {
        file.as_ref().map(|(name, text)| RunFile { name, text })
    }
    let diagnosis = crash::diagnose(&CrashInput {
        exit,
        output,
        crash_report: as_file(&files.crash_report),
        jvm_error: as_file(&files.jvm_error),
        max_heap_mib: state.instance(instance_id).map_or(0, |i| i.max_heap_mib),
        recommended_heap_mib: state.recommend_memory_for(instance_id).ok().map(|r| r.mib),
        console_question,
    });
    // Kept with the run's output for the export and a report (D-061).
    if let Some(diagnosis) = &diagnosis {
        let record = serde_json::json!({
            "analyzerVersion": crash::ANALYZER_VERSION,
            "diagnosis": diagnosis,
        });
        if let Ok(json) = serde_json::to_vec_pretty(&record) {
            let _ = crate::storage::atomic::write(&logs_dir.join(crash::DIAGNOSIS_FILE), &json);
        }
        for (file, name) in [
            (&files.crash_report, crash::REPORT_COPY),
            (&files.jvm_error, crash::JVM_ERROR_COPY),
        ] {
            if let Some((_, text)) = file {
                let _ = crate::storage::atomic::write(&logs_dir.join(name), text.as_bytes());
            }
        }
    }
    diagnosis
}

#[cfg(test)]
#[cfg(unix)]
mod tests;
