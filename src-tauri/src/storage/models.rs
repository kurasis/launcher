//! App-owned persistent documents (docs/spec/02 "Persistent models").
//!
//! None of these types may hold a token, authorization code or other secret. The
//! account document stores only display metadata and a credential-vault lookup key.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::document::SCHEMA_VERSION;
use super::paths::{ImageType, InstanceId, JavaProvider, Sha1, Sha256, TargetArch, TargetOs};
use crate::error::ErrorCode;
use crate::loader::{LoaderChoice, LoaderKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    #[default]
    System,
    Light,
    Dark,
}

/// Interface language as a BCP 47 tag such as `en`, `ru`, `ar` or `pt-BR`
/// (D-038). The core checks only the form; the interface knows which languages
/// it has and falls back to English for the rest. English is the default.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct Language(String);

impl Language {
    pub const ENGLISH: &'static str = "en";

    /// `language[-Script][-REGION]`: `en`, `zh-Hant`, `pt-BR`, `es-419`.
    pub fn parse(tag: &str) -> Option<Self> {
        let mut parts = tag.split('-');
        let language = parts.next()?;
        let mut ok =
            (2..=3).contains(&language.len()) && language.bytes().all(|b| b.is_ascii_lowercase());
        let mut seen_region = false;
        for part in parts {
            let script = part.len() == 4
                && part.as_bytes()[0].is_ascii_uppercase()
                && part.bytes().skip(1).all(|b| b.is_ascii_lowercase());
            let region = (part.len() == 2 && part.bytes().all(|b| b.is_ascii_uppercase()))
                || (part.len() == 3 && part.bytes().all(|b| b.is_ascii_digit()));
            ok &= !seen_region && (script || region);
            seen_region |= region;
        }
        (ok && tag.len() <= 16).then(|| Self(tag.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for Language {
    fn default() -> Self {
        Self(Self::ENGLISH.to_owned())
    }
}

/// A malformed tag in `settings.json` means English, not unreadable settings.
impl<'de> Deserialize<'de> for Language {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let tag = String::deserialize(deserializer)?;
        Ok(Self::parse(&tag).unwrap_or_default())
    }
}

/// The screen the launcher opens at start (Settings → General). `Last` reopens
/// the screen remembered in [`Settings::last_view`]. Screens the interface does
/// not have yet open Installations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum StartView {
    /// The news feed.
    #[default]
    Feed,
    /// Installations.
    Home,
    Mods,
    Skins,
    Account,
    /// The screen open when the launcher was last used.
    Last,
}

impl StartView {
    pub const ALL: [Self; 6] = [
        Self::Feed,
        Self::Home,
        Self::Mods,
        Self::Skins,
        Self::Account,
        Self::Last,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Feed => "feed",
            Self::Home => "home",
            Self::Mods => "mods",
            Self::Skins => "skins",
            Self::Account => "account",
            Self::Last => "last",
        }
    }

    /// One of the closed set of names; anything else is `None`.
    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|view| view.as_str() == value)
    }

    /// A screen that can be remembered as the last one opened: every start
    /// view but `Last` itself.
    pub fn parse_remembered(value: &str) -> Option<Self> {
        Self::parse(value).filter(|view| *view != Self::Last)
    }
}

/// An unknown or damaged value in `settings.json` means the feed, not
/// unreadable settings.
impl<'de> Deserialize<'de> for StartView {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = serde_json::Value::deserialize(deserializer)?;
        Ok(value.as_str().and_then(Self::parse).unwrap_or_default())
    }
}

/// `Settings::last_view`: as [`StartView`], but `last` is not a screen.
fn remembered_view<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<StartView, D::Error> {
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(value
        .as_str()
        .and_then(StartView::parse_remembered)
        .unwrap_or_default())
}

/// The language of the feed's videos and streams (D-067): the interface's, or
/// one of the feed's own languages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum VideoLanguage {
    #[default]
    Ui,
    En,
    Ru,
    Ar,
}

/// Where a video or stream plays: the launcher's player window or the
/// system browser.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PlayerMode {
    #[default]
    Window,
    Browser,
}

/// A part of the feed the player can hide.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FeedBlock {
    Versions,
    News,
    Videos,
    Streams,
}

impl FeedBlock {
    pub const ALL: [Self; 4] = [Self::Versions, Self::News, Self::Videos, Self::Streams];
}

/// The feed's settings (Settings → Feed, D-067). A damaged stored value
/// reads as its default, never as unreadable settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedSettings {
    /// False hides the Feed tab, and the launcher asks the feed for nothing.
    pub enabled: bool,
    /// Snapshots, pre-releases and release candidates in the feed.
    pub snapshots: bool,
    pub video_language: VideoLanguage,
    /// YouTube Shorts among the videos.
    pub shorts: bool,
    /// Patch notes and news machine-translated into the interface language.
    pub machine_translation: bool,
    pub player: PlayerMode,
    /// Parts of the feed not shown; sorted, each once.
    pub hidden: Vec<FeedBlock>,
}

impl Default for FeedSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            snapshots: true,
            video_language: VideoLanguage::Ui,
            shorts: false,
            machine_translation: true,
            player: PlayerMode::Window,
            hidden: Vec::new(),
        }
    }
}

impl FeedSettings {
    /// Each block once, in a fixed order.
    pub fn hidden_set(blocks: impl IntoIterator<Item = FeedBlock>) -> Vec<FeedBlock> {
        let mut hidden: Vec<FeedBlock> = blocks.into_iter().collect();
        hidden.sort();
        hidden.dedup();
        hidden
    }
}

impl<'de> Deserialize<'de> for FeedSettings {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = serde_json::Value::deserialize(deserializer)?;
        let default = Self::default();
        let Some(fields) = value.as_object() else {
            return Ok(default);
        };
        let flag = |name: &str, default: bool| {
            fields
                .get(name)
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(default)
        };
        fn named<T: serde::de::DeserializeOwned>(value: Option<&serde_json::Value>) -> Option<T> {
            value
                .filter(|v| v.is_string())
                .and_then(|v| T::deserialize(v).ok())
        }
        let hidden = fields
            .get("hidden")
            .and_then(serde_json::Value::as_array)
            .map(|items| {
                Self::hidden_set(
                    items
                        .iter()
                        .take(FeedBlock::ALL.len() * 4)
                        .filter_map(|item| named::<FeedBlock>(Some(item))),
                )
            })
            .unwrap_or_default();
        Ok(Self {
            enabled: flag("enabled", default.enabled),
            snapshots: flag("snapshots", default.snapshots),
            video_language: named(fields.get("videoLanguage")).unwrap_or(default.video_language),
            shorts: flag("shorts", default.shorts),
            machine_translation: flag("machineTranslation", default.machine_translation),
            player: named(fields.get("player")).unwrap_or(default.player),
            hidden,
        })
    }
}

/// What happens to the launcher window while a game runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum DuringGameMode {
    #[default]
    UnloadAndReturn,
    KeepOpen,
    /// Quit the launcher once the game window is up; the game keeps running.
    CloseLauncher,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Resolution {
    pub width: u32,
    pub height: u32,
}

impl Default for Resolution {
    fn default() -> Self {
        Self {
            width: 1280,
            height: 720,
        }
    }
}

/// `settings.json`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub schema_version: u64,
    #[serde(default)]
    pub theme: Theme,
    #[serde(default)]
    pub during_game_mode: DuringGameMode,
    #[serde(default)]
    pub selected_instance_id: Option<InstanceId>,
    /// Default maximum heap for new instances; `None` uses the recommended value
    /// derived from physical memory.
    #[serde(default)]
    pub default_max_heap_mib: Option<u32>,
    #[serde(default)]
    pub default_resolution: Resolution,
    /// Provider for runtimes of new installations (D-036). Installed versions keep
    /// the runtime recorded in their lock.
    #[serde(default)]
    pub java_provider: JavaProvider,
    #[serde(default)]
    pub language: Language,
    /// Back up the worlds a game session changed, after it ends (D-047).
    #[serde(default)]
    pub backup_after_play: bool,
    /// Automatic backups kept per world; older ones are deleted.
    #[serde(default = "default_backup_keep")]
    pub backup_keep: u32,
    /// The connection is metered: big downloads ask first (D-051).
    #[serde(default)]
    pub metered: bool,
    /// On a metered connection, downloads of at least this many MiB ask first.
    #[serde(default = "default_metered_warn_mib")]
    pub metered_warn_mib: u32,
    /// Monthly traffic limit in MiB on a metered connection; 0 is no limit.
    #[serde(default)]
    pub monthly_limit_mib: u32,
    /// New installations start the game in fullscreen (D-056).
    #[serde(default)]
    pub default_fullscreen: bool,
    /// Random ID sent with crash reports (D-061); made with the first report,
    /// replaced on request. Never the account or player.
    #[serde(default)]
    pub install_id: Option<String>,
    /// Anonymous statistics and explanation feedback (D-061 stage 3); off by
    /// default.
    #[serde(default)]
    pub share_stats: bool,
    /// Where downloads come from: origins, the mirror as fallback, or the
    /// mirror first (D-062).
    #[serde(default)]
    pub download_source: crate::mirror::DownloadSource,
    /// Download updates and install them on close, only tell, or never check
    /// (D-063).
    #[serde(default)]
    pub update_mode: crate::update::UpdateMode,
    /// Random, never sent: decides whether this install is within a release's
    /// rollout percent.
    #[serde(default)]
    pub update_seed: Option<u32>,
    /// The version that last ran, to notice the first start of a new one.
    #[serde(default)]
    pub last_version: Option<String>,
    /// Stable versions, or beta ones first (D-063).
    #[serde(default)]
    pub update_channel: crate::update::UpdateChannel,
    /// A version that did not start here; it is not offered again.
    #[serde(default)]
    pub skipped_version: Option<String>,
    /// A short sound when a chat message or friend request comes while the
    /// launcher is in front.
    #[serde(default = "on")]
    pub chat_sound: bool,
    /// A system notification for one while the launcher is not in front.
    #[serde(default = "on")]
    pub chat_notifications: bool,
    /// The launcher account plays offline in the full game instead of the
    /// demo; only honoured while a Microsoft account with play access is
    /// signed in here.
    #[serde(default)]
    pub launcher_account_offline: bool,
    /// The same for the profiles without an account.
    #[serde(default)]
    pub notoken_offline: bool,
    /// The screen opened at start.
    #[serde(default)]
    pub start_view: StartView,
    /// The last screen opened among the start views, for `StartView::Last`.
    /// Never `Last` itself.
    #[serde(default, deserialize_with = "remembered_view")]
    pub last_view: StartView,
    /// The start page feed (D-067).
    #[serde(default)]
    pub feed: FeedSettings,
}

fn on() -> bool {
    true
}

fn default_metered_warn_mib() -> u32 {
    crate::traffic::DEFAULT_WARN_MIB
}

fn default_backup_keep() -> u32 {
    crate::backup::DEFAULT_KEEP
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            theme: Theme::default(),
            during_game_mode: DuringGameMode::default(),
            selected_instance_id: None,
            default_max_heap_mib: None,
            default_resolution: Resolution::default(),
            java_provider: JavaProvider::default(),
            language: Language::default(),
            backup_after_play: false,
            backup_keep: default_backup_keep(),
            metered: false,
            metered_warn_mib: default_metered_warn_mib(),
            monthly_limit_mib: 0,
            default_fullscreen: false,
            install_id: None,
            share_stats: false,
            download_source: crate::mirror::DownloadSource::Automatic,
            update_mode: crate::update::UpdateMode::Auto,
            update_seed: None,
            last_version: None,
            update_channel: crate::update::UpdateChannel::Stable,
            skipped_version: None,
            chat_sound: true,
            chat_notifications: true,
            launcher_account_offline: false,
            notoken_offline: false,
            start_view: StartView::Feed,
            last_view: StartView::Feed,
            feed: FeedSettings::default(),
        }
    }
}

/// The signed-in account's non-secret identity. Tokens live only in the OS
/// credential vault under `vault_key`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountRecord {
    pub profile_id: String,
    pub profile_name: String,
    pub vault_key: String,
}

/// A local player name for Minecraft's demo, playable without an account (D-045).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NoTokenRecord {
    pub id: String,
    pub name: String,
}

/// The launcher account signed in on this computer (D-064): who it is, never
/// a token (the refresh token is in the OS credential vault).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LauncherAccountRecord {
    pub id: String,
    pub nick: String,
    #[serde(default)]
    pub email_verified: bool,
}

/// Who plays: a Microsoft account (by Minecraft profile ID), a no-token
/// profile, or the launcher account (D-064).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProfileRef {
    Microsoft {
        id: String,
    },
    #[serde(rename = "notoken")]
    NoToken {
        id: String,
    },
    Launcher {
        id: String,
    },
}

/// `accounts.json`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountsFile {
    pub schema_version: u64,
    /// The single account of launchers before D-045; moved into `accounts` on load.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active: Option<AccountRecord>,
    /// Before D-045: `active` could not be signed out yet.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub sign_out_pending: bool,
    /// Saved Microsoft accounts, in the order they were added.
    #[serde(default)]
    pub accounts: Vec<AccountRecord>,
    #[serde(default)]
    pub notoken_profiles: Vec<NoTokenRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub launcher_account: Option<LauncherAccountRecord>,
    /// The profile installations play as unless they name another one.
    #[serde(default)]
    pub default_profile: Option<ProfileRef>,
    /// Accounts whose vault credential could not be deleted at sign-out (doc 03):
    /// retried at start and never restored.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pending_sign_outs: Vec<AccountRecord>,
}

impl Default for AccountsFile {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            active: None,
            sign_out_pending: false,
            accounts: Vec::new(),
            notoken_profiles: Vec::new(),
            launcher_account: None,
            default_profile: None,
            pending_sign_outs: Vec::new(),
        }
    }
}

impl AccountsFile {
    /// Moves the single account of the earlier format into the account list.
    pub fn migrated(mut self) -> Self {
        if let Some(active) = self.active.take() {
            if self.sign_out_pending {
                self.pending_sign_outs.push(active);
            } else if !self
                .accounts
                .iter()
                .any(|a| a.profile_id == active.profile_id)
            {
                self.default_profile.get_or_insert(ProfileRef::Microsoft {
                    id: active.profile_id.clone(),
                });
                self.accounts.push(active);
            }
        }
        self.sign_out_pending = false;
        self
    }

    /// Whether `profile` names a saved account or profile.
    pub fn contains(&self, profile: &ProfileRef) -> bool {
        match profile {
            ProfileRef::Microsoft { id } => self.accounts.iter().any(|a| a.profile_id == *id),
            ProfileRef::NoToken { id } => self.notoken_profiles.iter().any(|p| p.id == *id),
            ProfileRef::Launcher { id } => {
                self.launcher_account.as_ref().is_some_and(|a| a.id == *id)
            }
        }
    }

    /// Forgets a removed account or profile, and the default if it named it.
    pub fn remove(&mut self, profile: &ProfileRef) {
        match profile {
            ProfileRef::Microsoft { id } => self.accounts.retain(|a| a.profile_id != *id),
            ProfileRef::NoToken { id } => self.notoken_profiles.retain(|p| p.id != *id),
            ProfileRef::Launcher { id } => {
                if self.launcher_account.as_ref().is_some_and(|a| a.id == *id) {
                    self.launcher_account = None;
                }
            }
        }
        if self.default_profile.as_ref() == Some(profile) {
            self.default_profile = None;
        }
    }
}

/// Persistent installation health, separate from operation progress (doc 06).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum InstallationHealth {
    #[default]
    NotInstalled,
    Ready,
    NeedsVerification,
    RepairRequired,
    Unsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationKind {
    Install,
    Repair,
    Launch,
    /// Copying the launcher data to another folder (D-043).
    Relocate,
    /// Making or deleting a backup (D-047).
    Backup,
    /// Restoring a backup (D-047).
    Restore,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Succeeded,
    Failed,
    Cancelled,
    Interrupted,
}

/// The last terminal result recorded for an instance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LastResult {
    pub kind: OperationKind,
    pub outcome: Outcome,
    pub finished_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_code: Option<ErrorCode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
}

/// `instances/<uuid>/instance.json`: the user-facing instance definition and a
/// derived health summary. `install.lock.json` is authoritative for installation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstanceFile {
    pub schema_version: u64,
    pub id: InstanceId,
    pub display_name: String,
    /// Immutable for the instance's lifetime.
    pub version_id: String,
    /// Null for a draft until initial resolution pins a verified descriptor.
    pub manifest_sha1: Option<Sha1>,
    pub created_at: DateTime<Utc>,
    #[serde(default)]
    pub last_played_at: Option<DateTime<Utc>>,
    pub max_heap_mib: u32,
    pub resolution: Resolution,
    #[serde(default)]
    pub health: InstallationHealth,
    #[serde(default)]
    pub last_result: Option<LastResult>,
    /// The private Java runtime of the last verified install or repair; display
    /// only (the lock is authoritative).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub java: Option<InstalledJava>,
    /// The mod loader and loader version; immutable like the game version
    /// (D-039). None for vanilla.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loader: Option<LoaderChoice>,
    /// The user's own notes about the installation.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub notes: String,
    /// A provider for the private runtime that the next install or repair
    /// switches to (D-044); cleared once the lock records it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub java_provider: Option<JavaProvider>,
    /// The user's own Java, used instead of the private runtime (D-044).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_java: Option<CustomJava>,
    /// Extra JVM options as the user typed them, added after the version's own.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub jvm_arguments: String,
    /// Who plays this installation; `None` uses the default profile (D-045).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<ProfileRef>,
    /// Extra Minecraft (game) arguments as the user typed them (D-056).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub game_arguments: String,
    /// Start the game in fullscreen (D-056).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub fullscreen: bool,
    /// Add Launcher Skins to the game where a jar exists for it (D-065). On
    /// by default: a file without the field, from before it, reads as on, and
    /// only "off" is written.
    #[serde(default = "launcher_skins_default", skip_serializing_if = "is_on")]
    pub launcher_skins: bool,
    /// A room instance: made by the world flow for one room of worlds with
    /// friends (D-068), launched only into that room, never with the
    /// player's typed arguments.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub world_room: Option<String>,
}

fn launcher_skins_default() -> bool {
    true
}

fn is_on(value: &bool) -> bool {
    *value
}

impl InstanceFile {
    /// "Minecraft 1.21.1", or "Minecraft 1.21.1 · Fabric 0.16.14" with a loader;
    /// for logs and records, not translated.
    pub fn version_label(&self) -> String {
        match &self.loader {
            Some(loader) => format!(
                "Minecraft {} · {} {}",
                self.version_id,
                loader.kind.name(),
                loader.version
            ),
            None => format!("Minecraft {}", self.version_id),
        }
    }
}

/// A Java installation the user chose for an installation (D-044).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomJava {
    pub path: std::path::PathBuf,
    /// What the Java reported when it was chosen.
    pub java_version: String,
    pub major: u16,
    /// Compatibility warnings the user accepted.
    #[serde(default)]
    pub accepted: Vec<JavaIssue>,
}

/// Why a Java may not suit a game version (D-044).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JavaIssue {
    /// Older than the version the game was built for: it will not start.
    Older,
    /// Newer: recent versions usually run, older versions and mod loaders often do not.
    Newer,
    /// Built for another processor architecture.
    Architecture,
}

/// Which private Java runtime an installation uses, for display.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledJava {
    pub provider: JavaProvider,
    pub major: u16,
    /// Provider release label, e.g. `21.0.7` or `jdk-21.0.12.1+1`.
    pub release: String,
}

impl InstalledJava {
    pub fn from_lock(runtime: &LockedRuntime) -> Option<Self> {
        Some(Self {
            provider: JavaProvider::parse(&runtime.provider)?,
            major: runtime.major,
            release: runtime.release_name.clone(),
        })
    }
}

/// Locked descriptor identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LockedDescriptor {
    pub url: String,
    pub sha1: Sha1,
}

/// Locked private Java runtime identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LockedRuntime {
    pub provider: String,
    pub release_name: String,
    pub major: u16,
    pub image_type: ImageType,
    pub archive_url: String,
    pub archive_sha256: Sha256,
    pub archive_size: u64,
    /// SHA-256 of the canonical extracted-tree receipt document.
    pub tree_receipt_sha256: Sha256,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactRole {
    Client,
    Library,
    AssetIndex,
    AssetObject,
    LoggingConfig,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HashAlgorithm {
    Sha1,
    Sha256,
}

/// One verified managed artifact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LockedArtifact {
    pub role: ArtifactRole,
    /// Path relative to DATA_ROOT, using `/` separators.
    pub path: String,
    pub size: u64,
    pub algorithm: HashAlgorithm,
    pub digest: String,
    pub url: String,
    /// Original Maven coordinate or logical name, kept as provenance only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provenance: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LockedAssetIndex {
    pub id: String,
    pub sha1: Sha1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LockedLogging {
    pub id: String,
    pub sha1: Sha1,
    pub argument: String,
}

/// `instances/<uuid>/install.lock.json`: the authoritative completed installation
/// generation. Contains no tokens and no launch command.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallLock {
    pub schema_version: u64,
    pub instance_id: InstanceId,
    pub version_id: String,
    pub descriptor: LockedDescriptor,
    pub java_major: u16,
    pub os: TargetOs,
    pub arch: TargetArch,
    pub runtime: LockedRuntime,
    /// Ordered classpath entries, relative to DATA_ROOT.
    pub classpath: Vec<String>,
    pub asset_index: LockedAssetIndex,
    #[serde(default)]
    pub logging: Option<LockedLogging>,
    pub artifacts: Vec<LockedArtifact>,
    /// The mod loader of a loader installation (D-039).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loader: Option<LockedLoader>,
    pub validated_at: DateTime<Utc>,
    pub complete: bool,
}

/// The resolved mod loader of an installation (D-039).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LockedLoader {
    pub kind: LoaderKind,
    pub version: String,
    /// SHA-1 of the stored loader manifest the plan derives from.
    pub manifest_sha1: Sha1,
    /// Files the loader's installer produced (unpacked or built by its
    /// processors), verified after it ran. Their `url` is empty.
    #[serde(default)]
    pub generated: Vec<LockedArtifact>,
}

impl LockedLoader {
    pub fn choice(&self) -> LoaderChoice {
        LoaderChoice {
            kind: self.kind,
            version: self.version.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_defaults_fill_missing_fields() {
        let settings: Settings = serde_json::from_str(r#"{"schemaVersion":1}"#).unwrap();
        assert_eq!(settings, Settings::default());
        assert_eq!(settings.during_game_mode, DuringGameMode::UnloadAndReturn);
        let closing: Settings =
            serde_json::from_str(r#"{"schemaVersion":1,"duringGameMode":"close_launcher"}"#)
                .unwrap();
        assert_eq!(closing.during_game_mode, DuringGameMode::CloseLauncher);
        assert_eq!(settings.language.as_str(), "en", "English unless chosen");
        let arabic: Settings =
            serde_json::from_str(r#"{"schemaVersion":1,"language":"ar"}"#).unwrap();
        assert_eq!(arabic.language.as_str(), "ar");
        let broken: Settings =
            serde_json::from_str(r#"{"schemaVersion":1,"language":"../x"}"#).unwrap();
        assert_eq!(broken.language.as_str(), "en", "a bad tag means English");
    }

    #[test]
    fn start_view_is_a_closed_set_and_junk_reads_as_feed() {
        let default: Settings = serde_json::from_str(r#"{"schemaVersion":1}"#).unwrap();
        assert_eq!(default.start_view, StartView::Feed);
        assert_eq!(default.last_view, StartView::Feed);
        for view in StartView::ALL {
            let json = format!(r#"{{"schemaVersion":1,"startView":"{}"}}"#, view.as_str());
            let read: Settings = serde_json::from_str(&json).unwrap();
            assert_eq!(read.start_view, view);
        }
        for junk in [
            r#""Home""#,
            "\"\"",
            r#""../feed""#,
            r#""feed ""#,
            r#""settings""#,
            r#""about""#,
            "42",
            "null",
            "true",
            r#"["home"]"#,
            r#"{"view":"home"}"#,
        ] {
            let json = format!(r#"{{"schemaVersion":1,"startView":{junk},"lastView":{junk}}}"#);
            let read: Settings = serde_json::from_str(&json).unwrap();
            assert_eq!(read.start_view, StartView::Feed, "{junk}");
            assert_eq!(read.last_view, StartView::Feed, "{junk}");
        }
        // `last` is a start view, not a remembered screen.
        let last: Settings =
            serde_json::from_str(r#"{"schemaVersion":1,"startView":"last","lastView":"last"}"#)
                .unwrap();
        assert_eq!(last.start_view, StartView::Last);
        assert_eq!(last.last_view, StartView::Feed);
    }

    #[test]
    fn feed_settings_default_and_junk_reads_as_default() {
        let default: Settings = serde_json::from_str(r#"{"schemaVersion":1}"#).unwrap();
        assert_eq!(default.feed, FeedSettings::default());
        assert!(default.feed.enabled && default.feed.snapshots);
        assert!(default.feed.machine_translation && !default.feed.shorts);
        assert_eq!(default.feed.player, PlayerMode::Window);
        assert_eq!(default.feed.video_language, VideoLanguage::Ui);
        assert!(default.feed.hidden.is_empty());
        for junk in [
            "null",
            "7",
            r#""feed""#,
            "[]",
            r#"{"enabled":"no","snapshots":1,"shorts":null,"player":"popup","videoLanguage":"de","hidden":"videos","machineTranslation":[]}"#,
            r#"{"player":{"x":1},"videoLanguage":["ru"],"hidden":[1,null,{"a":1},"<script>"]}"#,
        ] {
            let json = format!(r#"{{"schemaVersion":1,"theme":"dark","feed":{junk}}}"#);
            let read: Settings = serde_json::from_str(&json).unwrap();
            assert_eq!(read.feed, FeedSettings::default(), "{junk}");
            assert_eq!(read.theme, Theme::Dark, "{junk}");
        }
        let read: Settings = serde_json::from_str(
            r#"{"schemaVersion":1,"feed":{"enabled":false,"snapshots":false,"videoLanguage":"ar",
            "shorts":true,"machineTranslation":false,"player":"browser",
            "hidden":["videos","news","videos","bogus","streams"],"future":1}}"#,
        )
        .unwrap();
        assert_eq!(
            read.feed,
            FeedSettings {
                enabled: false,
                snapshots: false,
                video_language: VideoLanguage::Ar,
                shorts: true,
                machine_translation: false,
                player: PlayerMode::Browser,
                hidden: vec![FeedBlock::News, FeedBlock::Videos, FeedBlock::Streams],
            }
        );
        let json = serde_json::to_value(&read).unwrap();
        assert_eq!(json["feed"]["videoLanguage"], "ar");
        assert_eq!(
            json["feed"]["hidden"],
            serde_json::json!(["news", "videos", "streams"])
        );
        let back: Settings = serde_json::from_value(json).unwrap();
        assert_eq!(back.feed, read.feed);
    }

    #[test]
    fn start_view_round_trips() {
        let settings = Settings {
            start_view: StartView::Last,
            last_view: StartView::Account,
            ..Settings::default()
        };
        let json = serde_json::to_value(&settings).unwrap();
        assert_eq!(json["startView"], "last");
        assert_eq!(json["lastView"], "account");
        let back: Settings = serde_json::from_value(json).unwrap();
        assert_eq!(back, settings);
        for view in StartView::ALL {
            assert_eq!(StartView::parse(view.as_str()), Some(view));
            assert_eq!(serde_json::to_value(view).unwrap(), view.as_str());
        }
        assert_eq!(StartView::parse_remembered("last"), None);
        assert_eq!(StartView::parse_remembered("skins"), Some(StartView::Skins));
    }

    #[test]
    fn language_tags_are_checked_for_form() {
        for good in [
            "en",
            "ru",
            "ar",
            "fil",
            "pt-BR",
            "zh-Hant",
            "zh-Hant-TW",
            "es-419",
        ] {
            assert!(Language::parse(good).is_some(), "{good}");
        }
        for bad in [
            "",
            "e",
            "EN",
            "en_US",
            "en-us",
            "pt-BR-x",
            "zh-hant",
            "en-BR-US",
            "../../etc",
            "en-",
            "-en",
            "abcd",
        ] {
            assert!(Language::parse(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn draft_instance_serializes_null_manifest_hash() {
        let instance = InstanceFile {
            schema_version: SCHEMA_VERSION,
            id: InstanceId::generate(),
            display_name: "Survival".into(),
            version_id: "1.21.1".into(),
            manifest_sha1: None,
            created_at: Utc::now(),
            last_played_at: None,
            max_heap_mib: 4096,
            resolution: Resolution::default(),
            health: InstallationHealth::NotInstalled,
            last_result: None,
            java: None,
            loader: None,
            notes: String::new(),
            java_provider: None,
            custom_java: None,
            jvm_arguments: String::new(),
            profile: None,
            game_arguments: String::new(),
            fullscreen: false,
            launcher_skins: true,
            world_room: None,
        };
        let json = serde_json::to_value(&instance).unwrap();
        assert!(json["manifestSha1"].is_null());
        assert_eq!(json["health"], "not_installed");
        // On is the default and is not written; a file from before the
        // switch existed reads as on.
        assert!(json.get("launcherSkins").is_none());
        let back: InstanceFile = serde_json::from_value(json.clone()).unwrap();
        assert_eq!(back, instance);
        let off = InstanceFile {
            launcher_skins: false,
            ..instance
        };
        let json = serde_json::to_value(&off).unwrap();
        assert_eq!(json["launcherSkins"], false);
        let back: InstanceFile = serde_json::from_value(json).unwrap();
        assert!(!back.launcher_skins);
    }

    #[test]
    fn account_document_has_no_secret_fields() {
        let accounts = AccountsFile {
            accounts: vec![AccountRecord {
                profile_id: "id".into(),
                profile_name: "Player".into(),
                vault_key: "key".into(),
            }],
            notoken_profiles: vec![NoTokenRecord {
                id: "p".into(),
                name: "Alex".into(),
            }],
            default_profile: Some(ProfileRef::NoToken { id: "p".into() }),
            ..AccountsFile::default()
        };
        // "notoken" names the profiles that have no token (D-040).
        let text = serde_json::to_string(&accounts)
            .unwrap()
            .to_lowercase()
            .replace("notoken", "");
        for forbidden in ["token", "secret", "password", "verifier", "code\""] {
            assert!(!text.contains(forbidden), "{forbidden}");
        }
    }

    #[test]
    fn the_single_account_of_the_earlier_format_is_migrated() {
        let record = AccountRecord {
            profile_id: "id".into(),
            profile_name: "Player".into(),
            vault_key: "key".into(),
        };
        let old: AccountsFile = serde_json::from_value(serde_json::json!({
            "schemaVersion": 1,
            "active": {"profileId": "id", "profileName": "Player", "vaultKey": "key"},
            "signOutPending": false
        }))
        .unwrap();
        let new = old.migrated();
        assert_eq!(new.accounts, std::slice::from_ref(&record));
        assert_eq!(
            new.default_profile,
            Some(ProfileRef::Microsoft { id: "id".into() })
        );
        let text = serde_json::to_value(&new).unwrap();
        assert!(text.get("active").is_none() && text.get("signOutPending").is_none());

        let pending = AccountsFile {
            active: Some(record.clone()),
            sign_out_pending: true,
            ..AccountsFile::default()
        }
        .migrated();
        assert!(pending.accounts.is_empty());
        assert_eq!(pending.pending_sign_outs, [record]);

        let mut file = new;
        file.remove(&ProfileRef::Microsoft { id: "id".into() });
        assert!(file.accounts.is_empty() && file.default_profile.is_none());
    }
}
