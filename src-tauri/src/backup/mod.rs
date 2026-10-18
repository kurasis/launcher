//! Incremental backups of worlds and installations (D-047).
//!
//! The store (see [`store`]) keeps each distinct file once; this module adds the
//! rules around it: one backup or restore at a time under the activity permit
//! (never while the installation's game runs), a backup of the current state
//! before a restore replaces it, and automatic backups after a game session.

pub mod store;

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::{ErrorCode, LauncherError, LauncherResult};
use crate::state::AppState;
use crate::storage::models::{InstanceFile, OperationKind};
use crate::storage::paths::{DataRoot, InstanceId};
pub use store::{Origin, Reason, Scope, SnapshotInfo, Store};

/// Automatic backups kept per world unless the user changes it.
pub const DEFAULT_KEEP: u32 = 10;
/// Most automatic backups kept per world.
pub const MAX_KEEP: u32 = 100;

pub fn validate_keep(keep: u32) -> LauncherResult<u32> {
    if (1..=MAX_KEEP).contains(&keep) {
        Ok(keep)
    } else {
        Err(LauncherError::templated(
            ErrorCode::InvalidInput,
            "Keep between 1 and {max} automatic backups.",
            crate::error::params([("max", u64::from(MAX_KEEP).into())]),
        ))
    }
}

/// The worlds of one installation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstanceWorlds {
    pub instance_id: InstanceId,
    pub worlds: Vec<String>,
}

/// Every backup, the worlds that can be backed up, and the space used.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupOverview {
    pub backups: Vec<SnapshotInfo>,
    pub worlds: Vec<InstanceWorlds>,
    /// Bytes all stored files take; shared files count once.
    pub store_bytes: u64,
}

/// Where a restore wrote to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreResult {
    /// The world folder name (a new one for a copy); `None` for an installation.
    pub world: Option<String>,
}

/// World folders (with a `level.dat`) of a game folder, by name.
pub fn worlds(game_dir: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(game_dir.join("saves")) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .filter_map(Result::ok)
        .filter(|e| e.path().join("level.dat").is_file())
        .filter_map(|e| e.file_name().to_str().map(str::to_owned))
        .filter(|n| store::valid_world_name(n))
        .collect();
    names.sort_by_key(|n| n.to_lowercase());
    names
}

fn version_label(instance: &InstanceFile) -> String {
    match &instance.loader {
        Some(loader) => format!(
            "Minecraft {} · {} {}",
            instance.version_id,
            loader.kind.name(),
            loader.version
        ),
        None => format!("Minecraft {}", instance.version_id),
    }
}

fn source(root: &DataRoot, id: InstanceId, scope: &Scope) -> LauncherResult<PathBuf> {
    let game = root.game_dir(id);
    match scope {
        Scope::Installation => Ok(game),
        Scope::World { name } if store::valid_world_name(name) => Ok(game.join("saves").join(name)),
        Scope::World { .. } => Err(LauncherError::invalid_input("This world name is invalid.")),
    }
}

fn data_root(state: &AppState) -> LauncherResult<DataRoot> {
    state.data_root().ok_or_else(|| {
        LauncherError::new(
            ErrorCode::StorageUnavailable,
            "Launcher data is unavailable.",
        )
    })
}

fn instance(state: &AppState, id: InstanceId) -> LauncherResult<InstanceFile> {
    state.instance(id).ok_or_else(|| {
        LauncherError::new(
            ErrorCode::InstanceNotFound,
            "The installation no longer exists.",
        )
    })
}

fn describe(scope: &Scope) -> String {
    match scope {
        Scope::World { name } => format!("world \"{name}\""),
        Scope::Installation => "the whole installation".to_owned(),
    }
}

fn mib(bytes: u64) -> String {
    format!("{:.1} MiB", bytes as f64 / (1024.0 * 1024.0))
}

/// Every backup and every installation's worlds.
pub fn overview(state: &AppState) -> LauncherResult<BackupOverview> {
    let root = data_root(state)?;
    let store = Store::new(root.path());
    Ok(BackupOverview {
        backups: store.list().into_iter().map(|s| s.info).collect(),
        worlds: state
            .instance_ids()
            .into_iter()
            .map(|id| InstanceWorlds {
                instance_id: id,
                worlds: worlds(&root.game_dir(id)),
            })
            .collect(),
        store_bytes: store.size(),
    })
}

/// Backs up while the caller holds the permit.
fn create_locked(
    state: &AppState,
    root: &DataRoot,
    instance: &InstanceFile,
    scope: Scope,
    reason: Reason,
) -> LauncherResult<SnapshotInfo> {
    let store = Store::new(root.path());
    let folder = source(root, instance.id, &scope)?;
    let console = state.console();
    console.info(format!(
        "Backing up {} of \"{}\"",
        describe(&scope),
        instance.display_name
    ));
    let previous = store.latest(instance.id, &scope);
    let (snapshot, skipped) = store.create(
        &folder,
        Origin {
            instance_id: instance.id,
            instance_name: instance.display_name.clone(),
            version_label: version_label(instance),
            scope,
            reason,
        },
        previous.as_ref(),
    )?;
    let info = snapshot.info;
    console.info(format!(
        "Backup done: {} files, {}, {} new",
        info.file_count,
        mib(info.total_bytes),
        mib(info.new_bytes)
    ));
    if skipped > 0 {
        console.warn(format!(
            "{skipped} symbolic links or unreadable names were not backed up"
        ));
    }
    Ok(info)
}

/// Runs `work` under the activity permit, refusing while anything else runs
/// (in particular this installation's game).
fn with_permit<T>(
    state: &AppState,
    kind: OperationKind,
    instance: Option<InstanceId>,
    work: impl FnOnce() -> LauncherResult<T>,
) -> LauncherResult<T> {
    let permit = state
        .activity()
        .try_acquire(kind, instance, Uuid::new_v4().to_string())?;
    state.announce_runs();
    let result = work();
    drop(permit);
    state.announce_runs();
    if let Err(error) = &result {
        state.console().warn(format!("Backup: {}", error.message));
    }
    result
}

/// Backs up a world or the whole installation now.
pub fn create(
    state: &AppState,
    instance_id: InstanceId,
    scope: Scope,
    reason: Reason,
) -> LauncherResult<SnapshotInfo> {
    let root = data_root(state)?;
    let instance = instance(state, instance_id)?;
    with_permit(state, OperationKind::Backup, Some(instance_id), || {
        create_locked(state, &root, &instance, scope, reason)
    })
}

/// A world folder name for a restored copy that does not exist yet.
fn copy_name(saves: &Path, name: &str, at: chrono::DateTime<chrono::Utc>) -> String {
    let base = format!("{name} (backup {})", at.format("%Y-%m-%d %H-%M"));
    let mut candidate = base.clone();
    let mut n = 2;
    while saves.join(&candidate).exists() {
        candidate = format!("{base} {n}");
        n += 1;
    }
    candidate
}

/// Restores a backup in place of the current world or installation (after
/// backing up the current state), or a world beside it as a copy.
pub fn restore(state: &AppState, backup_id: &str, as_copy: bool) -> LauncherResult<RestoreResult> {
    let root = data_root(state)?;
    let store = Store::new(root.path());
    let snapshot = store.load(backup_id)?;
    let instance = instance(state, snapshot.info.instance_id)?;
    with_permit(state, OperationKind::Restore, Some(instance.id), || {
        let console = state.console();
        let scope = snapshot.info.scope.clone();
        match (&scope, as_copy) {
            (Scope::World { name }, true) => {
                let saves = root.game_dir(instance.id).join("saves");
                let copy = copy_name(&saves, name, snapshot.info.created_at);
                store.restore_into(&snapshot, &saves.join(&copy))?;
                console.info(format!("Backup restored as the world \"{copy}\""));
                Ok(RestoreResult { world: Some(copy) })
            }
            (Scope::Installation, true) => Err(LauncherError::invalid_input(
                "A whole installation can only be restored in its place.",
            )),
            (_, false) => {
                let target = source(&root, instance.id, &scope)?;
                if target.is_dir() {
                    create_locked(
                        state,
                        &root,
                        &instance,
                        scope.clone(),
                        Reason::BeforeRestore,
                    )?;
                }
                store.restore_replacing(&snapshot, &target)?;
                console.info(format!(
                    "Restored {} of \"{}\" from the backup of {}",
                    describe(&scope),
                    instance.display_name,
                    snapshot.info.created_at.format("%Y-%m-%d %H:%M")
                ));
                Ok(RestoreResult {
                    world: match scope {
                        Scope::World { name } => Some(name),
                        Scope::Installation => None,
                    },
                })
            }
        }
    })
}

/// Deletes a backup and the stored files no other backup uses.
pub fn delete(state: &AppState, backup_id: &str) -> LauncherResult<u64> {
    let root = data_root(state)?;
    with_permit(state, OperationKind::Backup, None, || {
        let store = Store::new(root.path());
        store.delete(backup_id)?;
        let freed = store.collect();
        state
            .console()
            .info(format!("Backup deleted; {} freed", mib(freed)));
        Ok(freed)
    })
}

/// After a game session: backs up each world it changed, then keeps the newest
/// automatic backups per world (D-047). Does nothing unless switched on.
pub fn after_play(state: &AppState, instance_id: InstanceId) {
    let settings = state.settings();
    if !settings.backup_after_play {
        return;
    }
    let (Ok(root), Ok(instance)) = (data_root(state), instance(state, instance_id)) else {
        return;
    };
    let keep = settings.backup_keep as usize;
    let _ = with_permit(state, OperationKind::Backup, Some(instance_id), || {
        let store = Store::new(root.path());
        for name in worlds(&root.game_dir(instance_id)) {
            let scope = Scope::World { name };
            let folder = source(&root, instance_id, &scope)?;
            if store
                .latest(instance_id, &scope)
                .is_some_and(|last| Store::unchanged(&folder, &last))
            {
                continue;
            }
            create_locked(state, &root, &instance, scope.clone(), Reason::AfterPlay)?;
            store.prune(instance_id, &scope, keep);
        }
        store.collect();
        Ok(())
    });
}

#[cfg(test)]
mod tests;
