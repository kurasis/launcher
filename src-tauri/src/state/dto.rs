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
    DuringGameMode, InstallationHealth, InstalledJava, InstanceFile, Language, LastResult,
    OperationKind, Outcome, ProfileRef, Resolution, Settings, StartView, Theme,
};
use crate::storage::paths::{InstanceId, JavaProvider, Sha1};
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
    /// A mod loader to install on top of the game (D-039).
    #[serde(default)]
    pub loader: Option<crate::loader::LoaderChoice>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SetInstanceNotesPayload {
    pub instance_id: InstanceId,
    pub notes: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RenameInstancePayload {
    pub instance_id: InstanceId,
    pub display_name: String,
}

/// Which Java an installation runs (D-044).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum JavaChoice {
    /// A private runtime from this provider; a change takes effect at the next repair.
    Managed { provider: JavaProvider },
    /// The user's own Java program.
    Custom { path: std::path::PathBuf },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SetInstanceJavaPayload {
    pub instance_id: InstanceId,
    pub java: JavaChoice,
    #[serde(default)]
    pub jvm_arguments: String,
    /// The user saw the compatibility warnings of a custom Java and chose it anyway.
    #[serde(default)]
    pub accept_issues: bool,
}

/// Deletes an installation, after a backup of its game folder if asked (D-055).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeleteInstancePayload {
    pub instance_id: InstanceId,
    pub backup: bool,
}

/// A new version of the installation's mod loader; the loader itself stays.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SetInstanceLoaderPayload {
    pub instance_id: InstanceId,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CheckJavaPayload {
    pub instance_id: InstanceId,
    pub path: std::path::PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdateInstanceSettingsPayload {
    pub instance_id: InstanceId,
    pub max_heap_mib: u32,
    pub resolution: Resolution,
    /// Left unchanged when omitted (D-056).
    #[serde(default)]
    pub fullscreen: Option<bool>,
    /// Extra Minecraft arguments; left unchanged when omitted (D-056).
    #[serde(default)]
    pub game_arguments: Option<String>,
    /// Launcher Skins in the game (D-065); left unchanged when omitted.
    #[serde(default)]
    pub launcher_skins: Option<bool>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_fullscreen: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub java_provider: Option<JavaProvider>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backup_after_play: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backup_keep: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metered: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metered_warn_mib: Option<u32>,
    /// 0 removes the limit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub monthly_limit_mib: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub share_stats: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chat_sound: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chat_notifications: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub launcher_account_offline: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notoken_offline: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub download_source: Option<crate::mirror::DownloadSource>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub update_mode: Option<crate::update::UpdateMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub update_channel: Option<crate::update::UpdateChannel>,
    /// True replaces the crash report install ID with a new one (D-061).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_install_id: Option<bool>,
    /// One of `StartView`'s names; the core checks it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_view: Option<String>,
    /// The screen just opened, remembered for `StartView::Last`; never `last`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_view: Option<String>,
    /// The feed's settings (D-067); fields omitted are left unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feed: Option<FeedSettingsPatch>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worlds_relay_only: Option<bool>,
}

/// Changes to [`crate::storage::models::FeedSettings`]; omitted fields stay.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FeedSettingsPatch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshots: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub video_language: Option<crate::storage::models::VideoLanguage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shorts: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub machine_translation: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub player: Option<crate::storage::models::PlayerMode>,
    /// The whole list of hidden blocks; at most one of each.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hidden: Option<Vec<crate::storage::models::FeedBlock>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub live_notifications: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsView {
    pub theme: Theme,
    pub during_game_mode: DuringGameMode,
    pub selected_instance_id: Option<InstanceId>,
    pub default_max_heap_mib: Option<u32>,
    pub default_resolution: Resolution,
    pub default_fullscreen: bool,
    pub java_provider: JavaProvider,
    pub language: Language,
    pub backup_after_play: bool,
    pub backup_keep: u32,
    pub metered: bool,
    pub metered_warn_mib: u32,
    pub monthly_limit_mib: u32,
    pub share_stats: bool,
    pub chat_sound: bool,
    pub chat_notifications: bool,
    pub launcher_account_offline: bool,
    pub notoken_offline: bool,
    pub download_source: crate::mirror::DownloadSource,
    pub update_mode: crate::update::UpdateMode,
    pub update_channel: crate::update::UpdateChannel,
    /// The crash report install ID, once a report made one (D-061).
    pub install_id: Option<String>,
    pub start_view: StartView,
    pub last_view: StartView,
    pub feed: crate::storage::models::FeedSettings,
    pub worlds_relay_only: bool,
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
            default_fullscreen: settings.default_fullscreen,
            java_provider: settings.java_provider,
            language: settings.language.clone(),
            backup_after_play: settings.backup_after_play,
            backup_keep: settings.backup_keep,
            metered: settings.metered,
            metered_warn_mib: settings.metered_warn_mib,
            monthly_limit_mib: settings.monthly_limit_mib,
            share_stats: settings.share_stats,
            chat_sound: settings.chat_sound,
            chat_notifications: settings.chat_notifications,
            launcher_account_offline: settings.launcher_account_offline,
            notoken_offline: settings.notoken_offline,
            download_source: settings.download_source,
            update_mode: settings.update_mode,
            update_channel: settings.update_channel,
            install_id: settings.install_id.clone(),
            start_view: settings.start_view,
            last_view: settings.last_view,
            feed: settings.feed.clone(),
            worlds_relay_only: settings.worlds_relay_only,
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
    pub java: Option<InstalledJava>,
    pub loader: Option<crate::loader::LoaderChoice>,
    pub notes: String,
    /// A provider the next repair switches the private runtime to.
    pub java_provider: Option<JavaProvider>,
    pub custom_java: Option<crate::storage::models::CustomJava>,
    pub jvm_arguments: String,
    /// Who plays this installation; `None` is the default profile (D-045).
    pub profile: Option<ProfileRef>,
    pub game_arguments: String,
    pub fullscreen: bool,
    /// Launcher Skins is added to the game where a jar exists for it (D-065).
    pub launcher_skins: bool,
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
            java: instance.java.clone(),
            loader: instance.loader.clone(),
            notes: instance.notes.clone(),
            java_provider: instance.java_provider,
            custom_java: instance.custom_java.clone(),
            jvm_arguments: instance.jvm_arguments.clone(),
            profile: instance.profile.clone(),
            game_arguments: instance.game_arguments.clone(),
            fullscreen: instance.fullscreen,
            launcher_skins: instance.launcher_skins,
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

/// One Microsoft account as the interface shows it. Never contains tokens.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MicrosoftAccount {
    pub profile_id: String,
    pub profile_name: String,
    pub state: AccountState,
    /// The last verification failure, if the state is an error state.
    pub error: Option<LauncherError>,
}

/// An account whose saved credential could not be deleted yet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingSignOut {
    pub profile_id: String,
    pub profile_name: String,
}

/// What the auth service reports: Microsoft accounts and the browser sign-in.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthStatus {
    pub accounts: Vec<MicrosoftAccount>,
    /// The active browser sign-in attempt, cancellable with `cancel_login`.
    pub login_operation_id: Option<String>,
    /// `signing_in` during a browser sign-in; after a failed one, the state its
    /// error stands for; otherwise `signed_out`.
    pub login_state: AccountState,
    /// Why the last sign-in did not add an account.
    pub login_error: Option<LauncherError>,
    /// True when credentials are kept in memory only (no usable OS vault).
    pub session_only: bool,
    pub pending_sign_outs: Vec<PendingSignOut>,
}

impl AuthStatus {
    pub fn signed_out() -> Self {
        Self {
            accounts: Vec::new(),
            login_operation_id: None,
            login_state: AccountState::SignedOut,
            login_error: None,
            session_only: false,
            pending_sign_outs: Vec::new(),
        }
    }

    /// Adds or updates one account.
    pub fn upsert(&mut self, account: MicrosoftAccount) {
        match self
            .accounts
            .iter_mut()
            .find(|a| a.profile_id == account.profile_id)
        {
            Some(existing) => *existing = account,
            None => self.accounts.push(account),
        }
    }
}

/// A no-token profile as the interface shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NoTokenProfileView {
    pub id: String,
    pub name: String,
}

/// Everything about accounts and profiles (D-045). Never contains tokens or vault
/// contents.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountSummary {
    #[serde(flatten)]
    pub auth: AuthStatus,
    pub notoken_profiles: Vec<NoTokenProfileView>,
    /// The profile installations play as unless they name another one.
    pub default_profile: Option<ProfileRef>,
    /// The launcher account (D-064).
    pub launcher_account: crate::launcher_account::LauncherAccountView,
    /// A Microsoft account with play access is signed in here: profiles without
    /// an account may then play offline in the full game.
    pub full_game_allowed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProfileIdPayload {
    pub profile_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AddNoTokenProfilePayload {
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SetDefaultProfilePayload {
    pub profile: Option<ProfileRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SetInstanceProfilePayload {
    pub instance_id: InstanceId,
    /// `None` plays as the default profile.
    pub profile: Option<ProfileRef>,
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
    /// Ended by the user's Stop; not an error of the game.
    Stopped,
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
    /// True when the run's output log could not be written (disk full,
    /// permissions); output was still drained and kept in memory.
    #[serde(default)]
    pub logging_degraded: bool,
    /// The Unix signal that ended the game, if any.
    #[serde(default)]
    pub signal: Option<i32>,
    /// Why the game stopped, when it crashed (D-059).
    #[serde(default)]
    pub diagnosis: Option<crate::launch::crash::CrashDiagnosis>,
    /// The server the game last joined (`host:port`), from its own log: what
    /// the player may invite friends to.
    #[serde(default)]
    pub server: Option<String>,
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
    /// Installs and repairs waiting their turn, first first.
    pub queue: Vec<QueuedOperation>,
    /// Whether the internet can be reached (D-052).
    pub network: NetworkStatus,
    /// Download hosts served by the mirror now (D-062).
    pub mirrored_hosts: Vec<String>,
    /// A launcher update and where it stands (D-063).
    pub update: crate::update::UpdateView,
    pub current_run: Option<RunSummary>,
    pub last_run: Option<RunSummary>,
    /// True in debug builds, which include the lifecycle test harness.
    pub lifecycle_harness_available: bool,
    pub supported_versions: &'static [SupportedVersion],
    pub memory: MemoryLimits,
    pub storage_issues: Vec<StorageIssue>,
    /// Latest progress of the running install/repair operation, if any.
    pub operation_progress: Option<OperationProgress>,
    /// Terminal outcomes of recent install/repair operations, newest first.
    pub recent_operations: Vec<OperationResult>,
}

/// Phase of an install or repair operation (doc 06 `InstallState`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstallState {
    Resolving,
    CheckingFiles,
    Downloading,
    ExtractingRuntime,
    /// Running a mod loader's installer step (D-039).
    PreparingLoader,
    Finalizing,
    CancelRequested,
    /// Waiting for the user to allow a download on a metered connection (D-051).
    AwaitingApproval,
}

/// A download that waits for the user's answer on a metered connection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApprovalRequest {
    pub bytes: u64,
    pub files: u64,
    pub reason: crate::traffic::Caution,
    /// Downloaded this month before this download.
    pub month_bytes: u64,
    /// 0 is no limit.
    pub monthly_limit_mib: u32,
}

/// Coalesced progress of one install/repair operation. Published without a new
/// state revision; `sequence` increases per operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationProgress {
    pub session_epoch: String,
    pub operation_id: String,
    pub instance_id: Option<InstanceId>,
    pub kind: OperationKind,
    pub sequence: u64,
    pub revision: u64,
    pub phase: InstallState,
    pub downloaded_bytes: u64,
    pub total_download_bytes: Option<u64>,
    pub verified_files: u64,
    pub total_files: Option<u64>,
    pub bytes_per_second: Option<u64>,
    /// Names of the files being downloaded right now.
    pub current_files: Vec<String>,
    /// Set while the operation waits for the user's answer (D-051).
    pub approval: Option<ApprovalRequest>,
    pub timestamp: DateTime<Utc>,
}

/// The one terminal outcome of an install/repair operation.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationResult {
    pub operation_id: String,
    pub instance_id: Option<InstanceId>,
    pub kind: OperationKind,
    pub outcome: Outcome,
    pub error: Option<LauncherError>,
    pub finished_at: DateTime<Utc>,
    /// Files downloaded and network bytes received (including retries).
    pub downloaded_files: u64,
    pub network_bytes: u64,
    pub reused_files: u64,
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
pub struct InstanceRemoved {
    pub instance_id: InstanceId,
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
    pub queue: Vec<QueuedOperation>,
    pub network: NetworkStatus,
    pub mirrored_hosts: Vec<String>,
    pub update: crate::update::UpdateView,
}

/// Whether the launcher can reach the internet (D-052).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NetworkStatus {
    /// Not checked yet.
    #[default]
    Unknown,
    Online,
    Offline,
}

/// An install or repair waiting for the running operation to end (D-049).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueuedOperation {
    pub queue_id: String,
    pub instance_id: InstanceId,
    pub kind: OperationKind,
    pub queued_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationFinished {
    pub result: OperationResult,
    pub active_operation: Option<ActiveActivity>,
}

/// Backend events, named as in doc 06.
#[derive(Debug, Clone)]
pub enum BackendEvent {
    InstanceChanged(Box<EventEnvelope<InstanceChanged>>),
    InstanceRemoved(Box<EventEnvelope<InstanceRemoved>>),
    SettingsChanged(EventEnvelope<SettingsChanged>),
    GameStateChanged(Box<EventEnvelope<GameStateChanged>>),
    AccountStateChanged(Box<EventEnvelope<AccountStateChanged>>),
    /// Carries the current revision; does not advance it.
    OperationProgress(Box<OperationProgress>),
    OperationFinished(Box<EventEnvelope<OperationFinished>>),
}

impl BackendEvent {
    pub fn name(&self) -> &'static str {
        match self {
            Self::InstanceChanged(_) => "instance_changed",
            Self::InstanceRemoved(_) => "instance_removed",
            Self::SettingsChanged(_) => "settings_changed",
            Self::GameStateChanged(_) => "game_state_changed",
            Self::AccountStateChanged(_) => "account_state_changed",
            Self::OperationProgress(_) => "operation_progress",
            Self::OperationFinished(_) => "operation_finished",
        }
    }

    pub fn revision(&self) -> u64 {
        match self {
            Self::InstanceChanged(e) => e.revision,
            Self::InstanceRemoved(e) => e.revision,
            Self::SettingsChanged(e) => e.revision,
            Self::GameStateChanged(e) => e.revision,
            Self::AccountStateChanged(e) => e.revision,
            Self::OperationProgress(e) => e.revision,
            Self::OperationFinished(e) => e.revision,
        }
    }

    pub fn payload(&self) -> serde_json::Value {
        match self {
            Self::InstanceChanged(e) => serde_json::to_value(e),
            Self::InstanceRemoved(e) => serde_json::to_value(e),
            Self::SettingsChanged(e) => serde_json::to_value(e),
            Self::GameStateChanged(e) => serde_json::to_value(e),
            Self::AccountStateChanged(e) => serde_json::to_value(e),
            Self::OperationProgress(e) => serde_json::to_value(e),
            Self::OperationFinished(e) => serde_json::to_value(e),
        }
        .unwrap_or(serde_json::Value::Null)
    }
}
