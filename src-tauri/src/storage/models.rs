//! App-owned persistent documents (docs/spec/02 "Persistent models").
//!
//! None of these types may hold a token, authorization code or other secret. The
//! account document stores only display metadata and a credential-vault lookup key.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::document::SCHEMA_VERSION;
use super::paths::{ImageType, InstanceId, JavaProvider, Sha1, Sha256, TargetArch, TargetOs};
use crate::error::ErrorCode;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    #[default]
    System,
    Light,
    Dark,
}

/// Interface language (D-038). English is the default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Language {
    #[default]
    En,
    Ru,
}

/// What happens to the launcher window while a game runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum DuringGameMode {
    #[default]
    UnloadAndReturn,
    KeepOpen,
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

/// `accounts.json`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountsFile {
    pub schema_version: u64,
    #[serde(default)]
    pub active: Option<AccountRecord>,
    /// Set when local sign-out could not delete vault credentials (doc 03); while
    /// set, saved credentials must not be restored automatically.
    #[serde(default)]
    pub sign_out_pending: bool,
}

impl Default for AccountsFile {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            active: None,
            sign_out_pending: false,
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
    pub validated_at: DateTime<Utc>,
    pub complete: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_defaults_fill_missing_fields() {
        let settings: Settings = serde_json::from_str(r#"{"schemaVersion":1}"#).unwrap();
        assert_eq!(settings, Settings::default());
        assert_eq!(settings.during_game_mode, DuringGameMode::UnloadAndReturn);
        assert_eq!(settings.language, Language::En, "English unless chosen");
        let russian: Settings =
            serde_json::from_str(r#"{"schemaVersion":1,"language":"ru"}"#).unwrap();
        assert_eq!(russian.language, Language::Ru);
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
            active: Some(AccountRecord {
                profile_id: "id".into(),
                profile_name: "Player".into(),
                vault_key: "key".into(),
            }),
            ..AccountsFile::default()
        };
        let text = serde_json::to_string(&accounts).unwrap().to_lowercase();
        for forbidden in ["token", "secret", "password", "verifier", "code\""] {
            assert!(!text.contains(forbidden), "{forbidden}");
        }
    }
}
