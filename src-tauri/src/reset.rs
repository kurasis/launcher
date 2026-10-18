//! Settings → Reset (D-057): put chosen parts of the launcher back to how they
//! were at first start.
//!
//! The preview lists what each part holds, in particular the worlds, mods and
//! screenshots of every installation, so the user can back them up first. The
//! reset itself runs while nothing else does: backups are deleted first, then
//! installations (after a backup of those that hold the user's data, when
//! asked; those new backups are kept), then downloaded game files, temporary
//! files, traffic statistics, profiles and settings. Microsoft accounts are
//! signed out by the caller through the auth service.

use std::path::Path;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::backup::{self, Reason, Scope, Store, UserData};
use crate::error::{ErrorCode, LauncherError, LauncherResult};
use crate::state::AppState;
use crate::storage::models::OperationKind;
use crate::storage::paths::{DataRoot, InstanceId};

/// One installation and what the user made in it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallationData {
    pub instance_id: InstanceId,
    pub display_name: String,
    pub version_label: String,
    pub data: UserData,
}

/// What each part of a reset would remove.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResetPreview {
    pub installations: Vec<InstallationData>,
    /// Libraries, assets, Java runtimes and version metadata shared by all
    /// installations.
    pub game_files_bytes: u64,
    pub backup_count: u64,
    pub backup_bytes: u64,
    pub microsoft_accounts: u64,
    pub notoken_profiles: u64,
    /// Unfinished downloads, thumbnails and game logs.
    pub temporary_bytes: u64,
}

/// The parts to reset.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResetChoice {
    #[serde(default)]
    pub settings: bool,
    #[serde(default)]
    pub installations: bool,
    /// Back up installations holding worlds, mods, packs or screenshots first.
    #[serde(default)]
    pub back_up_installations: bool,
    #[serde(default)]
    pub game_files: bool,
    #[serde(default)]
    pub accounts: bool,
    #[serde(default)]
    pub backups: bool,
    #[serde(default)]
    pub traffic: bool,
    #[serde(default)]
    pub temporary: bool,
}

/// What a reset did.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResetReport {
    pub backed_up: u64,
    pub deleted_installations: u64,
    pub deleted_backups: u64,
    pub freed_bytes: u64,
    /// Microsoft accounts signed out (by the caller).
    pub signed_out: u64,
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

/// Downloaded files every installation shares; installing fetches them again.
fn game_file_dirs(root: &DataRoot) -> Vec<std::path::PathBuf> {
    let base = root.path();
    vec![
        base.join("shared"),
        base.join("metadata"),
        root.runtimes_dir(),
    ]
}

/// Files nothing depends on: partial downloads, thumbnails, game logs. The
/// launcher's own log is in use and stays.
fn temporary_dirs(root: &DataRoot) -> Vec<std::path::PathBuf> {
    let base = root.path();
    vec![
        root.downloads_dir(),
        base.join("cache"),
        base.join("logs").join("runs"),
    ]
}

fn data_root(state: &AppState) -> LauncherResult<DataRoot> {
    state.data_root().ok_or_else(|| {
        LauncherError::new(
            ErrorCode::StorageUnavailable,
            "Launcher data is unavailable.",
        )
    })
}

pub fn preview(state: &AppState) -> LauncherResult<ResetPreview> {
    let root = data_root(state)?;
    let accounts = state.accounts_file();
    let store = Store::new(root.path());
    let mut installations: Vec<InstallationData> = state
        .instance_ids()
        .into_iter()
        .filter_map(|id| state.instance(id))
        .map(|i| InstallationData {
            instance_id: i.id,
            version_label: i.version_label(),
            display_name: i.display_name,
            data: backup::user_data(&root.game_dir(i.id)),
        })
        .collect();
    installations.sort_by_key(|i| i.display_name.to_lowercase());
    Ok(ResetPreview {
        installations,
        game_files_bytes: game_file_dirs(&root).iter().map(|d| size(d)).sum(),
        backup_count: store.list().len() as u64,
        backup_bytes: store.size(),
        microsoft_accounts: accounts.accounts.len() as u64,
        notoken_profiles: accounts.notoken_profiles.len() as u64,
        temporary_bytes: temporary_dirs(&root).iter().map(|d| size(d)).sum(),
    })
}

/// Removes `dir` and reports the bytes it held; a missing folder is fine.
fn remove(dir: &Path) -> LauncherResult<u64> {
    let bytes = size(dir);
    match std::fs::remove_dir_all(dir) {
        Ok(()) => Ok(bytes),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(0),
        Err(e) => Err(LauncherError::from_io(&e, "Removing launcher files")),
    }
}

/// Resets the chosen parts except Microsoft accounts, under the activity
/// permit: refused while a game, install or backup runs.
pub fn run(state: &AppState, choice: &ResetChoice) -> LauncherResult<ResetReport> {
    let root = data_root(state)?;
    let permit =
        state
            .activity()
            .try_acquire(OperationKind::Backup, None, Uuid::new_v4().to_string())?;
    state.announce_runs();
    let result = run_locked(state, &root, choice);
    drop(permit);
    state.announce_runs();
    let console = state.console();
    match &result {
        Ok(report) => console.info(format!(
            "Reset done: {} installations deleted ({} backed up first), {} backups deleted, \
             {:.1} MiB freed",
            report.deleted_installations,
            report.backed_up,
            report.deleted_backups,
            report.freed_bytes as f64 / (1024.0 * 1024.0)
        )),
        Err(error) => console.warn(format!("Reset stopped: {}", error.message)),
    }
    result
}

fn run_locked(
    state: &AppState,
    root: &DataRoot,
    choice: &ResetChoice,
) -> LauncherResult<ResetReport> {
    let mut report = ResetReport::default();
    let store = Store::new(root.path());
    if choice.backups {
        let before = store.size();
        for snapshot in store.list() {
            store.delete(&snapshot.info.id)?;
            report.deleted_backups += 1;
        }
        store.collect();
        report.freed_bytes += before.saturating_sub(store.size());
    }
    if choice.installations {
        for id in state.instance_ids() {
            let Some(instance) = state.instance(id) else {
                continue;
            };
            let game = root.game_dir(id);
            if choice.back_up_installations && backup::user_data(&game).any() {
                backup::create_locked(
                    state,
                    root,
                    &instance,
                    Scope::Installation,
                    Reason::BeforeDelete,
                )?;
                report.backed_up += 1;
            }
            report.freed_bytes += size(&root.instance_dir(id));
            state.remove_instance(id)?;
            report.deleted_installations += 1;
        }
    }
    if choice.game_files {
        for dir in game_file_dirs(root) {
            report.freed_bytes += remove(&dir)?;
        }
        let _ = std::fs::remove_file(root.compatibility());
        state.game_files_removed();
    }
    if choice.temporary {
        for dir in temporary_dirs(root) {
            report.freed_bytes += remove(&dir)?;
        }
    }
    if choice.traffic {
        crate::traffic::reset(root)
            .map_err(|e| LauncherError::from_io(&e, "Resetting the traffic statistics"))?;
    }
    if choice.accounts {
        state.update_accounts_file(Box::new(|file| {
            file.notoken_profiles.clear();
            if matches!(
                file.default_profile,
                Some(crate::storage::models::ProfileRef::NoToken { .. })
            ) {
                file.default_profile = None;
            }
        }))?;
    }
    if choice.settings {
        state.reset_settings()?;
    }
    Ok(report)
}

#[cfg(test)]
mod tests;
