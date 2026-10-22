//! IPC command handlers. Handlers validate input and delegate; they hold no business
//! logic. Every command must also be listed in build.rs and granted in capabilities/.
//!
//! A plain `#[tauri::command]` runs on the main thread, which also draws the
//! window. Commands that read or write files, wait for the shared state or start
//! programs are `#[tauri::command(async)]` and run on a worker thread instead, so
//! a file held by antivirus software or a slow disk never freezes the window.

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, Runtime, State};

use std::sync::Arc;

use crate::auth::client_id::{self, ClientIdStatus};
use crate::auth::service::AuthService;
use crate::catalog::source::{CatalogView, MetadataService, PlanSummary};
use crate::error::LauncherResult;
use crate::external::{self, ExternalLink};
use crate::lifecycle::{LifecycleController, RunHandles};
use crate::net::PublicClient;
use crate::state::AppState;
use crate::state::dto::{
    AddNoTokenProfilePayload, AppSnapshot, CheckJavaPayload, CreateInstancePayload,
    DeleteInstancePayload, InstanceSummary, JavaChoice, MutationRequest, NoTokenProfileView,
    ProfileIdPayload, RenameInstancePayload, SetDefaultProfilePayload, SetInstanceJavaPayload,
    SetInstanceLoaderPayload, SetInstanceNotesPayload, SetInstanceProfilePayload, SettingsPatch,
    SettingsView, UpdateInstanceSettingsPayload,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BuildProfile {
    Debug,
    Release,
}

impl BuildProfile {
    pub const fn current() -> Self {
        if cfg!(debug_assertions) {
            Self::Debug
        } else {
            Self::Release
        }
    }
}

/// Non-secret build and platform facts. Mirrors `AppInfo` in src/lib/contracts.ts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub product_name: String,
    pub app_version: String,
    pub tauri_version: String,
    pub os: String,
    pub arch: String,
    pub build_profile: BuildProfile,
    pub microsoft_client_id: ClientIdStatus,
    /// Name of the debug-build test profile that authorizes launches, if active.
    /// Always `None` in release builds.
    pub test_profile: Option<String>,
}

impl AppInfo {
    pub fn collect(package: &tauri::PackageInfo) -> Self {
        Self {
            product_name: package.name.clone(),
            app_version: package.version.to_string(),
            tauri_version: tauri::VERSION.to_owned(),
            os: std::env::consts::OS.to_owned(),
            arch: std::env::consts::ARCH.to_owned(),
            build_profile: BuildProfile::current(),
            microsoft_client_id: client_id::build_time_status(),
            test_profile: active_test_profile(),
        }
    }
}

#[cfg(debug_assertions)]
fn active_test_profile() -> Option<String> {
    crate::launch::notoken::NoTokenProfile::from_env().map(|p| p.name().to_owned())
}

#[cfg(not(debug_assertions))]
fn active_test_profile() -> Option<String> {
    None
}

#[tauri::command]
pub fn get_app_info<R: tauri::Runtime>(app: tauri::AppHandle<R>) -> AppInfo {
    AppInfo::collect(app.package_info())
}

#[tauri::command(async)]
pub fn get_app_snapshot(state: State<'_, AppState>) -> AppSnapshot {
    state.snapshot()
}

#[tauri::command(async)]
pub fn create_instance(
    state: State<'_, AppState>,
    request: MutationRequest<CreateInstancePayload>,
) -> LauncherResult<InstanceSummary> {
    state.create_instance(request)
}

#[tauri::command(async)]
pub fn update_instance_settings(
    state: State<'_, AppState>,
    request: MutationRequest<UpdateInstanceSettingsPayload>,
) -> LauncherResult<InstanceSummary> {
    state.update_instance_settings(request)
}

/// Saves an installation's notes.
#[tauri::command(async)]
pub fn set_instance_notes(
    state: State<'_, AppState>,
    request: MutationRequest<SetInstanceNotesPayload>,
) -> LauncherResult<InstanceSummary> {
    state.set_instance_notes(request)
}

/// Renames an installation.
#[tauri::command(async)]
pub fn rename_instance(
    state: State<'_, AppState>,
    request: MutationRequest<RenameInstancePayload>,
) -> LauncherResult<InstanceSummary> {
    state.rename_instance(request)
}

/// Starts a Java program and tells how it suits an installed version (D-044).
#[tauri::command]
pub async fn check_java(
    state: State<'_, AppState>,
    payload: CheckJavaPayload,
) -> LauncherResult<crate::runtime::custom::JavaCheck> {
    let (major, arch) = state.required_java(payload.instance_id)?;
    probe_java(payload.path, major, arch).await
}

async fn probe_java(
    path: std::path::PathBuf,
    major: u16,
    arch: crate::storage::paths::TargetArch,
) -> LauncherResult<crate::runtime::custom::JavaCheck> {
    tauri::async_runtime::spawn_blocking(move || crate::runtime::custom::check(&path, major, arch))
        .await
        .map_err(|_| {
            crate::error::LauncherError::internal("A runtime check stopped unexpectedly.")
        })?
}

/// Chooses an installation's Java and JVM options (D-044). A custom Java is
/// checked again here; its warnings must have been accepted.
#[tauri::command]
pub async fn set_instance_java(
    state: State<'_, AppState>,
    request: MutationRequest<SetInstanceJavaPayload>,
) -> LauncherResult<InstanceSummary> {
    let check = match &request.payload.java {
        JavaChoice::Custom { path } => {
            let (major, arch) = state.required_java(request.payload.instance_id)?;
            Some(probe_java(path.clone(), major, arch).await?)
        }
        JavaChoice::Managed { .. } => None,
    };
    state.set_instance_java(request, check)
}

/// Switches an installation to another version of its mod loader (D-054).
#[tauri::command(async)]
pub fn set_instance_loader(
    state: State<'_, AppState>,
    request: MutationRequest<SetInstanceLoaderPayload>,
) -> LauncherResult<InstanceSummary> {
    state.set_instance_loader(request)
}

/// Every backup, every installation's worlds and the space used (D-047).
#[tauri::command]
pub async fn list_backups<R: Runtime>(
    app: AppHandle<R>,
) -> LauncherResult<crate::backup::BackupOverview> {
    blocking(app, crate::backup::overview).await
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateBackupPayload {
    pub instance_id: crate::storage::paths::InstanceId,
    pub scope: crate::backup::Scope,
}

/// Backs up a world or a whole installation now.
#[tauri::command]
pub async fn create_backup<R: Runtime>(
    app: AppHandle<R>,
    request: MutationRequest<CreateBackupPayload>,
) -> LauncherResult<crate::backup::SnapshotInfo> {
    crate::state::ledger::validate_request_id(&request.request_id)?;
    let payload = request.payload;
    blocking(app, move |state| {
        crate::backup::create(
            state,
            payload.instance_id,
            payload.scope,
            crate::backup::Reason::Manual,
        )
    })
    .await
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RestoreBackupPayload {
    pub backup_id: String,
    /// Restore a world beside the current one instead of replacing it.
    #[serde(default)]
    pub as_copy: bool,
}

/// Restores a backup (the replaced state is backed up first).
#[tauri::command]
pub async fn restore_backup<R: Runtime>(
    app: AppHandle<R>,
    request: MutationRequest<RestoreBackupPayload>,
) -> LauncherResult<crate::backup::RestoreResult> {
    crate::state::ledger::validate_request_id(&request.request_id)?;
    let payload = request.payload;
    blocking(app, move |state| {
        crate::backup::restore(state, &payload.backup_id, payload.as_copy)
    })
    .await
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BackupIdPayload {
    pub backup_id: String,
}

/// Deletes a backup; returns the bytes freed.
#[tauri::command]
pub async fn delete_backup<R: Runtime>(
    app: AppHandle<R>,
    request: MutationRequest<BackupIdPayload>,
) -> LauncherResult<u64> {
    crate::state::ledger::validate_request_id(&request.request_id)?;
    let id = request.payload.backup_id;
    blocking(app, move |state| crate::backup::delete(state, &id)).await
}

/// What deleting an installation would lose: worlds, mods, packs and
/// screenshots (D-055).
#[tauri::command]
pub async fn inspect_instance<R: Runtime>(
    app: AppHandle<R>,
    instance_id: crate::storage::paths::InstanceId,
) -> LauncherResult<crate::backup::UserData> {
    blocking(app, move |state| crate::backup::inspect(state, instance_id)).await
}

/// Deletes an installation, after a backup of its game folder if asked
/// (D-055); returns that backup.
#[tauri::command]
pub async fn delete_instance<R: Runtime>(
    app: AppHandle<R>,
    request: MutationRequest<DeleteInstancePayload>,
) -> LauncherResult<Option<crate::backup::SnapshotInfo>> {
    crate::state::ledger::validate_request_id(&request.request_id)?;
    let payload = request.payload;
    mods(&app).content.ensure_idle(payload.instance_id)?;
    blocking(app, move |state| {
        crate::backup::delete_instance(state, payload.instance_id, payload.backup)
    })
    .await
}

/// The memory recommended for an installation (counting its mods) or for a
/// new one of a version, with or without a mod loader (D-058).
#[tauri::command]
pub async fn recommend_memory<R: Runtime>(
    app: AppHandle<R>,
    instance_id: Option<crate::storage::paths::InstanceId>,
    version_id: Option<String>,
    modded: Option<bool>,
) -> LauncherResult<crate::validation::MemoryRecommendation> {
    blocking(app, move |state| match instance_id {
        Some(id) => state.recommend_memory_for(id),
        None => state.recommend_memory(
            version_id.as_deref().unwrap_or_default(),
            modded.unwrap_or(false),
        ),
    })
    .await
}

/// What each part of a reset would remove (D-057).
#[tauri::command]
pub async fn reset_preview<R: Runtime>(
    app: AppHandle<R>,
) -> LauncherResult<crate::reset::ResetPreview> {
    blocking(app, crate::reset::preview).await
}

/// Resets the chosen parts of the launcher (D-057); Microsoft accounts are
/// signed out last, through the auth service.
#[tauri::command]
pub async fn reset_launcher<R: Runtime>(
    app: AppHandle<R>,
    auth: State<'_, Arc<AuthService>>,
    request: MutationRequest<crate::reset::ResetChoice>,
) -> LauncherResult<crate::reset::ResetReport> {
    crate::state::ledger::validate_request_id(&request.request_id)?;
    let choice = request.payload;
    let accounts = choice.accounts;
    let state_app = app.clone();
    let mut report = blocking(app, move |state| crate::reset::run(state, &choice)).await?;
    if accounts {
        let profiles: Vec<String> = state_app
            .state::<AppState>()
            .accounts_file()
            .accounts
            .into_iter()
            .map(|a| a.profile_id)
            .collect();
        for profile in profiles {
            auth.sign_out(&profile).await?;
            report.signed_out += 1;
        }
    }
    Ok(report)
}

/// Every installation's screenshots (D-050).
#[tauri::command]
pub async fn list_screenshots<R: Runtime>(
    app: AppHandle<R>,
) -> LauncherResult<crate::gallery::Gallery> {
    blocking(app, crate::gallery::list).await
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ScreenshotPayload {
    pub instance_id: crate::storage::paths::InstanceId,
    pub file: String,
}

/// Deletes a screenshot after the user confirmed it.
#[tauri::command]
pub async fn delete_screenshot<R: Runtime>(
    app: AppHandle<R>,
    request: MutationRequest<ScreenshotPayload>,
) -> LauncherResult<()> {
    crate::state::ledger::validate_request_id(&request.request_id)?;
    let payload = request.payload;
    blocking(app, move |state| {
        crate::gallery::delete(state, payload.instance_id, &payload.file)
    })
    .await
}

/// Opens a screenshot in the system image viewer, or (with `reveal`) shows it
/// in the file manager. The path is resolved and checked in Rust.
#[tauri::command(async)]
pub fn open_screenshot<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
    request: MutationRequest<OpenScreenshotPayload>,
) -> LauncherResult<()> {
    use tauri_plugin_opener::OpenerExt;
    crate::state::ledger::validate_request_id(&request.request_id)?;
    let payload = request.payload;
    let path = crate::gallery::path(&state, payload.instance_id, &payload.file)?;
    let result = if payload.reveal {
        app.opener().reveal_item_in_dir(&path)
    } else {
        app.opener().open_path(path.to_string_lossy(), None::<&str>)
    };
    result.map_err(|_| crate::error::LauncherError::internal("The screenshot could not be opened."))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OpenScreenshotPayload {
    pub instance_id: crate::storage::paths::InstanceId,
    pub file: String,
    /// Show it in the file manager instead of opening it.
    #[serde(default)]
    pub reveal: bool,
}

/// Runs file work for a command off the async runtime.
async fn blocking<R: Runtime, T: Send + 'static>(
    app: AppHandle<R>,
    work: impl FnOnce(&AppState) -> LauncherResult<T> + Send + 'static,
) -> LauncherResult<T> {
    tauri::async_runtime::spawn_blocking(move || work(&app.state::<AppState>()))
        .await
        .map_err(|_| {
            crate::error::LauncherError::internal("A file operation stopped unexpectedly.")
        })?
}

/// The title of a native dialog or window, in the interface's language: what
/// the interface sent, as plain text of at most 120 characters, else
/// `fallback`.
pub(crate) fn window_title(title: Option<String>, fallback: &str) -> String {
    let title: String = title
        .unwrap_or_default()
        .chars()
        .filter(|c| !c.is_control())
        .take(120)
        .collect();
    let title = title.trim();
    if title.is_empty() {
        fallback.to_owned()
    } else {
        title.to_owned()
    }
}

/// Native file dialog for choosing a Java program.
#[tauri::command]
pub async fn pick_java_file<R: Runtime>(
    app: AppHandle<R>,
    title: Option<String>,
) -> LauncherResult<Option<String>> {
    use tauri_plugin_dialog::DialogExt;
    let title = window_title(title, "Choose the Java program (java or javaw.exe)");
    let picked = tauri::async_runtime::spawn_blocking(move || {
        app.dialog().file().set_title(title).blocking_pick_file()
    })
    .await
    .map_err(|_| crate::error::LauncherError::internal("The file dialog failed."))?;
    Ok(picked
        .and_then(|p| p.into_path().ok())
        .and_then(|p| p.to_str().map(str::to_owned)))
}

#[tauri::command(async)]
pub fn set_app_settings(
    state: State<'_, AppState>,
    stats: State<'_, Arc<crate::stats::Stats>>,
    request: MutationRequest<SettingsPatch>,
) -> LauncherResult<SettingsView> {
    let off = request.payload.stats == Some(false);
    let view = state.set_app_settings(request)?;
    // Turning the statistics off erases what waits to go (D-071).
    if off && !view.stats {
        stats.forget(&state);
        crate::reports::forget_stats(&state);
    }
    Ok(view)
}

/// Settings → Privacy → What is sent: the last ping and ad counts as they
/// went, and today's ping before it goes (D-071).
#[tauri::command(async)]
pub fn stats_sent(
    state: State<'_, AppState>,
    stats: State<'_, Arc<crate::stats::Stats>>,
) -> crate::stats::SentView {
    stats.sent(&state)
}

/// Opens (or focuses) the console window (D-037).
#[tauri::command]
pub fn open_console<R: Runtime>(app: AppHandle<R>) {
    app.state::<LifecycleController>().show_console_window(&app);
}

/// Console journal entries after `after`; the console window polls this.
#[tauri::command(async)]
pub fn read_console(state: State<'_, AppState>, after: u64) -> crate::console::Page {
    state.console().read(after)
}

#[tauri::command]
pub fn show_main_window<R: Runtime>(app: AppHandle<R>) {
    app.state::<LifecycleController>().show_main_window(&app);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuitAction {
    /// Quit; when a run is active the user is asked to choose first.
    Quit,
    /// Close the window and keep supervising the active run (the default choice).
    KeepInBackground,
    /// Confirmed: stop the active run (unsaved progress may be lost), then quit.
    StopAndQuit,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QuitPayload {
    pub action: QuitAction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "status")]
pub enum QuitResult {
    /// The launcher is exiting.
    Quitting,
    /// The window closes; supervision continues in the background.
    InBackground,
    /// A run is active or a world is open to friends; the user must choose an
    /// action.
    #[serde(rename_all = "camelCase")]
    ChoiceRequired {
        operation_id: String,
        /// A world server runs: quitting closes the world for its players.
        hosting: bool,
    },
}

/// Explicit Quit (doc 05 "Window and application states"). Idle: exit. With an
/// active run: ask; keep supervising in the background; or stop exactly that owned
/// process, wait (bounded) for its supervisor to finish cleanup, then exit.
#[tauri::command]
pub fn request_quit<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
    request: MutationRequest<QuitPayload>,
) -> LauncherResult<QuitResult> {
    crate::state::ledger::validate_request_id(&request.request_id)?;
    let controller = app.state::<LifecycleController>();
    let active = state.activity().current();
    let hosting = state.activity().hosting();
    let Some(operation_id) = active
        .as_ref()
        .map(|a| a.operation_id.clone())
        .or_else(|| hosting.as_ref().map(|h| h.operation_id.clone()))
    else {
        controller.set_quit_intent();
        app.exit(0);
        return Ok(QuitResult::Quitting);
    };
    match request.payload.action {
        QuitAction::Quit => Ok(QuitResult::ChoiceRequired {
            operation_id,
            hosting: hosting.is_some(),
        }),
        QuitAction::KeepInBackground => {
            controller.close_main_window(&app);
            Ok(QuitResult::InBackground)
        }
        QuitAction::StopAndQuit => {
            // Stop exactly the active owned work: a game process (or a game that
            // survived a restart), a launch still in preflight, or an install/repair.
            // A world server is closed for its players by the exit itself
            // (`Worlds::shutdown` saves and stops it).
            if let Some(active) = &active {
                let id = &active.operation_id;
                let mut stopped = app.state::<RunHandles>().kill(id);
                if let Some(launcher) = app.state::<Launcher>().0.as_ref() {
                    stopped |= launcher.cancel_pending(id).is_some() || launcher.stop(id);
                }
                if !stopped && let Some(installer) = app.state::<Installer>().0.as_ref() {
                    let _ = installer.cancel(id);
                }
            }
            controller.set_quit_intent();
            let handle = app.clone();
            std::thread::spawn(move || {
                // Bounded wait for the supervisor to record the result and release
                // the permit; OS shutdown paths must never hang here indefinitely.
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
                while handle.state::<AppState>().activity().current().is_some()
                    && std::time::Instant::now() < deadline
                {
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
                handle.exit(0);
            });
            Ok(QuitResult::Quitting)
        }
    }
}

/// Debug builds only: starts the lifecycle test child (docs/spec/phases/P03).
#[cfg(debug_assertions)]
#[tauri::command]
pub fn start_lifecycle_test<R: Runtime>(
    app: AppHandle<R>,
    request: MutationRequest<crate::lifecycle::harness::Scenario>,
) -> LauncherResult<crate::state::dto::RunSummary> {
    crate::state::ledger::validate_request_id(&request.request_id)?;
    crate::lifecycle::harness::start(&app, request.payload)
}

/// Debug builds only: stops the running lifecycle test child.
#[cfg(debug_assertions)]
#[tauri::command]
pub fn stop_lifecycle_test<R: Runtime>(app: AppHandle<R>, run_id: String) -> bool {
    app.state::<RunHandles>().kill(&run_id)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EmptyPayload {}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OperationPayload {
    pub operation_id: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationStarted {
    pub operation_id: String,
}

/// Starts the system-browser Microsoft sign-in. The browser is opened by Rust.
#[tauri::command]
pub async fn begin_login(
    auth: State<'_, Arc<AuthService>>,
    request: MutationRequest<EmptyPayload>,
) -> LauncherResult<OperationStarted> {
    crate::state::ledger::validate_request_id(&request.request_id)?;
    let operation_id = auth.inner().begin_login().await?;
    Ok(OperationStarted { operation_id })
}

#[tauri::command(async)]
pub fn cancel_login(
    auth: State<'_, Arc<AuthService>>,
    request: MutationRequest<OperationPayload>,
) -> LauncherResult<()> {
    crate::state::ledger::validate_request_id(&request.request_id)?;
    auth.cancel_login(&request.payload.operation_id)
}

/// Local sign-out of one account: clears its launch authorization and removes
/// its saved credentials (or retries a pending removal).
#[tauri::command]
pub async fn sign_out(
    auth: State<'_, Arc<AuthService>>,
    request: MutationRequest<ProfileIdPayload>,
) -> LauncherResult<()> {
    crate::state::ledger::validate_request_id(&request.request_id)?;
    auth.sign_out(&request.payload.profile_id).await
}

/// Re-checks one account's access with fresh service data (for "Check again").
/// The launch token is not exposed; launches re-verify independently.
#[tauri::command]
pub async fn verify_account(
    auth: State<'_, Arc<AuthService>>,
    request: MutationRequest<ProfileIdPayload>,
) -> LauncherResult<()> {
    crate::state::ledger::validate_request_id(&request.request_id)?;
    auth.verify_for_launch(&request.payload.profile_id)
        .await
        .map(|_| ())
}

/// Adds a named no-token profile for the demo (D-045).
#[tauri::command(async)]
pub fn add_notoken_profile(
    state: State<'_, AppState>,
    request: MutationRequest<AddNoTokenProfilePayload>,
) -> LauncherResult<NoTokenProfileView> {
    state.add_notoken_profile(request)
}

#[tauri::command(async)]
pub fn remove_notoken_profile(
    state: State<'_, AppState>,
    request: MutationRequest<ProfileIdPayload>,
) -> LauncherResult<()> {
    state.remove_notoken_profile(request)
}

/// Chooses the profile installations play as unless they name another one.
#[tauri::command(async)]
pub fn set_default_profile(
    state: State<'_, AppState>,
    request: MutationRequest<SetDefaultProfilePayload>,
) -> LauncherResult<()> {
    state.set_default_profile(request)
}

/// Chooses who plays one installation.
#[tauri::command(async)]
pub fn set_instance_profile(
    state: State<'_, AppState>,
    request: MutationRequest<SetInstanceProfilePayload>,
) -> LauncherResult<InstanceSummary> {
    state.set_instance_profile(request)
}

/// Opens one of the fixed official destinations in the system browser.
#[tauri::command(async)]
pub fn open_external<R: Runtime>(app: AppHandle<R>, link: ExternalLink) -> LauncherResult<()> {
    use tauri_plugin_opener::OpenerExt;
    let url = external::check_allowed(link.url())?;
    app.opener()
        .open_url(url.as_str(), None::<&str>)
        .map_err(|_| {
            crate::error::LauncherError::internal("The system browser could not be opened.")
        })
}

/// The metadata service, absent when DATA_ROOT is unavailable.
pub struct Metadata(pub Option<Arc<MetadataService<PublicClient>>>);

impl Metadata {
    fn get(&self) -> LauncherResult<Arc<MetadataService<PublicClient>>> {
        self.0.clone().ok_or_else(|| {
            crate::error::LauncherError::new(
                crate::error::ErrorCode::StorageUnavailable,
                "Launcher data is unavailable.",
            )
        })
    }
}

/// Supported versions intersected with the Mojang catalog (read-only query).
#[tauri::command]
pub async fn list_supported_versions(
    metadata: State<'_, Metadata>,
    refresh: bool,
) -> LauncherResult<CatalogView> {
    Ok(metadata.get()?.supported_versions(refresh).await)
}

/// Resolves the real game plan for this computer and summarizes it. Downloads
/// only metadata; Java runtime size stays unknown until installation (P07).
#[tauri::command]
pub async fn preview_install_plan(
    sources: State<'_, Sources>,
    version_id: String,
    loader: Option<crate::loader::LoaderChoice>,
) -> LauncherResult<PlanSummary> {
    let source = sources.get()?;
    if let Some(loader) = &loader {
        loader.validate()?;
    }
    let plan = source.plan_game(&version_id, loader.as_ref()).await?;
    Ok(source.metadata().summarize(&plan))
}

/// Plan sources, absent when DATA_ROOT is unavailable.
pub struct Sources(pub Option<Arc<crate::install::service::LiveSource>>);

impl Sources {
    fn get(&self) -> LauncherResult<Arc<crate::install::service::LiveSource>> {
        self.0.clone().ok_or_else(|| {
            crate::error::LauncherError::new(
                crate::error::ErrorCode::StorageUnavailable,
                "Launcher data is unavailable.",
            )
        })
    }
}

/// Game versions a mod loader supports (D-039; read-only query).
#[tauri::command]
pub async fn list_loader_game_versions(
    sources: State<'_, Sources>,
    kind: crate::loader::LoaderKind,
) -> LauncherResult<Vec<String>> {
    sources.get()?.loader_game_versions(kind).await
}

/// Versions of a mod loader for one game version, newest first (D-039).
#[tauri::command]
pub async fn list_loader_versions(
    sources: State<'_, Sources>,
    kind: crate::loader::LoaderKind,
    game_version: String,
) -> LauncherResult<Vec<crate::loader::meta::LoaderVersion>> {
    sources.get()?.loader_versions(kind, &game_version).await
}

/// The install service, absent when DATA_ROOT is unavailable.
pub struct Installer(pub Option<Arc<crate::install::service::InstallService>>);

impl Installer {
    fn get(&self) -> LauncherResult<&crate::install::service::InstallService> {
        self.0.as_deref().ok_or_else(|| {
            crate::error::LauncherError::new(
                crate::error::ErrorCode::StorageUnavailable,
                "Launcher data is unavailable.",
            )
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InstancePayload {
    pub instance_id: crate::storage::paths::InstanceId,
}

/// Installs (or completes and verifies) an installation in the background.
#[tauri::command]
pub async fn install_instance(
    installer: State<'_, Installer>,
    request: MutationRequest<InstancePayload>,
) -> LauncherResult<crate::install::service::OperationStarted> {
    installer.get()?.enqueue(
        &request.request_id,
        crate::storage::models::OperationKind::Install,
        request.payload.instance_id,
    )
}

/// Fully re-verifies an installation against its lock and replaces bad files.
#[tauri::command]
pub async fn repair_instance(
    installer: State<'_, Installer>,
    request: MutationRequest<InstancePayload>,
) -> LauncherResult<crate::install::service::OperationStarted> {
    installer.get()?.enqueue(
        &request.request_id,
        crate::storage::models::OperationKind::Repair,
        request.payload.instance_id,
    )
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AnswerDownloadPayload {
    pub operation_id: String,
    pub allow: bool,
}

/// Allows or declines a download that waits on a metered connection (D-051).
#[tauri::command(async)]
pub fn answer_download(
    installer: State<'_, Installer>,
    request: MutationRequest<AnswerDownloadPayload>,
) -> LauncherResult<()> {
    crate::state::ledger::validate_request_id(&request.request_id)?;
    installer
        .get()?
        .answer_download(&request.payload.operation_id, request.payload.allow)
}

/// Checks the internet connection now instead of at the next interval (D-052).
#[tauri::command(async)]
pub fn check_network(monitor: State<'_, Arc<crate::connectivity::Monitor>>) {
    monitor.check_soon();
}

fn traffic_root(state: &AppState) -> LauncherResult<crate::storage::paths::DataRoot> {
    state.data_root().ok_or_else(|| {
        crate::error::LauncherError::new(
            crate::error::ErrorCode::StorageUnavailable,
            "Launcher data is unavailable.",
        )
    })
}

/// Traffic the launcher downloaded this month and earlier (D-051).
#[tauri::command(async)]
pub fn get_traffic(state: State<'_, AppState>) -> LauncherResult<crate::traffic::TrafficSummary> {
    Ok(crate::traffic::summary(&traffic_root(&state)?))
}

/// Starts counting traffic from zero.
#[tauri::command(async)]
pub fn reset_traffic(
    state: State<'_, AppState>,
    request: MutationRequest<EmptyPayload>,
) -> LauncherResult<crate::traffic::TrafficSummary> {
    crate::state::ledger::validate_request_id(&request.request_id)?;
    let root = traffic_root(&state)?;
    crate::traffic::reset(&root)
        .map_err(|e| crate::error::LauncherError::from_io(&e, "Resetting the traffic count"))?;
    state.console().info("Traffic count reset");
    Ok(crate::traffic::summary(&root))
}

/// Requests cancellation of an install, a repair or a launch that has not
/// started Java yet. A running game is not stopped here; use `stop_game`.
#[tauri::command(async)]
pub fn cancel_operation(
    installer: State<'_, Installer>,
    launcher: State<'_, Launcher>,
    state: State<'_, AppState>,
    request: MutationRequest<OperationPayload>,
) -> LauncherResult<crate::install::service::CancelAck> {
    crate::state::ledger::validate_request_id(&request.request_id)?;
    let operation_id = &request.payload.operation_id;
    // A request still waiting in the queue is simply removed (D-049).
    if installer.get()?.dequeue(operation_id) {
        return Ok(crate::install::service::CancelAck {
            state: "removed".into(),
        });
    }
    match installer.get()?.cancel(operation_id) {
        Err(error) if error.code == crate::error::ErrorCode::OperationNotFound => {}
        other => return other,
    }
    if launcher.get()?.cancel_pending(operation_id).is_some() {
        return Ok(crate::install::service::CancelAck {
            state: "cancel_requested".into(),
        });
    }
    if let Some(run) = state.current_run().filter(|r| r.run_id == *operation_id) {
        // After spawn only the explicit Stop action ends the game.
        let value = serde_json::to_value(run.state).unwrap_or_default();
        return Ok(crate::install::service::CancelAck {
            state: value.as_str().unwrap_or_default().to_owned(),
        });
    }
    Err(crate::error::LauncherError::new(
        crate::error::ErrorCode::OperationNotFound,
        "This operation is not running.",
    ))
}

/// The launch service, absent when DATA_ROOT is unavailable.
pub struct Launcher(pub Option<Arc<crate::launch::service::LaunchService>>);

impl Launcher {
    fn get(&self) -> LauncherResult<&crate::launch::service::LaunchService> {
        self.0.as_deref().ok_or_else(|| {
            crate::error::LauncherError::new(
                crate::error::ErrorCode::StorageUnavailable,
                "Launcher data is unavailable.",
            )
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RunPayload {
    pub run_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LaunchPayload {
    pub instance_id: crate::storage::paths::InstanceId,
    /// A server to start the game straight into (a friend's invite).
    #[serde(default)]
    pub server: Option<String>,
    /// The server ad whose Play started it (D-069), for the statistics'
    /// counts (D-071); only an ad the feed was given, with its server.
    #[serde(default)]
    pub ad: Option<String>,
}

/// Checks the installation and the account of the installation's profile (or
/// uses a no-token profile), then starts Minecraft under supervision (D-045).
/// Debug builds started with `LAUNCHER_NOTOKEN_PROFILE` always play as that
/// test profile (D-030).
#[tauri::command]
pub async fn launch_game(
    launcher: State<'_, Launcher>,
    mods: State<'_, Arc<crate::mods::Mods>>,
    ads: State<'_, Arc<crate::feed::ads::ServerAds>>,
    stats: State<'_, Arc<crate::stats::Stats>>,
    request: MutationRequest<LaunchPayload>,
) -> LauncherResult<crate::state::dto::RunSummary> {
    let launcher = launcher.get()?;
    let id = request.payload.instance_id;
    let ad = request
        .payload
        .ad
        .as_deref()
        .filter(|ad| request.payload.server.is_some() && crate::stats::ad_id(ad) && ads.known(ad))
        .map(|ad| Arc::new(stats.ad_run(ad)));
    // Not while mods are being put into its game folder (D-066).
    mods.content.ensure_idle(id)?;
    #[cfg(debug_assertions)]
    let mode = crate::launch::notoken::NoTokenProfile::from_env()
        .map(crate::launch::service::LaunchMode::NoToken)
        .unwrap_or_else(|| launcher.mode_for(id));
    #[cfg(not(debug_assertions))]
    let mode = launcher.mode_for(id);
    launcher
        .launch_from(
            &request.request_id,
            id,
            mode,
            request.payload.server.as_deref(),
            ad,
        )
        .await
}

/// Terminates the owned game process of `runId` after the user confirmed it.
#[tauri::command(async)]
pub fn stop_game(
    launcher: State<'_, Launcher>,
    request: MutationRequest<RunPayload>,
) -> LauncherResult<bool> {
    crate::state::ledger::validate_request_id(&request.request_id)?;
    Ok(launcher.get()?.stop(&request.payload.run_id))
}

/// A folder inside an installation's game folder that the launcher opens.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GameFolder {
    Mods,
    Config,
    CrashReports,
    Logs,
    Saves,
    Resourcepacks,
    Shaderpacks,
}

impl GameFolder {
    fn name(self) -> &'static str {
        match self {
            Self::Mods => "mods",
            Self::Config => "config",
            Self::CrashReports => "crash-reports",
            Self::Logs => "logs",
            Self::Saves => "saves",
            Self::Resourcepacks => "resourcepacks",
            Self::Shaderpacks => "shaderpacks",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FolderPayload {
    pub instance_id: crate::storage::paths::InstanceId,
    /// A known folder inside the game folder; the game folder itself if absent.
    #[serde(default)]
    pub folder: Option<GameFolder>,
}

/// Opens an installation's game folder, or one of its known folders, in the
/// system file manager. The path is resolved in Rust from the instance ID;
/// the webview never supplies a path.
#[tauri::command(async)]
pub fn open_instance_folder<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
    request: MutationRequest<FolderPayload>,
) -> LauncherResult<()> {
    use tauri_plugin_opener::OpenerExt;
    crate::state::ledger::validate_request_id(&request.request_id)?;
    let id = request.payload.instance_id;
    state.instance(id).ok_or_else(|| {
        crate::error::LauncherError::new(
            crate::error::ErrorCode::InstanceNotFound,
            "The installation no longer exists.",
        )
    })?;
    let root = state.data_root().ok_or_else(|| {
        crate::error::LauncherError::new(
            crate::error::ErrorCode::StorageUnavailable,
            "Launcher data is unavailable.",
        )
    })?;
    let mut dir = root.game_dir(id);
    if let Some(folder) = request.payload.folder {
        dir.push(folder.name());
    }
    std::fs::create_dir_all(&dir)
        .map_err(|e| crate::error::LauncherError::from_io(&e, "Opening the game folder"))?;
    app.opener()
        .open_path(dir.to_string_lossy(), None::<&str>)
        .map_err(|_| crate::error::LauncherError::internal("The file manager could not be opened."))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExportPayload {
    #[serde(default)]
    pub run_id: Option<String>,
    /// The save dialog's title in the interface's language.
    #[serde(default)]
    pub title: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportResult {
    /// False when the user closed the save dialog.
    pub saved: bool,
    /// File name of the written archive (not the full path).
    pub file_name: Option<String>,
}

/// Builds the redacted diagnostic archive and saves it where the user chooses.
#[tauri::command]
pub async fn export_diagnostics<R: Runtime>(
    app: AppHandle<R>,
    request: MutationRequest<ExportPayload>,
) -> LauncherResult<ExportResult> {
    use tauri_plugin_dialog::DialogExt;
    crate::state::ledger::validate_request_id(&request.request_id)?;
    let handle = app.clone();
    let run = request.payload.run_id;
    let title = window_title(request.payload.title, "Save diagnostic export");
    let target = tauri::async_runtime::spawn_blocking(move || {
        handle
            .dialog()
            .file()
            .set_title(title)
            .set_file_name(format!(
                "launcher-diagnostics-{}.zip",
                chrono::Utc::now().format("%Y%m%d-%H%M%S")
            ))
            .add_filter("ZIP archive", &["zip"])
            .blocking_save_file()
    })
    .await
    .map_err(|_| crate::error::LauncherError::internal("The save dialog failed."))?;
    let Some(target) = target else {
        return Ok(ExportResult {
            saved: false,
            file_name: None,
        });
    };
    let path = target.into_path().map_err(|_| {
        crate::error::LauncherError::new(
            crate::error::ErrorCode::InvalidInput,
            "This location cannot be used for the export.",
        )
    })?;
    let state = app.state::<AppState>();
    let root = state.data_root().ok_or_else(|| {
        crate::error::LauncherError::new(
            crate::error::ErrorCode::StorageUnavailable,
            "Launcher data is unavailable.",
        )
    })?;
    let home = app.path().home_dir().ok();
    let bytes = crate::diagnostics::build(state.inner(), &root, run.as_deref(), home.as_deref())?;
    crate::storage::atomic::write(&path, &bytes)
        .map_err(|e| crate::error::LauncherError::from_io(&e, "Saving the diagnostic export"))?;
    Ok(ExportResult {
        saved: true,
        file_name: path.file_name().map(|n| n.to_string_lossy().into_owned()),
    })
}

/// The data folder in use (D-043).
pub struct Location {
    current: std::sync::Mutex<LauncherResult<crate::location::DataLocation>>,
    config_dir: Option<std::path::PathBuf>,
}

impl Location {
    pub fn new(found: LauncherResult<crate::location::DataLocation>) -> Self {
        Self {
            current: std::sync::Mutex::new(found),
            config_dir: None,
        }
    }

    pub fn with_config_dir(mut self, dir: Option<std::path::PathBuf>) -> Self {
        self.config_dir = dir;
        self
    }

    pub fn current(&self) -> LauncherResult<crate::location::DataLocation> {
        self.current
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

/// Where the launcher keeps its data and how it was chosen.
#[tauri::command(async)]
pub fn get_data_location(
    location: State<'_, Location>,
) -> LauncherResult<crate::location::DataLocation> {
    location.current()
}

/// Where to move the launcher data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LocationTarget {
    /// The per-user default folder.
    Default,
    /// A folder the user chose.
    Custom,
    /// Beside the launcher, in portable mode.
    Portable,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MoveDataPayload {
    pub target: LocationTarget,
    /// The folder for `custom`.
    #[serde(default)]
    pub path: Option<String>,
}

/// Copies the launcher data to another folder while nothing else runs and
/// records it for the next start. The old folder is kept.
#[tauri::command]
pub async fn move_data_location<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
    location: State<'_, Location>,
    request: MutationRequest<MoveDataPayload>,
) -> LauncherResult<crate::location::DataLocation> {
    use crate::location::{self as loc, LocationMode};
    crate::state::ledger::validate_request_id(&request.request_id)?;
    let current = location.current()?;
    if current.pending_path.is_some() {
        return Err(crate::error::LauncherError::invalid_input(
            "The data folder was already moved. Restart the launcher to use it.",
        ));
    }
    let config_dir = location.config_dir.clone().ok_or_else(|| {
        crate::error::LauncherError::internal("The configuration folder is unknown.")
    })?;
    let launcher_dir = loc::launcher_folder();
    let payload = request.payload;
    let target = match payload.target {
        LocationTarget::Default => current.default_path.clone(),
        LocationTarget::Portable => current.portable_path.clone().ok_or_else(|| {
            crate::error::LauncherError::internal("The launcher's own folder is unknown.")
        })?,
        LocationTarget::Custom => std::path::PathBuf::from(
            payload
                .path
                .as_deref()
                .map(str::trim)
                .filter(|p| !p.is_empty())
                .ok_or_else(|| crate::error::LauncherError::invalid_input("Choose a folder."))?,
        ),
    };
    let (target_root, target_state) = loc::check_target(&current.path, &target)?;
    let reuse = target_state == loc::TargetState::LauncherData;
    let operation_id = uuid::Uuid::new_v4().to_string();
    let permit = state.activity().try_acquire(
        crate::storage::models::OperationKind::Relocate,
        None,
        operation_id,
    )?;
    state.announce_runs();
    state.console().info(if reuse {
        format!("Switching to the launcher data in {}", target.display())
    } else {
        format!(
            "Copying launcher data from {} to {}",
            current.path.display(),
            target.display()
        )
    });
    let (from, to) = (current.path.clone(), target_root.path().to_path_buf());
    let progress_app = app.clone();
    let copied = tauri::async_runtime::spawn_blocking(move || {
        let report = |files: u64, bytes: u64| {
            progress_app.state::<AppState>().console().info(format!(
                "Copied {files} files ({} MiB)",
                bytes / (1024 * 1024)
            ));
        };
        if reuse {
            return Ok(());
        }
        let result = loc::copy_tree(&from, &to, &report);
        if result.is_err() {
            // The target was empty or missing; leave nothing half-copied.
            let _ = std::fs::remove_dir_all(&to);
        }
        result
    })
    .await
    .map_err(|_| crate::error::LauncherError::internal("Copying stopped unexpectedly."));
    let result = copied.and_then(|r| r).and_then(|()| match payload.target {
        LocationTarget::Portable => {
            let dir = launcher_dir.as_deref().ok_or_else(|| {
                crate::error::LauncherError::internal("The launcher's own folder is unknown.")
            })?;
            loc::write_marker(dir)
        }
        LocationTarget::Default | LocationTarget::Custom => {
            if current.mode == LocationMode::Portable
                && let Some(dir) = launcher_dir.as_deref()
            {
                std::fs::remove_file(dir.join(loc::PORTABLE_MARKER)).map_err(|e| {
                    crate::error::LauncherError::from_io(&e, "Turning off portable mode")
                })?;
            }
            let pointer = (payload.target == LocationTarget::Custom).then_some(target.as_path());
            loc::write_pointer(&config_dir, pointer)
        }
    });
    drop(permit);
    state.announce_runs();
    match result {
        Ok(()) => {
            state
                .console()
                .info("Launcher data copied; the new folder is used from the next start");
            let mut guard = location
                .current
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Ok(found) = guard.as_mut() {
                found.pending_path = Some(target);
                found.pending_reused = reuse;
            }
            guard.clone()
        }
        Err(error) => {
            state.console().warn(format!(
                "Moving the launcher data failed: {}",
                error.message
            ));
            Err(error)
        }
    }
}

/// Lets the user pick a folder for the launcher data; `None` when cancelled.
#[tauri::command]
pub async fn pick_data_folder<R: Runtime>(
    app: AppHandle<R>,
    title: Option<String>,
) -> LauncherResult<Option<String>> {
    use tauri_plugin_dialog::DialogExt;
    let title = window_title(title, "Choose a folder for the launcher data");
    let picked = tauri::async_runtime::spawn_blocking(move || {
        app.dialog().file().set_title(title).blocking_pick_folder()
    })
    .await
    .map_err(|_| crate::error::LauncherError::internal("The folder dialog failed."))?;
    Ok(picked
        .and_then(|p| p.into_path().ok())
        .and_then(|p| p.to_str().map(str::to_owned)))
}

/// An error in the window's code (D-061): written to the console and kept in
/// the launcher's failure records.
#[tauri::command(async)]
pub fn report_frontend_error(state: State<'_, AppState>, message: String, stack: Option<String>) {
    let first: String = message
        .lines()
        .next()
        .unwrap_or_default()
        .chars()
        .take(300)
        .collect();
    state
        .console()
        .error(format!("Error in the window: {first}"));
    crate::journal::frontend_error(&message, stack.as_deref());
}

/// An error the window showed to the player: its code and diagnostic ID go to
/// the launcher log, so a report can be matched with what the player saw.
#[tauri::command(async)]
pub fn note_shown_error(
    state: State<'_, AppState>,
    command: String,
    code: String,
    diagnostic_id: String,
) {
    let plain = |s: &str| {
        s.len() <= 64
            && s.bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
    };
    if plain(&command) && plain(&code) && plain(&diagnostic_id) {
        state.console().warn(format!(
            "{command} failed: {code} (diagnostic {diagnostic_id})"
        ));
    }
}

/// The launcher's own failures, newest first, for the "send a report" offer
/// (D-061).
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FailureSummary {
    pub id: String,
    pub kind: crate::journal::FailureKind,
    pub at: chrono::DateTime<chrono::Utc>,
    pub version: String,
    pub message: String,
    pub reported: bool,
}

#[tauri::command(async)]
pub fn list_launcher_failures(state: State<'_, AppState>) -> Vec<FailureSummary> {
    let Some(root) = state.data_root() else {
        return Vec::new();
    };
    crate::journal::failures(&root.logs_dir())
        .into_iter()
        .map(|f| FailureSummary {
            message: f
                .message
                .lines()
                .next()
                .unwrap_or_default()
                .chars()
                .take(300)
                .collect(),
            id: f.id,
            kind: f.kind,
            at: f.at,
            version: f.version,
            reported: f.reported,
        })
        .collect()
}

/// The player chose not to send this failure; it is no longer offered.
#[tauri::command(async)]
pub fn dismiss_launcher_failure(state: State<'_, AppState>, id: String) -> LauncherResult<()> {
    let root = state.data_root().ok_or_else(|| {
        crate::error::LauncherError::new(
            crate::error::ErrorCode::StorageUnavailable,
            "Launcher data is unavailable.",
        )
    })?;
    crate::journal::mark_reported(&root.logs_dir(), &id)
        .map_err(|e| crate::error::LauncherError::from_io(&e, "Saving the failure record"))
}

/// What a crash report would send, scrubbed, for the player to read first.
#[tauri::command]
pub async fn preview_crash_report<R: Runtime>(
    app: AppHandle<R>,
    subject: crate::reports::Subject,
) -> LauncherResult<crate::reports::Preview> {
    tauri::async_runtime::spawn_blocking(move || {
        let home = app.path().home_dir().ok();
        let reports = app.state::<Arc<crate::reports::Reports>>();
        reports.preview(&app.state::<AppState>(), &subject, home.as_deref())
    })
    .await
    .map_err(|_| {
        crate::error::LauncherError::internal("Preparing the report stopped unexpectedly.")
    })?
}

/// Sends the files the player kept, with their note.
#[tauri::command]
pub async fn send_crash_report<R: Runtime>(
    app: AppHandle<R>,
    preview_id: String,
    files: Vec<String>,
    note: String,
) -> LauncherResult<crate::reports::Delivery> {
    let home = app.path().home_dir().ok();
    let reports = Arc::clone(&app.state::<Arc<crate::reports::Reports>>());
    reports
        .send(
            &app.state::<AppState>(),
            &preview_id,
            &files,
            &note,
            home.as_deref(),
        )
        .await
}

/// "Did this explanation help?" (D-061 stage 3).
#[tauri::command]
pub async fn send_explanation_feedback(
    state: State<'_, AppState>,
    reports: State<'_, Arc<crate::reports::Reports>>,
    kind: String,
    signature: String,
    helpful: bool,
) -> LauncherResult<()> {
    let plain = kind.len() <= 40 && kind.bytes().all(|b| b.is_ascii_lowercase() || b == b'_');
    if !plain || signature.len() > 300 {
        return Err(crate::error::LauncherError::invalid_input(
            "This answer is not valid.",
        ));
    }
    reports.feedback(&state, &kind, &signature, helpful).await
}

fn reviews_of<R: Runtime>(app: &AppHandle<R>) -> Arc<crate::reviews::Reviews> {
    Arc::clone(&app.state::<Arc<crate::reviews::Reviews>>())
}

fn launcher_account_of<R: Runtime>(
    app: &AppHandle<R>,
) -> Arc<crate::launcher_account::LauncherAccount> {
    Arc::clone(&app.state::<Arc<crate::launcher_account::LauncherAccount>>())
}

/// The latest logs for a review (D-070), scrubbed, for the player to read
/// and pick from: of an installation, or of one game run.
#[tauri::command]
pub async fn reviews_logs<R: Runtime>(
    app: AppHandle<R>,
    instance_id: Option<String>,
    run_id: Option<String>,
) -> LauncherResult<crate::reviews::Found> {
    tauri::async_runtime::spawn_blocking(move || {
        let home = app.path().home_dir().ok();
        reviews_of(&app).logs(
            &app.state::<AppState>(),
            instance_id.as_deref(),
            run_id.as_deref(),
            home.as_deref(),
        )
    })
    .await
    .map_err(|_| {
        crate::error::LauncherError::internal("Collecting the logs stopped unexpectedly.")
    })?
}

/// A text file the player attached to a review, scrubbed like the logs.
#[tauri::command]
pub async fn reviews_text_file<R: Runtime>(
    app: AppHandle<R>,
    name: String,
    text: String,
) -> LauncherResult<crate::reviews::Found> {
    if text.len() > 4 * crate::reviews::MAX_TEXT_BYTES || name.len() > 1024 {
        return Err(crate::error::LauncherError::invalid_input(
            "This file is too large to attach.",
        ));
    }
    tauri::async_runtime::spawn_blocking(move || {
        let home = app.path().home_dir().ok();
        reviews_of(&app).text_file(&app.state::<AppState>(), &name, &text, home.as_deref())
    })
    .await
    .map_err(|_| crate::error::LauncherError::internal("Reading the file stopped unexpectedly."))?
}

/// A screenshot from the gallery for a review: scaled to 2560 pixels and
/// written again as PNG (nothing but pixels), base64.
#[tauri::command]
pub async fn reviews_screenshot<R: Runtime>(
    app: AppHandle<R>,
    instance_id: String,
    file: String,
) -> LauncherResult<String> {
    let id: crate::storage::paths::InstanceId = instance_id.parse().map_err(|_| {
        crate::error::LauncherError::invalid_input("This screenshot does not exist.")
    })?;
    blocking(app, move |state| {
        let path = crate::gallery::path(state, id, &file)?;
        let png = crate::gallery::make_thumbnail(&path, 2560)?;
        Ok(base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            png,
        ))
    })
    .await
}

/// Sends a review, or keeps it to send once the service can be reached.
#[tauri::command]
pub async fn reviews_send<R: Runtime>(
    app: AppHandle<R>,
    draft: crate::reviews::Draft,
) -> LauncherResult<crate::reviews::Delivery> {
    let home = app.path().home_dir().ok();
    let accounts = launcher_account_of(&app);
    reviews_of(&app)
        .send(&app.state::<AppState>(), &*accounts, draft, home.as_deref())
        .await
}

/// The player's reviews and the answers; `refresh` asks the service now.
#[tauri::command]
pub async fn reviews_mine<R: Runtime>(
    app: AppHandle<R>,
    refresh: bool,
) -> LauncherResult<crate::reviews::Mine> {
    let accounts = launcher_account_of(&app);
    reviews_of(&app)
        .mine(&app.state::<AppState>(), &*accounts, refresh)
        .await
}

/// The player has read the answers to their reviews.
#[tauri::command]
pub async fn reviews_seen<R: Runtime>(app: AppHandle<R>) -> LauncherResult<()> {
    blocking(app.clone(), move |state| reviews_of(&app).seen(state)).await
}

/// Deletes one of the player's reviews, on the service too.
#[tauri::command]
pub async fn reviews_delete<R: Runtime>(app: AppHandle<R>, id: i64) -> LauncherResult<()> {
    reviews_of(&app).delete(&app.state::<AppState>(), id).await
}

/// Looks for a launcher update now (D-063).
#[tauri::command]
pub async fn check_for_update<R: Runtime>(app: AppHandle<R>) -> crate::update::UpdateView {
    let updates = Arc::clone(&app.state::<Arc<crate::update::Updates>>());
    updates.check(&app, true).await;
    crate::update::refresh_skins_mod(&app.state::<AppState>(), true).await;
    app.state::<AppState>().update()
}

/// Downloads the offered update (notify mode, metered connection).
#[tauri::command]
pub async fn download_update<R: Runtime>(app: AppHandle<R>) -> crate::update::UpdateView {
    let updates = Arc::clone(&app.state::<Arc<crate::update::Updates>>());
    updates.download(&app).await;
    app.state::<AppState>().update()
}

/// Installs the downloaded update and starts the new version.
#[tauri::command]
pub fn restart_to_update<R: Runtime>(app: AppHandle<R>) -> LauncherResult<()> {
    let updates = Arc::clone(&app.state::<Arc<crate::update::Updates>>());
    updates.restart_now(&app)
}

/// "What's new" was read.
#[tauri::command(async)]
pub fn dismiss_whats_new(state: State<'_, AppState>) {
    crate::update::Updates::dismiss_whats_new(&state);
}

/// The interface has shown: this start of a new version can count as good.
#[tauri::command(async)]
pub fn interface_ready<R: Runtime>(app: AppHandle<R>) {
    app.state::<Arc<crate::update::Updates>>()
        .interface_shown(&app);
}

/// Opens the launcher account's sign-in window on `screen`: `login` or
/// `register` (D-064).
#[tauri::command]
pub fn launcher_account_open<R: Runtime>(app: AppHandle<R>, screen: String, title: Option<String>) {
    let account = Arc::clone(&app.state::<Arc<crate::launcher_account::LauncherAccount>>());
    let handle = app.clone();
    let fallback = if screen == "register" {
        "Launcher account: create"
    } else {
        "Launcher account: sign in"
    };
    let title = window_title(title, fallback);
    // Queued, never inline: this command runs on the main thread, where a new
    // window would stay empty and hang on Windows.
    crate::lifecycle::queue_on_main_thread(&app, move || account.open(&handle, &screen, &title));
}

#[tauri::command]
pub async fn launcher_account_sign_out<R: Runtime>(app: AppHandle<R>) {
    let account = Arc::clone(&app.state::<Arc<crate::launcher_account::LauncherAccount>>());
    account.sign_out(&app.state::<AppState>()).await;
    friends(&app).wake();
    skins(&app).forget();
}

/// Changes the nick; what went wrong comes back as a problem code.
#[tauri::command]
pub async fn launcher_account_set_nick<R: Runtime>(
    app: AppHandle<R>,
    nick: String,
) -> Option<crate::launcher_account::AccountProblem> {
    if nick.len() > 64 {
        return Some(crate::launcher_account::AccountProblem::NickInvalid);
    }
    let account = Arc::clone(&app.state::<Arc<crate::launcher_account::LauncherAccount>>());
    account
        .set_nick(&app.state::<AppState>(), nick.trim())
        .await
        .err()
}

/// Reads the account from the server again (after the player confirmed the
/// email in a browser); friends tries again with the fresh token. Whether the
/// email is confirmed now, or what went wrong.
#[tauri::command]
pub async fn launcher_account_recheck<R: Runtime>(
    app: AppHandle<R>,
) -> Result<bool, crate::launcher_account::AccountProblem> {
    let account = Arc::clone(&app.state::<Arc<crate::launcher_account::LauncherAccount>>());
    let result = account.recheck(&app.state::<AppState>()).await;
    friends(&app).wake();
    result
}

#[tauri::command]
pub async fn launcher_account_resend_confirmation<R: Runtime>(
    app: AppHandle<R>,
) -> Option<crate::launcher_account::AccountProblem> {
    let account = Arc::clone(&app.state::<Arc<crate::launcher_account::LauncherAccount>>());
    account
        .resend_confirmation(&app.state::<AppState>())
        .await
        .err()
}

/// Saves what the account service and friends keep about the launcher
/// account as one JSON file where the player chooses. `Ok(None)` when the
/// save dialog was closed; else the file's name.
#[tauri::command]
pub async fn launcher_account_export<R: Runtime>(
    app: AppHandle<R>,
    title: Option<String>,
) -> Result<Option<String>, crate::launcher_account::AccountProblem> {
    use crate::launcher_account::AccountProblem;
    use tauri_plugin_dialog::DialogExt;
    let account = Arc::clone(&app.state::<Arc<crate::launcher_account::LauncherAccount>>());
    let state = app.state::<AppState>();
    let nick = state
        .accounts_file()
        .launcher_account
        .map(|a| a.nick)
        .unwrap_or_default();
    let mut data = serde_json::json!({
        "exported": chrono::Utc::now().to_rfc3339(),
        "account": account.export(&state).await?,
    });
    // A service may be down; what the others keep still comes.
    let (friends_data, worlds_data, mods_data, reviews_data) = tokio::join!(
        async { friends(&app).export(&app).await.ok() },
        async { worlds(&app).client.export().await.ok() },
        async { mods(&app).client.export(&app).await.ok() },
        async { reviews_of(&app).export(&state, account.as_ref()).await },
    );
    for (name, value) in [
        ("friends", friends_data),
        ("worlds", worlds_data),
        ("mods", mods_data),
        ("reviews", reviews_data),
    ] {
        data[name] = value.unwrap_or(serde_json::Value::Null);
    }
    let title = window_title(title, "Save my launcher account data");
    let handle = app.clone();
    let name = format!(
        "launcher-account-{nick}-{}.json",
        chrono::Utc::now().format("%Y%m%d")
    );
    let target = tauri::async_runtime::spawn_blocking(move || {
        handle
            .dialog()
            .file()
            .set_title(title)
            .set_file_name(name)
            .add_filter("JSON", &["json"])
            .blocking_save_file()
    })
    .await
    .map_err(|_| AccountProblem::NotSaved)?;
    let Some(path) = target.and_then(|p| p.into_path().ok()) else {
        return Ok(None);
    };
    let text = serde_json::to_vec_pretty(&data).map_err(|_| AccountProblem::NotSaved)?;
    std::fs::write(&path, text).map_err(|_| AccountProblem::NotSaved)?;
    state
        .console()
        .info("Launcher account: the data export was saved");
    Ok(path.file_name().and_then(|n| n.to_str()).map(str::to_owned))
}

// Friends and chats (D-064). What went wrong comes back as a problem code.

fn friends<R: Runtime>(app: &AppHandle<R>) -> Arc<crate::friends::Friends> {
    Arc::clone(&app.state::<Arc<crate::friends::Friends>>())
}

type FriendsResult<T> = Result<T, crate::friends::FriendsProblem>;

#[tauri::command]
pub fn friends_state<R: Runtime>(app: AppHandle<R>) -> crate::friends::FriendsView {
    friends(&app).view()
}

#[tauri::command]
pub async fn friends_request<R: Runtime>(app: AppHandle<R>, nick: String) -> FriendsResult<String> {
    if nick.len() > 64 {
        return Err(crate::friends::FriendsProblem::Invalid);
    }
    friends(&app).request(&app, nick.trim()).await
}

#[tauri::command]
pub async fn friends_act<R: Runtime>(
    app: AppHandle<R>,
    action: String,
    id: String,
) -> FriendsResult<()> {
    friends(&app).act(&app, &action, &id).await
}

#[tauri::command]
pub async fn friends_open_direct<R: Runtime>(
    app: AppHandle<R>,
    id: String,
) -> FriendsResult<String> {
    if uuid::Uuid::parse_str(&id).is_err() {
        return Err(crate::friends::FriendsProblem::Invalid);
    }
    friends(&app).open_direct(&app, &id).await
}

#[tauri::command]
pub async fn friends_create_group<R: Runtime>(
    app: AppHandle<R>,
    name: String,
    members: Vec<String>,
) -> FriendsResult<String> {
    if name.len() > 200 || members.len() > 20 {
        return Err(crate::friends::FriendsProblem::Invalid);
    }
    friends(&app).create_group(&app, &name, &members).await
}

#[tauri::command]
pub async fn friends_change_room<R: Runtime>(
    app: AppHandle<R>,
    room: String,
    action: String,
    value: String,
) -> FriendsResult<()> {
    if value.len() > 200 {
        return Err(crate::friends::FriendsProblem::Invalid);
    }
    friends(&app)
        .change_room(&app, &room, &action, &value)
        .await
}

#[tauri::command]
pub async fn friends_messages<R: Runtime>(
    app: AppHandle<R>,
    room: String,
    before: Option<i64>,
) -> FriendsResult<Vec<crate::friends::ChatMessage>> {
    friends(&app).messages(&app, &room, before).await
}

#[tauri::command]
pub async fn friends_send<R: Runtime>(
    app: AppHandle<R>,
    room: String,
    body: String,
) -> FriendsResult<crate::friends::ChatMessage> {
    friends(&app).send(&app, &room, &body).await
}

#[tauri::command]
pub async fn friends_read<R: Runtime>(
    app: AppHandle<R>,
    room: String,
    upto: i64,
) -> FriendsResult<()> {
    friends(&app).read(&app, &room, upto).await
}

/// Declines every friend request waiting for the account.
#[tauri::command]
pub async fn friends_decline_all<R: Runtime>(app: AppHandle<R>) -> FriendsResult<()> {
    friends(&app).decline_all(&app).await
}

/// Reports a waiting friend request as spam.
#[tauri::command]
pub async fn friends_report_request<R: Runtime>(
    app: AppHandle<R>,
    id: String,
) -> FriendsResult<()> {
    friends(&app).report_request(&app, &id).await
}

/// Shows or hides the account's online status from its friends.
#[tauri::command]
pub async fn friends_set_visible<R: Runtime>(
    app: AppHandle<R>,
    visible: bool,
) -> FriendsResult<()> {
    friends(&app).set_visible(&app, visible).await
}

#[tauri::command]
pub async fn friends_report<R: Runtime>(
    app: AppHandle<R>,
    message: i64,
    reason: String,
) -> FriendsResult<()> {
    friends(&app).report(&app, message, &reason).await
}

// The launcher account's skin and cape (D-065). What went wrong comes back as
// a problem code. The interface never names a file: it picks one in the
// dialog or sends the bytes of one dropped on it, and uploads what was picked
// for a kind.

fn skins<R: Runtime>(app: &AppHandle<R>) -> Arc<crate::skins::Skins> {
    Arc::clone(&app.state::<Arc<crate::skins::Skins>>())
}

type SkinsResult<T> = Result<T, crate::skins::SkinsProblem>;

/// What the account wears.
#[tauri::command]
pub async fn skins_mine<R: Runtime>(app: AppHandle<R>) -> SkinsResult<crate::skins::Wardrobe> {
    skins(&app).mine(&app).await
}

/// A worn texture by its hash, as a `data:` URL for the preview.
#[tauri::command]
pub async fn skins_texture<R: Runtime>(app: AppHandle<R>, hash: String) -> SkinsResult<String> {
    skins(&app).texture(&hash).await
}

/// Other players' heads by nick, for friends, rooms and chats: a square PNG
/// as a `data:` URL each, or none.
#[tauri::command]
pub async fn skins_heads<R: Runtime>(
    app: AppHandle<R>,
    nicks: Vec<String>,
) -> SkinsResult<std::collections::BTreeMap<String, Option<String>>> {
    if nicks.len() > crate::skins::MAX_HEAD_NICKS {
        return Err(crate::skins::SkinsProblem::Invalid);
    }
    skins(&app).heads(&nicks).await
}

/// The heads of the Microsoft accounts signed in on this computer, by
/// profile ID: the skin Mojang serves for each, or none (the default skin).
#[tauri::command]
pub async fn skins_microsoft_heads<R: Runtime>(
    app: AppHandle<R>,
) -> SkinsResult<std::collections::BTreeMap<String, Option<String>>> {
    let ids: Vec<String> = app
        .state::<AppState>()
        .accounts_file()
        .accounts
        .into_iter()
        .map(|a| a.profile_id)
        .collect();
    Ok(skins(&app).microsoft_heads(&ids).await)
}

/// Takes the skin or cape another player wears, by nick, as a picked one to
/// try on and upload.
#[tauri::command]
pub async fn skins_by_nick<R: Runtime>(
    app: AppHandle<R>,
    kind: crate::skins::Kind,
    nick: String,
) -> SkinsResult<crate::skins::PickedView> {
    skins(&app).by_nick(kind, &nick).await
}

/// Picks a PNG, GIF or animated PNG for the skin or cape in the native
/// dialog and checks it. `Ok(None)` when the dialog was closed.
#[tauri::command]
pub async fn skins_pick<R: Runtime>(
    app: AppHandle<R>,
    kind: crate::skins::Kind,
    title: Option<String>,
) -> SkinsResult<Option<crate::skins::PickedView>> {
    use tauri_plugin_dialog::DialogExt;
    let fallback = match kind {
        crate::skins::Kind::Skin => "Choose a skin (PNG, GIF or APNG)",
        crate::skins::Kind::Cape => "Choose a cape (PNG, GIF or APNG)",
    };
    let title = window_title(title, fallback);
    let handle = app.clone();
    let picked = tauri::async_runtime::spawn_blocking(move || {
        handle
            .dialog()
            .file()
            .set_title(title)
            .add_filter("PNG, GIF, APNG", &["png", "gif", "apng"])
            .blocking_pick_file()
    })
    .await
    .map_err(|_| crate::skins::SkinsProblem::Unreadable)?;
    let Some(path) = picked.and_then(|p| p.into_path().ok()) else {
        return Ok(None);
    };
    skins(&app).pick(kind, path).await.map(Some)
}

/// Checks a file dropped on the page as a picked one: its name and its bytes
/// as base64 (the window takes no drops itself, so no path ever comes).
#[tauri::command]
pub async fn skins_take<R: Runtime>(
    app: AppHandle<R>,
    kind: crate::skins::Kind,
    name: String,
    data: String,
) -> SkinsResult<crate::skins::PickedView> {
    use crate::skins::SkinsProblem;
    use base64::Engine as _;
    if name.len() > crate::skins::DROPPED_NAME_BYTES {
        return Err(SkinsProblem::Invalid);
    }
    if data.len() > crate::skins::base64_len(crate::skins_import::IMPORT_MAX_BYTES) {
        return Err(SkinsProblem::TooLarge);
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data.as_bytes())
        .map_err(|_| SkinsProblem::BadPng)?;
    drop(data);
    skins(&app).take(kind, &name, bytes).await
}

/// Uploads the PNG picked for `kind`: a skin with its model, an animation
/// with one frame duration or one per frame.
#[tauri::command]
pub async fn skins_upload<R: Runtime>(
    app: AppHandle<R>,
    kind: crate::skins::Kind,
    slim: bool,
    frame_ms: Option<Vec<u32>>,
) -> SkinsResult<crate::skins::Wardrobe> {
    if frame_ms
        .as_ref()
        .is_some_and(|list| list.len() > crate::skins::MAX_FRAMES as usize)
    {
        return Err(crate::skins::SkinsProblem::BadFrameMs);
    }
    let wardrobe = skins(&app)
        .upload(&app, kind, slim, frame_ms.as_deref())
        .await?;
    app.state::<AppState>().console().info(format!(
        "Launcher account: the {} was uploaded",
        kind.as_str()
    ));
    Ok(wardrobe)
}

/// The account's wardrobe: what it wears, keeps and may keep.
#[tauri::command]
pub async fn skins_wardrobe<R: Runtime>(app: AppHandle<R>) -> SkinsResult<crate::skins::Closet> {
    skins(&app).closet(&app).await
}

/// Uploads the PNG picked for `kind` into the wardrobe without wearing it,
/// as `skins_upload` does, with a name (up to 32 characters; empty lets the
/// interface name it).
#[tauri::command]
pub async fn skins_keep<R: Runtime>(
    app: AppHandle<R>,
    kind: crate::skins::Kind,
    slim: bool,
    frame_ms: Option<Vec<u32>>,
    name: String,
) -> SkinsResult<crate::skins::Closet> {
    if frame_ms
        .as_ref()
        .is_some_and(|list| list.len() > crate::skins::MAX_FRAMES as usize)
    {
        return Err(crate::skins::SkinsProblem::BadFrameMs);
    }
    let closet = skins(&app)
        .keep_picked(&app, kind, slim, frame_ms.as_deref(), &name)
        .await?;
    app.state::<AppState>().console().info(format!(
        "Launcher account: the {} was kept in the wardrobe",
        kind.as_str()
    ));
    Ok(closet)
}

/// Keeps what is worn, renames, marks, removes or puts on a wardrobe item
/// or set; the wardrobe as it is then.
#[tauri::command]
pub async fn skins_wardrobe_change<R: Runtime>(
    app: AppHandle<R>,
    change: crate::skins::WardrobeChange,
) -> SkinsResult<crate::skins::Closet> {
    let closet = skins(&app).change(&app, &change).await?;
    app.state::<AppState>()
        .console()
        .info(format!("Launcher account: wardrobe {}", change.word()));
    Ok(closet)
}

/// Takes the skin or cape off.
#[tauri::command]
pub async fn skins_remove<R: Runtime>(
    app: AppHandle<R>,
    kind: crate::skins::Kind,
) -> SkinsResult<crate::skins::Wardrobe> {
    let wardrobe = skins(&app).remove(&app, kind).await?;
    app.state::<AppState>().console().info(format!(
        "Launcher account: the {} was removed",
        kind.as_str()
    ));
    Ok(wardrobe)
}

/// Saves a picture of the character the page drew (a PNG as base64, without
/// the `data:` prefix) where the player chooses. `Ok(None)` when the save
/// dialog was closed; else the file's name.
#[tauri::command]
pub async fn skins_save_picture<R: Runtime>(
    app: AppHandle<R>,
    data: String,
    name: String,
    title: Option<String>,
) -> SkinsResult<Option<String>> {
    use crate::skins::SkinsProblem;
    use tauri_plugin_dialog::DialogExt;
    if name.len() > crate::skins::DROPPED_NAME_BYTES {
        return Err(SkinsProblem::Invalid);
    }
    let png = tauri::async_runtime::spawn_blocking(move || crate::skins::picture(&data))
        .await
        .map_err(|_| SkinsProblem::Invalid)??;
    let title = window_title(title, "Save a picture of the skin");
    let file_name = crate::skins::picture_file_name(&name);
    let handle = app.clone();
    let target = tauri::async_runtime::spawn_blocking(move || {
        handle
            .dialog()
            .file()
            .set_title(title)
            .set_file_name(file_name)
            .add_filter("PNG", &["png"])
            .blocking_save_file()
    })
    .await
    .map_err(|_| SkinsProblem::NotSaved)?;
    let Some(path) = target.and_then(|p| p.into_path().ok()) else {
        return Ok(None);
    };
    let saved = path.file_name().and_then(|n| n.to_str()).map(str::to_owned);
    tauri::async_runtime::spawn_blocking(move || std::fs::write(&path, png))
        .await
        .map_err(|_| SkinsProblem::NotSaved)?
        .map_err(|_| SkinsProblem::NotSaved)?;
    app.state::<AppState>()
        .console()
        .info("Launcher account: a picture of the skin was saved");
    Ok(saved)
}

/// Opens where to get the update by hand (a `.deb`, or after installing
/// failed); only addresses on upd.lapi.sh.
#[tauri::command(async)]
pub fn open_update_page<R: Runtime>(app: AppHandle<R>) -> LauncherResult<()> {
    use tauri_plugin_opener::OpenerExt;
    let url = app
        .state::<AppState>()
        .update()
        .download_url
        .and_then(|u| url::Url::parse(&u).ok())
        .filter(|u| u.scheme() == "https" && u.host_str() == Some("upd.lapi.sh"))
        .ok_or_else(|| {
            crate::error::LauncherError::invalid_input("There is no update page to open.")
        })?;
    app.opener()
        .open_url(url.as_str(), None::<&str>)
        .map_err(|_| {
            crate::error::LauncherError::internal("The system browser could not be opened.")
        })
}

// ----- the catalog of mods (D-066) -----

fn mods<R: Runtime>(app: &AppHandle<R>) -> Arc<crate::mods::Mods> {
    Arc::clone(&app.state::<Arc<crate::mods::Mods>>())
}

/// Projects of the catalog.
#[tauri::command]
pub async fn mods_search<R: Runtime>(
    app: AppHandle<R>,
    query: crate::mods::SearchQuery,
) -> LauncherResult<crate::mods::SearchPage> {
    crate::mods::online(&app.state::<AppState>())?;
    mods(&app).client.search(&query).await
}

/// How many projects a search finds in each category of its kind, the
/// categories it asks for aside (D-072).
#[tauri::command]
pub async fn mods_categories<R: Runtime>(
    app: AppHandle<R>,
    query: crate::mods::SearchQuery,
) -> LauncherResult<Vec<crate::mods::CategoryCount>> {
    crate::mods::online(&app.state::<AppState>())?;
    mods(&app).client.categories(&query).await
}

/// Up to five projects like one, for a game version and a loader.
#[tauri::command]
pub async fn mods_similar<R: Runtime>(
    app: AppHandle<R>,
    project: String,
    game: Option<String>,
    loader: Option<String>,
) -> LauncherResult<Vec<crate::mods::Hit>> {
    crate::mods::online(&app.state::<AppState>())?;
    mods(&app)
        .client
        .similar(&project, game.as_deref(), loader.as_deref())
        .await
}

/// The catalog's collections for a kind, a game version and a loader.
#[tauri::command]
pub async fn mods_collections<R: Runtime>(
    app: AppHandle<R>,
    kind: String,
    game: Option<String>,
    loader: Option<String>,
) -> LauncherResult<Vec<crate::mods::Collection>> {
    crate::mods::online(&app.state::<AppState>())?;
    mods(&app)
        .client
        .collections(&kind, game.as_deref(), loader.as_deref())
        .await
}

/// A page of a project's versions, newest first, with what they are for.
#[tauri::command]
pub async fn mods_versions_page<R: Runtime>(
    app: AppHandle<R>,
    project: String,
    query: crate::mods::VersionsQuery,
) -> LauncherResult<crate::mods::VersionPage> {
    crate::mods::online(&app.state::<AppState>())?;
    mods(&app).client.versions_page(&project, &query).await
}

/// A project's page, by our ID or its slug.
#[tauri::command]
pub async fn mods_project<R: Runtime>(
    app: AppHandle<R>,
    project: String,
) -> LauncherResult<crate::mods::Project> {
    use crate::mods::Catalog;
    crate::mods::online(&app.state::<AppState>())?;
    mods(&app).client.project(&project).await
}

/// A project's versions for a game version and loader, newest first.
#[tauri::command]
pub async fn mods_versions<R: Runtime>(
    app: AppHandle<R>,
    project: String,
    game: Option<String>,
    loader: Option<String>,
) -> LauncherResult<Vec<crate::mods::Version>> {
    use crate::mods::Catalog;
    crate::mods::online(&app.state::<AppState>())?;
    mods(&app)
        .client
        .versions(&project, game.as_deref(), loader.as_deref())
        .await
}

/// One version with its changelog.
#[tauri::command]
pub async fn mods_version<R: Runtime>(
    app: AppHandle<R>,
    version: String,
) -> LauncherResult<crate::mods::Version> {
    use crate::mods::Catalog;
    crate::mods::online(&app.state::<AppState>())?;
    mods(&app).client.version(&version).await
}

/// Asks the catalog to keep an older version's files.
#[tauri::command]
pub async fn mods_fetch_version<R: Runtime>(
    app: AppHandle<R>,
    version: String,
) -> LauncherResult<()> {
    use crate::mods::Catalog;
    crate::mods::online(&app.state::<AppState>())?;
    mods(&app).client.fetch(&version).await
}

/// The launcher account's requests to add to the catalog.
#[tauri::command]
pub async fn mods_imports<R: Runtime>(
    app: AppHandle<R>,
) -> LauncherResult<Vec<crate::mods::ImportRequest>> {
    crate::mods::online(&app.state::<AppState>())?;
    mods(&app).client.imports(&app).await
}

/// "Add to the catalog": an address or a name.
#[tauri::command]
pub async fn mods_import<R: Runtime>(
    app: AppHandle<R>,
    input: String,
) -> LauncherResult<crate::mods::ImportRequest> {
    crate::mods::online(&app.state::<AppState>())?;
    mods(&app).client.import(&app, &input).await
}

#[tauri::command]
pub async fn mods_import_state<R: Runtime>(
    app: AppHandle<R>,
    request: String,
) -> LauncherResult<crate::mods::ImportRequest> {
    crate::mods::online(&app.state::<AppState>())?;
    mods(&app).client.import_state(&app, &request).await
}

/// The project a name found that the player picked.
#[tauri::command]
pub async fn mods_choose<R: Runtime>(
    app: AppHandle<R>,
    request: String,
    source: String,
    id: String,
) -> LauncherResult<crate::mods::ImportRequest> {
    crate::mods::online(&app.state::<AppState>())?;
    mods(&app).client.choose(&app, &request, &source, &id).await
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ModsInstallPayload {
    pub instance_id: crate::storage::paths::InstanceId,
    pub version: String,
    /// The world a data pack goes into.
    pub world: Option<String>,
    /// A file of the player's that this replaces (an update).
    pub replaces: Option<String>,
}

async fn mods_plan_for<R: Runtime>(
    app: &AppHandle<R>,
    payload: &ModsInstallPayload,
) -> LauncherResult<(crate::storage::paths::DataRoot, crate::mods::content::Plan)> {
    use crate::mods::content;
    let state = app.state::<AppState>();
    crate::mods::online(&state)?;
    let (root, instance) = content::instance_of(&state, payload.instance_id)?;
    let target = content::Target::of(&instance);
    let mut lock = content::load_lock(&root, payload.instance_id);
    let game = root.game_dir(payload.instance_id);
    let wanted = content::Wanted {
        version: &payload.version,
        world: payload.world.as_deref(),
        replaces: payload.replaces.as_deref(),
    };
    // The player's own files the catalog knows: a mod they put there is not
    // brought again.
    // And what the jars there say, to check the set as a whole.
    let mods = mods(app);
    let (files, local) = {
        let (mods, root) = (Arc::clone(&mods), root.clone());
        let id = payload.instance_id;
        tauri::async_runtime::spawn_blocking(move || {
            (
                mods.content.unknown_files(&root, id),
                mods.content.local_jars(&root, id),
            )
        })
        .await
        .map_err(|_| {
            crate::error::LauncherError::internal("A file operation stopped unexpectedly.")
        })?
    };
    let identified = content::identify(&mods.client, &files).await?;
    content::follow_joins(&mods.client, &mut lock).await;
    let mut plan = content::checked_plan(
        &mods.client,
        &game,
        &target,
        &lock,
        &wanted,
        &identified,
        &local,
    )
    .await?;
    // A metered connection (D-051): the plan says why it asks first.
    let settings = state.settings();
    let month_bytes = crate::traffic::month_total(&root);
    plan.approval = crate::traffic::caution(
        settings.metered,
        settings.metered_warn_mib,
        settings.monthly_limit_mib,
        month_bytes,
        plan.total_bytes,
    )
    .map(|reason| crate::state::dto::ApprovalRequest {
        bytes: plan.total_bytes,
        files: plan.items.iter().filter(|i| !i.present).count() as u64,
        reason,
        month_bytes,
        monthly_limit_mib: settings.monthly_limit_mib,
    });
    Ok((root, plan))
}

/// What installing a version into an installation would add.
#[tauri::command]
pub async fn mods_plan<R: Runtime>(
    app: AppHandle<R>,
    payload: ModsInstallPayload,
) -> LauncherResult<crate::mods::content::Plan> {
    mods_plan_for(&app, &payload).await.map(|(_, plan)| plan)
}

/// What the mods' own jars in an installation say may stop the game from
/// starting (D-066), asked before a launch: the jars only, offline too.
#[tauri::command]
pub async fn mods_check<R: Runtime>(
    app: AppHandle<R>,
    instance_id: crate::storage::paths::InstanceId,
) -> LauncherResult<Vec<crate::mods::content::SetProblem>> {
    use crate::mods::content;
    let (root, instance) = content::instance_of(&app.state::<AppState>(), instance_id)?;
    let target = content::Target::of(&instance);
    if target.setup().is_none() {
        return Ok(Vec::new());
    }
    let lock = content::load_lock(&root, instance_id);
    let mods = mods(&app);
    // The catalog's list of clashes when it answers quickly; else the one last
    // read, so a launch never waits on the network.
    let clashes = if crate::mods::online(&app.state::<AppState>()).is_ok() {
        use crate::mods::Catalog as _;
        tokio::time::timeout(CLASHES_WAIT, mods.client.clashes())
            .await
            .ok()
            .and_then(Result::ok)
            .unwrap_or_else(|| mods.client.known_clashes())
    } else {
        mods.client.known_clashes()
    };
    let local = {
        let mods = Arc::clone(&mods);
        tauri::async_runtime::spawn_blocking(move || mods.content.local_jars(&root, instance_id))
            .await
            .map_err(|_| {
                crate::error::LauncherError::internal("A file operation stopped unexpectedly.")
            })?
    };
    Ok(content::check_folder(&target, &lock, &local, &clashes))
}

/// How long the check before a launch waits for the catalog's list of clashes.
const CLASHES_WAIT: std::time::Duration = std::time::Duration::from_secs(3);

/// Installs a version (and the mods it needs) into an installation; progress
/// comes as `mods://progress`. Returns how many files are in place.
#[tauri::command]
pub async fn mods_install<R: Runtime>(
    app: AppHandle<R>,
    request: MutationRequest<ModsInstallPayload>,
) -> LauncherResult<usize> {
    use tauri::Emitter;
    crate::state::ledger::validate_request_id(&request.request_id)?;
    let payload = request.payload;
    let id = payload.instance_id;
    let mods = mods(&app);
    let claim = mods.content.claim(id)?;
    app.state::<AppState>()
        .activity()
        .ensure_instance_idle(id)?;
    let (root, plan) = mods_plan_for(&app, &payload).await?;
    let emitter = app.clone();
    let emit = move |progress: crate::mods::content::Progress| {
        let _ = emitter.emit(crate::mods::PROGRESS_EVENT, progress);
    };
    let result = mods
        .content
        .install(&mods.client, &root, id, plan, &claim.cancel, &emit)
        .await;
    if let Err(error) = &result {
        let phase = if error.code == crate::error::ErrorCode::OperationCancelled {
            "cancelled"
        } else {
            "failed"
        };
        emit(crate::mods::content::Progress {
            instance_id: id.to_string(),
            phase,
            files: 0,
            total_files: 0,
            bytes: 0,
            total_bytes: 0,
        });
    }
    drop(claim);
    result
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ModsWorldPayload {
    pub instance_id: crate::storage::paths::InstanceId,
    pub version: String,
}

/// What adding a map from the catalog to an installation would do.
#[tauri::command]
pub async fn mods_world_plan<R: Runtime>(
    app: AppHandle<R>,
    payload: ModsWorldPayload,
) -> LauncherResult<crate::mods::world::WorldPlan> {
    let state = app.state::<AppState>();
    crate::mods::online(&state)?;
    let (root, _) = crate::mods::content::instance_of(&state, payload.instance_id)?;
    let game = root.game_dir(payload.instance_id);
    let mut plan = crate::mods::world::plan(&mods(&app).client, &game, &payload.version).await?;
    // A metered connection (D-051): the plan says why it asks first.
    let settings = state.settings();
    let month_bytes = crate::traffic::month_total(&root);
    plan.approval = crate::traffic::caution(
        settings.metered,
        settings.metered_warn_mib,
        settings.monthly_limit_mib,
        month_bytes,
        plan.size,
    )
    .map(|reason| crate::state::dto::ApprovalRequest {
        bytes: plan.size,
        files: 1,
        reason,
        month_bytes,
        monthly_limit_mib: settings.monthly_limit_mib,
    });
    Ok(plan)
}

/// Adds a map from the catalog to an installation as a new world; progress
/// comes as `mods://progress`. Returns the world's folder name.
#[tauri::command]
pub async fn mods_install_world<R: Runtime>(
    app: AppHandle<R>,
    request: MutationRequest<ModsWorldPayload>,
) -> LauncherResult<String> {
    use tauri::Emitter;
    crate::state::ledger::validate_request_id(&request.request_id)?;
    let payload = request.payload;
    let id = payload.instance_id;
    let mods = mods(&app);
    let claim = mods.content.claim(id)?;
    let state = app.state::<AppState>();
    state.activity().ensure_instance_idle(id)?;
    crate::mods::online(&state)?;
    let (root, _) = crate::mods::content::instance_of(&state, id)?;
    let emitter = app.clone();
    let emit = move |progress: crate::mods::content::Progress| {
        let _ = emitter.emit(crate::mods::PROGRESS_EVENT, progress);
    };
    let result = mods
        .content
        .install_world(
            &mods.client,
            &root,
            id,
            &payload.version,
            &claim.cancel,
            &emit,
        )
        .await;
    if let Err(error) = &result {
        let phase = if error.code == crate::error::ErrorCode::OperationCancelled {
            "cancelled"
        } else {
            "failed"
        };
        emit(crate::mods::content::Progress {
            instance_id: id.to_string(),
            phase,
            files: 0,
            total_files: 0,
            bytes: 0,
            total_bytes: 0,
        });
    }
    drop(claim);
    result
}

fn picked_import(
    mods: &crate::mods::Mods,
) -> std::sync::MutexGuard<'_, Option<crate::mods::import::Picked>> {
    mods.import
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn nothing_picked() -> crate::error::LauncherError {
    crate::error::LauncherError::invalid_input("Choose what to bring in first.")
}

/// Picks a Prism Launcher or MultiMC instance, a game or mods folder, or a
/// zip of one in the native dialog, and reads what it holds. The core keeps
/// it for `mods_import_create`. `Ok(None)` when the dialog was closed.
#[tauri::command]
pub async fn mods_import_pick<R: Runtime>(
    app: AppHandle<R>,
    folder: bool,
    title: Option<String>,
) -> LauncherResult<Option<crate::mods::import::Found>> {
    use tauri_plugin_dialog::DialogExt;
    let fallback = if folder {
        "Choose the instance or mods folder"
    } else {
        "Choose the exported instance (zip)"
    };
    let title = window_title(title, fallback);
    let handle = app.clone();
    let picked = tauri::async_runtime::spawn_blocking(move || {
        let dialog = handle.dialog().file().set_title(title);
        if folder {
            dialog.blocking_pick_folder()
        } else {
            dialog.add_filter("Zip", &["zip"]).blocking_pick_file()
        }
    })
    .await
    .map_err(|_| crate::error::LauncherError::internal("The file dialog failed."))?;
    let Some(path) = picked.and_then(|p| p.into_path().ok()) else {
        return Ok(None);
    };
    let read = tauri::async_runtime::spawn_blocking(move || crate::mods::import::read(&path))
        .await
        .map_err(|_| {
            crate::error::LauncherError::internal("A file operation stopped unexpectedly.")
        })??;
    let found = read.found.clone();
    *picked_import(&mods(&app)) = Some(read);
    Ok(Some(found))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ModsImportCreatePayload {
    pub display_name: String,
    pub game_version: String,
    pub loader: Option<crate::loader::LoaderKind>,
}

/// Makes the installation for what was picked: its game version and loader
/// (the loader's version the source names, else the recommended one).
#[tauri::command]
pub async fn mods_import_create<R: Runtime>(
    app: AppHandle<R>,
    sources: State<'_, Sources>,
    request: MutationRequest<ModsImportCreatePayload>,
) -> LauncherResult<InstanceSummary> {
    crate::state::ledger::validate_request_id(&request.request_id)?;
    let payload = request.payload;
    let mods = mods(&app);
    let (hint_kind, hint, count) = {
        let picked = picked_import(&mods);
        let picked = picked.as_ref().ok_or_else(nothing_picked)?;
        if picked.instance.is_some() {
            return Err(crate::error::LauncherError::invalid_input(
                "An installation was made for this already.",
            ));
        }
        (
            picked.found.loader,
            picked.loader_hint.clone(),
            picked.found.mods,
        )
    };
    if !crate::loader::valid_version(&payload.game_version) {
        return Err(crate::error::LauncherError::invalid_input(
            "Choose a Minecraft version.",
        ));
    }
    let loader = match payload.loader {
        Some(kind) => {
            let list = sources
                .get()?
                .loader_versions(kind, &payload.game_version)
                .await?;
            let hint = hint.as_deref().filter(|_| hint_kind == Some(kind));
            let version =
                crate::mods::import::pick_loader_version(&list, hint).ok_or_else(|| {
                    crate::error::LauncherError::invalid_input(
                        "This mod loader has no version for this Minecraft version.",
                    )
                })?;
            Some(crate::loader::LoaderChoice { kind, version })
        }
        None => None,
    };
    let create = MutationRequest {
        request_id: request.request_id,
        payload: CreateInstancePayload {
            display_name: payload.display_name,
            version_id: payload.game_version,
            loader,
        },
    };
    let summary = blocking(app, move |state| state.create_instance_with(create, count)).await?;
    if let Some(picked) = picked_import(&mods).as_mut() {
        picked.instance = Some(summary.id);
    }
    Ok(summary)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ModsImportApplyPayload {
    pub instance_id: crate::storage::paths::InstanceId,
}

/// Copies what was picked into the installation made for it; progress comes
/// as `mods://progress`. Again after a stop it finishes what is missing.
#[tauri::command]
pub async fn mods_import_apply<R: Runtime>(
    app: AppHandle<R>,
    request: MutationRequest<ModsImportApplyPayload>,
) -> LauncherResult<crate::mods::import::Report> {
    use tauri::Emitter;
    crate::state::ledger::validate_request_id(&request.request_id)?;
    let id = request.payload.instance_id;
    let mods = mods(&app);
    let picked = picked_import(&mods)
        .clone()
        .filter(|p| p.instance == Some(id))
        .ok_or_else(nothing_picked)?;
    let claim = mods.content.claim(id)?;
    let state = app.state::<AppState>();
    state.activity().ensure_instance_idle(id)?;
    let (root, _) = crate::mods::content::instance_of(&state, id)?;
    let game = root.game_dir(id);
    let emitter = app.clone();
    let cancel = claim.cancel.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let emit = move |progress: crate::mods::content::Progress| {
            let _ = emitter.emit(crate::mods::PROGRESS_EVENT, progress);
        };
        crate::mods::import::copy(&picked, &game, id, &cancel, &emit)
    })
    .await
    .map_err(|_| crate::error::LauncherError::internal("A file operation stopped unexpectedly."))?;
    let phase = match &result {
        Ok(_) => {
            *picked_import(&mods) = None;
            "done"
        }
        Err(e) if e.code == crate::error::ErrorCode::OperationCancelled => "cancelled",
        Err(_) => "failed",
    };
    let _ = app.emit(
        crate::mods::PROGRESS_EVENT,
        crate::mods::content::Progress {
            instance_id: id.to_string(),
            phase,
            files: 0,
            total_files: 0,
            bytes: 0,
            total_bytes: 0,
        },
    );
    drop(claim);
    result
}

/// A modpack version as the catalog has it (stage 3). `prepare` asks the
/// catalog to keep its files; the interface does that once, then looks
/// without it every few seconds until the pack is ready.
#[tauri::command]
pub async fn mods_pack<R: Runtime>(
    app: AppHandle<R>,
    version: String,
    prepare: bool,
) -> LauncherResult<crate::mods::pack::PackInfo> {
    let state = app.state::<AppState>();
    crate::mods::online(&state)?;
    let mut info = mods(&app).client.pack(&version, prepare).await?;
    // A metered connection (D-051): the pack says why it asks first.
    if info.ready()
        && let Some(root) = state.data_root()
    {
        let settings = state.settings();
        let month_bytes = crate::traffic::month_total(&root);
        info.approval = crate::traffic::caution(
            settings.metered,
            settings.metered_warn_mib,
            settings.monthly_limit_mib,
            month_bytes,
            info.total_bytes,
        )
        .map(|reason| crate::state::dto::ApprovalRequest {
            bytes: info.total_bytes,
            files: info.total + 1,
            reason,
            month_bytes,
            monthly_limit_mib: settings.monthly_limit_mib,
        });
    }
    Ok(info)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ModsPackInstancePayload {
    pub version: String,
    pub display_name: String,
}

/// A new installation for a ready modpack: its game version, its loader, and
/// memory for its mods (D-058). Its files come with `mods_apply_pack`.
#[tauri::command]
pub async fn mods_create_pack_instance<R: Runtime>(
    app: AppHandle<R>,
    request: MutationRequest<ModsPackInstancePayload>,
) -> LauncherResult<InstanceSummary> {
    crate::state::ledger::validate_request_id(&request.request_id)?;
    crate::mods::online(&app.state::<AppState>())?;
    let info = mods(&app)
        .client
        .pack(&request.payload.version, false)
        .await?;
    if !info.ready() {
        return Err(crate::error::LauncherError::invalid_input(
            "The catalog is still preparing this modpack. Try again soon.",
        ));
    }
    let loader = info.loader_choice()?;
    let game = info.game.clone().unwrap_or_default();
    let create = MutationRequest {
        request_id: request.request_id,
        payload: CreateInstancePayload {
            display_name: request.payload.display_name,
            version_id: game,
            loader,
        },
    };
    blocking(app, move |state| {
        state.create_instance_with(create, info.mods)
    })
    .await
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ModsApplyPackPayload {
    pub instance_id: crate::storage::paths::InstanceId,
    pub version: String,
}

/// A ready modpack version checked against an installation: the same
/// Minecraft version and kind of loader. Returns the pack, its loader and who
/// it is.
async fn pack_for(
    client: &crate::mods::Client,
    instance: &crate::storage::models::InstanceFile,
    version: &str,
) -> LauncherResult<(
    crate::mods::pack::PackInfo,
    Option<crate::loader::LoaderChoice>,
    crate::mods::pack::PackMeta,
)> {
    use crate::mods::Catalog;
    let info = client.pack(version, false).await?;
    if !info.ready() {
        return Err(crate::error::LauncherError::invalid_input(
            "The catalog is still preparing this modpack. Try again soon.",
        ));
    }
    let loader = info.loader_choice()?;
    if info.game.as_deref() != Some(instance.version_id.as_str())
        || loader.as_ref().map(|l| l.kind) != instance.loader.as_ref().map(|l| l.kind)
    {
        return Err(crate::error::LauncherError::invalid_input(
            "This modpack needs another Minecraft version or mod loader than this installation.",
        ));
    }
    let found = client.version(version).await?;
    let project = client.project(&found.project_id).await?;
    let meta = crate::mods::pack::PackMeta::new(&project, &found)?;
    Ok((info, loader, meta))
}

/// Puts a pack's files in with progress as `mods://progress`, and says when
/// it stopped.
async fn run_pack<R: Runtime>(
    app: &AppHandle<R>,
    mods: &crate::mods::Mods,
    root: &crate::storage::paths::DataRoot,
    id: crate::storage::paths::InstanceId,
    info: &crate::mods::pack::PackInfo,
    meta: crate::mods::pack::PackMeta,
    cancel: &crate::download::CancelToken,
) -> LauncherResult<crate::mods::pack::PackReport> {
    use tauri::Emitter;
    let emitter = app.clone();
    let emit = move |progress: crate::mods::content::Progress| {
        let _ = emitter.emit(crate::mods::PROGRESS_EVENT, progress);
    };
    let result = mods
        .content
        .apply_pack(root, id, info, meta, cancel, &emit)
        .await;
    if let Err(error) = &result {
        let phase = if error.code == crate::error::ErrorCode::OperationCancelled {
            "cancelled"
        } else {
            "failed"
        };
        emit(crate::mods::content::Progress {
            instance_id: id.to_string(),
            phase,
            files: 0,
            total_files: 0,
            bytes: 0,
            total_bytes: 0,
        });
    }
    result
}

/// Puts a ready modpack's files and overrides into an installation made for
/// it; progress comes as `mods://progress`. Again on the same installation it
/// finishes what stopped, changing only what is still as the pack left it.
#[tauri::command]
pub async fn mods_apply_pack<R: Runtime>(
    app: AppHandle<R>,
    request: MutationRequest<ModsApplyPackPayload>,
) -> LauncherResult<crate::mods::pack::PackReport> {
    crate::state::ledger::validate_request_id(&request.request_id)?;
    let payload = request.payload;
    let id = payload.instance_id;
    let mods = mods(&app);
    let claim = mods.content.claim(id)?;
    let state = app.state::<AppState>();
    state.activity().ensure_instance_idle(id)?;
    crate::mods::online(&state)?;
    let (root, instance) = crate::mods::content::instance_of(&state, id)?;
    let (info, _, meta) = pack_for(&mods.client, &instance, &payload.version).await?;
    let result = run_pack(&app, &mods, &root, id, &info, meta, &claim.cancel).await;
    drop(claim);
    result
}

/// Updates an installation made from a modpack to another version of the
/// same pack (D-066). The whole installation is backed up first (D-047);
/// the player's own changes stay, and the loader follows the pack.
#[tauri::command]
pub async fn mods_update_pack<R: Runtime>(
    app: AppHandle<R>,
    request: MutationRequest<ModsApplyPackPayload>,
) -> LauncherResult<crate::mods::pack::PackReport> {
    crate::state::ledger::validate_request_id(&request.request_id)?;
    let payload = request.payload;
    let id = payload.instance_id;
    let mods = mods(&app);
    let claim = mods.content.claim(id)?;
    let state = app.state::<AppState>();
    state.activity().ensure_instance_idle(id)?;
    crate::mods::online(&state)?;
    let (root, instance) = crate::mods::content::instance_of(&state, id)?;
    let Some(installed) = crate::mods::content::load_lock(&root, id).pack else {
        return Err(crate::error::LauncherError::invalid_input(
            "This installation was not made from a modpack.",
        ));
    };
    let (info, loader, meta) = pack_for(&mods.client, &instance, &payload.version).await?;
    if meta.project_id != installed.project_id {
        return Err(crate::error::LauncherError::invalid_input(
            "This is another modpack than the one the installation was made from.",
        ));
    }
    blocking(app.clone(), move |state| {
        crate::backup::create(
            state,
            id,
            crate::backup::Scope::Installation,
            crate::backup::Reason::BeforeUpdate,
        )
    })
    .await?;
    let report = run_pack(&app, &mods, &root, id, &info, meta, &claim.cancel).await?;
    if let Some(loader) = loader
        && instance.loader.as_ref() != Some(&loader)
    {
        let change = MutationRequest {
            request_id: uuid::Uuid::new_v4().to_string(),
            payload: crate::state::dto::SetInstanceLoaderPayload {
                instance_id: id,
                version: loader.version,
            },
        };
        blocking(app.clone(), move |state| state.set_instance_loader(change)).await?;
    }
    drop(claim);
    Ok(report)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ModsExportPackPayload {
    pub instance_id: crate::storage::paths::InstanceId,
    pub choice: crate::mods::pack::ExportChoice,
    pub title: Option<String>,
}

/// What an export wrote.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModsExportResult {
    /// False when the player closed the save dialog.
    pub saved: bool,
    /// File name of the written archive (not the full path).
    pub file_name: Option<String>,
    pub report: Option<crate::mods::pack::ExportReport>,
}

/// A file name for a modpack export.
fn pack_file_name(name: &str) -> String {
    let base = export_file_name(name);
    let base = base.strip_suffix(".zip").unwrap_or(&base);
    let base = if base == "world" { "modpack" } else { base };
    format!("{base}.mrpack")
}

/// Saves an installation as a Modrinth modpack (`.mrpack`) where the player
/// chooses (D-066): mods the catalog knows from Modrinth by address, the
/// rest copied in.
#[tauri::command]
pub async fn mods_export_pack<R: Runtime>(
    app: AppHandle<R>,
    payload: ModsExportPackPayload,
) -> LauncherResult<ModsExportResult> {
    use tauri_plugin_dialog::DialogExt;
    let id = payload.instance_id;
    let mods = mods(&app);
    let claim = mods.content.claim(id)?;
    let state = app.state::<AppState>();
    state.activity().ensure_instance_idle(id)?;
    let (root, instance) = crate::mods::content::instance_of(&state, id)?;
    let lock = crate::mods::content::load_lock(&root, id);
    let handle = app.clone();
    let title = window_title(payload.title, "Export the modpack");
    let file_name = pack_file_name(&instance.display_name);
    let target = tauri::async_runtime::spawn_blocking(move || {
        handle
            .dialog()
            .file()
            .set_title(title)
            .set_file_name(file_name)
            .add_filter("Modrinth modpack", &["mrpack"])
            .blocking_save_file()
    })
    .await
    .map_err(|_| crate::error::LauncherError::internal("The save dialog failed."))?;
    let Some(target) = target else {
        return Ok(ModsExportResult {
            saved: false,
            file_name: None,
            report: None,
        });
    };
    let path = target.into_path().map_err(|_| {
        crate::error::LauncherError::new(
            crate::error::ErrorCode::InvalidInput,
            "This location cannot be used for the export.",
        )
    })?;
    // The dialog took a while; the game may have started meanwhile.
    state.activity().ensure_instance_idle(id)?;
    let info = crate::mods::pack::ExportInfo {
        name: instance.display_name.clone(),
        version_number: lock
            .pack
            .as_ref()
            .map_or_else(|| "1.0.0".to_owned(), |p| p.version_number.clone()),
        game: instance.version_id.clone(),
        loader: instance.loader.clone(),
    };
    let report = crate::mods::pack::export(
        &mods.client,
        &root.game_dir(id),
        info,
        payload.choice,
        path.clone(),
        &claim.cancel,
    )
    .await?;
    drop(claim);
    Ok(ModsExportResult {
        saved: true,
        file_name: path.file_name().map(|n| n.to_string_lossy().into_owned()),
        report: Some(report),
    })
}

/// What publishing an installation as a pack would take, and the pack it
/// was published as, if any (D-066, stage 6).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModsPublishCheck {
    pub check: crate::mods::publish::PublishCheck,
    /// The account's pack this installation was published as.
    pub pack: Option<crate::mods::OwnPack>,
    /// The installation's name, for the title.
    pub name: String,
}

#[tauri::command]
pub async fn mods_publish_check<R: Runtime>(
    app: AppHandle<R>,
    instance_id: crate::storage::paths::InstanceId,
    choice: crate::mods::pack::ExportChoice,
) -> LauncherResult<ModsPublishCheck> {
    crate::mods::online(&app.state::<AppState>())?;
    let mods = mods(&app);
    let state = app.state::<AppState>();
    let (root, instance) = crate::mods::content::instance_of(&state, instance_id)?;
    let lock = crate::mods::content::load_lock(&root, instance_id);
    let sorted =
        crate::mods::publish::sort(&mods.client, &root.game_dir(instance_id), choice).await?;
    let mut check = sorted.check;
    check.published.clone_from(&lock.published);
    let pack = match &lock.published {
        Some(id) => mods
            .client
            .own_packs(&app)
            .await?
            .into_iter()
            .find(|p| &p.id == id),
        None => None,
    };
    Ok(ModsPublishCheck {
        check,
        pack,
        name: instance.display_name,
    })
}

/// A picture for a pack's icon, checked; the next publish takes it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModsPickedIcon {
    /// `data:` address of the picture, for the form to show.
    pub preview: String,
}

/// Picks a pack's icon in the native dialog. `Ok(None)` when it was closed.
#[tauri::command]
pub async fn mods_publish_icon<R: Runtime>(
    app: AppHandle<R>,
    title: Option<String>,
) -> LauncherResult<Option<ModsPickedIcon>> {
    use base64::Engine as _;
    use tauri_plugin_dialog::DialogExt;
    let title = window_title(title, "Choose the pack's icon");
    let handle = app.clone();
    let picked = tauri::async_runtime::spawn_blocking(move || {
        handle
            .dialog()
            .file()
            .set_title(title)
            .add_filter("PNG, JPEG, WebP", &["png", "jpg", "jpeg", "webp"])
            .blocking_pick_file()
    })
    .await
    .map_err(|_| crate::error::LauncherError::internal("The file dialog failed."))?;
    let Some(path) = picked.and_then(|p| p.into_path().ok()) else {
        return Ok(None);
    };
    let (name, bytes) =
        tauri::async_runtime::spawn_blocking(move || crate::mods::publish::read_icon(&path))
            .await
            .map_err(|_| crate::error::LauncherError::internal("The icon could not be read."))??;
    let media_type = match name {
        "icon.png" => "image/png",
        "icon.jpg" => "image/jpeg",
        _ => "image/webp",
    };
    let preview = format!(
        "data:{media_type};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(&bytes)
    );
    *mods(&app)
        .icon
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some((name, bytes));
    Ok(Some(ModsPickedIcon { preview }))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ModsPublishPayload {
    pub instance_id: crate::storage::paths::InstanceId,
    pub form: crate::mods::publish::PublishForm,
    /// With the icon picked last.
    #[serde(default)]
    pub icon: bool,
    /// A new pack even when this installation was published before.
    #[serde(default)]
    pub as_new: bool,
}

/// Publishes an installation as the player's own pack of the catalog, or a
/// new version of the pack it was published as.
#[tauri::command]
pub async fn mods_publish<R: Runtime>(
    app: AppHandle<R>,
    payload: ModsPublishPayload,
) -> LauncherResult<crate::mods::publish::Published> {
    use crate::mods::publish;
    crate::mods::online(&app.state::<AppState>())?;
    let id = payload.instance_id;
    let mods = mods(&app);
    let meta = publish::meta(&payload.form)?;
    let icon =
        if payload.icon {
            let picked = mods
                .icon
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone();
            Some(picked.ok_or_else(|| {
                crate::error::LauncherError::invalid_input("Choose the icon again.")
            })?)
        } else {
            None
        };
    let claim = mods.content.claim(id)?;
    let state = app.state::<AppState>();
    state.activity().ensure_instance_idle(id)?;
    let (root, instance) = crate::mods::content::instance_of(&state, id)?;
    let lock = crate::mods::content::load_lock(&root, id);
    let sorted = publish::sort(&mods.client, &root.game_dir(id), payload.form.choice).await?;
    if !sorted.check.ready() {
        return Err(crate::error::LauncherError::invalid_input(
            "Some files of this installation cannot go into a pack. Check the list.",
        ));
    }
    let target = publish::Target {
        game: instance.version_id.clone(),
        loader: instance.loader.clone(),
    };
    let part = publish::part_path(&root.instance_dir(id));
    {
        let part = part.clone();
        let cancel = claim.cancel.clone();
        tauri::async_runtime::spawn_blocking(move || {
            publish::write(&part, &sorted, &target, &meta, icon, &cancel)
        })
        .await
        .map_err(|_| {
            crate::error::LauncherError::internal("The modpack could not be written.")
        })??;
    }
    let project = if payload.as_new {
        None
    } else {
        lock.published.as_deref()
    };
    let emitter = app.clone();
    let instance_id = id.to_string();
    let sent = move |bytes: u64, total: u64| {
        let _ = tauri::Emitter::emit(
            &emitter,
            crate::mods::PROGRESS_EVENT,
            crate::mods::content::Progress {
                instance_id: instance_id.clone(),
                phase: "uploading",
                files: 0,
                total_files: 0,
                bytes,
                total_bytes: total,
            },
        );
    };
    let published = mods
        .client
        .publish(&app, &part, project, false, &claim.cancel, &sent)
        .await;
    let _ = std::fs::remove_file(&part);
    let published = published?;
    if let publish::Published::Published { project_id, .. } = &published {
        let mut lock = crate::mods::content::load_lock(&root, id);
        lock.published = Some(project_id.clone());
        crate::mods::content::save_published(&root, id, &lock)?;
        *mods
            .icon
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
        state
            .console()
            .info("Mods: an installation was published as a pack");
    }
    drop(claim);
    Ok(published)
}

/// Hides the player's own pack from the catalog, or shows it again.
#[tauri::command]
pub async fn mods_pack_hidden<R: Runtime>(
    app: AppHandle<R>,
    project: String,
    hidden: bool,
) -> LauncherResult<String> {
    crate::mods::online(&app.state::<AppState>())?;
    mods(&app).client.set_hidden(&app, &project, hidden).await
}

/// Reports a player's own pack to the catalog's owner.
#[tauri::command]
pub async fn mods_report<R: Runtime>(
    app: AppHandle<R>,
    project: String,
    reason: String,
    text: String,
) -> LauncherResult<()> {
    crate::mods::online(&app.state::<AppState>())?;
    mods(&app)
        .client
        .report(&app, &project, &reason, &text)
        .await
}

/// Stops an install into an installation.
#[tauri::command]
pub fn mods_cancel<R: Runtime>(
    app: AppHandle<R>,
    instance_id: crate::storage::paths::InstanceId,
) -> bool {
    mods(&app).content.cancel(instance_id)
}

/// The mods, resource packs, shaders and data packs of an installation.
#[tauri::command]
pub async fn mods_content<R: Runtime>(
    app: AppHandle<R>,
    instance_id: crate::storage::paths::InstanceId,
) -> LauncherResult<crate::mods::content::ContentView> {
    blocking(app, move |state| {
        let (root, _) = crate::mods::content::instance_of(state, instance_id)?;
        Ok(crate::mods::content::list(&root, instance_id))
    })
    .await
}

/// Files of an installation the catalog holds back as harmful (D-066,
/// stage 6): their paths, to warn and offer to turn them off.
#[tauri::command]
pub async fn mods_harmful<R: Runtime>(
    app: AppHandle<R>,
    instance_id: crate::storage::paths::InstanceId,
) -> LauncherResult<Vec<String>> {
    crate::mods::online(&app.state::<AppState>())?;
    let mods = mods(&app);
    let harmful = mods.harmful().await?;
    blocking(app, move |state| {
        let (root, _) = crate::mods::content::instance_of(state, instance_id)?;
        Ok(mods.content.harmful_files(&root, instance_id, &harmful))
    })
    .await
}

/// The player's own files the catalog recognizes by their hash.
#[tauri::command]
pub async fn mods_identify<R: Runtime>(
    app: AppHandle<R>,
    instance_id: crate::storage::paths::InstanceId,
) -> LauncherResult<Vec<crate::mods::content::Identified>> {
    crate::mods::online(&app.state::<AppState>())?;
    let mods = mods(&app);
    let files = {
        let mods = Arc::clone(&mods);
        blocking(app, move |state| {
            let (root, _) = crate::mods::content::instance_of(state, instance_id)?;
            Ok(mods.content.unknown_files(&root, instance_id))
        })
        .await?
    };
    crate::mods::content::identify(&mods.client, &files).await
}

/// Newer versions of what is in an installation.
#[tauri::command]
pub async fn mods_updates<R: Runtime>(
    app: AppHandle<R>,
    instance_id: crate::storage::paths::InstanceId,
) -> LauncherResult<Vec<crate::mods::content::UpdateOffer>> {
    use crate::mods::content;
    let state = app.state::<AppState>();
    crate::mods::online(&state)?;
    let (root, instance) = content::instance_of(&state, instance_id)?;
    let mods = mods(&app);
    let files = {
        let (mods, root) = (Arc::clone(&mods), root.clone());
        tauri::async_runtime::spawn_blocking(move || mods.content.unknown_files(&root, instance_id))
            .await
            .map_err(|_| {
                crate::error::LauncherError::internal("A file operation stopped unexpectedly.")
            })?
    };
    let identified = content::identify(&mods.client, &files).await?;
    let mut lock = content::load_lock(&root, instance_id);
    content::follow_joins(&mods.client, &mut lock).await;
    content::updates(
        &mods.client,
        &content::Target::of(&instance),
        &lock,
        &identified,
        &root.game_dir(instance_id),
    )
    .await
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ModsFilePayload {
    pub instance_id: crate::storage::paths::InstanceId,
    /// Relative to the game folder.
    pub path: String,
    #[serde(default)]
    pub enabled: bool,
}

/// Turns a file of an installation on or off.
#[tauri::command]
pub async fn mods_set_enabled<R: Runtime>(
    app: AppHandle<R>,
    request: MutationRequest<ModsFilePayload>,
) -> LauncherResult<()> {
    crate::state::ledger::validate_request_id(&request.request_id)?;
    let payload = request.payload;
    let mods = mods(&app);
    let _claim = mods.content.claim(payload.instance_id)?;
    blocking(app, move |state| {
        state.activity().ensure_instance_idle(payload.instance_id)?;
        let (root, _) = crate::mods::content::instance_of(state, payload.instance_id)?;
        crate::mods::content::set_enabled(
            &root,
            payload.instance_id,
            &payload.path,
            payload.enabled,
        )
    })
    .await
}

/// Which mods need a mod about to be turned off or removed, and what only it
/// needed; without the catalog, what the launcher noted when it installed
/// them.
#[tauri::command]
pub async fn mods_needed_by<R: Runtime>(
    app: AppHandle<R>,
    instance_id: crate::storage::paths::InstanceId,
    path: String,
) -> LauncherResult<crate::mods::content::NeededBy> {
    use crate::mods::content;
    let state = app.state::<AppState>();
    let (root, _) = content::instance_of(&state, instance_id)?;
    content::content_path(&root.game_dir(instance_id), &path)?;
    let online = crate::mods::online(&state).is_ok();
    let mods = mods(&app);
    let identified = if online {
        let files = {
            let (mods, root) = (Arc::clone(&mods), root.clone());
            tauri::async_runtime::spawn_blocking(move || {
                mods.content.unknown_files(&root, instance_id)
            })
            .await
            .map_err(|_| {
                crate::error::LauncherError::internal("A file operation stopped unexpectedly.")
            })?
        };
        content::identify(&mods.client, &files)
            .await
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    Ok(mods
        .content
        .needed_by(
            online.then_some(&mods.client),
            &root,
            instance_id,
            &identified,
            &path,
        )
        .await)
}

/// Deletes a file from an installation's content folders.
#[tauri::command]
pub async fn mods_remove<R: Runtime>(
    app: AppHandle<R>,
    request: MutationRequest<ModsFilePayload>,
) -> LauncherResult<()> {
    crate::state::ledger::validate_request_id(&request.request_id)?;
    let payload = request.payload;
    let mods = mods(&app);
    let _claim = mods.content.claim(payload.instance_id)?;
    blocking(app, move |state| {
        state.activity().ensure_instance_idle(payload.instance_id)?;
        let (root, _) = crate::mods::content::instance_of(state, payload.instance_id)?;
        crate::mods::content::remove(&root, payload.instance_id, &payload.path)
    })
    .await
}

/// Opens a link from a project's page in the system browser: `https:` only;
/// the interface asks the player first.
#[tauri::command(async)]
pub fn mods_open_link<R: Runtime>(app: AppHandle<R>, url: String) -> LauncherResult<()> {
    use tauri_plugin_opener::OpenerExt;
    let url = crate::mods::markdown::link(&url)
        .ok_or_else(|| crate::error::LauncherError::invalid_input("This link cannot be opened."))?;
    app.opener()
        .open_url(url.as_str(), None::<&str>)
        .map_err(|_| {
            crate::error::LauncherError::internal("The system browser could not be opened.")
        })
}

// ---- The start page feed (D-067) ----------------------------------------------

use crate::feed::model::{FeedPage, OwnChannel, OwnStreamer, Stream, Video};
use crate::feed::{FeedProblem, FeedResult, FeedService, FeedView, Languages, NoteView};

fn feed_service<R: Runtime>(app: &AppHandle<R>) -> Arc<FeedService> {
    Arc::clone(&app.state::<Arc<FeedService>>())
}

/// The languages of the feed's parts, or `Disabled` when the feed is off.
fn feed_languages<R: Runtime>(
    app: &AppHandle<R>,
) -> FeedResult<(Languages, crate::storage::models::FeedSettings)> {
    let settings = app.state::<AppState>().settings();
    if !settings.feed.enabled {
        return Err(FeedProblem::Disabled);
    }
    Ok((
        Languages::of(settings.language.as_str(), &settings.feed),
        settings.feed,
    ))
}

fn feed_root<R: Runtime>(app: &AppHandle<R>) -> FeedResult<crate::storage::paths::DataRoot> {
    app.state::<AppState>()
        .data_root()
        .ok_or(FeedProblem::Unavailable)
}

/// The feed in the languages the settings ask for; `refresh` asks the
/// service again even when it answered a moment ago.
#[tauri::command]
pub async fn feed_get<R: Runtime>(app: AppHandle<R>, refresh: bool) -> FeedResult<FeedView> {
    let (languages, _) = feed_languages(&app)?;
    let root = app.state::<AppState>().data_root();
    feed_service(&app)
        .view(root.as_ref(), languages, refresh)
        .await
}

/// A view or a press in a server ad's tile, for the anonymous statistics
/// (D-071). Only ads the feed was given count; nothing is said back.
#[tauri::command(async)]
pub fn feed_server_ad_event(
    state: State<'_, AppState>,
    ads: State<'_, Arc<crate::feed::ads::ServerAds>>,
    stats: State<'_, Arc<crate::stats::Stats>>,
    ad: String,
    event: crate::stats::AdEvent,
) -> LauncherResult<()> {
    if !crate::stats::ad_id(&ad) {
        return Err(crate::error::LauncherError::invalid_input(
            "This is not an ad of the feed.",
        ));
    }
    if ads.known(&ad) {
        stats.ad_event(&state, &ad, event);
    }
    Ok(())
}

/// The server ads of `place` showing now (D-069), in the interface's
/// language; `refresh` asks the service again.
#[tauri::command]
pub async fn feed_server_ads<R: Runtime>(
    app: AppHandle<R>,
    place: String,
    refresh: bool,
) -> FeedResult<crate::feed::ads::ServerAdsView> {
    let (languages, _) = feed_languages(&app)?;
    let root = app.state::<AppState>().data_root();
    let ads = Arc::clone(&app.state::<Arc<crate::feed::ads::ServerAds>>());
    ads.get(root.as_ref(), &place, languages.interface, refresh)
        .await
}

/// A page of the endless list (D-067): `filter` one part or all of them, or
/// `shorts_only` the Shorts alone; `before` the cursor the page before ended
/// with.
#[tauri::command]
pub async fn feed_page<R: Runtime>(
    app: AppHandle<R>,
    filter: Option<crate::storage::models::FeedBlock>,
    shorts_only: Option<bool>,
    before: Option<String>,
) -> FeedResult<FeedPage> {
    let (languages, settings) = feed_languages(&app)?;
    feed_service(&app)
        .page(
            languages,
            &settings,
            filter,
            shorts_only.unwrap_or(false),
            before.as_deref(),
        )
        .await
}

/// A whole patch note or news article; `original` asks for the English text
/// instead of the translation.
#[tauri::command]
pub async fn feed_note<R: Runtime>(
    app: AppHandle<R>,
    id: String,
    original: bool,
) -> FeedResult<NoteView> {
    let (languages, _) = feed_languages(&app)?;
    if !crate::feed::model::note_id(&id) {
        return Err(FeedProblem::NotFound);
    }
    let lang = if original {
        crate::feed::model::Lang::En
    } else {
        languages.notes
    };
    let root = app.state::<AppState>().data_root();
    feed_service(&app).note(root.as_ref(), &id, lang).await
}

/// Plays a video (`yt`) or stream (`twitch`) where the settings say: the
/// player window (made through the queue, never inline) or the browser.
#[tauri::command(async)]
pub fn feed_play<R: Runtime>(
    app: AppHandle<R>,
    kind: String,
    id: String,
    title: Option<String>,
) -> FeedResult<()> {
    let (languages, settings) = feed_languages(&app)?;
    let target = crate::feed::player::Target::parse(&kind, &id).ok_or(FeedProblem::BadInput)?;
    match settings.player {
        crate::storage::models::PlayerMode::Browser => {
            if !crate::feed::player::open_in_browser(&app, target.browser_url().as_str()) {
                return Err(FeedProblem::Server);
            }
        }
        crate::storage::models::PlayerMode::Window => {
            crate::feed::player::show_player_window(
                &app,
                target.player_url(languages.interface),
                window_title(title, "Player"),
            );
        }
    }
    // A video played is shown dimmed in the feed after; a list that cannot
    // be written only loses that.
    if let crate::feed::player::Target::Video(video) = &target
        && let Ok(root) = feed_root(&app)
    {
        let _ = crate::feed::remember_watched(&root, video);
    }
    Ok(())
}

/// The videos the player played from the feed, oldest first.
#[tauri::command(async)]
pub fn feed_watched<R: Runtime>(app: AppHandle<R>) -> Vec<String> {
    feed_root(&app)
        .map(|root| crate::feed::watched(&root))
        .unwrap_or_default()
}

/// Opens a link of the feed in the browser: only addresses the launcher may
/// open (external::check_allowed).
#[tauri::command(async)]
pub fn feed_open_link<R: Runtime>(app: AppHandle<R>, url: String) -> FeedResult<()> {
    if !crate::feed::model::link(&url) {
        return Err(FeedProblem::BadInput);
    }
    if crate::feed::player::open_in_browser(&app, &url) {
        Ok(())
    } else {
        Err(FeedProblem::Server)
    }
}

/// The YouTube channels the player added to the feed.
#[tauri::command(async)]
pub fn feed_channels<R: Runtime>(app: AppHandle<R>) -> Vec<OwnChannel> {
    feed_root(&app)
        .map(|root| crate::feed::own_channels(&root))
        .unwrap_or_default()
}

/// Looks a channel up by its link, @handle or id and adds it; the list after.
#[tauri::command]
pub async fn feed_add_channel<R: Runtime>(
    app: AppHandle<R>,
    input: String,
) -> FeedResult<Vec<OwnChannel>> {
    if input.len() > crate::feed::MAX_CHANNEL_INPUT * 4 {
        return Err(FeedProblem::BadInput);
    }
    let root = feed_root(&app)?;
    feed_service(&app).add_channel(&root, &input).await
}

/// Removes one of the player's channels; the list after.
#[tauri::command(async)]
pub fn feed_remove_channel<R: Runtime>(
    app: AppHandle<R>,
    channel_id: String,
) -> FeedResult<Vec<OwnChannel>> {
    if !crate::feed::model::channel_id(&channel_id) {
        return Err(FeedProblem::BadInput);
    }
    crate::feed::remove_own_channel(&feed_root(&app)?, &channel_id)
}

/// The newest videos of the player's own channels.
#[tauri::command]
pub async fn feed_own_videos<R: Runtime>(app: AppHandle<R>) -> FeedResult<Vec<Video>> {
    feed_languages(&app)?;
    let root = feed_root(&app)?;
    feed_service(&app).own_videos(&root).await
}

/// The Twitch channels the player added to the feed.
#[tauri::command(async)]
pub fn feed_streamers<R: Runtime>(app: AppHandle<R>) -> Vec<OwnStreamer> {
    feed_root(&app)
        .map(|root| crate::feed::own_streamers(&root))
        .unwrap_or_default()
}

/// Looks a Twitch channel up by its link or login and adds it; the list after.
#[tauri::command]
pub async fn feed_add_streamer<R: Runtime>(
    app: AppHandle<R>,
    input: String,
) -> FeedResult<Vec<OwnStreamer>> {
    if input.len() > crate::feed::MAX_CHANNEL_INPUT * 4 {
        return Err(FeedProblem::BadInput);
    }
    let root = feed_root(&app)?;
    feed_service(&app).add_streamer(&root, &input).await
}

/// Removes one of the player's Twitch channels; the list after.
#[tauri::command(async)]
pub fn feed_remove_streamer<R: Runtime>(
    app: AppHandle<R>,
    login: String,
) -> FeedResult<Vec<OwnStreamer>> {
    if !crate::feed::model::twitch_login(&login) {
        return Err(FeedProblem::BadInput);
    }
    crate::feed::remove_own_streamer(&feed_root(&app)?, &login)
}

/// The live streams of the player's own Twitch channels.
#[tauri::command]
pub async fn feed_own_streams<R: Runtime>(app: AppHandle<R>) -> FeedResult<Vec<Stream>> {
    feed_languages(&app)?;
    let root = feed_root(&app)?;
    feed_service(&app).own_streams(&root).await
}

/// The channels the player hid from the feed and the ones they put first.
#[tauri::command(async)]
pub fn feed_marks<R: Runtime>(app: AppHandle<R>) -> crate::feed::FeedMarks {
    feed_root(&app)
        .map(|root| crate::feed::marks(&root))
        .unwrap_or_default()
}

/// Puts a channel (`youtube` or `twitch`) on a list (`hidden` or
/// `favorite`), or with no list takes it off both; the marks after.
#[tauri::command(async)]
pub fn feed_mark<R: Runtime>(
    app: AppHandle<R>,
    kind: String,
    id: String,
    name: String,
    list: Option<String>,
) -> FeedResult<crate::feed::FeedMarks> {
    let kind = crate::feed::model::MarkKind::parse(&kind).ok_or(FeedProblem::BadInput)?;
    if !kind.id(&id) || name.len() > crate::feed::MAX_CHANNEL_INPUT * 4 {
        return Err(FeedProblem::BadInput);
    }
    let name = crate::feed::model::mark_name(&name).unwrap_or_else(|| id.clone());
    let list = match list.as_deref() {
        None => None,
        Some(list) => Some(crate::feed::MarkList::parse(list).ok_or(FeedProblem::BadInput)?),
    };
    let channel = crate::feed::model::ChannelMark { kind, id, name };
    crate::feed::mark(&feed_root(&app)?, channel, list)
}

/// The newest videos and live streams of the favourite channels.
#[tauri::command]
pub async fn feed_favorites<R: Runtime>(
    app: AppHandle<R>,
) -> FeedResult<crate::feed::FavoriteFeed> {
    feed_languages(&app)?;
    let root = feed_root(&app)?;
    feed_service(&app).favorites(&root).await
}

// ----- worlds with friends (D-068) -----

fn worlds<R: Runtime>(app: &AppHandle<R>) -> Arc<crate::worlds::app::Worlds> {
    Arc::clone(&app.state::<Arc<crate::worlds::app::Worlds>>())
}

/// A room's page: the room and its members.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorldsRoomPage {
    pub room: crate::worlds::service::Room,
    pub members: Vec<crate::worlds::service::Member>,
}

/// The installation a room plays and whether it is installed.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorldsInstance {
    pub instance_id: crate::storage::paths::InstanceId,
    pub ready: bool,
    /// The room's mods wait for the player's word: nothing was installed.
    pub consent: Option<crate::worlds::pack::RoomConsent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorldsCreatePayload {
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub minecraft: String,
    #[serde(default)]
    pub settings: crate::worlds::properties::WorldSettings,
    /// An installation with a mod loader whose mods the world takes (W12).
    #[serde(default)]
    pub instance_id: Option<crate::storage::paths::InstanceId>,
}

/// An update of a world's mods (W12): what [`worlds_update_preview`] showed,
/// and the host's answer to each changed setting.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorldsUpdatePayload {
    pub room: String,
    pub world: crate::storage::paths::WorldId,
    pub instance_id: crate::storage::paths::InstanceId,
    pub fingerprint: String,
    #[serde(default)]
    pub choices: std::collections::HashMap<String, crate::worlds::update::Keep>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorldsRoomPayload {
    pub room: String,
}

/// What this computer does for a room now.
#[tauri::command]
pub fn worlds_state<R: Runtime>(app: AppHandle<R>) -> crate::worlds::app::WorldsView {
    worlds(&app).view()
}

/// The Minecraft versions a world can be made for, newest first; snapshots
/// too when the player turned them on.
#[tauri::command]
pub async fn worlds_versions<R: Runtime>(app: AppHandle<R>, snapshots: bool) -> Vec<String> {
    worlds(&app).versions(snapshots).await
}

/// The worlds kept on this computer.
#[tauri::command]
pub async fn worlds_local<R: Runtime>(
    app: AppHandle<R>,
) -> LauncherResult<Vec<crate::worlds::app::LocalWorld>> {
    let worlds = worlds(&app);
    tauri::async_runtime::spawn_blocking(move || worlds.local())
        .await
        .map_err(|_| crate::error::LauncherError::internal("Reading the worlds stopped."))?
}

/// The rooms the launcher account is in.
#[tauri::command]
pub async fn worlds_rooms<R: Runtime>(
    app: AppHandle<R>,
) -> LauncherResult<Vec<crate::worlds::service::Room>> {
    Ok(worlds(&app).client.rooms().await?)
}

#[tauri::command]
pub async fn worlds_room<R: Runtime>(
    app: AppHandle<R>,
    room: String,
) -> LauncherResult<WorldsRoomPage> {
    let worlds = worlds(&app);
    let (found, members) = tokio::join!(worlds.client.room(&room), worlds.client.members(&room));
    Ok(WorldsRoomPage {
        room: found?,
        members: members?,
    })
}

/// A new world on this computer and its room.
#[tauri::command]
pub async fn worlds_create<R: Runtime>(
    app: AppHandle<R>,
    mods: State<'_, Arc<crate::mods::Mods>>,
    payload: WorldsCreatePayload,
) -> LauncherResult<crate::worlds::service::Room> {
    worlds(&app)
        .create(
            &app.state::<AppState>(),
            &mods,
            &payload.name,
            &payload.description,
            &payload.minecraft,
            payload.settings,
            payload.instance_id,
        )
        .await
}

/// What updating a world's mods from an installation would change (W12).
#[tauri::command]
pub async fn worlds_update_preview<R: Runtime>(
    app: AppHandle<R>,
    mods: State<'_, Arc<crate::mods::Mods>>,
    world: crate::storage::paths::WorldId,
    instance_id: crate::storage::paths::InstanceId,
) -> LauncherResult<crate::worlds::update::Preview> {
    worlds(&app)
        .update_preview(&app.state::<AppState>(), &mods, world, instance_id)
        .await
}

/// Updates a stopped world's mods: backed up, tried on a copy, then kept
/// with the way back (W12).
#[tauri::command]
pub async fn worlds_update<R: Runtime>(
    app: AppHandle<R>,
    mods: State<'_, Arc<crate::mods::Mods>>,
    payload: WorldsUpdatePayload,
) -> LauncherResult<()> {
    worlds(&app)
        .update(
            &app.state::<AppState>(),
            &mods,
            &payload.room,
            payload.world,
            payload.instance_id,
            &payload.fingerprint,
            payload.choices,
        )
        .await
}

/// Installations a world with mods can be made from (W12).
#[tauri::command]
pub fn worlds_mod_sources<R: Runtime>(app: AppHandle<R>) -> Vec<crate::worlds::app::ModSource> {
    crate::worlds::app::mod_sources(&app.state::<AppState>())
}

/// The player accepted the Minecraft EULA for a world.
#[tauri::command]
pub fn worlds_accept_eula<R: Runtime>(
    app: AppHandle<R>,
    world: crate::storage::paths::WorldId,
) -> LauncherResult<crate::worlds::app::LocalWorld> {
    worlds(&app).accept_eula(world)
}

/// The installations whose single-player worlds can be brought in.
#[tauri::command]
pub async fn worlds_importable<R: Runtime>(
    app: AppHandle<R>,
) -> LauncherResult<Vec<crate::worlds::safety::Importable>> {
    tauri::async_runtime::spawn_blocking(move || worlds(&app).importable(&app.state::<AppState>()))
        .await
        .map_err(|_| crate::error::LauncherError::internal("Reading the worlds stopped."))?
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorldsImportPayload {
    pub instance_id: crate::storage::paths::InstanceId,
    pub save: String,
    pub name: String,
    pub description: String,
}

/// A new world from a copy of a single-player world, and its room.
#[tauri::command]
pub async fn worlds_import<R: Runtime>(
    app: AppHandle<R>,
    mods: State<'_, Arc<crate::mods::Mods>>,
    payload: WorldsImportPayload,
) -> LauncherResult<crate::worlds::service::Room> {
    worlds(&app)
        .import(
            &app.state::<AppState>(),
            &mods,
            payload.instance_id,
            payload.save,
            payload.name,
            &payload.description,
        )
        .await
}

/// A world's backups, newest first.
#[tauri::command]
pub async fn worlds_backups<R: Runtime>(
    app: AppHandle<R>,
    world: crate::storage::paths::WorldId,
) -> LauncherResult<Vec<crate::backup::store::SnapshotInfo>> {
    let worlds = worlds(&app);
    tauri::async_runtime::spawn_blocking(move || worlds.backups(world))
        .await
        .map_err(|_| crate::error::LauncherError::internal("Reading the backups stopped."))?
}

/// Backs up a stopped world now.
#[tauri::command]
pub async fn worlds_back_up<R: Runtime>(
    app: AppHandle<R>,
    world: crate::storage::paths::WorldId,
) -> LauncherResult<crate::backup::store::SnapshotInfo> {
    worlds(&app).back_up(world).await
}

/// Puts a backup in place of a stopped world; the world is backed up first.
#[tauri::command]
pub async fn worlds_restore<R: Runtime>(
    app: AppHandle<R>,
    world: crate::storage::paths::WorldId,
    backup: String,
) -> LauncherResult<()> {
    worlds(&app).restore(world, backup).await
}

/// A world's name made fit for a file name: letters, digits, spaces, `-`
/// and `_`, at most 60 characters.
fn export_file_name(name: &str) -> String {
    let kept: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' || c == ' ' {
                c
            } else {
                '_'
            }
        })
        .take(60)
        .collect();
    let kept = kept.trim().trim_matches('_');
    format!("{}.zip", if kept.is_empty() { "world" } else { kept })
}

/// Saves a stopped world as a zip where the player chooses.
#[tauri::command]
pub async fn worlds_export<R: Runtime>(
    app: AppHandle<R>,
    world: crate::storage::paths::WorldId,
    title: Option<String>,
) -> LauncherResult<ExportResult> {
    use tauri_plugin_dialog::DialogExt;
    let worlds = worlds(&app);
    let name = {
        let root = app.state::<AppState>().data_root().ok_or_else(|| {
            crate::error::LauncherError::new(
                crate::error::ErrorCode::StorageUnavailable,
                "Launcher data is unavailable.",
            )
        })?;
        crate::worlds::world::load(&root, world)?.name
    };
    let handle = app.clone();
    let title = window_title(title, "Export the world");
    let target = tauri::async_runtime::spawn_blocking(move || {
        handle
            .dialog()
            .file()
            .set_title(title)
            .set_file_name(export_file_name(&name))
            .add_filter("ZIP archive", &["zip"])
            .blocking_save_file()
    })
    .await
    .map_err(|_| crate::error::LauncherError::internal("The save dialog failed."))?;
    let Some(target) = target else {
        return Ok(ExportResult {
            saved: false,
            file_name: None,
        });
    };
    let path = target.into_path().map_err(|_| {
        crate::error::LauncherError::new(
            crate::error::ErrorCode::InvalidInput,
            "This location cannot be used for the export.",
        )
    })?;
    worlds.export(world, path.clone()).await?;
    Ok(ExportResult {
        saved: true,
        file_name: path.file_name().map(|n| n.to_string_lossy().into_owned()),
    })
}

/// Removes, bans or unbans a member.
#[tauri::command]
pub async fn worlds_moderate<R: Runtime>(
    app: AppHandle<R>,
    room: String,
    account: String,
    action: String,
) -> LauncherResult<()> {
    Ok(worlds(&app)
        .client
        .moderate(&room, &account, &action)
        .await?)
}

/// Leaves a room the account is a member of.
#[tauri::command]
pub async fn worlds_leave_room<R: Runtime>(app: AppHandle<R>, room: String) -> LauncherResult<()> {
    Ok(worlds(&app).client.leave(&room).await?)
}

/// A computer of the account, for the device list.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorldsDevice {
    pub id: String,
    pub name: String,
    pub created: i64,
    /// This computer.
    pub current: bool,
}

/// The account's computers for worlds, this one first.
#[tauri::command]
pub async fn worlds_devices<R: Runtime>(app: AppHandle<R>) -> LauncherResult<Vec<WorldsDevice>> {
    let (devices, current) = worlds(&app).client.devices().await?;
    let mut list: Vec<WorldsDevice> = devices
        .into_iter()
        .map(|d| WorldsDevice {
            current: current.as_deref() == Some(d.id.as_str()),
            id: d.id,
            name: d.name,
            created: d.created,
        })
        .collect();
    list.sort_by_key(|d| !d.current);
    Ok(list)
}

/// Signs another computer out of worlds; the worlds it hosts close.
#[tauri::command]
pub async fn worlds_revoke_device<R: Runtime>(
    app: AppHandle<R>,
    device: String,
) -> LauncherResult<()> {
    Ok(worlds(&app).client.revoke_device(&device).await?)
}

/// A new invite code: valid for `hours` (1–168) and `uses` (1–10).
#[tauri::command]
pub async fn worlds_invite<R: Runtime>(
    app: AppHandle<R>,
    room: String,
    hours: u32,
    uses: u32,
) -> LauncherResult<crate::worlds::service::NewInvite> {
    if !(1..=168).contains(&hours) || !(1..=10).contains(&uses) {
        return Err(crate::worlds::service::Problem::Invalid.into());
    }
    Ok(worlds(&app)
        .client
        .create_invite(&room, hours, uses)
        .await?)
}

/// What an invite code leads to, before joining.
#[tauri::command]
pub async fn worlds_preview<R: Runtime>(
    app: AppHandle<R>,
    code: String,
) -> LauncherResult<crate::worlds::service::Preview> {
    if code.len() > 512 {
        return Err(crate::worlds::service::Problem::InviteInvalid.into());
    }
    Ok(worlds(&app).client.preview(&code).await?)
}

/// Becomes a member of the room an invite code leads to.
#[tauri::command]
pub async fn worlds_redeem<R: Runtime>(
    app: AppHandle<R>,
    code: String,
) -> LauncherResult<crate::worlds::service::Room> {
    if code.len() > 512 {
        return Err(crate::worlds::service::Problem::InviteInvalid.into());
    }
    Ok(worlds(&app).client.redeem(&code).await?)
}

/// Hosts a room from its world on this computer; returns once friends can
/// join. Stages and download progress come in `worlds://state`. A world with
/// mods publishes its room pack to the catalog first, as this account.
#[tauri::command]
pub async fn worlds_host<R: Runtime>(app: AppHandle<R>, room: String) -> LauncherResult<()> {
    let (mods, handle) = (mods(&app), app.clone());
    let publish: crate::worlds::pack::PublishRoom = Box::new(move |archive, project| {
        Box::pin(async move {
            crate::mods::online(&handle.state::<AppState>())?;
            let cancel = crate::download::CancelToken::new();
            mods.client
                .publish(
                    &handle,
                    &archive,
                    project.as_deref(),
                    true,
                    &cancel,
                    &|_, _| {},
                )
                .await
        })
    });
    worlds(&app)
        .host(&app.state::<AppState>(), &room, publish)
        .await
}

/// Stops hosting; the world is saved.
#[tauri::command]
pub async fn worlds_stop_hosting<R: Runtime>(app: AppHandle<R>) {
    worlds(&app).stop_hosting().await;
}

/// Joins a hosted room as a guest.
#[tauri::command]
pub async fn worlds_join<R: Runtime>(app: AppHandle<R>, room: String) -> LauncherResult<()> {
    worlds(&app).join(&app.state::<AppState>(), &room).await
}

/// Leaves the room this computer is a guest in.
#[tauri::command]
pub async fn worlds_disconnect<R: Runtime>(app: AppHandle<R>) {
    worlds(&app).disconnect().await;
}

/// Stops getting a room ready.
#[tauri::command]
pub fn worlds_cancel<R: Runtime>(app: AppHandle<R>) {
    worlds(&app).cancel();
}

/// The installation a room plays, made on first use. A guest of a world
/// with mods gets the room pack in it, once they agreed: `accept` is the
/// SHA-256 of the room pack they were shown.
#[tauri::command]
pub async fn worlds_instance<R: Runtime>(
    app: AppHandle<R>,
    room: String,
    accept: Option<String>,
) -> LauncherResult<WorldsInstance> {
    let worlds = worlds(&app);
    let found = worlds.client.room(&room).await?;
    let state = app.state::<AppState>();
    // The host's own world with mods: the installation runs its loader and
    // holds its client mods.
    let local = worlds.local_world_of(&found);
    if local.is_none()
        && let Some(pack) = &found.pack
    {
        return room_pack_instance(&app, &found, pack, accept.as_deref()).await;
    }
    let generation = local.as_ref().and_then(|w| w.pack.as_ref());
    let instance = state.room_instance(
        &found.id,
        &found.name,
        &found.minecraft,
        generation.map(|p| p.loader.clone()),
        generation.map_or(0, |p| u32::try_from(p.mods().count()).unwrap_or(u32::MAX)),
    )?;
    if let Some(world) = local.filter(|w| w.pack.is_some()) {
        worlds.place_client(&world, instance.id).await?;
    }
    Ok(WorldsInstance {
        instance_id: instance.id,
        ready: instance.health == crate::storage::models::InstallationHealth::Ready,
        consent: None,
    })
}

fn room_pack_changed() -> crate::error::LauncherError {
    crate::error::LauncherError::invalid_input(
        "The room's mods changed or are not ready yet. Try again in a moment.",
    )
}

/// A guest's room installation with the room pack in it (W12): the pack the
/// session names, checked against its SHA-256, the loader and Minecraft
/// version the room plays, and a loader the live test covers. Mods the
/// installation does not have yet wait for the player's word; then the pack
/// goes in as any catalog pack does, every file checked by SHA-256.
async fn room_pack_instance<R: Runtime>(
    app: &AppHandle<R>,
    room: &crate::worlds::service::Room,
    pack: &crate::worlds::service::SessionPack,
    accept: Option<&str>,
) -> LauncherResult<WorldsInstance> {
    use crate::mods::Catalog;
    let state = app.state::<AppState>();
    if !pack.valid() {
        return Err(room_pack_changed());
    }
    crate::mods::online(&state)?;
    let mods = mods(app);
    let info = mods.client.pack(&pack.version, false).await?;
    if !info.ready() || info.archive.as_ref().map(|a| a.sha256.as_str()) != Some(&pack.sha256) {
        return Err(room_pack_changed());
    }
    let loader = info.loader_choice()?.ok_or_else(room_pack_changed)?;
    if info.game.as_deref() != Some(room.minecraft.as_str()) {
        return Err(room_pack_changed());
    }
    crate::worlds::pack::check_supported(&loader, &room.minecraft)?;
    let version = mods.client.version(&pack.version).await?;
    if version.id != pack.version {
        return Err(room_pack_changed());
    }
    let instance = state.room_instance(
        &room.id,
        &room.name,
        &room.minecraft,
        Some(loader),
        info.mods,
    )?;
    let id = instance.id;
    let ready = instance.health == crate::storage::models::InstallationHealth::Ready;
    let (root, _) = crate::mods::content::instance_of(&state, id)?;
    let installed = crate::mods::content::load_lock(&root, id).pack;
    match crate::worlds::pack::room_step(&info, installed.as_ref(), pack, accept) {
        crate::worlds::pack::RoomStep::Installed => {
            return Ok(WorldsInstance {
                instance_id: id,
                ready,
                consent: None,
            });
        }
        crate::worlds::pack::RoomStep::Ask(consent) => {
            return Ok(WorldsInstance {
                instance_id: id,
                ready: false,
                consent: Some(consent),
            });
        }
        crate::worlds::pack::RoomStep::Install => {}
    }
    let claim = mods.content.claim(id)?;
    state.activity().ensure_instance_idle(id)?;
    let meta = crate::mods::pack::PackMeta {
        project_id: version.project_id.clone(),
        version_id: version.id.clone(),
        title: crate::mods::line(&room.name, 256),
        version_number: crate::mods::line(&version.number, 128),
        icon: None,
    };
    run_pack(app, &mods, &root, id, &info, meta, &claim.cancel).await?;
    drop(claim);
    Ok(WorldsInstance {
        instance_id: id,
        ready,
        consent: None,
    })
}

/// Starts the game into the room this computer hosts or is a guest in,
/// playing the launcher account's nick.
#[tauri::command]
pub async fn worlds_play<R: Runtime>(
    app: AppHandle<R>,
    launcher: State<'_, Launcher>,
    mods: State<'_, Arc<crate::mods::Mods>>,
    request: MutationRequest<WorldsRoomPayload>,
) -> LauncherResult<crate::state::dto::RunSummary> {
    let launcher = launcher.get()?;
    let state = app.state::<AppState>();
    let room = request.payload.room;
    let port = worlds(&app).play_port(&room).ok_or_else(|| {
        crate::error::LauncherError::new(
            crate::error::ErrorCode::InvalidInput,
            "This computer is not in that room now.",
        )
    })?;
    let nick = crate::worlds::app::nick_of(&state)?;
    let instance = state
        .instance_ids()
        .into_iter()
        .filter_map(|id| state.instance(id))
        .find(|i| i.world_room.as_deref() == Some(room.as_str()))
        .ok_or_else(|| {
            crate::error::LauncherError::new(
                crate::error::ErrorCode::InstanceNotFound,
                "The room's installation no longer exists.",
            )
        })?;
    mods.content.ensure_idle(instance.id)?;
    // A guest plays the mods the room plays now, never an older set.
    let found = worlds(&app).client.room(&room).await?;
    if worlds(&app).local_world_of(&found).is_none()
        && let Some(root) = state.data_root()
    {
        let installed = crate::mods::content::load_lock(&root, instance.id)
            .pack
            .map(|p| p.version_id);
        if installed.as_deref() != found.pack.as_ref().map(|p| p.version.as_str()) {
            return Err(crate::error::LauncherError::invalid_input(
                "The host changed the room's mods. Press Play again to get them.",
            ));
        }
    }
    let mode =
        crate::launch::service::LaunchMode::World(crate::launch::service::WorldPlay { nick, port });
    launcher
        .launch_joining(&request.request_id, instance.id, mode, None)
        .await
}

#[cfg(test)]
mod tests {
    use super::{export_file_name, window_title};

    #[test]
    fn export_names_are_plain_file_names() {
        assert_eq!(export_file_name("Our base"), "Our base.zip");
        assert_eq!(export_file_name("../../etc/passwd"), "etc_passwd.zip");
        assert_eq!(export_file_name("a\\b:c*?"), "a_b_c.zip");
        assert_eq!(export_file_name("   "), "world.zip");
        assert_eq!(export_file_name("Мир 1"), "Мир 1.zip");
        assert_eq!(export_file_name(&"x".repeat(200)).len(), 64);
    }

    #[test]
    fn titles_from_the_interface_are_plain_and_short() {
        assert_eq!(
            window_title(Some(" Выберите папку ".into()), "Choose"),
            "Выберите папку"
        );
        assert_eq!(
            window_title(Some("a\u{0}b\nc\u{1b}[2J".into()), "x"),
            "abc[2J"
        );
        assert_eq!(
            window_title(Some("x".repeat(500)), "f").chars().count(),
            120
        );
        assert_eq!(window_title(Some("  \n ".into()), "Choose"), "Choose");
        assert_eq!(window_title(None, "Choose"), "Choose");
    }
}
