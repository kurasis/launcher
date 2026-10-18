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

/// Who plays: a Microsoft account (by Minecraft profile ID) or a no-token profile.
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
        }
    }

    /// Forgets a removed account or profile, and the default if it named it.
    pub fn remove(&mut self, profile: &ProfileRef) {
        match profile {
            ProfileRef::Microsoft { id } => self.accounts.retain(|a| a.profile_id != *id),
            ProfileRef::NoToken { id } => self.notoken_profiles.retain(|p| p.id != *id),
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
        };
        let json = serde_json::to_value(&instance).unwrap();
        assert!(json["manifestSha1"].is_null());
        assert_eq!(json["health"], "not_installed");
        let back: InstanceFile = serde_json::from_value(json).unwrap();
        assert_eq!(back, instance);
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
