//! IPC data transfer objects. Mirrored in `src/lib/contracts.ts`.
//!
//! These types are the only state the webview receives. They never contain tokens,
//! OAuth parameters, credential-vault contents or launch command lines; a test
//! serializes a full snapshot and checks for secret-looking field names.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::activity::ActiveActivity;
use crate::auth::client_id::ClientIdStatus;
use crate::catalog::compatibility::SupportedVersion;
use crate::error::LauncherError;
use crate::storage::StorageIssue;
use crate::storage::models::{
    DuringGameMode, InstallationHealth, InstanceFile, LastResult, Resolution, Settings, Theme,
};
use crate::storage::paths::{InstanceId, Sha1};
use crate::validation::{MemoryPolicy, ResolutionBounds};

/// Bumped on any incompatible change to the snapshot or event shapes.
pub const API_SCHEMA_VERSION: u32 = 1;

/// Envelope for every mutating command (doc 06 "Commands").
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MutationRequest<T> {
    pub request_id: String,
    pub payload: T,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateInstancePayload {
    pub display_name: String,
    pub version_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdateInstanceSettingsPayload {
    pub instance_id: InstanceId,
    pub max_heap_mib: u32,
    pub resolution: Resolution,
}

/// Fields omitted from the patch are left unchanged.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SettingsPatch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub theme: Option<Theme>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub during_game_mode: Option<DuringGameMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_instance_id: Option<InstanceId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_max_heap_mib: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_resolution: Option<Resolution>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsView {
    pub theme: Theme,
    pub during_game_mode: DuringGameMode,
    pub selected_instance_id: Option<InstanceId>,
    pub default_max_heap_mib: Option<u32>,
    pub default_resolution: Resolution,
    /// False when settings were written by a newer launcher or could not be read.
    pub writable: bool,
}

impl SettingsView {
    pub fn new(settings: &Settings, writable: bool) -> Self {
        Self {
            theme: settings.theme,
            during_game_mode: settings.during_game_mode,
            selected_instance_id: settings.selected_instance_id,
            default_max_heap_mib: settings.default_max_heap_mib,
            default_resolution: settings.default_resolution,
            writable,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstanceSummary {
    pub id: InstanceId,
    pub display_name: String,
    pub version_id: String,
    pub manifest_sha1: Option<Sha1>,
    pub created_at: DateTime<Utc>,
    pub last_played_at: Option<DateTime<Utc>>,
    pub max_heap_mib: u32,
    pub resolution: Resolution,
    pub health: InstallationHealth,
    pub last_result: Option<LastResult>,
    /// True while an operation holds this instance.
    pub busy: bool,
}

impl InstanceSummary {
    pub fn new(instance: &InstanceFile, busy: bool) -> Self {
        Self {
            id: instance.id,
            display_name: instance.display_name.clone(),
            version_id: instance.version_id.clone(),
            manifest_sha1: instance.manifest_sha1.clone(),
            created_at: instance.created_at,
            last_played_at: instance.last_played_at,
            max_heap_mib: instance.max_heap_mib,
            resolution: instance.resolution,
            health: instance.health,
            last_result: instance.last_result.clone(),
            busy,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccountState {
    SignedOut,
    SigningIn,
    Refreshing,
    VerifiedAccess,
    NoAccess,
    ProfileSetupRequired,
    ReauthRequired,
    ServiceUnavailable,
    AppNotApproved,
    Restricted,
    ProtocolError,
    SignOutPending,
}

/// Account display state. Never contains tokens or vault contents.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountSummary {
    pub state: AccountState,
    pub profile_name: Option<String>,
    pub profile_id: Option<String>,
    /// The last sign-in or verification failure, if the state is an error state.
    pub error: Option<LauncherError>,
    /// True when credentials are kept in memory only (no usable OS vault).
    pub session_only: bool,
    /// The active browser sign-in attempt, cancellable with `cancel_login`.
    pub login_operation_id: Option<String>,
}

impl AccountSummary {
    pub fn signed_out() -> Self {
        Self {
            state: AccountState::SignedOut,
            profile_name: None,
            profile_id: None,
            error: None,
            session_only: false,
            login_operation_id: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DataRootStatus {
    Ready,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DataRootInfo {
    pub status: DataRootStatus,
    /// Shown read-only in Settings; `None` when it could not be determined.
    pub path: Option<String>,
    pub error: Option<LauncherError>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryLimits {
    pub policy: Option<MemoryPolicy>,
    pub error: Option<LauncherError>,
    pub resolution: ResolutionBounds,
}

/// Lifecycle of one supervised process (doc 06 "Snapshot and control details").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunState {
    Preflight,
    Spawning,
    Running,
    Stopping,
    Exited,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunKind {
    /// Harmless child used to prove window lifecycle behaviour (debug builds only).
    LifecycleTest,
    Game,
}

/// A supervised run. Output lines are redacted before they reach this type.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunSummary {
    pub run_id: String,
    pub kind: RunKind,
    pub instance_id: Option<InstanceId>,
    pub state: RunState,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    pub exit_code: Option<i32>,
    pub duration_ms: Option<u64>,
    /// True when output pipes stayed open after exit and capture was closed.
    pub drain_timed_out: bool,
    pub output_tail: Vec<String>,
    pub output_lines_dropped: u64,
    pub error: Option<LauncherError>,
}

/// The complete state a freshly created webview needs (doc 06 "Bootstrap").
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSnapshot {
    pub api_schema_version: u32,
    pub session_epoch: String,
    pub revision: u64,
    pub app_version: String,
    pub data_root: DataRootInfo,
    pub settings: SettingsView,
    pub account: AccountSummary,
    pub microsoft_client_id: ClientIdStatus,
    pub instances: Vec<InstanceSummary>,
    pub active_operation: Option<ActiveActivity>,
    pub current_run: Option<RunSummary>,
    pub last_run: Option<RunSummary>,
    /// True in debug builds, which include the lifecycle test harness.
    pub lifecycle_harness_available: bool,
    pub supported_versions: &'static [SupportedVersion],
    pub memory: MemoryLimits,
    pub storage_issues: Vec<StorageIssue>,
}

/// Events pushed to the webview. Each carries the epoch and the revision it
/// produced so clients can detect gaps and refetch the snapshot.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EventEnvelope<T: Serialize> {
    pub session_epoch: String,
    pub revision: u64,
    pub timestamp: DateTime<Utc>,
    #[serde(flatten)]
    pub body: T,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstanceChanged {
    pub instance: InstanceSummary,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsChanged {
    pub settings: SettingsView,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountStateChanged {
    pub account: AccountSummary,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameStateChanged {
    pub current_run: Option<RunSummary>,
    pub last_run: Option<RunSummary>,
    pub active_operation: Option<ActiveActivity>,
}

/// Backend events, named as in doc 06.
#[derive(Debug, Clone)]
pub enum BackendEvent {
    InstanceChanged(EventEnvelope<InstanceChanged>),
    SettingsChanged(EventEnvelope<SettingsChanged>),
    GameStateChanged(Box<EventEnvelope<GameStateChanged>>),
    AccountStateChanged(Box<EventEnvelope<AccountStateChanged>>),
}

impl BackendEvent {
    pub fn name(&self) -> &'static str {
        match self {
            Self::InstanceChanged(_) => "instance_changed",
            Self::SettingsChanged(_) => "settings_changed",
            Self::GameStateChanged(_) => "game_state_changed",
            Self::AccountStateChanged(_) => "account_state_changed",
        }
    }

    pub fn revision(&self) -> u64 {
        match self {
            Self::InstanceChanged(e) => e.revision,
            Self::SettingsChanged(e) => e.revision,
            Self::GameStateChanged(e) => e.revision,
            Self::AccountStateChanged(e) => e.revision,
        }
    }

    pub fn payload(&self) -> serde_json::Value {
        match self {
            Self::InstanceChanged(e) => serde_json::to_value(e),
            Self::SettingsChanged(e) => serde_json::to_value(e),
            Self::GameStateChanged(e) => serde_json::to_value(e),
            Self::AccountStateChanged(e) => serde_json::to_value(e),
        }
        .unwrap_or(serde_json::Value::Null)
    }
}
