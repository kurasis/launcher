//! IPC command handlers. Handlers validate input and delegate; they hold no business
//! logic. Every command must also be listed in build.rs and granted in capabilities/.

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, Runtime, State};

use crate::auth::client_id::{self, ClientIdStatus};
use crate::error::LauncherResult;
use crate::lifecycle::{LifecycleController, RunHandles};
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
        }
    }
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
            app.state::<RunHandles>().kill(&active.operation_id);
            controller.set_quit_intent();
            let handle = app.clone();
            std::thread::spawn(move || {
                // Bounded wait for the supervisor to record the result and release
                // the permit; OS shutdown paths must never hang here indefinitely.
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
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
