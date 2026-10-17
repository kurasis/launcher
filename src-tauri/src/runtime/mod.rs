//! Private Java runtime installation (docs/spec/04 "Managed Java", P07).
//!
//! A pinned [`RuntimePackage`] is downloaded through the verified download
//! executor (SHA-256), extracted safely into a fresh staging directory, described
//! by an extracted-tree receipt, verified, probed with a sanitized environment and
//! only then promoted to `runtimes/temurin/<os>-<arch>/<major>/<image>/<sha256>/`.
//! An installed runtime is reused only after its receipts and every file match.
//! Nothing outside DATA_ROOT is changed: no PATH, JAVA_HOME or registry entries.

pub mod adoptium;
pub mod extract;
pub mod probe;
pub mod tree;

#[cfg(test)]
#[cfg(unix)]
mod tests;

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use url::Url;
use uuid::Uuid;

use crate::download::transport::HttpTransport;
use crate::download::{
    CancelToken, Digest, DownloadRequest, Executor, ExecutorConfig, ProgressSink,
    resolve_managed_path,
};
use crate::error::{ErrorCode, LauncherError, LauncherResult};
use crate::net::SourcePolicy;
use crate::storage::atomic;
use crate::storage::document::SCHEMA_VERSION;
use crate::storage::models::LockedRuntime;
use crate::storage::paths::{DataRoot, Sha256};
pub use adoptium::RuntimePackage;
use extract::{Limits, unsafe_archive};
use probe::JavaProbe;
use tree::{EntryKind, TreeReceipt};

pub const RUNTIME_RECEIPT: &str = ".launcher-runtime.json";
pub const TREE_RECEIPT: &str = ".launcher-tree.json";

/// Written into the runtime root after a successful install.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeReceipt {
    pub schema_version: u64,
    pub package: RuntimePackage,
    pub tree_receipt_sha256: Sha256,
    /// Java executable relative to the runtime root.
    pub java: String,
    pub probe: JavaProbe,
    pub installed_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct InstalledRuntime {
    pub dir: PathBuf,
    pub java: PathBuf,
    pub receipt: RuntimeReceipt,
    /// True when an existing verified runtime was used without downloading.
    pub reused: bool,
    /// Transfer report of the archive download when the runtime was (re)built.
    pub archive_download: Option<crate::download::DownloadReport>,
}

impl InstalledRuntime {
    /// The identity recorded in an instance's install lock.
    pub fn locked(&self) -> LockedRuntime {
        let package = &self.receipt.package;
        LockedRuntime {
            provider: package.provider.clone(),
            release_name: package.release_name.clone(),
            major: package.key.major,
            image_type: package.key.image_type,
            archive_url: package.archive_url.clone(),
            archive_sha256: package.key.archive_sha256.clone(),
            archive_size: package.archive_size,
            tree_receipt_sha256: self.receipt.tree_receipt_sha256.clone(),
        }
    }
}

fn io_error(error: &io::Error) -> LauncherError {
    LauncherError::from_io(error, "Installing the Java runtime failed")
}

fn join_error() -> LauncherError {
    LauncherError::internal("A runtime task stopped unexpectedly.")
}

/// Why an installed runtime cannot be reused as it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verification {
    Valid(Box<InstalledRuntime>),
    Missing,
    /// Receipts unreadable, for another package, or files changed (path given).
    Invalid(String),
}

/// Checks an installed runtime against its receipts without executing it.
pub fn verify_installed(root: &DataRoot, package: &RuntimePackage) -> LauncherResult<Verification> {
    let dir = root.runtime_dir(&package.key);
    if fs::symlink_metadata(&dir).is_err() {
        return Ok(Verification::Missing);
    }
    let invalid = |what: &str| Ok(Verification::Invalid(what.to_owned()));
    let Ok(bytes) = fs::read(dir.join(RUNTIME_RECEIPT)) else {
        return invalid(RUNTIME_RECEIPT);
    };
    let Ok(receipt) = serde_json::from_slice::<RuntimeReceipt>(&bytes) else {
        return invalid(RUNTIME_RECEIPT);
    };
    if receipt.schema_version != SCHEMA_VERSION || !receipt.package.same_package(package) {
        return invalid(RUNTIME_RECEIPT);
    }
    let Ok(tree_bytes) = fs::read(dir.join(TREE_RECEIPT)) else {
        return invalid(TREE_RECEIPT);
    };
    if tree::sha256_hex(&tree_bytes) != receipt.tree_receipt_sha256.as_str() {
        return invalid(TREE_RECEIPT);
    }
    let Ok(expected) = serde_json::from_slice::<TreeReceipt>(&tree_bytes) else {
        return invalid(TREE_RECEIPT);
    };
    if let Some(problem) = tree::verify(&dir, &expected)? {
        return invalid(problem.path());
    }
    if !has_java(&expected, &receipt.java) {
        return invalid(&receipt.java);
    }
    let mut java = dir.clone();
    java.extend(receipt.java.split('/'));
    Ok(Verification::Valid(Box::new(InstalledRuntime {
        dir,
        java,
        receipt,
        reused: true,
        archive_download: None,
    })))
}

impl PartialEq for InstalledRuntime {
    fn eq(&self, other: &Self) -> bool {
        self.dir == other.dir && self.receipt == other.receipt
    }
}

impl Eq for InstalledRuntime {}

/// The Java executable must be a regular (and on Unix executable) file.
fn has_java(tree: &TreeReceipt, java: &str) -> bool {
    tree.entries
        .iter()
        .any(|e| e.path == java && e.kind == EntryKind::File && (cfg!(not(unix)) || e.executable))
}

/// Removes a directory on drop unless disarmed.
struct DirGuard(Option<PathBuf>);

impl Drop for DirGuard {
    fn drop(&mut self) {
        if let Some(path) = self.0.take() {
            let _ = fs::remove_dir_all(path);
        }
    }
}

/// Moves a fully verified `staged` tree to `dir`. An existing directory is kept
/// as `.old-<name>-<uuid>` until the new one is in place, so an interruption
/// leaves something [`reconcile_runtimes`] can restore.
fn promote(staged: &Path, dir: &Path) -> LauncherResult<()> {
    let parent = dir
        .parent()
        .ok_or_else(|| LauncherError::internal("Runtime path has no parent."))?;
    fs::create_dir_all(parent).map_err(|e| io_error(&e))?;
    let name = dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let old = parent.join(format!(".old-{name}-{}", Uuid::new_v4().simple()));
    let had_old = fs::symlink_metadata(dir).is_ok();
    if had_old {
        fs::rename(dir, &old).map_err(|e| io_error(&e))?;
    }
    if let Err(error) = fs::rename(staged, dir) {
        if had_old {
            let _ = fs::rename(&old, dir);
        }
        return Err(io_error(&error));
    }
    if had_old {
        let _ = fs::remove_dir_all(&old);
    }
    Ok(())
}

/// Startup cleanup under the DATA_ROOT lock: removes staging directories and
/// leftover archives and finishes interrupted promotions.
pub fn reconcile_runtimes(root: &DataRoot) -> LauncherResult<()> {
    let runtimes = root.runtimes_dir();
    let read = |dir: &Path| -> LauncherResult<Vec<fs::DirEntry>> {
        match fs::read_dir(dir) {
            Ok(entries) => entries.collect::<Result<_, _>>().map_err(|e| io_error(&e)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(e) => Err(io_error(&e)),
        }
    };
    for entry in read(&runtimes)? {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with(".staging-") || name == ".archives" {
            fs::remove_dir_all(entry.path()).map_err(|e| io_error(&e))?;
        }
    }
    // runtimes/temurin/<os-arch>/<major>/<image>/<sha256 | .old-...>
    let mut level = vec![runtimes.join("temurin")];
    for _ in 0..3 {
        let mut next = Vec::new();
        for dir in &level {
            for entry in read(dir)? {
                if entry.file_type().is_ok_and(|t| t.is_dir()) {
                    next.push(entry.path());
                }
            }
        }
        level = next;
    }
    for dir in level {
        for entry in read(&dir)? {
            let name = entry.file_name().to_string_lossy().into_owned();
            let Some(rest) = name.strip_prefix(".old-") else {
                continue;
            };
            let Some((sha, _)) = rest.rsplit_once('-') else {
                continue;
            };
            let target = dir.join(sha);
            if fs::symlink_metadata(&target).is_ok() {
                fs::remove_dir_all(entry.path()).map_err(|e| io_error(&e))?;
            } else {
                fs::rename(entry.path(), &target).map_err(|e| io_error(&e))?;
            }
        }
    }
    Ok(())
}

pub struct RuntimeInstaller {
    root: DataRoot,
    executor: Executor,
    limits: Limits,
    probe_timeout: Duration,
}

impl RuntimeInstaller {
    pub fn new(root: DataRoot, transport: HttpTransport, config: ExecutorConfig) -> Self {
        Self {
            executor: Executor::new(transport, root.clone(), config),
            root,
            limits: Limits::default(),
            probe_timeout: probe::PROBE_TIMEOUT,
        }
    }

    /// Returns a verified runtime for `package`, installing or rebuilding it from
    /// the pinned archive when it is missing or any file differs.
    pub async fn ensure(
        &self,
        package: &RuntimePackage,
        job: Uuid,
        cancel: &CancelToken,
        sink: &dyn ProgressSink,
    ) -> LauncherResult<InstalledRuntime> {
        let (root, pinned) = (self.root.clone(), package.clone());
        let existing = tokio::task::spawn_blocking(move || verify_installed(&root, &pinned))
            .await
            .map_err(|_| join_error())??;
        if let Verification::Valid(installed) = existing {
            let java = installed.java.clone();
            let timeout = self.probe_timeout;
            let probe = tokio::task::spawn_blocking(move || probe::run(&java, timeout))
                .await
                .map_err(|_| join_error())??;
            probe::check(&probe, package.key.major, package.key.arch)?;
            return Ok(*installed);
        }

        let archive_path = format!(
            "runtimes/.archives/{}.{}",
            package.key.archive_sha256,
            package.archive_format.extension()
        );
        let request = DownloadRequest {
            source: SourcePolicy::Adoptium,
            url: Url::parse(&package.archive_url).map_err(|_| {
                LauncherError::new(
                    ErrorCode::RuntimeUnavailable,
                    "The runtime location is invalid.",
                )
            })?,
            path: archive_path.clone(),
            size: package.archive_size,
            digest: Digest::Sha256(package.key.archive_sha256.clone()),
        };
        let download = self.executor.run(job, vec![request], cancel, sink).await?;
        let archive = resolve_managed_path(&self.root, &archive_path)?;

        let (root, pinned, limits, timeout, cancel) = (
            self.root.clone(),
            package.clone(),
            self.limits,
            self.probe_timeout,
            cancel.clone(),
        );
        let result = tokio::task::spawn_blocking(move || {
            install_from_archive(&root, &pinned, &archive, limits, timeout, &cancel)
        })
        .await
        .map_err(|_| join_error())?;
        // The archive is only needed for this install; repair downloads the
        // pinned package again.
        let archive = resolve_managed_path(&self.root, &archive_path)?;
        let _ = fs::remove_file(&archive);
        if let Some(parent) = archive.parent() {
            let _ = fs::remove_dir(parent);
        }
        result.map(|installed| InstalledRuntime {
            archive_download: Some(download),
            ..installed
        })
    }
}

/// Blocking part of an install: extract, describe, verify, probe, promote.
fn install_from_archive(
    root: &DataRoot,
    package: &RuntimePackage,
    archive: &Path,
    limits: Limits,
    timeout: Duration,
    cancel: &CancelToken,
) -> LauncherResult<InstalledRuntime> {
    let staging = root
        .runtimes_dir()
        .join(format!(".staging-{}", Uuid::new_v4().simple()));
    fs::create_dir_all(root.runtimes_dir()).map_err(|e| io_error(&e))?;
    fs::create_dir(&staging).map_err(|e| io_error(&e))?;
    let _guard = DirGuard(Some(staging.clone()));

    extract::extract(archive, package.archive_format, &staging, limits)?;
    let top = extract::single_root(&staging)?;
    let tree = tree::build(&top)?;
    let java_rel = probe::java_relative_path(package.key.os);
    if !has_java(&tree, java_rel) {
        return Err(unsafe_archive("java executable not found"));
    }
    let tree_bytes = tree::to_bytes(&tree);
    atomic::write(&top.join(TREE_RECEIPT), &tree_bytes).map_err(|e| io_error(&e))?;
    // The receipt is verified against the files before anything is executed.
    if tree::verify(&top, &tree)?.is_some() {
        return Err(unsafe_archive("extracted files changed"));
    }
    if cancel.is_cancelled() {
        return Err(LauncherError::new(
            ErrorCode::OperationCancelled,
            "The installation was cancelled.",
        ));
    }
    let probe = probe::run(&probe::java_path(&top, package.key.os), timeout)?;
    probe::check(&probe, package.key.major, package.key.arch)?;
    let receipt = RuntimeReceipt {
        schema_version: SCHEMA_VERSION,
        package: package.clone(),
        tree_receipt_sha256: Sha256::parse(&tree::sha256_hex(&tree_bytes))?,
        java: java_rel.to_owned(),
        probe,
        installed_at: Utc::now(),
    };
    let mut receipt_bytes = serde_json::to_vec_pretty(&receipt)
        .map_err(|_| LauncherError::internal("Serializing the runtime receipt failed."))?;
    receipt_bytes.push(b'\n');
    atomic::write(&top.join(RUNTIME_RECEIPT), &receipt_bytes).map_err(|e| io_error(&e))?;

    let dir = root.runtime_dir(&package.key);
    promote(&top, &dir)?;
    let java = probe::java_path(&dir, package.key.os);
    Ok(InstalledRuntime {
        dir,
        java,
        receipt,
        reused: false,
        archive_download: None,
    })
}

#[cfg(test)]
mod reconcile_tests {
    use super::*;

    #[test]
    fn startup_reconciles_interrupted_promotions() {
        let dir = tempfile::tempdir().unwrap();
        let root = DataRoot::new(dir.path().to_path_buf()).unwrap();
        let parent = root.runtimes_dir().join("temurin/linux-x64/21/jre");
        let sha_a = "a".repeat(64);
        let sha_b = "b".repeat(64);
        // A: interrupted after moving the old directory aside → restored.
        fs::create_dir_all(parent.join(format!(".old-{sha_a}-1234"))).unwrap();
        // B: new directory in place, old one left behind → removed.
        fs::create_dir_all(parent.join(&sha_b)).unwrap();
        fs::create_dir_all(parent.join(format!(".old-{sha_b}-5678"))).unwrap();
        fs::create_dir_all(root.runtimes_dir().join(".staging-x/jdk")).unwrap();
        fs::create_dir_all(root.runtimes_dir().join(".archives")).unwrap();
        reconcile_runtimes(&root).unwrap();
        let mut names: Vec<_> = fs::read_dir(&parent)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(names, vec![sha_a, sha_b]);
        let top: Vec<_> = fs::read_dir(root.runtimes_dir())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(top, vec![std::ffi::OsString::from("temurin")]);
    }
}
