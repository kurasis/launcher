//! The activity permits (docs/spec/02 "Concurrency and recovery", D-068).
//!
//! The main permit covers installation/repair OR launch preflight/running game/cleanup
//! across all instances. Install/repair work and the game never overlap, so shared
//! artifacts and runtimes are never mutated while Java may read them.
//!
//! Beside it, one hosting permit covers a world server (Worlds with Friends). A server
//! runs next to a game, and next to an install, a backup or a restore of an installation:
//! the host installs the room's game while its world is open. Deleting a backup is work
//! of the backup's installation too; a server never reads the backup store. An install
//! only adds missing files and puts a runtime in its own content-addressed folder, so the
//! server's Java is not replaced. Never next to a repair (it replaces files) or whole-data
//! operations: data relocation, and backup or restore work of no installation (reset).

use std::sync::{Arc, Mutex, PoisonError};

use serde::Serialize;
use uuid::Uuid;

use crate::error::{ErrorCode, LauncherError, LauncherResult};
use crate::storage::models::OperationKind;
use crate::storage::paths::InstanceId;

/// The activity currently holding the main permit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActiveActivity {
    pub kind: OperationKind,
    /// `None` for the lifecycle test harness, which belongs to no installation.
    pub instance_id: Option<InstanceId>,
    pub operation_id: String,
}

/// The world server currently holding the hosting permit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostingActivity {
    pub world_id: Uuid,
    pub operation_id: String,
}

#[derive(Debug, Default)]
struct Slots {
    current: Option<ActiveActivity>,
    hosting: Option<HostingActivity>,
}

#[derive(Debug, Default, Clone)]
pub struct ActivityRegistry {
    slots: Arc<Mutex<Slots>>,
}

/// Releases the main permit when dropped.
#[derive(Debug)]
pub struct ActivityPermit {
    slots: Arc<Mutex<Slots>>,
    activity: ActiveActivity,
}

impl ActivityPermit {
    pub fn activity(&self) -> &ActiveActivity {
        &self.activity
    }
}

impl Drop for ActivityPermit {
    fn drop(&mut self) {
        let mut slots = self.slots.lock().unwrap_or_else(PoisonError::into_inner);
        if slots.current.as_ref() == Some(&self.activity) {
            slots.current = None;
        }
    }
}

/// Releases the hosting permit when dropped.
#[derive(Debug)]
pub struct HostingPermit {
    slots: Arc<Mutex<Slots>>,
    activity: HostingActivity,
}

impl HostingPermit {
    pub fn activity(&self) -> &HostingActivity {
        &self.activity
    }
}

impl Drop for HostingPermit {
    fn drop(&mut self) {
        let mut slots = self.slots.lock().unwrap_or_else(PoisonError::into_inner);
        if slots.hosting.as_ref() == Some(&self.activity) {
            slots.hosting = None;
        }
    }
}

/// Whether main-permit work may run while a world server runs.
fn compatible_with_hosting(kind: OperationKind, instance_id: Option<InstanceId>) -> bool {
    match kind {
        OperationKind::Launch => true,
        OperationKind::Install | OperationKind::Backup | OperationKind::Restore => {
            instance_id.is_some()
        }
        OperationKind::Repair | OperationKind::Relocate => false,
    }
}

impl ActivityRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    fn slots(&self) -> std::sync::MutexGuard<'_, Slots> {
        self.slots.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Takes the main permit or explains which activity holds it.
    pub fn try_acquire(
        &self,
        kind: OperationKind,
        instance_id: Option<InstanceId>,
        operation_id: impl Into<String>,
    ) -> LauncherResult<ActivityPermit> {
        let mut slots = self.slots();
        if let Some(active) = slots.current.as_ref() {
            return Err(busy_error(active, kind));
        }
        if let Some(hosting) = slots.hosting.as_ref()
            && !compatible_with_hosting(kind, instance_id)
        {
            return Err(hosting_busy_error(hosting));
        }
        let activity = ActiveActivity {
            kind,
            instance_id,
            operation_id: operation_id.into(),
        };
        slots.current = Some(activity.clone());
        Ok(ActivityPermit {
            slots: Arc::clone(&self.slots),
            activity,
        })
    }

    /// Takes the hosting permit for a world server or explains what prevents it.
    pub fn try_acquire_hosting(
        &self,
        world_id: Uuid,
        operation_id: impl Into<String>,
    ) -> LauncherResult<HostingPermit> {
        let mut slots = self.slots();
        if let Some(hosting) = slots.hosting.as_ref() {
            return Err(hosting_busy_error(hosting));
        }
        if let Some(active) = slots.current.as_ref()
            && !compatible_with_hosting(active.kind, active.instance_id)
        {
            return Err(busy_error(active, active.kind));
        }
        let activity = HostingActivity {
            world_id,
            operation_id: operation_id.into(),
        };
        slots.hosting = Some(activity.clone());
        Ok(HostingPermit {
            slots: Arc::clone(&self.slots),
            activity,
        })
    }

    /// The activity holding the main permit.
    pub fn current(&self) -> Option<ActiveActivity> {
        self.slots().current.clone()
    }

    /// The world server holding the hosting permit.
    pub fn hosting(&self) -> Option<HostingActivity> {
        self.slots().hosting.clone()
    }

    /// Whether the core supervises anything: work under the main permit or a world server.
    pub fn is_busy(&self) -> bool {
        let slots = self.slots();
        slots.current.is_some() || slots.hosting.is_some()
    }

    /// Fails with `INSTANCE_BUSY` when the given instance owns the permit.
    pub fn ensure_instance_idle(&self, instance_id: InstanceId) -> LauncherResult<()> {
        match self.current() {
            Some(active) if active.instance_id == Some(instance_id) => Err(LauncherError::new(
                ErrorCode::InstanceBusy,
                "This installation is busy. Wait for the current operation to finish.",
            )
            .with_operation(active.operation_id)),
            _ => Ok(()),
        }
    }
}

fn busy_error(active: &ActiveActivity, requested: OperationKind) -> LauncherError {
    let kind = match active.kind {
        OperationKind::Install => "install",
        OperationKind::Repair => "repair",
        OperationKind::Launch => "launch",
        OperationKind::Relocate => "relocate",
        OperationKind::Backup => "backup",
        OperationKind::Restore => "restore",
    };
    let error = if active.kind == OperationKind::Launch && requested == OperationKind::Launch {
        LauncherError::new(
            ErrorCode::GameAlreadyRunning,
            "A game is already running. Only one game can run at a time.",
        )
    } else {
        LauncherError::new(
            ErrorCode::AppBusy,
            "Another operation is in progress. Wait for it to finish, or cancel it.",
        )
    };
    let error = error
        .with_operation(active.operation_id.clone())
        .with_detail("activeKind", kind);
    match active.instance_id {
        Some(id) => error.with_detail("activeInstanceId", id.to_string()),
        None => error,
    }
}

fn hosting_busy_error(hosting: &HostingActivity) -> LauncherError {
    LauncherError::new(
        ErrorCode::AppBusy,
        "Another operation is in progress. Wait for it to finish, or cancel it.",
    )
    .with_operation(hosting.operation_id.clone())
    .with_detail("activeKind", "host")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::SafeValue;

    #[test]
    fn permit_is_exclusive_across_instances() {
        let registry = ActivityRegistry::new();
        let a = InstanceId::generate();
        let b = InstanceId::generate();
        let permit = registry
            .try_acquire(OperationKind::Install, Some(a), "op-1")
            .unwrap();
        let error = registry
            .try_acquire(OperationKind::Repair, Some(b), "op-2")
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::AppBusy);
        assert_eq!(error.operation_id.as_deref(), Some("op-1"));
        let error = registry
            .try_acquire(OperationKind::Launch, Some(b), "op-3")
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::AppBusy);
        assert_eq!(permit.activity().instance_id, Some(a));
        drop(permit);
        assert!(
            registry
                .try_acquire(OperationKind::Launch, Some(b), "op-4")
                .is_ok()
        );
    }

    #[test]
    fn second_game_is_reported_as_already_running() {
        let registry = ActivityRegistry::new();
        let _game = registry
            .try_acquire(OperationKind::Launch, None, "run-1")
            .unwrap();
        let error = registry
            .try_acquire(OperationKind::Launch, Some(InstanceId::generate()), "run-2")
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::GameAlreadyRunning);
    }

    #[test]
    fn instance_busy_only_for_owner() {
        let registry = ActivityRegistry::new();
        let a = InstanceId::generate();
        let b = InstanceId::generate();
        let _permit = registry
            .try_acquire(OperationKind::Install, Some(a), "op")
            .unwrap();
        assert_eq!(
            registry.ensure_instance_idle(a).unwrap_err().code,
            ErrorCode::InstanceBusy
        );
        assert!(registry.ensure_instance_idle(b).is_ok());
    }

    #[test]
    fn concurrent_acquisition_admits_exactly_one() {
        let registry = ActivityRegistry::new();
        let barrier = Arc::new(std::sync::Barrier::new(8));
        let handles: Vec<_> = (0..8)
            .map(|i| {
                let registry = registry.clone();
                let barrier = Arc::clone(&barrier);
                std::thread::spawn(move || {
                    barrier.wait();
                    registry
                        .try_acquire(
                            OperationKind::Install,
                            Some(InstanceId::generate()),
                            format!("{i}"),
                        )
                        .ok()
                })
            })
            .collect();
        let permits: Vec<_> = handles
            .into_iter()
            .filter_map(|h| h.join().unwrap())
            .collect();
        assert_eq!(permits.len(), 1);
    }

    #[test]
    fn server_runs_beside_a_game_and_instance_backups() {
        let registry = ActivityRegistry::new();
        let instance = InstanceId::generate();
        let host = registry
            .try_acquire_hosting(Uuid::new_v4(), "host-1")
            .unwrap();
        let game = registry
            .try_acquire(OperationKind::Launch, Some(instance), "run-1")
            .unwrap();
        assert!(registry.is_busy());
        drop(game);
        for kind in [OperationKind::Backup, OperationKind::Restore] {
            let permit = registry.try_acquire(kind, Some(instance), "op").unwrap();
            drop(permit);
        }
        assert_eq!(host.activity().operation_id, "host-1");
        assert!(registry.current().is_none());
    }

    #[test]
    fn server_starts_beside_installation_backup_work() {
        let registry = ActivityRegistry::new();
        for kind in [OperationKind::Backup, OperationKind::Restore] {
            let _work = registry
                .try_acquire(kind, Some(InstanceId::generate()), "op-1")
                .unwrap();
            let host = registry
                .try_acquire_hosting(Uuid::new_v4(), "host-1")
                .unwrap();
            drop(host);
        }
    }

    #[test]
    fn game_first_then_server() {
        let registry = ActivityRegistry::new();
        let _game = registry
            .try_acquire(OperationKind::Launch, Some(InstanceId::generate()), "run-1")
            .unwrap();
        assert!(
            registry
                .try_acquire_hosting(Uuid::new_v4(), "host-1")
                .is_ok()
        );
    }

    #[test]
    fn server_excludes_work_that_replaces_shared_files() {
        let registry = ActivityRegistry::new();
        let _host = registry
            .try_acquire_hosting(Uuid::new_v4(), "host-1")
            .unwrap();
        let instance = Some(InstanceId::generate());
        for (kind, instance_id) in [
            (OperationKind::Install, None),
            (OperationKind::Repair, instance),
            (OperationKind::Relocate, None),
            (OperationKind::Backup, None),
            (OperationKind::Restore, None),
        ] {
            let error = registry.try_acquire(kind, instance_id, "op").unwrap_err();
            assert_eq!(error.code, ErrorCode::AppBusy, "{kind:?}");
            assert_eq!(error.operation_id.as_deref(), Some("host-1"));
            let details = error.safe_details.as_ref().unwrap();
            assert_eq!(
                details.get("activeKind"),
                Some(&SafeValue::Text("host".to_owned()))
            );
        }
        assert!(registry.current().is_none(), "refusals leave no permit");
    }

    #[test]
    fn install_or_reset_blocks_a_server() {
        let registry = ActivityRegistry::new();
        for (kind, instance_id) in [
            (OperationKind::Repair, Some(InstanceId::generate())),
            (OperationKind::Relocate, None),
            (OperationKind::Backup, None),
        ] {
            let permit = registry.try_acquire(kind, instance_id, "op-1").unwrap();
            let error = registry
                .try_acquire_hosting(Uuid::new_v4(), "host-1")
                .unwrap_err();
            assert_eq!(error.code, ErrorCode::AppBusy, "{kind:?}");
            assert_eq!(error.operation_id.as_deref(), Some("op-1"));
            drop(permit);
        }
        assert!(registry.hosting().is_none());
    }

    #[test]
    fn one_server_at_a_time_and_released_on_drop() {
        let registry = ActivityRegistry::new();
        let host = registry
            .try_acquire_hosting(Uuid::new_v4(), "host-1")
            .unwrap();
        let error = registry
            .try_acquire_hosting(Uuid::new_v4(), "host-2")
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::AppBusy);
        assert_eq!(error.operation_id.as_deref(), Some("host-1"));
        drop(host);
        assert!(!registry.is_busy());
        assert!(
            registry
                .try_acquire(OperationKind::Install, Some(InstanceId::generate()), "op")
                .is_ok()
        );
    }

    #[test]
    fn the_host_installs_a_game_while_its_world_is_open() {
        let registry = ActivityRegistry::new();
        let host = registry
            .try_acquire_hosting(Uuid::new_v4(), "host-1")
            .unwrap();
        let install = registry
            .try_acquire(OperationKind::Install, Some(InstanceId::generate()), "op-1")
            .unwrap();
        drop(install);
        drop(host);
        // And a world opens while an install runs.
        let _install = registry
            .try_acquire(OperationKind::Install, Some(InstanceId::generate()), "op-2")
            .unwrap();
        assert!(
            registry
                .try_acquire_hosting(Uuid::new_v4(), "host-2")
                .is_ok()
        );
    }

    #[test]
    fn concurrent_server_and_repair_never_overlap() {
        for _ in 0..200 {
            let registry = ActivityRegistry::new();
            let barrier = Arc::new(std::sync::Barrier::new(2));
            let repair = {
                let registry = registry.clone();
                let barrier = Arc::clone(&barrier);
                std::thread::spawn(move || {
                    barrier.wait();
                    registry
                        .try_acquire(OperationKind::Repair, Some(InstanceId::generate()), "op")
                        .ok()
                })
            };
            let host = {
                let registry = registry.clone();
                let barrier = Arc::clone(&barrier);
                std::thread::spawn(move || {
                    barrier.wait();
                    registry.try_acquire_hosting(Uuid::new_v4(), "host").ok()
                })
            };
            let repair = repair.join().unwrap();
            let host = host.join().unwrap();
            assert!(repair.is_some() != host.is_some());
        }
    }
}
