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

/// What deleting every backup of a deleted installation frees.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeletedBytes {
    pub instance_id: InstanceId,
    pub bytes: u64,
}

/// Every backup, the worlds that can be backed up, and the space used.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupOverview {
    pub backups: Vec<SnapshotInfo>,
    pub worlds: Vec<InstanceWorlds>,
    /// Bytes all stored files take; shared files count once.
    pub store_bytes: u64,
    /// Per deleted installation with backups, newest backup first: the bytes only
    /// its backups use, which deleting them all frees (D-055).
    pub deleted_bytes: Vec<DeletedBytes>,
    /// The bytes only deleted installations' backups use, together.
    pub deleted_total_bytes: u64,
}

/// Where a restore wrote to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreResult {
    /// The world folder name (a new one for a copy); `None` for an installation.
    pub world: Option<String>,
}

/// What the user made or added in an installation's game folder (D-055): the
/// files a delete would lose. Game files the launcher downloads are elsewhere.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserData {
    pub worlds: Vec<String>,
    pub mods: u64,
    pub resource_packs: u64,
    pub shader_packs: u64,
    pub screenshots: u64,
    /// Every file of the game folder, settings and logs included.
    pub bytes: u64,
}

impl UserData {
    /// Worlds, mods, packs or screenshots: worth a backup before deleting.
    pub fn any(&self) -> bool {
        !self.worlds.is_empty()
            || self.mods + self.resource_packs + self.shader_packs + self.screenshots > 0
    }
}

/// Entries directly inside `dir` (files or folders; packs can be either).
fn entries(dir: &Path) -> u64 {
    std::fs::read_dir(dir).map_or(0, |e| {
        e.filter_map(Result::ok)
            .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
            .count() as u64
    })
}

/// Bytes of every regular file under `dir`; symbolic links are not followed.
fn size(dir: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    entries
        .filter_map(Result::ok)
        .map(|e| match e.file_type() {
            Ok(t) if t.is_dir() => size(&e.path()),
            Ok(t) if t.is_file() => e.metadata().map_or(0, |m| m.len()),
            _ => 0,
        })
        .sum()
}

pub fn user_data(game_dir: &Path) -> UserData {
    UserData {
        worlds: worlds(game_dir),
        mods: entries(&game_dir.join("mods")),
        resource_packs: entries(&game_dir.join("resourcepacks")),
        shader_packs: entries(&game_dir.join("shaderpacks")),
        screenshots: entries(&game_dir.join("screenshots")),
        bytes: size(game_dir),
    }
}

/// What deleting an installation would lose.
pub fn inspect(state: &AppState, instance_id: InstanceId) -> LauncherResult<UserData> {
    let root = data_root(state)?;
    instance(state, instance_id)?;
    Ok(user_data(&root.game_dir(instance_id)))
}

/// Deletes an installation, after a backup of its whole game folder when
/// asked (D-055). Refused while anything runs; files shared with other
/// installations (libraries, assets, Java) stay.
pub fn delete_instance(
    state: &AppState,
    instance_id: InstanceId,
    backup: bool,
) -> LauncherResult<Option<SnapshotInfo>> {
    let root = data_root(state)?;
    let instance = instance(state, instance_id)?;
    with_permit(state, OperationKind::Backup, Some(instance_id), || {
        let saved = if backup && root.game_dir(instance_id).is_dir() {
            Some(create_locked(
                state,
                &root,
                &instance,
                Scope::Installation,
                Reason::BeforeDelete,
            )?)
        } else {
            None
        };
        state.remove_instance(instance_id)?;
        Ok(saved)
    })
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
    let snapshots = store.list();
    let live = state.instance_ids();
    let gone = |id: InstanceId| !live.contains(&id);
    let (each, deleted_total_bytes) = store::unshared_bytes(&snapshots, gone);
    let mut deleted_bytes: Vec<DeletedBytes> = Vec::new();
    for id in snapshots.iter().map(|s| s.info.instance_id) {
        if gone(id) && !deleted_bytes.iter().any(|d| d.instance_id == id) {
            deleted_bytes.push(DeletedBytes {
                instance_id: id,
                bytes: each.get(&id).copied().unwrap_or(0),
            });
        }
    }
    Ok(BackupOverview {
        worlds: live
            .iter()
            .map(|&id| InstanceWorlds {
                instance_id: id,
                worlds: worlds(&root.game_dir(id)),
            })
            .collect(),
        backups: snapshots.into_iter().map(|s| s.info).collect(),
        store_bytes: store.size(),
        deleted_bytes,
        deleted_total_bytes,
    })
}

/// Backs up while the caller holds the permit.
pub(crate) fn create_locked(
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
            version_label: instance.version_label(),
            version_id: instance.version_id.clone(),
            loader: instance.loader.clone(),
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
    let info = &snapshot.info;
    let instance = match (
        state.instance(info.instance_id),
        &info.scope,
        &info.version_id,
    ) {
        (Some(found), _, _) => found,
        // The installation was deleted: a backup of all of it brings it back.
        (None, Scope::Installation, Some(version)) => {
            let recreated = state.recreate_instance(
                info.instance_id,
                &info.instance_name,
                version,
                info.loader.clone(),
            )?;
            state.console().info(format!(
                "Brought back the deleted installation \"{}\" ({})",
                recreated.display_name,
                recreated.version_label()
            ));
            recreated
        }
        _ => {
            return Err(LauncherError::new(
                ErrorCode::InstanceNotFound,
                "The installation of this backup was deleted. Only a backup of a whole \
                 installation can bring it back.",
            ));
        }
    };
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

/// Deletes a backup and the stored files no other backup uses. It is work of the
/// backup's installation (which may be deleted): it runs beside a world server,
/// which never reads this store (D-068), but not beside a game, another backup or a
/// reset.
pub fn delete(state: &AppState, backup_id: &str) -> LauncherResult<u64> {
    let root = data_root(state)?;
    let store = Store::new(root.path());
    let owner = store.load(backup_id)?.info.instance_id;
    with_permit(state, OperationKind::Backup, Some(owner), || {
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
