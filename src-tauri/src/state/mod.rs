//! Authoritative application state (docs/spec/02 "Rust implementation building
//! blocks", doc 06 "Bootstrap and ownership").
//!
//! The webview is a disposable view of this state. Every mutation is validated
//! here, persisted before it becomes visible, increments the monotonic revision and
//! emits exactly one event per revision.

pub mod activity;
pub mod dto;
pub mod ledger;

use std::collections::BTreeMap;
use std::sync::{Mutex, MutexGuard, PoisonError};

use chrono::Utc;
use serde::Serialize;
use serde::de::DeserializeOwned;
use uuid::Uuid;

use crate::auth::client_id;
use crate::catalog::compatibility;
use crate::error::{ErrorCode, LauncherError, LauncherResult};
use crate::storage::document::SCHEMA_VERSION;
use crate::storage::models::{AccountsFile, InstallationHealth, InstanceFile, Settings};
use crate::storage::paths::{DataRoot, InstanceId};
use crate::storage::{StorageIssue, Store};
use crate::validation::{self, MemoryPolicy, RESOLUTION_BOUNDS};
use activity::ActivityRegistry;
use dto::*;
use ledger::{Lookup, RequestLedger};

/// Receives backend events; the Tauri implementation forwards them to webviews.
pub trait EventSink: Send + Sync {
    fn emit(&self, event: &BackendEvent);
}

/// A sink that drops events, for contexts without a webview.
pub struct NoopSink;

impl EventSink for NoopSink {
    fn emit(&self, _event: &BackendEvent) {}
}

enum Availability {
    Ready(Box<Store>),
    Unavailable(LauncherError),
}

struct Inner {
    revision: u64,
    availability: Availability,
    ledger: RequestLedger,
    data: Data,
}

/// State that mutations change; kept apart from the store so both can be borrowed.
struct Data {
    data_root_path: Option<String>,
    settings: Settings,
    settings_writable: bool,
    accounts: AccountsFile,
    instances: BTreeMap<InstanceId, InstanceFile>,
    issues: Vec<StorageIssue>,
}

pub struct AppState {
    epoch: String,
    app_version: String,
    memory: Result<MemoryPolicy, LauncherError>,
    inner: Mutex<Inner>,
    activity: ActivityRegistry,
    sink: Box<dyn EventSink>,
}

impl AppState {
    /// Opens storage at `root`. Never fails: an unusable data directory leaves the
    /// state unavailable with an explanatory error in the snapshot.
    pub fn initialize(
        root: LauncherResult<DataRoot>,
        app_version: impl Into<String>,
        memory: Result<MemoryPolicy, LauncherError>,
        sink: Box<dyn EventSink>,
    ) -> Self {
        let data_root_path = root
            .as_ref()
            .ok()
            .map(|r| r.path().to_string_lossy().into_owned());
        let mut inner = Inner {
            revision: 0,
            availability: Availability::Unavailable(LauncherError::new(
                ErrorCode::StorageUnavailable,
                "Storage is not initialized.",
            )),
            ledger: RequestLedger::default(),
            data: Data {
                data_root_path,
                settings: Settings::default(),
                settings_writable: false,
                accounts: AccountsFile::default(),
                instances: BTreeMap::new(),
                issues: Vec::new(),
            },
        };
        match root.and_then(Store::open) {
            Ok(store) => {
                let settings = store.load_settings();
                let accounts = store.load_accounts();
                let (instances, mut issues) = store.load_instances();
                issues.extend(settings.issue);
                issues.extend(accounts.issue);
                if let Err(error) = store.write_cache(
                    &store.root().compatibility(),
                    &compatibility::policy_document(),
                ) {
                    issues.push(StorageIssue {
                        code: error.code,
                        message: "The compatibility list copy could not be written.".into(),
                        document: "compatibility.json".into(),
                    });
                }
                let data = &mut inner.data;
                data.settings = settings.value;
                data.settings_writable = settings.writable;
                data.accounts = accounts.value;
                data.instances = instances.into_iter().map(|i| (i.id, i)).collect();
                data.issues = issues;
                // A selection that no longer exists is dropped in memory only.
                if let Some(selected) = data.settings.selected_instance_id
                    && !data.instances.contains_key(&selected)
                {
                    data.settings.selected_instance_id = None;
                }
                inner.availability = Availability::Ready(Box::new(store));
            }
            Err(error) => inner.availability = Availability::Unavailable(error),
        }
        Self {
            epoch: Uuid::new_v4().to_string(),
            app_version: app_version.into(),
            memory,
            inner: Mutex::new(inner),
            activity: ActivityRegistry::new(),
            sink,
        }
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn session_epoch(&self) -> &str {
        &self.epoch
    }

    pub fn activity(&self) -> &ActivityRegistry {
        &self.activity
    }

    /// The DATA_ROOT if storage is available.
    pub fn data_root(&self) -> Option<DataRoot> {
        match &self.lock().availability {
            Availability::Ready(store) => Some(store.root().clone()),
            Availability::Unavailable(_) => None,
        }
    }

    pub fn snapshot(&self) -> AppSnapshot {
        let inner = self.lock();
        let active = self.activity.current();
        let busy = |id: InstanceId| active.as_ref().is_some_and(|a| a.instance_id == id);
        let data = &inner.data;
        let mut instances: Vec<InstanceSummary> = data
            .instances
            .values()
            .map(|i| InstanceSummary::new(i, busy(i.id)))
            .collect();
        instances.sort_by(|a, b| a.created_at.cmp(&b.created_at).then(a.id.cmp(&b.id)));
        let (status, error) = match &inner.availability {
            Availability::Ready(_) => (DataRootStatus::Ready, None),
            Availability::Unavailable(error) => (DataRootStatus::Unavailable, Some(error.clone())),
        };
        AppSnapshot {
            api_schema_version: API_SCHEMA_VERSION,
            session_epoch: self.epoch.clone(),
            revision: inner.revision,
            app_version: self.app_version.clone(),
            data_root: DataRootInfo {
                status,
                path: data.data_root_path.clone(),
                error,
            },
            settings: SettingsView::new(&data.settings, data.settings_writable),
            account: AccountSummary {
                // Authentication arrives in P04; nothing is restored before then.
                state: if data.accounts.sign_out_pending {
                    AccountState::SignOutPending
                } else {
                    AccountState::SignedOut
                },
                profile_name: None,
                profile_id: None,
            },
            microsoft_client_id: client_id::build_time_status(),
            instances,
            active_operation: active,
            supported_versions: compatibility::SUPPORTED_VERSIONS,
            memory: MemoryLimits {
                policy: self.memory.as_ref().ok().copied(),
                error: self.memory.as_ref().err().cloned(),
                resolution: RESOLUTION_BOUNDS,
            },
            storage_issues: data.issues.clone(),
        }
    }

    /// Runs a mutation with request-ID idempotency. The closure persists its
    /// changes and returns the result plus the events it produced; the revision
    /// is advanced once per event.
    fn mutate<P, R>(
        &self,
        command: &str,
        request: MutationRequest<P>,
        apply: impl FnOnce(&mut Data, &Store, P, &mut Vec<Change>) -> LauncherResult<R>,
    ) -> LauncherResult<R>
    where
        P: Serialize,
        R: Serialize + DeserializeOwned,
    {
        ledger::validate_request_id(&request.request_id)?;
        let fingerprint = format!(
            "{command}:{}",
            serde_json::to_string(&request.payload)
                .map_err(|_| LauncherError::invalid_input("The request payload is invalid."))?
        );
        let mut guard = self.lock();
        let inner = &mut *guard;
        match inner.ledger.lookup(&request.request_id, &fingerprint)? {
            Lookup::Replay(Ok(value)) => {
                return serde_json::from_value(value)
                    .map_err(|_| LauncherError::internal("A recorded result could not be read."));
            }
            Lookup::Replay(Err(error)) => return Err(error),
            Lookup::New => {}
        }
        let store = match &inner.availability {
            Availability::Ready(store) => store,
            Availability::Unavailable(error) => {
                return Err(LauncherError::new(error.code, error.message.clone()));
            }
        };
        let mut changes = Vec::new();
        let result = apply(&mut inner.data, store, request.payload, &mut changes);
        let recorded = match &result {
            Ok(value) => serde_json::to_value(value)
                .map_err(|_| LauncherError::internal("Serializing a result failed.")),
            Err(error) => Err(error.clone()),
        };
        inner
            .ledger
            .record(request.request_id, fingerprint, recorded);
        for change in changes {
            inner.revision += 1;
            let event = self.event_for(inner.revision, &inner.data, change);
            self.sink.emit(&event);
        }
        result
    }

    fn event_for(&self, revision: u64, data: &Data, change: Change) -> BackendEvent {
        let session_epoch = self.epoch.clone();
        let timestamp = Utc::now();
        match change {
            Change::Instance(id) => {
                let busy = self.activity.current().is_some_and(|a| a.instance_id == id);
                let instance = data
                    .instances
                    .get(&id)
                    .map(|i| InstanceSummary::new(i, busy))
                    .expect("changed instance exists");
                BackendEvent::InstanceChanged(EventEnvelope {
                    session_epoch,
                    revision,
                    timestamp,
                    body: InstanceChanged { instance },
                })
            }
            Change::Settings => BackendEvent::SettingsChanged(EventEnvelope {
                session_epoch,
                revision,
                timestamp,
                body: SettingsChanged {
                    settings: SettingsView::new(&data.settings, data.settings_writable),
                },
            }),
        }
    }

    pub fn create_instance(
        &self,
        request: MutationRequest<CreateInstancePayload>,
    ) -> LauncherResult<InstanceSummary> {
        let memory = self.memory.clone();
        self.mutate(
            "create_instance",
            request,
            |data, store, payload, changes| {
                let display_name = validation::validate_display_name(&payload.display_name)?;
                let version = compatibility::find(&payload.version_id).ok_or_else(|| {
                    LauncherError::invalid_input(
                        "This Minecraft version is not in the launcher's supported list.",
                    )
                    .with_detail("versionId", payload.version_id.clone())
                })?;
                let memory = memory?;
                let max_heap_mib = data
                    .settings
                    .default_max_heap_mib
                    .map_or(memory.recommended_heap_mib, |mib| {
                        mib.clamp(memory.min_heap_mib, memory.max_heap_mib)
                    });
                let instance = InstanceFile {
                    schema_version: SCHEMA_VERSION,
                    id: InstanceId::generate(),
                    display_name,
                    version_id: version.id.to_owned(),
                    manifest_sha1: None,
                    created_at: Utc::now(),
                    last_played_at: None,
                    max_heap_mib,
                    resolution: validation::validate_resolution(data.settings.default_resolution)
                        .unwrap_or_default(),
                    health: InstallationHealth::NotInstalled,
                    last_result: None,
                };
                store.create_instance(&instance)?;
                let id = instance.id;
                data.instances.insert(id, instance);
                changes.push(Change::Instance(id));

                if data.settings.selected_instance_id.is_none() && data.settings_writable {
                    let mut settings = data.settings.clone();
                    settings.selected_instance_id = Some(id);
                    // The instance exists even if selecting it cannot be saved.
                    if store.save_settings(&settings).is_ok() {
                        data.settings = settings;
                        changes.push(Change::Settings);
                    }
                }
                Ok(InstanceSummary::new(&data.instances[&id], false))
            },
        )
    }

    pub fn update_instance_settings(
        &self,
        request: MutationRequest<UpdateInstanceSettingsPayload>,
    ) -> LauncherResult<InstanceSummary> {
        let memory = self.memory.clone();
        let activity = self.activity.clone();
        self.mutate(
            "update_instance_settings",
            request,
            |data, store, payload, changes| {
                let id = payload.instance_id;
                let current = data.instances.get(&id).ok_or_else(instance_not_found)?;
                activity.ensure_instance_idle(id)?;
                let max_heap_mib = memory?.validate_heap(payload.max_heap_mib)?;
                let resolution = validation::validate_resolution(payload.resolution)?;
                let mut updated = current.clone();
                updated.max_heap_mib = max_heap_mib;
                updated.resolution = resolution;
                if updated != *current {
                    store.save_instance(&updated)?;
                    data.instances.insert(id, updated);
                    changes.push(Change::Instance(id));
                }
                Ok(InstanceSummary::new(&data.instances[&id], false))
            },
        )
    }

    pub fn set_app_settings(
        &self,
        request: MutationRequest<SettingsPatch>,
    ) -> LauncherResult<SettingsView> {
        let memory = self.memory.clone();
        self.mutate(
            "set_app_settings",
            request,
            |data, store, patch, changes| {
                if !data.settings_writable {
                    return Err(LauncherError::new(
                        ErrorCode::UnsupportedSchema,
                        "Settings are read-only because they were written by a newer launcher \
                     version or could not be read.",
                    ));
                }
                let mut settings = data.settings.clone();
                if let Some(theme) = patch.theme {
                    settings.theme = theme;
                }
                if let Some(mode) = patch.during_game_mode {
                    settings.during_game_mode = mode;
                }
                if let Some(id) = patch.selected_instance_id {
                    if !data.instances.contains_key(&id) {
                        return Err(instance_not_found());
                    }
                    settings.selected_instance_id = Some(id);
                }
                if let Some(mib) = patch.default_max_heap_mib {
                    settings.default_max_heap_mib = Some(memory?.validate_heap(mib)?);
                }
                if let Some(resolution) = patch.default_resolution {
                    settings.default_resolution = validation::validate_resolution(resolution)?;
                }
                if settings != data.settings {
                    store.save_settings(&settings)?;
                    data.settings = settings;
                    changes.push(Change::Settings);
                }
                Ok(SettingsView::new(&data.settings, data.settings_writable))
            },
        )
    }
}

enum Change {
    Instance(InstanceId),
    Settings,
}

fn instance_not_found() -> LauncherError {
    LauncherError::new(
        ErrorCode::InstanceNotFound,
        "The installation no longer exists.",
    )
}

#[cfg(test)]
mod tests;
