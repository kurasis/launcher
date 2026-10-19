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
    ProfileIdPayload, SetDefaultProfilePayload, SetInstanceJavaPayload, SetInstanceLoaderPayload,
    SetInstanceNotesPayload, SetInstanceProfilePayload, SettingsPatch, SettingsView,
    UpdateInstanceSettingsPayload,
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

/// Native file dialog for choosing a Java program.
#[tauri::command]
pub async fn pick_java_file<R: Runtime>(app: AppHandle<R>) -> LauncherResult<Option<String>> {
    use tauri_plugin_dialog::DialogExt;
    let picked = tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .set_title("Choose the Java program (java or javaw.exe)")
            .blocking_pick_file()
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
    request: MutationRequest<SettingsPatch>,
) -> LauncherResult<SettingsView> {
    state.set_app_settings(request)
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
    /// A run is active; the user must choose an action.
    #[serde(rename_all = "camelCase")]
    ChoiceRequired { operation_id: String },
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
    let Some(active) = state.activity().current() else {
        controller.set_quit_intent();
        app.exit(0);
        return Ok(QuitResult::Quitting);
    };
    match request.payload.action {
        QuitAction::Quit => Ok(QuitResult::ChoiceRequired {
            operation_id: active.operation_id,
        }),
        QuitAction::KeepInBackground => {
            controller.close_main_window(&app);
            Ok(QuitResult::InBackground)
        }
        QuitAction::StopAndQuit => {
            // Stop exactly the active owned work: a game process (or a game that
            // survived a restart), a launch still in preflight, or an install/repair.
            let id = &active.operation_id;
            let mut stopped = app.state::<RunHandles>().kill(id);
            if let Some(launcher) = app.state::<Launcher>().0.as_ref() {
                stopped |= launcher.cancel_pending(id).is_some() || launcher.stop(id);
            }
            if !stopped && let Some(installer) = app.state::<Installer>().0.as_ref() {
                let _ = installer.cancel(id);
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
}

/// Checks the installation and the account of the installation's profile (or
/// uses a no-token profile), then starts Minecraft under supervision (D-045).
/// Debug builds started with `LAUNCHER_NOTOKEN_PROFILE` always play as that
/// test profile (D-030).
#[tauri::command]
pub async fn launch_game(
    launcher: State<'_, Launcher>,
    request: MutationRequest<LaunchPayload>,
) -> LauncherResult<crate::state::dto::RunSummary> {
    let launcher = launcher.get()?;
    let id = request.payload.instance_id;
    #[cfg(debug_assertions)]
    let mode = crate::launch::notoken::NoTokenProfile::from_env()
        .map(crate::launch::service::LaunchMode::NoToken)
        .unwrap_or_else(|| launcher.mode_for(id));
    #[cfg(not(debug_assertions))]
    let mode = launcher.mode_for(id);
    launcher.launch_as(&request.request_id, id, mode).await
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
}

impl GameFolder {
    fn name(self) -> &'static str {
        match self {
            Self::Mods => "mods",
            Self::Config => "config",
            Self::CrashReports => "crash-reports",
            Self::Logs => "logs",
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
    let target = tauri::async_runtime::spawn_blocking(move || {
        handle
            .dialog()
            .file()
            .set_title("Save diagnostic export")
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
pub async fn pick_data_folder<R: Runtime>(app: AppHandle<R>) -> LauncherResult<Option<String>> {
    use tauri_plugin_dialog::DialogExt;
    let picked = tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .set_title("Choose a folder for the launcher data")
            .blocking_pick_folder()
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

/// Looks for a launcher update now (D-063).
#[tauri::command]
pub async fn check_for_update<R: Runtime>(app: AppHandle<R>) -> crate::update::UpdateView {
    let updates = Arc::clone(&app.state::<Arc<crate::update::Updates>>());
    updates.check(&app, true).await;
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
pub fn launcher_account_open<R: Runtime>(app: AppHandle<R>, screen: String) {
    let account = Arc::clone(&app.state::<Arc<crate::launcher_account::LauncherAccount>>());
    let handle = app.clone();
    // Windows are made on the main thread.
    let _ = app.run_on_main_thread(move || account.open(&handle, &screen));
}

#[tauri::command]
pub async fn launcher_account_sign_out<R: Runtime>(app: AppHandle<R>) {
    let account = Arc::clone(&app.state::<Arc<crate::launcher_account::LauncherAccount>>());
    account.sign_out(&app.state::<AppState>()).await;
    friends(&app).wake();
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

#[tauri::command]
pub async fn friends_report<R: Runtime>(
    app: AppHandle<R>,
    message: i64,
    reason: String,
) -> FriendsResult<()> {
    friends(&app).report(&app, message, &reason).await
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
