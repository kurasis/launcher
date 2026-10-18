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
use super::plan::{self, GamePlan, LibraryLayout, RuntimeRequirement};
use super::rules::RuleContext;
use crate::catalog::source::MetadataService;
use crate::download::receipt::{self, JobReceipt};
use crate::download::transport::HttpTransport;
use crate::download::{
    CancelToken, DownloadPhase, DownloadProgress, DownloadReport, DownloadRequest, Executor,
    ExecutorConfig, ProgressSink,
};
use crate::error::{ErrorCode, LauncherError, LauncherResult};
use crate::loader::manifest::{self, Resolver};
use crate::loader::processors;
use crate::loader::{LoaderChoice, LoaderKind, meta};
use crate::net::{PublicClient, SourcePolicy};
use crate::runtime::adoptium::{Adoptium, RuntimePackage};
use crate::runtime::mojang::Mojang;
use crate::runtime::{InstalledRuntime, RuntimeInstaller};
use crate::state::dto::{InstallState, OperationResult};
use crate::state::{AppState, ProgressUpdate};
use crate::storage::document;
use crate::storage::models::{
    HashAlgorithm, InstallLock, InstallationHealth, InstalledJava, LastResult, LockedLoader,
    OperationKind, Outcome,
};
use crate::storage::paths::{InstanceId, JavaProvider, SafeName, Sha1};
use crate::storage::reconcile::{self, LockState};

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Where plans come from. Production uses Mojang metadata and Mojang or Adoptium
/// runtimes; tests use a local fixture server.
pub trait PlanSource: Send + Sync {
    /// Resolves a new installation: accepted descriptor and a pinned runtime from
    /// the preferred provider.
    fn resolve<'a>(
        &'a self,
        version_id: &'a str,
        loader: Option<&'a LoaderChoice>,
        java: JavaProvider,
    ) -> BoxFuture<'a, LauncherResult<(GamePlan, RuntimePackage)>>;

    /// Re-derives the plan of an installed version from its locked descriptor
    /// (and locked loader manifest), re-verifying (and if needed refetching by
    /// locked URL) the metadata sources.
    fn locked<'a>(&'a self, lock: &'a InstallLock) -> BoxFuture<'a, LauncherResult<GamePlan>>;

    /// Pins a runtime for `requirement` from the preferred provider, for an
    /// installed version whose Java the user switched (D-044).
    fn runtime<'a>(
        &'a self,
        requirement: &'a RuntimeRequirement,
        java: JavaProvider,
    ) -> BoxFuture<'a, LauncherResult<RuntimePackage>>;
}

pub struct LiveSource {
    metadata: Arc<MetadataService<PublicClient>>,
    mojang: Mojang,
    adoptium: Adoptium,
    /// Mod loader hosts and Mojang's artifact hosts, for loader profiles (D-039).
    loaders: PublicClient,
    artifacts: PublicClient,
}

impl LiveSource {
    pub fn new(
        metadata: Arc<MetadataService<PublicClient>>,
        app_version: &str,
    ) -> LauncherResult<Self> {
        Ok(Self {
            metadata,
            mojang: Mojang::new(PublicClient::new(
                SourcePolicy::MojangMetadata,
                app_version,
            )?)?,
            adoptium: Adoptium::new(PublicClient::new(SourcePolicy::Adoptium, app_version)?)?,
            loaders: PublicClient::new(SourcePolicy::ModLoaders, app_version)?,
            artifacts: PublicClient::new(SourcePolicy::MojangArtifacts, app_version)?,
        })
    }

    /// The game plan for a version, with a mod loader merged on top when chosen.
    pub async fn plan_game(
        &self,
        version_id: &str,
        loader: Option<&LoaderChoice>,
    ) -> LauncherResult<GamePlan> {
        let ctx = RuleContext::current()?;
        match loader {
            None => self.metadata.plan(version_id, &ctx).await,
            Some(choice) => {
                let base = self
                    .metadata
                    .plan_in(version_id, &ctx, LibraryLayout::Maven)
                    .await?;
                let (manifest, sha1) = self.resolver().resolve(choice, version_id).await?;
                plan::apply_loader(base, &manifest, &sha1, &ctx)
            }
        }
    }

    pub fn metadata(&self) -> &MetadataService<PublicClient> {
        &self.metadata
    }

    /// Game versions a loader supports (D-039).
    pub async fn loader_game_versions(&self, kind: LoaderKind) -> LauncherResult<Vec<String>> {
        meta::game_versions(&self.loaders, kind).await
    }

    /// Versions of a loader for a game version, newest first (D-039).
    pub async fn loader_versions(
        &self,
        kind: LoaderKind,
        game: &str,
    ) -> LauncherResult<Vec<meta::LoaderVersion>> {
        meta::loader_versions(&self.loaders, kind, game).await
    }

    fn resolver(&self) -> Resolver<'_> {
        Resolver {
            loaders: &self.loaders,
            mojang: &self.artifacts,
            root: self.metadata.root(),
        }
    }
}

impl PlanSource for LiveSource {
    fn resolve<'a>(
        &'a self,
        version_id: &'a str,
        loader: Option<&'a LoaderChoice>,
        java: JavaProvider,
    ) -> BoxFuture<'a, LauncherResult<(GamePlan, RuntimePackage)>> {
        Box::pin(async move {
            let game = self.plan_game(version_id, loader).await?;
            let runtime = self.runtime(&game.runtime, java).await?;
            Ok((game, runtime))
        })
    }

    fn runtime<'a>(
        &'a self,
        requirement: &'a RuntimeRequirement,
        java: JavaProvider,
    ) -> BoxFuture<'a, LauncherResult<RuntimePackage>> {
        Box::pin(async move {
            // Temurin covers what Mojang does not publish for this computer (for
            // example Java 8 on Apple silicon); a failed request is an error.
            let mojang = match java {
                JavaProvider::Mojang => self.mojang.resolve(requirement).await?,
                JavaProvider::Temurin => None,
            };
            match mojang {
                Some(runtime) => Ok(runtime),
                None => self.adoptium.resolve(requirement).await,
            }
        })
    }

    fn locked<'a>(&'a self, lock: &'a InstallLock) -> BoxFuture<'a, LauncherResult<GamePlan>> {
        Box::pin(async move {
            let ctx = RuleContext::current()?;
            let layout = if lock.loader.is_some() {
                LibraryLayout::Maven
            } else {
                LibraryLayout::Shared
            };
            let base = self
                .metadata
                .plan_locked(
                    &lock.version_id,
                    &lock.descriptor.url,
                    &lock.descriptor.sha1,
                    &ctx,
                    layout,
                )
                .await?;
            let Some(locked) = &lock.loader else {
                return Ok(base);
            };
            // The stored manifest, or the same one resolved again.
            let manifest = match manifest::load(self.metadata.root(), &locked.manifest_sha1) {
                Some(manifest) => manifest,
                None => {
                    let (manifest, sha1) = self
                        .resolver()
                        .resolve(&locked.choice(), &lock.version_id)
                        .await?;
                    if sha1 != locked.manifest_sha1 {
                        return Err(LauncherError::new(
                            ErrorCode::RepairRequired,
                            "The mod loader's published files changed since this installation \
                             was made. Create a new installation for this loader version.",
                        ));
                    }
                    manifest
                }
            };
            plan::apply_loader(base, &manifest, &locked.manifest_sha1, &ctx)
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
    /// The request waits in the queue; `operation_id` is its queue ID (D-049).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub queued: bool,
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
    gate: Arc<Gate>,
}

/// The user's answer to a download that waits on a metered connection (D-051).
#[derive(Default)]
struct Gate {
    answer: Mutex<Option<tokio::sync::oneshot::Sender<bool>>>,
    /// Allowed once: the rest of this operation downloads without asking again.
    granted: std::sync::atomic::AtomicBool,
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
            current_files: Vec::new(),
            approval: None,
        }));
        let gate = Arc::new(Gate::default());
        *lock_or(&self.active) = Some(Active {
            operation_id: operation_id.clone(),
            cancel: cancel.clone(),
            progress: Arc::clone(&progress),
            gate: Arc::clone(&gate),
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
            loader: instance.loader,
            cancel,
            progress,
            gate,
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

        let started = OperationStarted {
            operation_id,
            queued: false,
        };
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

    /// Starts an install or repair now, or queues it behind the running
    /// operation or game; queued requests start in order as soon as nothing
    /// else runs ([`Self::start_queued`]).
    pub fn enqueue(
        &self,
        request_id: &str,
        kind: OperationKind,
        instance_id: InstanceId,
    ) -> LauncherResult<OperationStarted> {
        crate::state::ledger::validate_request_id(request_id)?;
        let state = self.host.app_state();
        if let Some((_, started)) = lock_or(&self.requests).seen.get(request_id) {
            return Ok(started.clone());
        }
        let mut queue = state.queue();
        // Offline, a request waits in the queue for the connection (D-052).
        let offline = state.network() == crate::state::dto::NetworkStatus::Offline;
        if queue.is_empty() && state.activity().current().is_none() && !offline {
            return self.start(request_id, kind, instance_id);
        }
        let instance = state.instance(instance_id).ok_or_else(|| {
            LauncherError::new(
                ErrorCode::InstanceNotFound,
                "The installation no longer exists.",
            )
        })?;
        let active_here = state
            .activity()
            .current()
            .is_some_and(|a| a.instance_id == Some(instance_id) && a.kind != OperationKind::Launch);
        if active_here || queue.iter().any(|q| q.instance_id == instance_id) {
            return Err(LauncherError::new(
                ErrorCode::InstanceBusy,
                "This installation is already being installed or waiting in the queue.",
            ));
        }
        let queued = crate::state::dto::QueuedOperation {
            queue_id: Uuid::new_v4().to_string(),
            instance_id,
            kind,
            queued_at: chrono::Utc::now(),
        };
        let started = OperationStarted {
            operation_id: queued.queue_id.clone(),
            queued: true,
        };
        queue.push(queued);
        state.console().info(format!(
            "{} of \"{}\" queued (position {}){}",
            crate::console::name(&kind),
            instance.display_name,
            queue.len(),
            if offline {
                "; it starts when the internet connection returns"
            } else {
                ""
            }
        ));
        state.set_queue(queue);
        lock_or(&self.requests).seen.insert(
            request_id.to_owned(),
            (format!("queue:{instance_id}"), started.clone()),
        );
        Ok(started)
    }

    /// Removes a request from the queue. Returns false if it is not queued.
    pub fn dequeue(&self, queue_id: &str) -> bool {
        let state = self.host.app_state();
        let mut queue = state.queue();
        let before = queue.len();
        queue.retain(|q| q.queue_id != queue_id);
        let removed = queue.len() != before;
        if removed {
            state.console().info("Removed from the download queue");
            state.set_queue(queue);
        }
        removed
    }

    /// Starts the first queued request when nothing else runs. Called
    /// periodically; does nothing while any operation or game holds the permit.
    pub fn start_queued(&self) {
        let state = self.host.app_state();
        if state.network() == crate::state::dto::NetworkStatus::Offline {
            return;
        }
        while state.activity().current().is_none() {
            let mut queue = state.queue();
            if queue.is_empty() {
                return;
            }
            let next = queue.remove(0);
            state.set_queue(queue);
            match self.start(&Uuid::new_v4().to_string(), next.kind, next.instance_id) {
                Ok(_) => return,
                Err(error)
                    if matches!(
                        error.code,
                        ErrorCode::AppBusy
                            | ErrorCode::InstanceBusy
                            | ErrorCode::GameAlreadyRunning
                    ) =>
                {
                    // Something started in between: keep the request first in line.
                    let mut queue = state.queue();
                    queue.insert(0, next);
                    state.set_queue(queue);
                    return;
                }
                Err(error) => state.console().warn(format!(
                    "A queued {} could not start: {}",
                    crate::console::name(&next.kind),
                    error.message
                )),
            }
        }
    }

    /// Allows or declines the download the running operation waits for (D-051).
    pub fn answer_download(&self, operation_id: &str, allow: bool) -> LauncherResult<()> {
        let sender = lock_or(&self.active)
            .as_ref()
            .filter(|active| active.operation_id == operation_id)
            .and_then(|active| lock_or(&active.gate.answer).take());
        match sender {
            Some(sender) => {
                let _ = sender.send(allow);
                Ok(())
            }
            None => Err(LauncherError::new(
                ErrorCode::OperationNotFound,
                "No download is waiting for an answer.",
            )),
        }
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
    gate: &'a Gate,
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
        update.current_files.clone_from(&p.current_files);
        self.host.app_state().publish_progress(update.clone());
    }

    fn corrupt(&self, path: &str) {
        lock_or(&self.corrupt).insert(path.to_owned());
    }

    /// On a metered connection, a download that reaches the warning size or
    /// would pass the monthly limit waits until the user allows or declines it.
    fn approve(&self, bytes: u64, files: u64) -> crate::download::Approval<'_> {
        Box::pin(async move {
            use std::sync::atomic::Ordering;
            if self.gate.granted.load(Ordering::Relaxed) {
                return true;
            }
            let state = self.host.app_state();
            let settings = state.settings();
            let month_bytes = state
                .data_root()
                .map_or(0, |root| crate::traffic::month_total(&root));
            let Some(reason) = crate::traffic::caution(
                settings.metered,
                settings.metered_warn_mib,
                settings.monthly_limit_mib,
                month_bytes,
                bytes,
            ) else {
                return true;
            };
            let (sender, receiver) = tokio::sync::oneshot::channel();
            *lock_or(&self.gate.answer) = Some(sender);
            {
                let mut update = lock_or(self.progress);
                update.phase = InstallState::AwaitingApproval;
                update.total_download_bytes = Some(bytes);
                update.approval = Some(crate::state::dto::ApprovalRequest {
                    bytes,
                    files,
                    reason,
                    month_bytes,
                    monthly_limit_mib: settings.monthly_limit_mib,
                });
                state.publish_progress(update.clone());
            }
            state.console().info(format!(
                "Metered connection: waiting for permission to download {} in {files} file(s)",
                crate::state::mib(bytes)
            ));
            let approved = tokio::select! {
                answer = receiver => answer.unwrap_or(false),
                () = self.cancel.cancelled() => false,
            };
            *lock_or(&self.gate.answer) = None;
            {
                let mut update = lock_or(self.progress);
                update.approval = None;
                update.phase = if approved {
                    InstallState::Downloading
                } else {
                    InstallState::CancelRequested
                };
                state.publish_progress(update.clone());
            }
            state.console().info(if approved {
                "Download allowed"
            } else {
                "Download declined"
            });
            self.gate.granted.store(approved, Ordering::Relaxed);
            approved
        })
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
    loader: Option<LoaderChoice>,
    cancel: CancelToken,
    progress: Arc<Mutex<ProgressUpdate>>,
    gate: Arc<Gate>,
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
        crate::download::label(artifact.role, artifact.provenance.as_deref()),
    )
}

/// A game or mod loader artifact download. The production transport enforces
/// the source allowlist on the URL and every redirect; the planner checked it
/// already.
fn artifact_request(
    url: &str,
    path: &str,
    size: u64,
    digest: crate::download::Digest,
    label: Option<String>,
) -> LauncherResult<DownloadRequest> {
    // Mojang's category unless a loader host is named; the transport refuses
    // anything off the category's allowlist.
    let source = SourcePolicy::for_artifact(url).map_or(SourcePolicy::MojangArtifacts, |(p, _)| p);
    Ok(DownloadRequest {
        source,
        url: url::Url::parse(url).map_err(|_| {
            LauncherError::new(
                ErrorCode::UnsupportedVersionMetadata,
                "A download location is invalid.",
            )
        })?,
        path: path.to_owned(),
        size,
        digest,
        label,
    })
}

/// What a finished run produced, for bookkeeping.
#[derive(Default)]
struct RunReport {
    downloads: DownloadReport,
    corrupt: BTreeSet<String>,
    /// Descriptor hash of the committed (or verified) lock.
    committed: Option<Sha1>,
    java: Option<InstalledJava>,
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
            gate: &self.gate,
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
                if lock.instance_id == self.instance_id
                    && lock.version_id == self.version_id
                    && lock.loader.as_ref().map(LockedLoader::choice) == self.loader =>
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
            Some(plan)
                if plan.game.version_id == self.version_id
                    && plan.game.loader.as_ref().map(|l| LoaderChoice {
                        kind: l.kind,
                        version: l.version.clone(),
                    }) == self.loader =>
            {
                plan
            }
            _ => {
                // A provider chosen for this installation, else the default.
                let java = self
                    .state()
                    .instance(self.instance_id)
                    .and_then(|i| i.java_provider)
                    .unwrap_or_else(|| self.state().java_provider());
                let (game, runtime) = tokio::select! {
                    resolved = self.source.resolve(&self.version_id, self.loader.as_ref(), java) => resolved?,
                    () = self.cancel.cancelled() => return Err(cancelled()),
                };
                let console = self.state().console();
                console.info(format!(
                    "Minecraft {}: {} libraries, {} asset objects; Java {} from {} ({}, {})",
                    game.version_id,
                    game.libraries.len(),
                    game.assets.len(),
                    runtime.key.major,
                    runtime.provider,
                    runtime.release_name,
                    runtime.archive_name
                ));
                if let Some(loader) = &game.loader {
                    console.info(format!(
                        "{} {} on Minecraft {}: {} files from the loader",
                        loader.kind.name(),
                        loader.version,
                        game.version_id,
                        game.loader_files.len() + loader.provided.len()
                    ));
                }
                if java == JavaProvider::Mojang && runtime.key.provider != JavaProvider::Mojang {
                    console.warn(format!(
                        "Mojang publishes no Java {} for this computer; using Temurin",
                        game.runtime.major
                    ));
                }
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
                    crate::download::label(a.role, a.provenance.as_deref()),
                )
            })
            .collect::<LauncherResult<Vec<_>>>()?;
        let installed = self
            .files_and_runtime(requests, &plan.runtime, report)
            .await?;
        let generated = self.prepare_loader(&plan.game, &installed).await?;
        self.phase(InstallState::Finalizing);
        lock::project_asset_index(
            &self.root,
            &plan.game.asset_index.id,
            &plan.game.asset_index.artifact.sha1,
        )?;
        self.check_cancel()?;
        // Commit point: from here on the result is success.
        let mut lock = lock::build_lock(plan, installed.locked());
        if let Some(loader) = &mut lock.loader {
            loader.generated = generated;
        }
        document::save(&self.root.install_lock(self.instance_id), &lock, None)
            .map_err(|e| LauncherError::from_io(&e, "Saving the installation record"))?;
        report.committed = Some(lock.descriptor.sha1);
        report.java = InstalledJava::from_lock(&lock.runtime);
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
        let locked_runtime = lock::runtime_from_lock(lock)?;
        let derived = lock::build_lock(
            &FinalInstallPlan {
                instance_id: self.instance_id,
                game: game.clone(),
                runtime: locked_runtime.clone(),
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
        // The user chose another provider for the private runtime (D-044): pin
        // a new runtime; everything else stays as locked.
        let switch = self
            .state()
            .instance(self.instance_id)
            .and_then(|i| i.java_provider)
            .filter(|p| p.as_str() != lock.runtime.provider);
        let runtime = match switch {
            None => locked_runtime,
            Some(java) => {
                let runtime = tokio::select! {
                    runtime = self.source.runtime(&game.runtime, java) => runtime?,
                    () = self.cancel.cancelled() => return Err(cancelled()),
                };
                let console = self.state().console();
                console.info(format!(
                    "Switching Java to {} {} ({})",
                    runtime.provider, runtime.key.major, runtime.release_name
                ));
                if java == JavaProvider::Mojang && runtime.key.provider != JavaProvider::Mojang {
                    console.warn(format!(
                        "Mojang publishes no Java {} for this computer; using Temurin",
                        game.runtime.major
                    ));
                }
                runtime
            }
        };
        let installed = self.files_and_runtime(requests, &runtime, report).await?;
        if switch.is_none()
            && installed.receipt.tree_receipt_sha256 != lock.runtime.tree_receipt_sha256
        {
            return Err(LauncherError::new(
                ErrorCode::RuntimeMismatch,
                "The rebuilt Java runtime differs from the recorded one.",
            ));
        }
        if let Some(locked) = &lock.loader {
            let (root, generated) = (self.root.clone(), locked.generated.clone());
            let intact = tokio::task::spawn_blocking(move || {
                processors::verify_generated(&root, &generated)
            })
            .await
            .map_err(|_| LauncherError::internal("A file check stopped unexpectedly."))?;
            if !intact {
                // Rebuilt by the same installer from the same inputs.
                self.prepare_loader(&game, &installed).await?;
                if !processors::verify_generated(&self.root, &locked.generated) {
                    return Err(LauncherError::new(
                        ErrorCode::RepairRequired,
                        "The rebuilt mod loader files differ from the recorded ones. Create a \
                         new installation for this loader version.",
                    ));
                }
            }
        }
        self.phase(InstallState::Finalizing);
        let index_id = SafeName::parse(&lock.asset_index.id)?;
        lock::project_asset_index(&self.root, &index_id, &lock.asset_index.sha1)?;
        if switch.is_some() {
            self.check_cancel()?;
            // Commit point of the switch: the lock records the new runtime.
            let mut switched = lock.clone();
            switched.runtime = installed.locked();
            switched.validated_at = Utc::now();
            document::save(&self.root.install_lock(self.instance_id), &switched, None)
                .map_err(|e| LauncherError::from_io(&e, "Saving the installation record"))?;
            report.java = InstalledJava::from_lock(&switched.runtime);
        } else {
            report.java = InstalledJava::from_lock(&lock.runtime);
        }
        report.committed = Some(lock.descriptor.sha1.clone());
        Ok(())
    }

    /// Runs a Forge or NeoForge installer step after the downloads (D-039) and
    /// returns the files it produced. Nothing to do for other plans.
    async fn prepare_loader(
        &self,
        game: &GamePlan,
        installed: &InstalledRuntime,
    ) -> LauncherResult<Vec<crate::storage::models::LockedArtifact>> {
        let Some(loader) = &game.loader else {
            return Ok(Vec::new());
        };
        if !loader.kind.uses_installer() {
            return Ok(Vec::new());
        }
        self.phase(InstallState::PreparingLoader);
        let manifest = manifest::load(&self.root, &loader.manifest_sha1).ok_or_else(|| {
            LauncherError::new(
                ErrorCode::RepairRequired,
                "The mod loader record is missing. Repair the installation.",
            )
        })?;
        let console = self.state().console();
        console.info(format!(
            "Running the {} {} installer",
            loader.kind.name(),
            loader.version
        ));
        let (root, game, java, cancel) = (
            self.root.clone(),
            game.clone(),
            installed.java.clone(),
            self.cancel.clone(),
        );
        let work_dir = self.root.download_staging(self.job).join("loader");
        let host = Arc::clone(&self.host);
        let result = tokio::task::spawn_blocking(move || {
            let log = move |line: String| host.app_state().console().info(line);
            processors::prepare(&processors::Inputs {
                root: &root,
                manifest: &manifest,
                plan: &game,
                java: &java,
                work_dir,
                cancel: &cancel,
                log: &log,
            })
        })
        .await
        .map_err(|_| LauncherError::internal("The mod loader installer stopped unexpectedly."))?;
        match &result {
            Ok(files) => console.info(format!(
                "{} installer finished: {} files verified",
                loader.kind.name(),
                files.len()
            )),
            Err(error) => console.warn(format!(
                "{} installer failed: {}",
                loader.kind.name(),
                error.message
            )),
        }
        self.check_cancel()?;
        result
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
        let java = report.java.clone();
        let _ = state.update_instance(self.instance_id, |instance| {
            if let Some(sha1) = descriptor {
                instance.health = InstallationHealth::Ready;
                instance.manifest_sha1 = Some(sha1);
                instance.java = java;
                // The lock now records the runtime the user chose.
                instance.java_provider = None;
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
