//! Install and repair operations (docs/spec/04 "Explicit install-operation state
//! machine", doc 10 "Commit and recovery rules").
//!
//! One operation runs at a time under the global activity permit. A new install
//! resolves its plan once and freezes it in the job receipt; a retry after a
//! cancel, failure or crash reuses that plan and every file already verified.
//! Commit order is: artifacts and runtime tree → `install.lock.json` → instance
//! summary. Repair uses the locked artifact list and the pinned runtime package,
//! never fresh catalog data, and never touches the instance's game directory.

use std::collections::{BTreeSet, HashMap, VecDeque};
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex, PoisonError};

use chrono::Utc;
use serde::Serialize;
use uuid::Uuid;

use super::lock::{self, FinalInstallPlan};
use super::plan::GamePlan;
use super::rules::RuleContext;
use crate::catalog::source::MetadataService;
use crate::download::receipt::{self, JobReceipt};
use crate::download::transport::HttpTransport;
use crate::download::{
    CancelToken, DownloadPhase, DownloadProgress, DownloadReport, DownloadRequest, Executor,
    ExecutorConfig, ProgressSink,
};
use crate::error::{ErrorCode, LauncherError, LauncherResult};
use crate::net::{PublicClient, SourcePolicy};
use crate::runtime::adoptium::{Adoptium, RuntimePackage};
use crate::runtime::{InstalledRuntime, RuntimeInstaller};
use crate::state::dto::{InstallState, OperationResult};
use crate::state::{AppState, ProgressUpdate};
use crate::storage::document;
use crate::storage::models::{
    HashAlgorithm, InstallLock, InstallationHealth, LastResult, OperationKind, Outcome,
};
use crate::storage::paths::{InstanceId, SafeName, Sha1};
use crate::storage::reconcile::{self, LockState};

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Where plans come from. Production uses Mojang metadata and Adoptium; tests use
/// a local fixture server.
pub trait PlanSource: Send + Sync {
    /// Resolves a new installation: accepted descriptor and a pinned runtime.
    fn resolve<'a>(
        &'a self,
        version_id: &'a str,
    ) -> BoxFuture<'a, LauncherResult<(GamePlan, RuntimePackage)>>;

    /// Re-derives the plan of an installed version from its locked descriptor,
    /// re-verifying (and if needed refetching by locked URL) the metadata sources.
    fn locked<'a>(&'a self, lock: &'a InstallLock) -> BoxFuture<'a, LauncherResult<GamePlan>>;
}

pub struct LiveSource {
    metadata: Arc<MetadataService<PublicClient>>,
    adoptium: Adoptium,
}

impl LiveSource {
    pub fn new(
        metadata: Arc<MetadataService<PublicClient>>,
        app_version: &str,
    ) -> LauncherResult<Self> {
        Ok(Self {
            metadata,
            adoptium: Adoptium::new(PublicClient::new(SourcePolicy::Adoptium, app_version)?)?,
        })
    }
}

impl PlanSource for LiveSource {
    fn resolve<'a>(
        &'a self,
        version_id: &'a str,
    ) -> BoxFuture<'a, LauncherResult<(GamePlan, RuntimePackage)>> {
        Box::pin(async move {
            let ctx = RuleContext::current()?;
            let game = self.metadata.plan(version_id, &ctx).await?;
            let runtime = self.adoptium.resolve(&game.runtime).await?;
            Ok((game, runtime))
        })
    }

    fn locked<'a>(&'a self, lock: &'a InstallLock) -> BoxFuture<'a, LauncherResult<GamePlan>> {
        Box::pin(async move {
            let ctx = RuleContext::current()?;
            self.metadata
                .plan_locked(
                    &lock.version_id,
                    &lock.descriptor.url,
                    &lock.descriptor.sha1,
                    &ctx,
                )
                .await
        })
    }
}

/// Access to the application state from background tasks.
pub trait StateAccess: Send + Sync + 'static {
    fn app_state(&self) -> &AppState;

    /// An install or repair recorded its result; the UI host may need to show it.
    fn operation_finished(&self) {}
}

impl StateAccess for Arc<AppState> {
    fn app_state(&self) -> &AppState {
        self
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OperationStarted {
    pub operation_id: String,
}

/// Answer to `cancel_operation`.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CancelAck {
    /// `cancel_requested` while running, otherwise the recorded outcome.
    pub state: String,
}

struct Active {
    operation_id: String,
    cancel: CancelToken,
    progress: Arc<Mutex<ProgressUpdate>>,
}

/// Remembers accepted request IDs so a retried IPC call returns the same operation.
#[derive(Default)]
struct Requests {
    order: VecDeque<String>,
    seen: HashMap<String, (String, OperationStarted)>,
}

const REMEMBERED_REQUESTS: usize = 256;

pub struct InstallService {
    host: Arc<dyn StateAccess>,
    source: Arc<dyn PlanSource>,
    transport: HttpTransport,
    config: ExecutorConfig,
    active: Arc<Mutex<Option<Active>>>,
    requests: Mutex<Requests>,
}

fn lock_or<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl InstallService {
    pub fn new(
        host: Arc<dyn StateAccess>,
        source: Arc<dyn PlanSource>,
        transport: HttpTransport,
        config: ExecutorConfig,
    ) -> Self {
        Self {
            host,
            source,
            transport,
            config,
            active: Arc::default(),
            requests: Mutex::default(),
        }
    }

    /// Accepts an install or repair request and runs it in the background.
    pub fn start(
        &self,
        request_id: &str,
        kind: OperationKind,
        instance_id: InstanceId,
    ) -> LauncherResult<OperationStarted> {
        crate::state::ledger::validate_request_id(request_id)?;
        let fingerprint = format!("{kind:?}:{instance_id}");
        {
            let requests = lock_or(&self.requests);
            if let Some((seen, started)) = requests.seen.get(request_id) {
                return if *seen == fingerprint {
                    Ok(started.clone())
                } else {
                    Err(LauncherError::new(
                        ErrorCode::RequestIdConflict,
                        "This request ID was already used for another request.",
                    ))
                };
            }
        }
        let state = self.host.app_state();
        let instance = state.instance(instance_id).ok_or_else(|| {
            LauncherError::new(
                ErrorCode::InstanceNotFound,
                "The installation no longer exists.",
            )
        })?;
        if instance.health == InstallationHealth::Unsupported {
            return Err(LauncherError::new(
                ErrorCode::UnsupportedSchema,
                "This installation was created by a newer launcher and is read-only here.",
            ));
        }
        let root = state.data_root().ok_or_else(|| {
            LauncherError::new(
                ErrorCode::StorageUnavailable,
                "Launcher data is unavailable.",
            )
        })?;
        let job = Uuid::new_v4();
        let operation_id = job.to_string();
        let permit = state
            .activity()
            .try_acquire(kind, Some(instance_id), operation_id.clone())?;
        let cancel = CancelToken::new();
        let progress = Arc::new(Mutex::new(ProgressUpdate {
            operation_id: operation_id.clone(),
            instance_id: Some(instance_id),
            kind,
            phase: InstallState::Resolving,
            downloaded_bytes: 0,
            total_download_bytes: None,
            verified_files: 0,
            total_files: None,
            bytes_per_second: None,
        }));
        *lock_or(&self.active) = Some(Active {
            operation_id: operation_id.clone(),
            cancel: cancel.clone(),
            progress: Arc::clone(&progress),
        });
        state.announce_runs();
        state.publish_progress(lock_or(&progress).clone());

        let operation = Operation {
            host: Arc::clone(&self.host),
            source: Arc::clone(&self.source),
            executor: Executor::new(self.transport.clone(), root.clone(), self.config),
            runtime: RuntimeInstaller::new(root.clone(), self.transport.clone(), self.config),
            root,
            job,
            kind,
            instance_id,
            version_id: instance.version_id,
            cancel,
            progress,
        };
        let active = Arc::clone(&self.active);
        tokio::spawn(async move {
            let (outcome, report) = operation.run().await;
            // Release the permit before announcing the result so the announced
            // active operation is the truth.
            drop(permit);
            *lock_or(&active) = None;
            operation.finish(outcome, report);
        });

        let started = OperationStarted { operation_id };
        let mut requests = lock_or(&self.requests);
        requests.order.push_back(request_id.to_owned());
        requests
            .seen
            .insert(request_id.to_owned(), (fingerprint, started.clone()));
        while requests.order.len() > REMEMBERED_REQUESTS {
            if let Some(old) = requests.order.pop_front() {
                requests.seen.remove(&old);
            }
        }
        Ok(started)
    }

    /// Requests cooperative cancellation, or reports the recorded outcome.
    pub fn cancel(&self, operation_id: &str) -> LauncherResult<CancelAck> {
        let state = self.host.app_state();
        if let Some(active) = lock_or(&self.active).as_ref()
            && active.operation_id == operation_id
        {
            active.cancel.cancel();
            let mut update = lock_or(&active.progress).clone();
            update.phase = InstallState::CancelRequested;
            state.publish_progress(update);
            return Ok(CancelAck {
                state: "cancel_requested".into(),
            });
        }
        if let Some(result) = state.operation_result(operation_id) {
            let outcome = serde_json::to_value(result.outcome)
                .ok()
                .and_then(|v| v.as_str().map(str::to_owned))
                .unwrap_or_default();
            return Ok(CancelAck { state: outcome });
        }
        Err(LauncherError::new(
            ErrorCode::OperationNotFound,
            "This operation is not running.",
        ))
    }
}

/// Forwards executor progress to the app state, tagged with the install phase,
/// and collects files found corrupt.
struct Forward<'a> {
    host: &'a dyn StateAccess,
    progress: &'a Mutex<ProgressUpdate>,
    cancel: &'a CancelToken,
    stage: InstallState,
    corrupt: Mutex<BTreeSet<String>>,
}

impl ProgressSink for Forward<'_> {
    fn progress(&self, p: &DownloadProgress) {
        let mut update = lock_or(self.progress);
        update.phase = if self.cancel.is_cancelled() {
            InstallState::CancelRequested
        } else if self.stage == InstallState::ExtractingRuntime {
            self.stage
        } else {
            match p.phase {
                DownloadPhase::CheckingFiles => InstallState::CheckingFiles,
                DownloadPhase::Downloading => InstallState::Downloading,
            }
        };
        update.downloaded_bytes = p.downloaded_bytes;
        update.total_download_bytes = Some(p.total_download_bytes);
        update.verified_files = p.verified_files;
        update.total_files = Some(p.total_files);
        update.bytes_per_second = p.bytes_per_second;
        self.host.app_state().publish_progress(update.clone());
    }

    fn corrupt(&self, path: &str) {
        lock_or(&self.corrupt).insert(path.to_owned());
    }
}

struct Operation {
    host: Arc<dyn StateAccess>,
    source: Arc<dyn PlanSource>,
    executor: Executor,
    runtime: RuntimeInstaller,
    root: crate::storage::paths::DataRoot,
    job: Uuid,
    kind: OperationKind,
    instance_id: InstanceId,
    version_id: String,
    cancel: CancelToken,
    progress: Arc<Mutex<ProgressUpdate>>,
}

fn cancelled() -> LauncherError {
    LauncherError::new(
        ErrorCode::OperationCancelled,
        "The operation was cancelled.",
    )
}

fn request_from_locked(
    artifact: &crate::storage::models::LockedArtifact,
) -> LauncherResult<DownloadRequest> {
    if artifact.algorithm != HashAlgorithm::Sha1 {
        return Err(LauncherError::new(
            ErrorCode::RepairRequired,
            "The installation record lists an unsupported checksum.",
        ));
    }
    artifact_request(
        &artifact.url,
        &artifact.path,
        artifact.size,
        crate::download::Digest::Sha1(Sha1::parse(&artifact.digest)?),
    )
}

/// A Mojang artifact download. The production transport enforces the source
/// allowlist on the URL and every redirect; the planner checked it already.
fn artifact_request(
    url: &str,
    path: &str,
    size: u64,
    digest: crate::download::Digest,
) -> LauncherResult<DownloadRequest> {
    Ok(DownloadRequest {
        source: SourcePolicy::MojangArtifacts,
        url: url::Url::parse(url).map_err(|_| {
            LauncherError::new(
                ErrorCode::UnsupportedVersionMetadata,
                "A download location is invalid.",
            )
        })?,
        path: path.to_owned(),
        size,
        digest,
    })
}

/// What a finished run produced, for bookkeeping.
#[derive(Default)]
struct RunReport {
    downloads: DownloadReport,
    corrupt: BTreeSet<String>,
    /// Descriptor hash of the committed (or verified) lock.
    committed: Option<Sha1>,
}

impl Operation {
    fn state(&self) -> &AppState {
        self.host.app_state()
    }

    fn phase(&self, phase: InstallState) {
        let mut update = lock_or(&self.progress);
        update.phase = if self.cancel.is_cancelled() {
            InstallState::CancelRequested
        } else {
            phase
        };
        self.state().publish_progress(update.clone());
    }

    fn sink(&self, stage: InstallState) -> Forward<'_> {
        Forward {
            host: self.host.as_ref(),
            progress: &self.progress,
            cancel: &self.cancel,
            stage,
            corrupt: Mutex::default(),
        }
    }

    fn check_cancel(&self) -> LauncherResult<()> {
        if self.cancel.is_cancelled() {
            Err(cancelled())
        } else {
            Ok(())
        }
    }

    async fn run(&self) -> (LauncherResult<()>, RunReport) {
        let mut report = RunReport::default();
        let result = self.run_inner(&mut report).await;
        (result, report)
    }

    async fn run_inner(&self, report: &mut RunReport) -> LauncherResult<()> {
        let lock_state = reconcile::read_lock(&self.root.install_lock(self.instance_id))
            .map_err(|e| LauncherError::from_io(&e, "Reading the installation record"))?;
        match lock_state {
            LockState::Complete(lock)
                if lock.instance_id == self.instance_id && lock.version_id == self.version_id =>
            {
                self.repair(*lock, report).await
            }
            LockState::UnsupportedSchema => Err(LauncherError::new(
                ErrorCode::UnsupportedSchema,
                "This installation record was written by a newer launcher.",
            )),
            // Missing, incomplete or unreadable: install from the (frozen) plan.
            _ => self.install(report).await,
        }
    }

    async fn install(&self, report: &mut RunReport) -> LauncherResult<()> {
        self.phase(InstallState::Resolving);
        let plan = match receipt::resumable_plan(&self.root, self.instance_id)? {
            Some(plan) if plan.game.version_id == self.version_id => plan,
            _ => {
                let (game, runtime) = tokio::select! {
                    resolved = self.source.resolve(&self.version_id) => resolved?,
                    () = self.cancel.cancelled() => return Err(cancelled()),
                };
                FinalInstallPlan {
                    instance_id: self.instance_id,
                    game,
                    runtime,
                }
            }
        };
        let mut job = JobReceipt::start(
            &self.root,
            self.job,
            self.kind,
            Some(self.instance_id),
            Some(plan.clone()),
        )?;
        let result = self.install_plan(&plan, report).await;
        let _ = job.finish(
            &self.root,
            &result
                .as_ref()
                .map(|()| report.downloads.clone())
                .map_err(Clone::clone),
        );
        result
    }

    async fn install_plan(
        &self,
        plan: &FinalInstallPlan,
        report: &mut RunReport,
    ) -> LauncherResult<()> {
        let requests = plan
            .game
            .artifacts()
            .map(|a| {
                artifact_request(
                    &a.url,
                    &a.path,
                    a.size,
                    crate::download::Digest::Sha1(a.sha1.clone()),
                )
            })
            .collect::<LauncherResult<Vec<_>>>()?;
        let installed = self
            .files_and_runtime(requests, &plan.runtime, report)
            .await?;
        self.phase(InstallState::Finalizing);
        lock::project_asset_index(
            &self.root,
            &plan.game.asset_index.id,
            &plan.game.asset_index.artifact.sha1,
        )?;
        self.check_cancel()?;
        // Commit point: from here on the result is success.
        let lock = lock::build_lock(plan, installed.locked());
        document::save(&self.root.install_lock(self.instance_id), &lock, None)
            .map_err(|e| LauncherError::from_io(&e, "Saving the installation record"))?;
        report.committed = Some(lock.descriptor.sha1);
        Ok(())
    }

    async fn repair(&self, lock: InstallLock, report: &mut RunReport) -> LauncherResult<()> {
        // Until every check passes, the old lock proves nothing.
        self.state().update_instance(self.instance_id, |i| {
            i.health = InstallationHealth::NeedsVerification;
        })?;
        let mut job = JobReceipt::start(
            &self.root,
            self.job,
            self.kind,
            Some(self.instance_id),
            None,
        )?;
        let result = self.repair_locked(&lock, report).await;
        let _ = job.finish(
            &self.root,
            &result
                .as_ref()
                .map(|()| report.downloads.clone())
                .map_err(Clone::clone),
        );
        result
    }

    async fn repair_locked(
        &self,
        lock: &InstallLock,
        report: &mut RunReport,
    ) -> LauncherResult<()> {
        self.phase(InstallState::Resolving);
        // Original metadata bytes, by locked hash; the plan must match the lock.
        let game = tokio::select! {
            game = self.source.locked(lock) => game?,
            () = self.cancel.cancelled() => return Err(cancelled()),
        };
        let runtime = lock::runtime_from_lock(lock)?;
        let derived = lock::build_lock(
            &FinalInstallPlan {
                instance_id: self.instance_id,
                game: game.clone(),
                runtime: runtime.clone(),
            },
            lock.runtime.clone(),
        );
        if derived.artifacts != lock.artifacts || derived.classpath != lock.classpath {
            return Err(LauncherError::new(
                ErrorCode::RepairRequired,
                "The installation record does not match its version metadata. Create a new \
                 installation for this version.",
            ));
        }
        let requests = lock
            .artifacts
            .iter()
            .map(request_from_locked)
            .collect::<LauncherResult<Vec<_>>>()?;
        let installed = self.files_and_runtime(requests, &runtime, report).await?;
        if installed.receipt.tree_receipt_sha256 != lock.runtime.tree_receipt_sha256 {
            return Err(LauncherError::new(
                ErrorCode::RuntimeMismatch,
                "The rebuilt Java runtime differs from the recorded one.",
            ));
        }
        self.phase(InstallState::Finalizing);
        let index_id = SafeName::parse(&lock.asset_index.id)?;
        lock::project_asset_index(&self.root, &index_id, &lock.asset_index.sha1)?;
        report.committed = Some(lock.descriptor.sha1.clone());
        Ok(())
    }

    /// Verifies or downloads game files, then installs or verifies the runtime.
    async fn files_and_runtime(
        &self,
        requests: Vec<DownloadRequest>,
        runtime: &RuntimePackage,
        report: &mut RunReport,
    ) -> LauncherResult<InstalledRuntime> {
        self.phase(InstallState::CheckingFiles);
        let sink = self.sink(InstallState::CheckingFiles);
        let result = self
            .executor
            .run(self.job, requests, &self.cancel, &sink)
            .await;
        report
            .corrupt
            .extend(lock_or(&sink.corrupt).iter().cloned());
        report.downloads = result?;
        self.check_cancel()?;

        self.phase(InstallState::ExtractingRuntime);
        let sink = self.sink(InstallState::ExtractingRuntime);
        let installed = self
            .runtime
            .ensure(runtime, self.job, &self.cancel, &sink)
            .await?;
        if let Some(archive) = &installed.archive_download {
            report.downloads.downloaded_files += archive.downloaded_files;
            report.downloads.network_bytes += archive.network_bytes;
            report.downloads.committed_bytes += archive.committed_bytes;
        }
        self.check_cancel()?;
        Ok(installed)
    }

    /// Records the one terminal outcome on the instance and in the app state.
    fn finish(&self, outcome: LauncherResult<()>, report: RunReport) {
        let state = self.state();
        let (result_outcome, error) = match &outcome {
            Ok(()) => (Outcome::Succeeded, None),
            Err(e) if e.code == ErrorCode::OperationCancelled => {
                (Outcome::Cancelled, Some(e.clone()))
            }
            Err(e) => (Outcome::Failed, Some(e.clone())),
        };
        let finished_at = Utc::now();
        let error_code = error.as_ref().map(|e| e.code);
        let committed = report.committed.is_some();
        let found_corrupt = !report.corrupt.is_empty();
        let kind = self.kind;
        let installed_before = !committed && self.root.install_lock(self.instance_id).exists();
        let descriptor = report.committed.clone();
        let _ = state.update_instance(self.instance_id, |instance| {
            if let Some(sha1) = descriptor {
                instance.health = InstallationHealth::Ready;
                instance.manifest_sha1 = Some(sha1);
            } else if installed_before {
                instance.health = if found_corrupt {
                    InstallationHealth::RepairRequired
                } else {
                    InstallationHealth::NeedsVerification
                };
            }
            instance.last_result = Some(LastResult {
                kind,
                outcome: result_outcome,
                finished_at,
                error_code,
                exit_code: None,
            });
        });
        // Shared content found corrupt and not replaced invalidates every other
        // installation that uses it (doc 10 rule 4).
        if !committed && found_corrupt {
            self.invalidate_users_of(&report.corrupt);
        }
        state.finish_operation(OperationResult {
            operation_id: self.job.to_string(),
            instance_id: Some(self.instance_id),
            kind,
            outcome: result_outcome,
            error,
            finished_at,
            downloaded_files: report.downloads.downloaded_files,
            network_bytes: report.downloads.network_bytes,
            reused_files: report.downloads.reused_files,
        });
        self.host.operation_finished();
    }

    fn invalidate_users_of(&self, corrupt: &BTreeSet<String>) {
        let state = self.state();
        for id in state.instance_ids() {
            if id == self.instance_id {
                continue;
            }
            let Ok(LockState::Complete(lock)) = reconcile::read_lock(&self.root.install_lock(id))
            else {
                continue;
            };
            if lock.artifacts.iter().any(|a| corrupt.contains(&a.path)) {
                let _ = state.update_instance(id, |i| {
                    if i.health == InstallationHealth::Ready {
                        i.health = InstallationHealth::RepairRequired;
                    }
                });
            }
        }
    }
}

#[cfg(test)]
#[cfg(unix)]
pub(crate) mod tests;
