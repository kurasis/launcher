//! Authoritative application state (docs/spec/02 "Rust implementation building
//! blocks", doc 06 "Bootstrap and ownership").
//!
//! The webview is a disposable view of this state. Every mutation is validated
//! here, persisted before it becomes visible, increments the monotonic revision and
//! emits exactly one event per revision.

pub mod activity;
pub mod dto;
pub mod ledger;

use std::collections::{BTreeMap, VecDeque};
use std::sync::{Mutex, MutexGuard, PoisonError};

use chrono::Utc;
use serde::Serialize;
use serde::de::DeserializeOwned;
use uuid::Uuid;

use crate::auth::client_id;
use crate::catalog::compatibility;
use crate::console::{self, Console};
use crate::error::{ErrorCode, LauncherError, LauncherResult};
use crate::storage::document::SCHEMA_VERSION;
use crate::storage::models::{
    AccountsFile, InstallationHealth, InstanceFile, LastResult, OperationKind, Outcome, Settings,
};
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
    current_run: Option<RunSummary>,
    last_run: Option<RunSummary>,
    account: AccountSummary,
    operation_progress: Option<OperationProgress>,
    recent_operations: VecDeque<OperationResult>,
}

/// Recent terminal operation outcomes kept for recreated webviews.
const RECENT_OPERATIONS: usize = 10;

pub struct AppState {
    epoch: String,
    app_version: String,
    memory: Result<MemoryPolicy, LauncherError>,
    inner: Mutex<Inner>,
    activity: ActivityRegistry,
    sink: Box<dyn EventSink>,
    console: Console,
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
                current_run: None,
                last_run: None,
                account: AccountSummary::signed_out(),
                operation_progress: None,
                recent_operations: VecDeque::new(),
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
                        template: None,
                        params: std::collections::BTreeMap::new(),
                        code: error.code,
                        message: "The compatibility list copy could not be written.".into(),
                        document: "compatibility.json".into(),
                    });
                }
                // No job can be running yet: unfinished ones were interrupted.
                if let Err(error) = crate::runtime::reconcile_runtimes(store.root()) {
                    issues.push(StorageIssue {
                        template: None,
                        params: std::collections::BTreeMap::new(),
                        code: error.code,
                        message:
                            "An interrupted Java runtime installation could not be cleaned up."
                                .into(),
                        document: "runtimes".into(),
                    });
                }
                let interrupted = match crate::download::receipt::reconcile_jobs(store.root()) {
                    Ok(interrupted) => interrupted,
                    Err(error) => {
                        issues.push(StorageIssue {
                            template: None,
                            params: std::collections::BTreeMap::new(),
                            code: error.code,
                            message: "Unfinished download records could not be cleaned up.".into(),
                            document: "jobs".into(),
                        });
                        Vec::new()
                    }
                };
                let data = &mut inner.data;
                data.settings = settings.value;
                data.settings_writable = settings.writable;
                data.accounts = accounts.value;
                data.account.state = if data.accounts.sign_out_pending {
                    AccountState::SignOutPending
                } else if data.accounts.active.is_some() {
                    // The auth service restores and re-verifies the saved account.
                    AccountState::Refreshing
                } else {
                    AccountState::SignedOut
                };
                data.instances = instances.into_iter().map(|i| (i.id, i)).collect();
                for receipt in interrupted {
                    record_interrupted(data, &store, &receipt, &mut issues);
                }
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
        let state = Self {
            epoch: Uuid::new_v4().to_string(),
            app_version: app_version.into(),
            memory,
            inner: Mutex::new(inner),
            activity: ActivityRegistry::new(),
            sink,
            console: Console::default(),
        };
        state.log_startup();
        state
    }

    /// The console journal (D-037).
    pub fn console(&self) -> &Console {
        &self.console
    }

    fn log_startup(&self) {
        let inner = self.lock();
        let data = &inner.data;
        self.console.info(format!(
            "Launcher {} started on {} {}",
            self.app_version,
            std::env::consts::OS,
            std::env::consts::ARCH
        ));
        match &inner.availability {
            Availability::Ready(_) => self.console.info(format!(
                "Data directory {}; {} installation(s); Java from {}",
                data.data_root_path.as_deref().unwrap_or("?"),
                data.instances.len(),
                data.settings.java_provider.as_str()
            )),
            Availability::Unavailable(error) => self
                .console
                .error(format!("Data directory unavailable: {}", error.message)),
        }
        if let Ok(memory) = &self.memory {
            self.console.info(format!(
                "Memory {} MiB; game heap {}–{} MiB, recommended {} MiB",
                memory.physical_mib,
                memory.min_heap_mib,
                memory.max_heap_mib,
                memory.recommended_heap_mib
            ));
        }
        for issue in &data.issues {
            self.console.warn(format!(
                "Storage notice ({}): {}",
                issue.document, issue.message
            ));
        }
    }

    /// `"Name" (Minecraft 1.21.1)` for journal lines.
    fn instance_label(&self, id: Option<InstanceId>) -> String {
        let inner = self.lock();
        id.and_then(|id| inner.data.instances.get(&id)).map_or_else(
            || "the launcher".to_owned(),
            |i| format!("\"{}\" (Minecraft {})", i.display_name, i.version_id),
        )
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
        let busy = |id: InstanceId| active.as_ref().is_some_and(|a| a.instance_id == Some(id));
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
            account: data.account.clone(),
            microsoft_client_id: client_id::build_time_status(),
            instances,
            active_operation: active,
            current_run: data.current_run.clone(),
            last_run: data.last_run.clone(),
            lifecycle_harness_available: cfg!(debug_assertions),
            supported_versions: compatibility::SUPPORTED_VERSIONS,
            memory: MemoryLimits {
                policy: self.memory.as_ref().ok().copied(),
                error: self.memory.as_ref().err().cloned(),
                resolution: RESOLUTION_BOUNDS,
            },
            storage_issues: data.issues.clone(),
            operation_progress: data.operation_progress.clone(),
            recent_operations: data.recent_operations.iter().cloned().collect(),
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
                let busy = self
                    .activity
                    .current()
                    .is_some_and(|a| a.instance_id == Some(id));
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
            Change::Runs => BackendEvent::GameStateChanged(Box::new(EventEnvelope {
                session_epoch,
                revision,
                timestamp,
                body: GameStateChanged {
                    current_run: data.current_run.clone(),
                    last_run: data.last_run.clone(),
                    active_operation: self.activity.current(),
                },
            })),
            Change::Account => BackendEvent::AccountStateChanged(Box::new(EventEnvelope {
                session_epoch,
                revision,
                timestamp,
                body: AccountStateChanged {
                    account: data.account.clone(),
                },
            })),
            Change::OperationFinished(result) => {
                BackendEvent::OperationFinished(Box::new(EventEnvelope {
                    session_epoch,
                    revision,
                    timestamp,
                    body: OperationFinished {
                        result: *result,
                        active_operation: self.activity.current(),
                    },
                }))
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

    /// Applies a backend-initiated change (not a user request) and emits its event.
    fn publish(&self, apply: impl FnOnce(&mut Data) -> bool, change: Change) {
        let mut guard = self.lock();
        let inner = &mut *guard;
        if apply(&mut inner.data) {
            inner.revision += 1;
            let event = self.event_for(inner.revision, &inner.data, change);
            self.sink.emit(&event);
        }
    }

    /// Records the start of a supervised run.
    pub fn begin_run(&self, run: RunSummary) {
        self.console.info(format!(
            "Run {} of {}: {}",
            short(&run.run_id),
            self.instance_label(run.instance_id),
            console::name(&run.state)
        ));
        self.publish(
            |data| {
                data.current_run = Some(run);
                true
            },
            Change::Runs,
        );
    }

    /// Updates the current run if it is still `run_id`.
    pub fn update_run(&self, run_id: &str, update: impl FnOnce(&mut RunSummary)) {
        let console = &self.console;
        self.publish(
            |data| match data.current_run.as_mut() {
                Some(run) if run.run_id == run_id => {
                    let before = run.state;
                    update(run);
                    if run.state != before {
                        console.info(format!(
                            "Run {}: {}",
                            short(run_id),
                            console::name(&run.state)
                        ));
                    }
                    true
                }
                _ => false,
            },
            Change::Runs,
        );
    }

    /// Moves the current run to `last_run` with its final state.
    pub fn finish_run(&self, run_id: &str, finish: impl FnOnce(&mut RunSummary)) {
        self.publish(
            |data| match data.current_run.take() {
                Some(mut run) if run.run_id == run_id => {
                    finish(&mut run);
                    log_run_end(&self.console, &run);
                    data.last_run = Some(run);
                    true
                }
                other => {
                    data.current_run = other;
                    false
                }
            },
            Change::Runs,
        );
    }

    /// Re-announces run state, for example after the activity permit was released.
    pub fn announce_runs(&self) {
        self.publish(|_| true, Change::Runs);
    }

    /// Current account display state.
    pub fn account(&self) -> AccountSummary {
        self.lock().data.account.clone()
    }

    /// Replaces the account display state and announces it.
    pub fn set_account(&self, account: AccountSummary) {
        let console = &self.console;
        self.publish(
            |data| {
                if data.account == account {
                    false
                } else {
                    if data.account.state != account.state {
                        let text = format!("Account: {}", console::name(&account.state));
                        match &account.error {
                            Some(error) => console.warn(format!("{text} — {}", error.message)),
                            None => console.info(text),
                        }
                    }
                    data.account = account;
                    true
                }
            },
            Change::Account,
        );
    }

    /// The persisted account metadata (`accounts.json`).
    pub fn accounts_file(&self) -> AccountsFile {
        self.lock().data.accounts.clone()
    }

    /// Persists account metadata. Contains no secrets.
    pub fn save_accounts_file(&self, accounts: AccountsFile) -> LauncherResult<()> {
        let mut guard = self.lock();
        let inner = &mut *guard;
        let Availability::Ready(store) = &inner.availability else {
            return Err(LauncherError::new(
                ErrorCode::StorageUnavailable,
                "Launcher data is unavailable.",
            ));
        };
        store.save_accounts(&accounts)?;
        inner.data.accounts = accounts;
        Ok(())
    }

    pub fn current_run(&self) -> Option<RunSummary> {
        self.lock().data.current_run.clone()
    }

    pub fn during_game_mode(&self) -> crate::storage::models::DuringGameMode {
        self.lock().data.settings.during_game_mode
    }

    /// Runtime provider for new installations.
    pub fn java_provider(&self) -> crate::storage::paths::JavaProvider {
        self.lock().data.settings.java_provider
    }

    pub fn instance(&self, id: InstanceId) -> Option<InstanceFile> {
        self.lock().data.instances.get(&id).cloned()
    }

    pub fn instance_ids(&self) -> Vec<InstanceId> {
        self.lock().data.instances.keys().copied().collect()
    }

    /// Applies a backend-initiated change to an instance, persists it and
    /// announces it. Used by install/repair and launch bookkeeping.
    pub fn update_instance(
        &self,
        id: InstanceId,
        update: impl FnOnce(&mut InstanceFile),
    ) -> LauncherResult<InstanceSummary> {
        let mut guard = self.lock();
        let inner = &mut *guard;
        let Availability::Ready(store) = &inner.availability else {
            return Err(LauncherError::new(
                ErrorCode::StorageUnavailable,
                "Launcher data is unavailable.",
            ));
        };
        let current = inner
            .data
            .instances
            .get(&id)
            .ok_or_else(instance_not_found)?;
        let mut updated = current.clone();
        update(&mut updated);
        if updated != *current {
            store.save_instance(&updated)?;
            inner.data.instances.insert(id, updated);
            inner.revision += 1;
            let event = self.event_for(inner.revision, &inner.data, Change::Instance(id));
            self.sink.emit(&event);
        }
        let busy = self
            .activity
            .current()
            .is_some_and(|a| a.instance_id == Some(id));
        Ok(InstanceSummary::new(&inner.data.instances[&id], busy))
    }

    /// Publishes coalesced progress for a running operation. Progress does not
    /// advance the state revision; ignored once the operation has finished.
    pub fn publish_progress(&self, update: ProgressUpdate) {
        let mut guard = self.lock();
        let inner = &mut *guard;
        let data = &mut inner.data;
        if data
            .recent_operations
            .iter()
            .any(|r| r.operation_id == update.operation_id)
        {
            return;
        }
        let previous = data
            .operation_progress
            .as_ref()
            .filter(|p| p.operation_id == update.operation_id);
        let sequence = previous.map_or(1, |p| p.sequence + 1);
        if previous.is_none_or(|p| p.phase != update.phase) {
            let mut text = format!(
                "{} {}: {}",
                console::name(&update.kind),
                short(&update.operation_id),
                console::name(&update.phase)
            );
            if let Some(files) = update.total_files {
                text.push_str(&format!(" ({files} files"));
                if let Some(bytes) = update.total_download_bytes {
                    text.push_str(&format!(", {} to download", mib(bytes)));
                }
                text.push(')');
            }
            self.console.info(text);
        }
        let progress = OperationProgress {
            session_epoch: self.epoch.clone(),
            operation_id: update.operation_id,
            instance_id: update.instance_id,
            kind: update.kind,
            sequence,
            revision: inner.revision,
            phase: update.phase,
            downloaded_bytes: update.downloaded_bytes,
            total_download_bytes: update.total_download_bytes,
            verified_files: update.verified_files,
            total_files: update.total_files,
            bytes_per_second: update.bytes_per_second,
            timestamp: Utc::now(),
        };
        data.operation_progress = Some(progress.clone());
        self.sink
            .emit(&BackendEvent::OperationProgress(Box::new(progress)));
    }

    /// Records the single terminal outcome of an operation and announces it.
    pub fn finish_operation(&self, result: OperationResult) {
        let text = format!(
            "{} {} of {}: {}; downloaded {} file(s) ({}), reused {}",
            console::name(&result.kind),
            short(&result.operation_id),
            self.instance_label(result.instance_id),
            console::name(&result.outcome),
            result.downloaded_files,
            mib(result.network_bytes),
            result.reused_files
        );
        match &result.error {
            Some(error) if result.outcome == Outcome::Failed => self.console.error(format!(
                "{text} — {} ({})",
                error.message,
                console::name(&error.code)
            )),
            _ => self.console.info(text),
        }
        self.publish(
            |data| {
                if data
                    .recent_operations
                    .iter()
                    .any(|r| r.operation_id == result.operation_id)
                {
                    return false;
                }
                if data
                    .operation_progress
                    .as_ref()
                    .is_some_and(|p| p.operation_id == result.operation_id)
                {
                    data.operation_progress = None;
                }
                data.recent_operations.push_front(result.clone());
                data.recent_operations.truncate(RECENT_OPERATIONS);
                true
            },
            Change::OperationFinished(Box::new(result.clone())),
        );
    }

    /// A recorded terminal outcome, if the operation finished recently.
    pub fn operation_result(&self, operation_id: &str) -> Option<OperationResult> {
        self.lock()
            .data
            .recent_operations
            .iter()
            .find(|r| r.operation_id == operation_id)
            .cloned()
    }

    pub fn create_instance(
        &self,
        request: MutationRequest<CreateInstancePayload>,
    ) -> LauncherResult<InstanceSummary> {
        let result = self.create_instance_inner(request);
        match &result {
            Ok(i) => self.console.info(format!(
                "Created installation \"{}\" (Minecraft {}{})",
                i.display_name,
                i.version_id,
                i.loader
                    .as_ref()
                    .map(|l| format!(", {} {}", l.kind.name(), l.version))
                    .unwrap_or_default()
            )),
            Err(e) => self
                .console
                .warn(format!("Creating an installation failed: {}", e.message)),
        }
        result
    }

    fn create_instance_inner(
        &self,
        request: MutationRequest<CreateInstancePayload>,
    ) -> LauncherResult<InstanceSummary> {
        let memory = self.memory.clone();
        self.mutate(
            "create_instance",
            request,
            |data, store, payload, changes| {
                let display_name = validation::validate_display_name(&payload.display_name)?;
                // Any version ID that is safe as a name; installing checks it
                // against Mojang's catalog (D-035).
                let version = crate::storage::paths::SafeName::parse(payload.version_id.trim())
                    .map_err(|_| {
                        LauncherError::invalid_input("This is not a valid Minecraft version.")
                            .with_detail("versionId", payload.version_id.clone())
                    })?;
                if let Some(loader) = &payload.loader {
                    loader.validate()?;
                }
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
                    version_id: version.as_str().to_owned(),
                    manifest_sha1: None,
                    created_at: Utc::now(),
                    last_played_at: None,
                    max_heap_mib,
                    resolution: validation::validate_resolution(data.settings.default_resolution)
                        .unwrap_or_default(),
                    health: InstallationHealth::NotInstalled,
                    last_result: None,
                    java: None,
                    loader: payload.loader.clone(),
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
        let result = self.update_instance_settings_inner(request);
        match &result {
            Ok(i) => self.console.info(format!(
                "Settings of \"{}\": {} MiB, {}×{}",
                i.display_name, i.max_heap_mib, i.resolution.width, i.resolution.height
            )),
            Err(e) => self.console.warn(format!(
                "Changing installation settings failed: {}",
                e.message
            )),
        }
        result
    }

    fn update_instance_settings_inner(
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
        // Selecting an installation is not worth a journal line.
        let mut patch = serde_json::to_value(&request.payload).unwrap_or_default();
        if let Some(fields) = patch.as_object_mut() {
            fields.remove("selectedInstanceId");
        }
        let result = self.set_app_settings_inner(request);
        let changed = patch.as_object().is_some_and(|f| !f.is_empty());
        match &result {
            Ok(_) if changed => self.console.info(format!("Settings: {patch}")),
            Ok(_) => {}
            Err(e) => self
                .console
                .warn(format!("Changing settings failed: {}", e.message)),
        }
        result
    }

    fn set_app_settings_inner(
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
                if let Some(provider) = patch.java_provider {
                    settings.java_provider = provider;
                }
                if let Some(tag) = patch.language {
                    settings.language =
                        crate::storage::models::Language::parse(&tag).ok_or_else(|| {
                            LauncherError::invalid_input("This is not a valid language code.")
                        })?;
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
    Runs,
    Account,
    OperationFinished(Box<OperationResult>),
}

/// Progress fields reported by an operation; the state adds epoch, sequence,
/// revision and time.
#[derive(Debug, Clone)]
pub struct ProgressUpdate {
    pub operation_id: String,
    pub instance_id: Option<InstanceId>,
    pub kind: OperationKind,
    pub phase: InstallState,
    pub downloaded_bytes: u64,
    pub total_download_bytes: Option<u64>,
    pub verified_files: u64,
    pub total_files: Option<u64>,
    pub bytes_per_second: Option<u64>,
}

/// Turns a job interrupted by a crash into a visible, recoverable result.
fn record_interrupted(
    data: &mut Data,
    store: &Store,
    receipt: &crate::download::receipt::JobReceipt,
    issues: &mut Vec<StorageIssue>,
) {
    let finished_at = receipt.finished_at.unwrap_or_else(Utc::now);
    if let Some(id) = receipt.instance_id
        && let Some(instance) = data.instances.get(&id)
    {
        let mut updated = instance.clone();
        updated.last_result = Some(LastResult {
            kind: receipt.kind,
            outcome: Outcome::Interrupted,
            finished_at,
            error_code: Some(ErrorCode::OperationInterrupted),
            exit_code: None,
        });
        // An interrupted repair proves nothing about the files.
        if receipt.kind == OperationKind::Repair && updated.health == InstallationHealth::Ready {
            updated.health = InstallationHealth::NeedsVerification;
        }
        if store.save_instance(&updated).is_err() {
            issues.push(StorageIssue {
                template: None,
                params: std::collections::BTreeMap::new(),
                code: ErrorCode::StorageError,
                message: "An interrupted operation could not be recorded.".into(),
                document: format!("instances/{id}/instance.json"),
            });
        }
        data.instances.insert(id, updated);
    }
    data.recent_operations.push_back(OperationResult {
        operation_id: receipt.job_id.to_string(),
        instance_id: receipt.instance_id,
        kind: receipt.kind,
        outcome: Outcome::Interrupted,
        error: Some(LauncherError::new(
            ErrorCode::OperationInterrupted,
            "The launcher stopped before this operation finished. Verified files were kept; \
             start it again to continue.",
        )),
        finished_at,
        downloaded_files: 0,
        network_bytes: 0,
        reused_files: 0,
    });
    data.recent_operations.truncate(RECENT_OPERATIONS);
}

/// The first block of an operation or run ID, enough to tell them apart.
fn short(id: &str) -> &str {
    id.split('-').next().unwrap_or(id)
}

fn mib(bytes: u64) -> String {
    format!("{:.1} MiB", bytes as f64 / (1024.0 * 1024.0))
}

fn log_run_end(console: &Console, run: &RunSummary) {
    let mut text = format!("Run {}: {}", short(&run.run_id), console::name(&run.state));
    if let Some(code) = run.exit_code {
        text.push_str(&format!(", exit code {code}"));
    }
    if let Some(ms) = run.duration_ms {
        text.push_str(&format!(", ran {:.1} s", ms as f64 / 1000.0));
    }
    if run.logging_degraded {
        text.push_str("; the output log could not be written");
    }
    match &run.error {
        Some(error) => console.error(format!("{text} — {}", error.message)),
        None => console.info(text),
    }
}

fn instance_not_found() -> LauncherError {
    LauncherError::new(
        ErrorCode::InstanceNotFound,
        "The installation no longer exists.",
    )
}

#[cfg(test)]
mod tests;
