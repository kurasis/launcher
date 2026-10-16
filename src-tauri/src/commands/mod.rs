//! IPC command handlers. Handlers validate input and delegate; they hold no business
//! logic. Every command must also be listed in build.rs and granted in capabilities/.

use serde::Serialize;
use tauri::State;

use crate::auth::client_id::{self, ClientIdStatus};
use crate::error::LauncherResult;
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
