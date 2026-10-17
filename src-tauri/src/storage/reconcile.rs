//! Reconciles an instance summary with its authoritative install lock
//! (docs/spec/02 "Cross-version cache and commit authority", doc 10 rule 3, MV08).
//!
//! Commit order is: verified artifacts → `install.lock.json` → `instance.json`.
//! A crash between the last two steps leaves a stale summary that is restored from
//! the lock; a summary claiming `ready` without a valid lock is never trusted.

use std::path::Path;

use super::document::{self, Loaded};
use super::models::{InstallLock, InstallationHealth, InstanceFile};

/// What was found at `install.lock.json`.
#[derive(Debug)]
pub enum LockState {
    Missing,
    Complete(Box<InstallLock>),
    Incomplete,
    /// Present but unreadable, malformed or belonging to another instance/version.
    Invalid,
    UnsupportedSchema,
}

/// Reads the install lock without modifying it. A corrupt lock is preserved under a
/// `.corrupt-*` name by the document loader.
pub fn read_lock(path: &Path) -> std::io::Result<LockState> {
    Ok(
        match document::load_limited::<InstallLock>(path, None, document::MAX_LARGE_DOCUMENT_BYTES)?
        {
            Loaded::Missing => LockState::Missing,
            Loaded::Valid(lock) | Loaded::RecoveredFromBackup { value: lock, .. } => {
                if lock.complete {
                    LockState::Complete(Box::new(lock))
                } else {
                    LockState::Incomplete
                }
            }
            Loaded::Corrupt { .. } => LockState::Invalid,
            Loaded::UnsupportedSchema { .. } => LockState::UnsupportedSchema,
        },
    )
}

/// Outcome of reconciliation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reconciled {
    pub instance: InstanceFile,
    /// True when the summary changed and should be rewritten.
    pub changed: bool,
}

/// Derives the trustworthy summary from the summary on disk and the lock state.
pub fn reconcile(summary: &InstanceFile, lock: &LockState) -> Reconciled {
    let mut instance = summary.clone();
    match lock {
        LockState::Complete(lock)
            if lock.instance_id == summary.id && lock.version_id == summary.version_id =>
        {
            instance.manifest_sha1 = Some(lock.descriptor.sha1.clone());
            instance.health = match summary.health {
                // Interrupted verification or a known corruption stays visible; the
                // lock is historical evidence, not proof of current file health.
                InstallationHealth::NeedsVerification | InstallationHealth::RepairRequired => {
                    summary.health
                }
                // A crash after the lock commit but before the summary update.
                InstallationHealth::NotInstalled
                | InstallationHealth::Ready
                | InstallationHealth::Unsupported => InstallationHealth::Ready,
            };
        }
        LockState::Complete(_) | LockState::Invalid => {
            // Something was installed, but its record cannot be trusted.
            instance.health = InstallationHealth::RepairRequired;
        }
        LockState::UnsupportedSchema => {
            instance.health = InstallationHealth::Unsupported;
        }
        LockState::Missing | LockState::Incomplete => {
            instance.health = InstallationHealth::NotInstalled;
        }
    }
    Reconciled {
        changed: instance != *summary,
        instance,
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::*;
    use crate::storage::document::SCHEMA_VERSION;
    use crate::storage::models::*;
    use crate::storage::paths::*;

    const SHA1: &str = "499680d659b7a0ccb2fa9a809c345c5baf2ace7d";

    fn summary(health: InstallationHealth) -> InstanceFile {
        InstanceFile {
            schema_version: SCHEMA_VERSION,
            id: InstanceId::generate(),
            display_name: "A".into(),
            version_id: "1.21.1".into(),
            manifest_sha1: None,
            created_at: Utc::now(),
            last_played_at: None,
            max_heap_mib: 4096,
            resolution: Resolution::default(),
            health,
            last_result: None,
        }
    }

    pub(crate) fn lock_for(instance: &InstanceFile) -> InstallLock {
        InstallLock {
            schema_version: SCHEMA_VERSION,
            instance_id: instance.id,
            version_id: instance.version_id.clone(),
            descriptor: LockedDescriptor {
                url: "https://piston-meta.mojang.com/v1/packages/x/1.21.1.json".into(),
                sha1: Sha1::parse(SHA1).unwrap(),
            },
            java_major: 21,
            os: TargetOs::Linux,
            arch: TargetArch::X64,
            runtime: LockedRuntime {
                provider: "adoptium".into(),
                release_name: "jdk-21.0.8+9".into(),
                major: 21,
                image_type: ImageType::Jre,
                archive_url: "https://github.com/adoptium/x.tar.gz".into(),
                archive_sha256: Sha256::parse(&"a".repeat(64)).unwrap(),
                archive_size: 1,
                tree_receipt_sha256: Sha256::parse(&"b".repeat(64)).unwrap(),
            },
            classpath: vec![],
            asset_index: LockedAssetIndex {
                id: "17".into(),
                sha1: Sha1::parse(SHA1).unwrap(),
            },
            logging: None,
            artifacts: vec![],
            validated_at: Utc::now(),
            complete: true,
        }
    }

    /// Regression: a real 1.21.1 lock lists ~4,000 artifacts (about 1.6 MB) and
    /// was once rejected as corrupt by the 1 MiB app-document limit.
    #[test]
    fn realistic_large_lock_reads_as_complete() {
        let dir = tempfile::tempdir().unwrap();
        let instance = summary(InstallationHealth::NotInstalled);
        let mut lock = lock_for(&instance);
        lock.artifacts = (0..6000)
            .map(|i| LockedArtifact {
                role: ArtifactRole::AssetObject,
                path: format!("shared/assets/objects/ab/{i:040}"),
                size: 1000,
                algorithm: HashAlgorithm::Sha1,
                digest: format!("{i:040}"),
                url: format!("https://resources.download.minecraft.net/ab/{i:040}"),
                provenance: Some(format!("minecraft/sounds/ambient/cave/cave{i}.ogg")),
            })
            .collect();
        let path = dir.path().join("install.lock.json");
        crate::storage::document::save(&path, &lock, None).unwrap();
        assert!(std::fs::metadata(&path).unwrap().len() > 1024 * 1024);
        assert!(matches!(read_lock(&path).unwrap(), LockState::Complete(_)));
        assert!(path.exists(), "not set aside");
    }

    #[test]
    fn crash_after_lock_commit_is_reconciled_to_ready() {
        let draft = summary(InstallationHealth::NotInstalled);
        let lock = lock_for(&draft);
        let result = reconcile(&draft, &LockState::Complete(Box::new(lock)));
        assert!(result.changed);
        assert_eq!(result.instance.health, InstallationHealth::Ready);
        assert_eq!(result.instance.manifest_sha1.unwrap().as_str(), SHA1);
    }

    #[test]
    fn ready_summary_without_lock_is_not_trusted() {
        let ready = summary(InstallationHealth::Ready);
        for state in [LockState::Missing, LockState::Incomplete] {
            let result = reconcile(&ready, &state);
            assert_eq!(result.instance.health, InstallationHealth::NotInstalled);
            assert!(result.changed);
        }
    }

    #[test]
    fn interrupted_verification_is_kept_with_valid_lock() {
        for health in [
            InstallationHealth::NeedsVerification,
            InstallationHealth::RepairRequired,
        ] {
            let instance = summary(health);
            let lock = lock_for(&instance);
            let result = reconcile(&instance, &LockState::Complete(Box::new(lock)));
            assert_eq!(result.instance.health, health);
        }
    }

    #[test]
    fn foreign_or_invalid_lock_requires_repair() {
        let instance = summary(InstallationHealth::Ready);
        let mut foreign = lock_for(&instance);
        foreign.instance_id = InstanceId::generate();
        let result = reconcile(&instance, &LockState::Complete(Box::new(foreign)));
        assert_eq!(result.instance.health, InstallationHealth::RepairRequired);

        let mut other_version = lock_for(&instance);
        other_version.version_id = "26.3".into();
        let result = reconcile(&instance, &LockState::Complete(Box::new(other_version)));
        assert_eq!(result.instance.health, InstallationHealth::RepairRequired);

        let result = reconcile(&instance, &LockState::Invalid);
        assert_eq!(result.instance.health, InstallationHealth::RepairRequired);
    }

    #[test]
    fn consistent_state_is_unchanged() {
        let draft = summary(InstallationHealth::NotInstalled);
        assert!(!reconcile(&draft, &LockState::Missing).changed);
    }

    #[test]
    fn reads_lock_states_from_disk() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("install.lock.json");
        assert!(matches!(read_lock(&path).unwrap(), LockState::Missing));

        let instance = summary(InstallationHealth::NotInstalled);
        let mut lock = lock_for(&instance);
        document::save(&path, &lock, None).unwrap();
        assert!(matches!(read_lock(&path).unwrap(), LockState::Complete(_)));

        lock.complete = false;
        document::save(&path, &lock, None).unwrap();
        assert!(matches!(read_lock(&path).unwrap(), LockState::Incomplete));

        std::fs::write(&path, b"{\"schemaVersion\":7}").unwrap();
        assert!(matches!(
            read_lock(&path).unwrap(),
            LockState::UnsupportedSchema
        ));

        std::fs::write(&path, b"garbage").unwrap();
        assert!(matches!(read_lock(&path).unwrap(), LockState::Invalid));
    }
}
