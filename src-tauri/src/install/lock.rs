//! The frozen install plan and the install lock built from it (docs/spec/02
//! "InstallLock", doc 10 "Commit and recovery rules").
//!
//! A new installation resolves its [`FinalInstallPlan`] once and stores it in the
//! job receipt before any download, so a retry uses the same descriptor and
//! runtime package. The lock is written only after every artifact and the runtime
//! tree are verified.

use std::fs;

use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha1::Digest as _;

use super::plan::{GamePlan, PlannedArtifact};
use crate::error::{LauncherError, LauncherResult};
use crate::runtime::adoptium::{ArchiveFormat, RuntimePackage};
use crate::storage::atomic;
use crate::storage::document::SCHEMA_VERSION;
use crate::storage::models::{
    HashAlgorithm, InstallLock, LockedArtifact, LockedAssetIndex, LockedDescriptor, LockedLogging,
    LockedRuntime,
};
use crate::storage::paths::{DataRoot, InstanceId, JavaProvider, RuntimeKey, SafeName, Sha1};

/// Game plan plus pinned runtime package for one instance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FinalInstallPlan {
    pub instance_id: InstanceId,
    pub game: GamePlan,
    pub runtime: RuntimePackage,
}

fn locked_artifact(artifact: &PlannedArtifact) -> LockedArtifact {
    LockedArtifact {
        role: artifact.role,
        path: artifact.path.clone(),
        size: artifact.size,
        algorithm: HashAlgorithm::Sha1,
        digest: artifact.sha1.to_string(),
        url: artifact.url.clone(),
        provenance: artifact.provenance.clone(),
    }
}

/// The completed lock for a fully verified plan.
pub fn build_lock(plan: &FinalInstallPlan, runtime: LockedRuntime) -> InstallLock {
    let game = &plan.game;
    InstallLock {
        schema_version: SCHEMA_VERSION,
        instance_id: plan.instance_id,
        version_id: game.version_id.clone(),
        descriptor: LockedDescriptor {
            url: game.descriptor_url.clone(),
            sha1: game.descriptor_sha1.clone(),
        },
        java_major: game.runtime.major,
        os: game.runtime.os,
        arch: game.runtime.arch,
        runtime,
        classpath: game.classpath.clone(),
        asset_index: LockedAssetIndex {
            id: game.asset_index.id.as_str().to_owned(),
            sha1: game.asset_index.artifact.sha1.clone(),
        },
        logging: game.logging.as_ref().map(|l| LockedLogging {
            id: l.id.as_str().to_owned(),
            sha1: l.artifact.sha1.clone(),
            argument: l.argument.clone(),
        }),
        artifacts: game.artifacts().map(locked_artifact).collect(),
        validated_at: Utc::now(),
        complete: true,
    }
}

/// Rebuilds the pinned runtime package from an install lock, for Repair. Never
/// asks the provider for a newer build.
pub fn runtime_from_lock(lock: &InstallLock) -> LauncherResult<RuntimePackage> {
    let runtime = &lock.runtime;
    let name = runtime
        .archive_url
        .rsplit('/')
        .next()
        .unwrap_or_default()
        .to_owned();
    let unusable = || {
        LauncherError::new(
            crate::error::ErrorCode::RepairRequired,
            "The installation record names a runtime package the launcher cannot use.",
        )
    };
    let provider = JavaProvider::parse(&runtime.provider).ok_or_else(unusable)?;
    let format = match provider {
        JavaProvider::Mojang => ArchiveFormat::MojangManifest,
        JavaProvider::Temurin if name.ends_with(".zip") => ArchiveFormat::Zip,
        JavaProvider::Temurin if name.ends_with(".tar.gz") => ArchiveFormat::TarGz,
        JavaProvider::Temurin => return Err(unusable()),
    };
    Ok(RuntimePackage {
        provider: runtime.provider.clone(),
        key: RuntimeKey {
            provider,
            os: lock.os,
            arch: lock.arch,
            major: runtime.major,
            image_type: runtime.image_type,
            archive_sha256: runtime.archive_sha256.clone(),
        },
        release_name: runtime.release_name.clone(),
        openjdk_version: runtime.release_name.clone(),
        archive_name: name,
        archive_url: runtime.archive_url.clone(),
        archive_size: runtime.archive_size,
        archive_format: format,
        jdk_fallback: runtime.image_type == crate::storage::paths::ImageType::Jdk,
    })
}

/// Writes the game-facing asset index (`shared/assets/indexes/<id>.json`) from
/// the verified source. This is derived, mutable state: a different projection
/// left by another instance is normal and simply replaced (doc 10, MV06).
pub fn project_asset_index(root: &DataRoot, id: &SafeName, sha1: &Sha1) -> LauncherResult<()> {
    let storage = |e: &std::io::Error| LauncherError::from_io(e, "Preparing the asset index");
    let source = fs::read(root.asset_index_source(sha1)).map_err(|e| storage(&e))?;
    if hex::encode(sha1::Sha1::digest(&source)) != sha1.as_str() {
        return Err(LauncherError::new(
            crate::error::ErrorCode::RepairRequired,
            "A verified asset index is damaged. Repair the installation.",
        ));
    }
    let target = root.asset_index_projection(id);
    if fs::read(&target).is_ok_and(|current| current == source) {
        return Ok(());
    }
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(|e| storage(&e))?;
    }
    atomic::write(&target, &source).map_err(|e| storage(&e))
}
