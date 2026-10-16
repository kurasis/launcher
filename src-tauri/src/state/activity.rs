//! The global activity permit (docs/spec/02 "Concurrency and recovery").
//!
//! One permit covers installation/repair OR launch preflight/running game/cleanup
//! across all instances. Install/repair work and the game never overlap, so shared
//! artifacts and runtimes are never mutated while Java may read them.

use std::sync::{Arc, Mutex, PoisonError};

use serde::Serialize;

use crate::error::{ErrorCode, LauncherError, LauncherResult};
use crate::storage::models::OperationKind;
use crate::storage::paths::InstanceId;

/// The activity currently holding the permit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActiveActivity {
    pub kind: OperationKind,
    pub instance_id: InstanceId,
    pub operation_id: String,
}

#[derive(Debug, Default, Clone)]
pub struct ActivityRegistry {
    current: Arc<Mutex<Option<ActiveActivity>>>,
}

/// Releases the permit when dropped.
#[derive(Debug)]
pub struct ActivityPermit {
    current: Arc<Mutex<Option<ActiveActivity>>>,
    activity: ActiveActivity,
}

impl ActivityPermit {
    pub fn activity(&self) -> &ActiveActivity {
        &self.activity
    }
}

impl Drop for ActivityPermit {
    fn drop(&mut self) {
        let mut current = self.current.lock().unwrap_or_else(PoisonError::into_inner);
        if current.as_ref() == Some(&self.activity) {
            *current = None;
        }
    }
}

impl ActivityRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Takes the permit or explains which activity holds it.
    pub fn try_acquire(
        &self,
        kind: OperationKind,
        instance_id: InstanceId,
        operation_id: impl Into<String>,
    ) -> LauncherResult<ActivityPermit> {
        let mut current = self.current.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(active) = current.as_ref() {
            return Err(busy_error(active, kind));
        }
        let activity = ActiveActivity {
            kind,
            instance_id,
            operation_id: operation_id.into(),
        };
        *current = Some(activity.clone());
        Ok(ActivityPermit {
            current: Arc::clone(&self.current),
            activity,
        })
    }

    pub fn current(&self) -> Option<ActiveActivity> {
        self.current
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Fails with `INSTANCE_BUSY` when the given instance owns the permit.
    pub fn ensure_instance_idle(&self, instance_id: InstanceId) -> LauncherResult<()> {
        match self.current() {
            Some(active) if active.instance_id == instance_id => Err(LauncherError::new(
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
    error
        .with_operation(active.operation_id.clone())
        .with_detail("activeKind", kind)
        .with_detail("activeInstanceId", active.instance_id.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permit_is_exclusive_across_instances() {
        let registry = ActivityRegistry::new();
        let a = InstanceId::generate();
        let b = InstanceId::generate();
        let permit = registry
            .try_acquire(OperationKind::Install, a, "op-1")
            .unwrap();
        let error = registry
            .try_acquire(OperationKind::Repair, b, "op-2")
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::AppBusy);
        assert_eq!(error.operation_id.as_deref(), Some("op-1"));
        let error = registry
            .try_acquire(OperationKind::Launch, b, "op-3")
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::AppBusy);
        assert_eq!(permit.activity().instance_id, a);
        drop(permit);
        assert!(
            registry
                .try_acquire(OperationKind::Launch, b, "op-4")
                .is_ok()
        );
    }

    #[test]
    fn second_game_is_reported_as_already_running() {
        let registry = ActivityRegistry::new();
        let _game = registry
            .try_acquire(OperationKind::Launch, InstanceId::generate(), "run-1")
            .unwrap();
        let error = registry
            .try_acquire(OperationKind::Launch, InstanceId::generate(), "run-2")
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::GameAlreadyRunning);
    }

    #[test]
    fn instance_busy_only_for_owner() {
        let registry = ActivityRegistry::new();
        let a = InstanceId::generate();
        let b = InstanceId::generate();
        let _permit = registry
            .try_acquire(OperationKind::Install, a, "op")
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
                            InstanceId::generate(),
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
}
