//! Verified, cancellable artifact downloads (docs/spec/04 "Download executor",
//! doc 10 "Sharing is measured, not inferred").
//!
//! - Requests are deduplicated by canonical path; conflicting metadata for one
//!   path or digest is `METADATA_CONFLICT`.
//! - Existing files are reused only after their size and digest are verified.
//! - Bytes stream to a job-owned `.part` file while being hashed; a file is
//!   renamed into place only after its size and digest match. Commit is the one
//!   short step that cancellation does not interrupt.
//! - At most `concurrency` downloads run at once; transient failures are retried
//!   a bounded number of times with jittered backoff or `Retry-After`; a digest
//!   mismatch is retried once from scratch.
//! - Progress is aggregated and reported at most every `progress_interval`.

pub mod cancel;
pub mod receipt;
pub mod transport;

#[cfg(test)]
pub(crate) mod tests;

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use sha1::Digest as _;
use tokio::io::AsyncWriteExt;
use tokio::task::JoinSet;
use url::Url;
use uuid::Uuid;

use crate::error::{ErrorCode, LauncherError, LauncherResult};
use crate::install::plan::PlannedArtifact;
use crate::net::SourcePolicy;
use crate::storage::models::{ArtifactRole, HashAlgorithm};
use crate::storage::paths::{DataRoot, Sha1, Sha256, is_within};
pub use cancel::CancelToken;
use transport::{Failure, HttpTransport};

/// Expected content digest.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Digest {
    Sha1(Sha1),
    Sha256(Sha256),
}

impl Digest {
    pub fn algorithm(&self) -> HashAlgorithm {
        match self {
            Self::Sha1(_) => HashAlgorithm::Sha1,
            Self::Sha256(_) => HashAlgorithm::Sha256,
        }
    }

    pub fn hex(&self) -> &str {
        match self {
            Self::Sha1(d) => d.as_str(),
            Self::Sha256(d) => d.as_str(),
        }
    }

    fn label(&self) -> &'static str {
        match self {
            Self::Sha1(_) => "sha1",
            Self::Sha256(_) => "sha256",
        }
    }
}

enum Hasher {
    Sha1(sha1::Sha1),
    Sha256(sha2::Sha256),
}

impl Hasher {
    fn new(digest: &Digest) -> Self {
        match digest {
            Digest::Sha1(_) => Self::Sha1(sha1::Sha1::new()),
            Digest::Sha256(_) => Self::Sha256(sha2::Sha256::new()),
        }
    }

    fn update(&mut self, bytes: &[u8]) {
        match self {
            Self::Sha1(h) => h.update(bytes),
            Self::Sha256(h) => h.update(bytes),
        }
    }

    fn finish(self) -> String {
        match self {
            Self::Sha1(h) => hex::encode(h.finalize()),
            Self::Sha256(h) => hex::encode(h.finalize()),
        }
    }
}

/// One file to place at a canonical location under DATA_ROOT.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadRequest {
    pub source: SourcePolicy,
    pub url: Url,
    /// `/`-separated path relative to DATA_ROOT.
    pub path: String,
    pub size: u64,
    pub digest: Digest,
    /// What progress calls the file instead of its stored name (an asset's
    /// logical name, since objects are stored by hash); display only.
    pub label: Option<String>,
}

impl DownloadRequest {
    /// A Mojang artifact from a game plan; the URL is checked against `source`.
    pub fn from_planned(artifact: &PlannedArtifact, source: SourcePolicy) -> LauncherResult<Self> {
        Ok(Self {
            source,
            url: source.check(&artifact.url)?,
            path: artifact.path.clone(),
            size: artifact.size,
            digest: Digest::Sha1(artifact.sha1.clone()),
            label: label(artifact.role, artifact.provenance.as_deref()),
        })
    }

    /// The name progress shows while this file downloads.
    fn display_name(&self) -> String {
        self.label.clone().unwrap_or_else(|| file_name(&self.path))
    }
}

/// The display label of a planned or locked artifact: asset objects are named
/// by hash on disk, so they show their logical name instead.
pub fn label(role: ArtifactRole, provenance: Option<&str>) -> Option<String> {
    (role == ArtifactRole::AssetObject)
        .then_some(provenance)
        .flatten()
        .map(file_name)
}

/// Resolves a relative managed path, refusing anything that could leave the root.
pub fn resolve_managed_path(root: &DataRoot, relative: &str) -> LauncherResult<PathBuf> {
    let unsafe_path = || {
        LauncherError::new(
            ErrorCode::UnsupportedVersionMetadata,
            "A planned file location is not safe.",
        )
    };
    let mut path = root.path().to_path_buf();
    for part in relative.split('/') {
        let ok = !part.is_empty()
            && part != "."
            && part != ".."
            && part.len() <= 255
            && !part
                .chars()
                .any(|c| matches!(c, '\\' | ':' | '\0') || c.is_control());
        if !ok {
            return Err(unsafe_path());
        }
        path.push(part);
    }
    if !is_within(root.path(), &path) {
        return Err(unsafe_path());
    }
    Ok(path)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DownloadPhase {
    CheckingFiles,
    Downloading,
}

/// Aggregate progress of one download run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadProgress {
    pub phase: DownloadPhase,
    /// Bytes counted toward missing files; never exceeds the total, even with retries.
    pub downloaded_bytes: u64,
    pub total_download_bytes: u64,
    /// Files verified as present, either reused or newly committed.
    pub verified_files: u64,
    pub total_files: u64,
    pub bytes_per_second: Option<u64>,
    /// Names of the files being downloaded right now (D-049).
    pub current_files: Vec<String>,
}

pub trait ProgressSink: Send + Sync {
    fn progress(&self, progress: &DownloadProgress);

    /// An existing file had the wrong content and will be replaced.
    fn corrupt(&self, _path: &str) {}

    /// Whether `bytes` in `files` may be downloaded now; `false` cancels the
    /// run. Asked once the missing files are known (D-051).
    fn approve(&self, _bytes: u64, _files: u64) -> Approval<'_> {
        Box::pin(std::future::ready(true))
    }
}

/// The answer of [`ProgressSink::approve`].
pub type Approval<'a> = std::pin::Pin<Box<dyn std::future::Future<Output = bool> + Send + 'a>>;

/// Discards progress.
pub struct NoProgress;

impl ProgressSink for NoProgress {
    fn progress(&self, _progress: &DownloadProgress) {}
}

/// Measured result of a successful run.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadReport {
    pub unique_files: u64,
    pub reused_files: u64,
    pub downloaded_files: u64,
    /// Unique bytes newly committed.
    pub committed_bytes: u64,
    /// All bytes received, including failed attempts.
    pub network_bytes: u64,
    pub attempts: u64,
    pub retries: u64,
    /// Files that existed with wrong content and were replaced (at most 100 listed).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub corrupt_paths: Vec<String>,
}

const MAX_REPORTED_PATHS: usize = 100;

enum FileCheck {
    Valid,
    Missing,
    /// Present with the wrong type, size or digest.
    Invalid,
}

/// Returns available bytes on the filesystem holding `path`, if known.
pub type SpaceProbe = fn(&Path) -> Option<u64>;

/// Asks the filesystem that holds `path` itself: sysinfo's disk list leaves out
/// tmpfs and other filesystems a data directory may live on, and would then
/// report the free space of the parent mount.
#[cfg(unix)]
pub fn available_space(path: &Path) -> Option<u64> {
    let stat = rustix::fs::statvfs(path).ok()?;
    Some(stat.f_bavail.saturating_mul(stat.f_frsize))
}

/// Windows lists every volume, so the longest matching mount point is the one.
/// `canonicalize` would return a verbatim `\\?\D:\…` path that never starts
/// with a mount point such as `D:\`, so the existing path is only made absolute.
#[cfg(not(unix))]
pub fn available_space(path: &Path) -> Option<u64> {
    std::fs::metadata(path).ok()?;
    let path = std::path::absolute(path).ok()?;
    let disks = sysinfo::Disks::new_with_refreshed_list();
    disks
        .list()
        .iter()
        .filter(|d| path.starts_with(d.mount_point()))
        .max_by_key(|d| d.mount_point().as_os_str().len())
        .map(sysinfo::Disk::available_space)
}

#[derive(Debug, Clone, Copy)]
pub struct ExecutorConfig {
    pub concurrency: usize,
    /// Retries after transient failures (connection, timeout, 5xx, 429).
    pub max_retries: u32,
    pub backoff_base: Duration,
    /// A longer `Retry-After` fails the file instead of waiting.
    pub max_retry_after: Duration,
    /// Maximum time without receiving any bytes.
    pub stall_timeout: Duration,
    pub progress_interval: Duration,
    /// Free space kept on top of the missing bytes.
    pub disk_reserve: u64,
    pub space_probe: SpaceProbe,
}

impl Default for ExecutorConfig {
    fn default() -> Self {
        Self {
            concurrency: 8,
            max_retries: 3,
            backoff_base: Duration::from_millis(500),
            max_retry_after: Duration::from_secs(60),
            stall_timeout: Duration::from_secs(60),
            progress_interval: Duration::from_millis(100),
            disk_reserve: 256 * 1024 * 1024,
            space_probe: available_space,
        }
    }
}

#[derive(Default)]
struct Counters {
    credited: AtomicU64,
    verified: AtomicU64,
    network: AtomicU64,
    committed: AtomicU64,
    downloaded_files: AtomicU64,
    attempts: AtomicU64,
    retries: AtomicU64,
    /// File names in flight, for the download manager.
    active: std::sync::Mutex<Vec<String>>,
}

impl Counters {
    fn active(&self) -> std::sync::MutexGuard<'_, Vec<String>> {
        self.active
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

/// The last path component, the name a user recognizes.
fn file_name(path: &str) -> String {
    path.rsplit('/').next().unwrap_or(path).to_owned()
}

/// Progress credit for one file: the furthest any attempt got, so retries never
/// push the aggregate past the total.
struct Credit<'a> {
    counters: &'a Counters,
    best: u64,
}

impl Credit<'_> {
    fn reached(&mut self, bytes: u64) {
        if bytes > self.best {
            self.counters
                .credited
                .fetch_add(bytes - self.best, Ordering::Relaxed);
            self.best = bytes;
        }
    }
}

enum WorkerResult {
    Done,
    Cancelled,
    Failed(LauncherError),
}

fn cancelled_error() -> LauncherError {
    LauncherError::new(ErrorCode::OperationCancelled, "The download was cancelled.")
}

fn conflict(path: &str) -> LauncherError {
    LauncherError::new(
        ErrorCode::MetadataConflict,
        "Metadata lists the same file with different sizes or hashes.",
    )
    .with_detail("path", path.to_owned())
}

pub struct Executor {
    transport: HttpTransport,
    root: DataRoot,
    config: ExecutorConfig,
}

impl Executor {
    pub fn new(transport: HttpTransport, root: DataRoot, config: ExecutorConfig) -> Self {
        Self {
            transport,
            root,
            config,
        }
    }

    /// Deduplicates and validates requests without touching the network.
    fn prepare(
        &self,
        requests: Vec<DownloadRequest>,
    ) -> LauncherResult<Vec<(DownloadRequest, PathBuf)>> {
        let mut by_path: BTreeMap<String, DownloadRequest> = BTreeMap::new();
        let mut sizes: BTreeMap<(&'static str, String), u64> = BTreeMap::new();
        for request in requests {
            let key = (request.digest.label(), request.digest.hex().to_owned());
            if *sizes.entry(key).or_insert(request.size) != request.size {
                return Err(conflict(&request.path));
            }
            match by_path.get(&request.path) {
                Some(existing)
                    if existing.size != request.size || existing.digest != request.digest =>
                {
                    return Err(conflict(&request.path));
                }
                Some(_) => {}
                None => {
                    by_path.insert(request.path.clone(), request);
                }
            }
        }
        by_path
            .into_values()
            .map(|r| {
                let destination = resolve_managed_path(&self.root, &r.path)?;
                Ok((r, destination))
            })
            .collect()
    }

    /// Ensures every requested file is present and verified. Partial content
    /// never reaches a final path; verified files are kept on every outcome.
    pub async fn run(
        &self,
        job: Uuid,
        requests: Vec<DownloadRequest>,
        cancel: &CancelToken,
        sink: &dyn ProgressSink,
    ) -> LauncherResult<DownloadReport> {
        let items = self.prepare(requests)?;
        let result = self.run_prepared(job, items, cancel, sink).await;
        // Staging belongs to this job only; committed files are elsewhere.
        let _ = tokio::fs::remove_dir_all(self.root.download_staging(job)).await;
        result
    }

    async fn run_prepared(
        &self,
        job: Uuid,
        items: Vec<(DownloadRequest, PathBuf)>,
        cancel: &CancelToken,
        sink: &dyn ProgressSink,
    ) -> LauncherResult<DownloadReport> {
        let counters = Arc::new(Counters::default());
        let total_files = items.len() as u64;
        let progress = |phase, total_bytes, rate| DownloadProgress {
            phase,
            downloaded_bytes: counters.credited.load(Ordering::Relaxed),
            total_download_bytes: total_bytes,
            verified_files: counters.verified.load(Ordering::Relaxed),
            total_files,
            bytes_per_second: rate,
            current_files: counters.active().clone(),
        };

        // Phase 1: verify what is already present.
        sink.progress(&progress(DownloadPhase::CheckingFiles, 0, None));
        let mut missing = Vec::new();
        let mut corrupt = Vec::new();
        let mut checks = JoinSet::new();
        let mut queue = items.into_iter();
        let mut check_ticker = tokio::time::interval(self.config.progress_interval);
        check_ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            while checks.len() < self.config.concurrency {
                let Some((request, destination)) = queue.next() else {
                    break;
                };
                checks.spawn_blocking(move || {
                    let valid = verify_file(&destination, request.size, &request.digest);
                    (request, destination, valid)
                });
            }
            let next = tokio::select! {
                next = checks.join_next() => next,
                _ = check_ticker.tick() => {
                    sink.progress(&progress(DownloadPhase::CheckingFiles, 0, None));
                    continue;
                }
                () = cancel.cancelled() => return Err(cancelled_error()),
            };
            let Some(joined) = next else { break };
            let (request, destination, valid) = joined
                .map_err(|_| LauncherError::internal("A file check stopped unexpectedly."))?;
            match valid? {
                FileCheck::Valid => {
                    counters.verified.fetch_add(1, Ordering::Relaxed);
                }
                FileCheck::Missing => missing.push((request, destination)),
                FileCheck::Invalid => {
                    sink.corrupt(&request.path);
                    if corrupt.len() < MAX_REPORTED_PATHS {
                        corrupt.push(request.path.clone());
                    }
                    missing.push((request, destination));
                }
            }
        }
        let reused = counters.verified.load(Ordering::Relaxed);
        let total_bytes: u64 = missing.iter().map(|(r, _)| r.size).sum();

        // Phase 2: make sure the missing bytes fit.
        if !missing.is_empty()
            && let Some(available) = (self.config.space_probe)(self.root.path())
        {
            let required = total_bytes.saturating_add(self.config.disk_reserve);
            if available < required {
                return Err(LauncherError::new(
                    ErrorCode::DiskFull,
                    "There is not enough free disk space for the download.",
                )
                .with_detail("requiredBytes", i64::try_from(required).unwrap_or(i64::MAX))
                .with_detail(
                    "availableBytes",
                    i64::try_from(available).unwrap_or(i64::MAX),
                ));
            }
        }

        // On a metered connection a big download waits for the user (D-051).
        if !missing.is_empty() {
            let approved = tokio::select! {
                approved = sink.approve(total_bytes, missing.len() as u64) => approved,
                () = cancel.cancelled() => false,
            };
            if !approved {
                return Err(cancelled_error());
            }
        }

        // Phase 3: download with bounded concurrency.
        let stop = CancelToken::new();
        let started = Instant::now();
        let mut ticker = tokio::time::interval(self.config.progress_interval);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let rate = || {
            let secs = started.elapsed().as_secs_f64();
            (secs >= 1.0).then(|| (counters.network.load(Ordering::Relaxed) as f64 / secs) as u64)
        };
        sink.progress(&progress(DownloadPhase::Downloading, total_bytes, None));
        let mut workers = JoinSet::new();
        let mut queue = missing.into_iter();
        let mut failure: Option<LauncherError> = None;
        let mut incomplete = false;
        loop {
            while workers.len() < self.config.concurrency && !stop.is_cancelled() {
                let Some((request, destination)) = queue.next() else {
                    break;
                };
                let worker = Worker {
                    transport: self.transport.clone(),
                    root: self.root.clone(),
                    config: self.config,
                    job,
                    counters: Arc::clone(&counters),
                    stop: stop.clone(),
                };
                workers.spawn(async move { worker.download(request, destination).await });
            }
            if workers.is_empty() {
                incomplete |= queue.next().is_some();
                break;
            }
            tokio::select! {
                joined = workers.join_next() => match joined {
                    Some(Ok(WorkerResult::Done)) | None => {}
                    Some(Ok(WorkerResult::Cancelled)) => incomplete = true,
                    Some(Ok(WorkerResult::Failed(error))) => {
                        incomplete = true;
                        failure.get_or_insert(error);
                        stop.cancel();
                    }
                    Some(Err(_)) => {
                        incomplete = true;
                        failure.get_or_insert(LauncherError::internal("A download stopped unexpectedly."));
                        stop.cancel();
                    }
                },
                _ = ticker.tick() => sink.progress(&progress(DownloadPhase::Downloading, total_bytes, rate())),
                () = cancel.cancelled(), if !stop.is_cancelled() => stop.cancel(),
            }
        }
        sink.progress(&progress(DownloadPhase::Downloading, total_bytes, rate()));
        if let Some(error) = failure {
            return Err(error);
        }
        // A cancel that raced the last commit reports the actual success.
        if incomplete {
            return Err(cancelled_error());
        }
        Ok(DownloadReport {
            unique_files: total_files,
            reused_files: reused,
            downloaded_files: counters.downloaded_files.load(Ordering::Relaxed),
            committed_bytes: counters.committed.load(Ordering::Relaxed),
            network_bytes: counters.network.load(Ordering::Relaxed),
            attempts: counters.attempts.load(Ordering::Relaxed),
            retries: counters.retries.load(Ordering::Relaxed),
            corrupt_paths: corrupt,
        })
    }
}

/// Whether `path` is a regular file with the expected size and digest.
fn verify_file(path: &Path, size: u64, digest: &Digest) -> LauncherResult<FileCheck> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(FileCheck::Missing),
        Err(e) => return Err(LauncherError::from_io(&e, "Checking a game file failed")),
    };
    if !metadata.is_file() || metadata.len() != size {
        return Ok(FileCheck::Invalid);
    }
    let mut file = std::fs::File::open(path)
        .map_err(|e| LauncherError::from_io(&e, "Checking a game file failed"))?;
    let mut hasher = Hasher::new(digest);
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        let n = file
            .read(&mut buffer)
            .map_err(|e| LauncherError::from_io(&e, "Checking a game file failed"))?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }
    Ok(if hasher.finish() == digest.hex() {
        FileCheck::Valid
    } else {
        FileCheck::Invalid
    })
}

/// Removes the staging file unless the content was committed.
struct PartGuard(Option<PathBuf>);

impl Drop for PartGuard {
    fn drop(&mut self) {
        if let Some(path) = self.0.take() {
            let _ = std::fs::remove_file(path);
        }
    }
}

struct Worker {
    transport: HttpTransport,
    root: DataRoot,
    config: ExecutorConfig,
    job: Uuid,
    counters: Arc<Counters>,
    stop: CancelToken,
}

impl Worker {
    async fn download(&self, request: DownloadRequest, destination: PathBuf) -> WorkerResult {
        let name = request.display_name();
        self.counters.active().push(name.clone());
        let result = self.download_file(request, destination).await;
        let mut active = self.counters.active();
        if let Some(index) = active.iter().position(|n| *n == name) {
            active.remove(index);
        }
        result
    }

    async fn download_file(&self, request: DownloadRequest, destination: PathBuf) -> WorkerResult {
        let mut credit = Credit {
            counters: &self.counters,
            best: 0,
        };
        let mut transient = 0u32;
        let mut integrity_retried = false;
        loop {
            if self.stop.is_cancelled() {
                return WorkerResult::Cancelled;
            }
            self.counters.attempts.fetch_add(1, Ordering::Relaxed);
            let failure = match self.attempt(&request, &destination, &mut credit).await {
                Ok(()) => {
                    self.counters.verified.fetch_add(1, Ordering::Relaxed);
                    self.counters
                        .downloaded_files
                        .fetch_add(1, Ordering::Relaxed);
                    self.counters
                        .committed
                        .fetch_add(request.size, Ordering::Relaxed);
                    return WorkerResult::Done;
                }
                Err(failure) => failure,
            };
            let delay = match failure {
                Failure::Cancelled => return WorkerResult::Cancelled,
                Failure::Fatal(error) => return WorkerResult::Failed(error),
                Failure::Integrity(error) => {
                    if integrity_retried {
                        return WorkerResult::Failed(error);
                    }
                    integrity_retried = true;
                    Duration::ZERO
                }
                Failure::Transient { error, retry_after } => {
                    if transient >= self.config.max_retries {
                        return WorkerResult::Failed(error);
                    }
                    let delay = match retry_after {
                        Some(wait) if wait > self.config.max_retry_after => {
                            return WorkerResult::Failed(error.with_detail(
                                "retryAfterSeconds",
                                i64::try_from(wait.as_secs()).unwrap_or(i64::MAX),
                            ));
                        }
                        Some(wait) => wait,
                        None => backoff(self.config.backoff_base, transient),
                    };
                    transient += 1;
                    delay
                }
            };
            self.counters.retries.fetch_add(1, Ordering::Relaxed);
            tokio::select! {
                () = tokio::time::sleep(delay) => {}
                () = self.stop.cancelled() => return WorkerResult::Cancelled,
            }
        }
    }

    async fn attempt(
        &self,
        request: &DownloadRequest,
        destination: &Path,
        credit: &mut Credit<'_>,
    ) -> Result<(), Failure> {
        let storage = |e: std::io::Error| {
            Failure::Fatal(LauncherError::from_io(&e, "Saving a download failed"))
        };
        let part = self
            .root
            .download_part(self.job, request.digest.label(), request.digest.hex());
        if let Some(parent) = part.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(storage)?;
        }
        let mut response = tokio::select! {
            response = self.transport.open(request.source, &request.url) => response?,
            () = self.stop.cancelled() => return Err(Failure::Cancelled),
        };
        if response
            .content_length()
            .is_some_and(|len| len != request.size)
        {
            return Err(Failure::Integrity(size_mismatch()));
        }
        let guard = PartGuard(Some(part.clone()));
        let mut file = tokio::fs::File::create(&part).await.map_err(storage)?;
        let mut hasher = Hasher::new(&request.digest);
        let mut received = 0u64;
        loop {
            let chunk = tokio::select! {
                chunk = tokio::time::timeout(self.config.stall_timeout, response.chunk()) => chunk,
                () = self.stop.cancelled() => return Err(Failure::Cancelled),
            };
            let chunk = match chunk {
                Err(_) => {
                    return Err(Failure::transient(
                        LauncherError::new(
                            ErrorCode::NetworkError,
                            "A download stopped receiving data. Check the internet connection and try again.",
                        )
                        .with_detail("transport", "stall"),
                    ));
                }
                Ok(Err(error)) => return Err(transport::classify(&error)),
                Ok(Ok(None)) => break,
                Ok(Ok(Some(chunk))) => chunk,
            };
            received += chunk.len() as u64;
            self.counters
                .network
                .fetch_add(chunk.len() as u64, Ordering::Relaxed);
            crate::traffic::record(request.source, chunk.len() as u64);
            if received > request.size {
                return Err(Failure::Integrity(size_mismatch()));
            }
            hasher.update(&chunk);
            file.write_all(&chunk).await.map_err(storage)?;
            credit.reached(received);
        }
        if received < request.size {
            return Err(Failure::transient(
                LauncherError::new(
                    ErrorCode::NetworkError,
                    "A download ended early. Check the internet connection and try again.",
                )
                .with_detail("transport", "short_read"),
            ));
        }
        file.sync_all().await.map_err(storage)?;
        drop(file);
        if hasher.finish() != request.digest.hex() {
            return Err(Failure::Integrity(
                LauncherError::new(
                    ErrorCode::HashMismatch,
                    "A downloaded file did not match its expected checksum.",
                )
                .with_detail("algorithm", request.digest.label()),
            ));
        }
        // Commit: a same-filesystem rename, never interrupted by cancellation.
        if let Some(parent) = destination.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(storage)?;
        }
        // Antivirus software often opens a file it has just seen written.
        let started = std::time::Instant::now();
        let mut tries = 0;
        while let Err(error) = tokio::fs::rename(&part, destination).await {
            match crate::storage::atomic::busy_pause(&error, started, tries) {
                Some(pause) => {
                    tokio::time::sleep(pause).await;
                    tries += 1;
                }
                None => return Err(storage(error)),
            }
        }
        let mut guard = guard;
        guard.0 = None;
        Ok(())
    }
}

fn size_mismatch() -> LauncherError {
    LauncherError::new(
        ErrorCode::HashMismatch,
        "A downloaded file did not have its expected size.",
    )
    .with_detail("check", "size")
}

/// Exponential backoff with ±50 % jitter.
fn backoff(base: Duration, retry: u32) -> Duration {
    let mut random = [0u8; 4];
    let jitter = if getrandom::fill(&mut random).is_ok() {
        f64::from(u32::from_le_bytes(random)) / f64::from(u32::MAX)
    } else {
        0.5
    };
    base.mul_f64(f64::from(2u32.saturating_pow(retry.min(10))) * (0.5 + jitter))
}
