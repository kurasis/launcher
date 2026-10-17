//! IPC command handlers. Handlers validate input and delegate; they hold no business
//! logic. Every command must also be listed in build.rs and granted in capabilities/.

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
    AppSnapshot, CreateInstancePayload, InstanceSummary, MutationRequest, SettingsPatch,
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

#[tauri::command]
pub fn get_app_snapshot(state: State<'_, AppState>) -> AppSnapshot {
    state.snapshot()
}

#[tauri::command]
pub fn create_instance(
    state: State<'_, AppState>,
    request: MutationRequest<CreateInstancePayload>,
) -> LauncherResult<InstanceSummary> {
    state.create_instance(request)
}

#[tauri::command]
pub fn update_instance_settings(
    state: State<'_, AppState>,
    request: MutationRequest<UpdateInstanceSettingsPayload>,
) -> LauncherResult<InstanceSummary> {
    state.update_instance_settings(request)
}

#[tauri::command]
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
#[tauri::command]
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

#[tauri::command]
pub fn cancel_login(
    auth: State<'_, Arc<AuthService>>,
    request: MutationRequest<OperationPayload>,
) -> LauncherResult<()> {
    crate::state::ledger::validate_request_id(&request.request_id)?;
    auth.cancel_login(&request.payload.operation_id)
}

/// Local sign-out: clears launch authorization and removes saved credentials.
#[tauri::command]
pub async fn sign_out(
    auth: State<'_, Arc<AuthService>>,
    request: MutationRequest<EmptyPayload>,
) -> LauncherResult<()> {
    crate::state::ledger::validate_request_id(&request.request_id)?;
    auth.sign_out().await
}

/// Re-checks current access with fresh service data (for "Check again"). The
/// launch token is not exposed; launches re-verify independently.
#[tauri::command]
pub async fn verify_account(
    auth: State<'_, Arc<AuthService>>,
    request: MutationRequest<EmptyPayload>,
) -> LauncherResult<()> {
    crate::state::ledger::validate_request_id(&request.request_id)?;
    auth.verify_for_launch().await.map(|_| ())
}

/// Opens one of the fixed official destinations in the system browser.
#[tauri::command]
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
    installer.get()?.start(
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
    installer.get()?.start(
        &request.request_id,
        crate::storage::models::OperationKind::Repair,
        request.payload.instance_id,
    )
}

/// Requests cancellation of an install, a repair or a launch that has not
/// started Java yet. A running game is not stopped here; use `stop_game`.
#[tauri::command]
pub fn cancel_operation(
    installer: State<'_, Installer>,
    launcher: State<'_, Launcher>,
    state: State<'_, AppState>,
    request: MutationRequest<OperationPayload>,
) -> LauncherResult<crate::install::service::CancelAck> {
    crate::state::ledger::validate_request_id(&request.request_id)?;
    let operation_id = &request.payload.operation_id;
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
    /// The no-token profile, which plays Minecraft's demo without an account
    /// (D-033); otherwise the signed-in account.
    #[serde(default)]
    pub notoken: bool,
}

/// Checks the installation and account (or uses the no-token profile), then starts
/// Minecraft under supervision.
#[tauri::command]
pub async fn launch_game(
    launcher: State<'_, Launcher>,
    request: MutationRequest<LaunchPayload>,
) -> LauncherResult<crate::state::dto::RunSummary> {
    let mode = if request.payload.notoken {
        crate::launch::service::LaunchMode::NoToken
    } else {
        crate::launch::service::LaunchMode::Account
    };
    launcher
        .get()?
        .launch_as(&request.request_id, request.payload.instance_id, mode)
        .await
}

/// Terminates the owned game process of `runId` after the user confirmed it.
#[tauri::command]
pub fn stop_game(
    launcher: State<'_, Launcher>,
    request: MutationRequest<RunPayload>,
) -> LauncherResult<bool> {
    crate::state::ledger::validate_request_id(&request.request_id)?;
    Ok(launcher.get()?.stop(&request.payload.run_id))
}

/// Opens an installation's game folder in the system file manager. The path is
/// resolved in Rust from the instance ID; the webview never supplies a path.
#[tauri::command]
pub fn open_instance_folder<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
    request: MutationRequest<InstancePayload>,
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
    let dir = root.game_dir(id);
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
