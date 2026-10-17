//! Live download smoke test against the real Mojang hosts. Ignored by default; run
//! with `cargo test --test live_download -- --ignored --nocapture`. It downloads a
//! small subset of the 1.21.1 plan (logging config, a few libraries and about
//! 120 asset objects, roughly 6 MB), cancels part-way, retries, and checks reuse.

#![allow(clippy::print_stdout, clippy::unwrap_used)] // prints the evidence summary

use std::sync::{Arc, Mutex};

use launcher_lib::catalog::source::MetadataService;
use launcher_lib::download::transport::HttpTransport;
use launcher_lib::download::{
    CancelToken, DownloadProgress, DownloadRequest, Executor, ExecutorConfig, ProgressSink,
    resolve_managed_path,
};
use launcher_lib::error::ErrorCode;
use launcher_lib::install::rules::RuleContext;
use launcher_lib::net::{PublicClient, SourcePolicy};
use launcher_lib::storage::paths::DataRoot;
use uuid::Uuid;

#[derive(Default)]
struct Last(Mutex<Option<DownloadProgress>>, Mutex<usize>);

impl ProgressSink for Last {
    fn progress(&self, progress: &DownloadProgress) {
        *self.0.lock().unwrap() = Some(progress.clone());
        *self.1.lock().unwrap() += 1;
    }
}

/// Requests cancellation once `files` files are verified.
struct CancelAfter {
    cancel: CancelToken,
    files: u64,
    last: Mutex<Option<DownloadProgress>>,
}

impl ProgressSink for CancelAfter {
    fn progress(&self, progress: &DownloadProgress) {
        if progress.verified_files >= self.files {
            self.cancel.cancel();
        }
        *self.last.lock().unwrap() = Some(progress.clone());
    }
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "network: contacts Mojang metadata and artifact hosts"]
async fn live_small_set_cancel_retry_and_reuse() {
    let dir = tempfile::tempdir().unwrap();
    let root = DataRoot::new(dir.path().to_path_buf()).unwrap();
    let metadata = MetadataService::new(
        PublicClient::new(SourcePolicy::MojangMetadata, "live-test").unwrap(),
        root.clone(),
    );
    let plan = metadata
        .plan("1.21.1", &RuleContext::current().unwrap())
        .await
        .unwrap();
    let mut libraries = plan.libraries.clone();
    libraries.sort_by_key(|l| l.size);
    let selected: Vec<_> = plan
        .logging
        .iter()
        .map(|l| &l.artifact)
        .chain(libraries.iter().take(4))
        .chain(plan.assets.iter().take(120))
        .collect();
    let requests: Vec<DownloadRequest> = selected
        .iter()
        .map(|a| DownloadRequest::from_planned(a, SourcePolicy::MojangArtifacts).unwrap())
        .collect();
    let total: u64 = requests.iter().map(|r| r.size).sum();
    println!("selected {} files, {total} bytes", requests.len());

    let executor = Arc::new(Executor::new(
        HttpTransport::public("live-test").unwrap(),
        root.clone(),
        ExecutorConfig::default(),
    ));

    // Interrupt once part of the set has been committed.
    let cancel = CancelToken::new();
    let first = tokio::spawn({
        let (executor, cancel, requests) =
            (Arc::clone(&executor), cancel.clone(), requests.clone());
        async move {
            let sink = CancelAfter {
                cancel: cancel.clone(),
                files: 30,
                last: Mutex::default(),
            };
            let result = executor.run(Uuid::new_v4(), requests, &cancel, &sink).await;
            (result, sink.last.lock().unwrap().clone())
        }
    });
    let (result, progress) = first.await.unwrap();
    match &result {
        Err(error) => assert_eq!(error.code, ErrorCode::OperationCancelled),
        Ok(_) => println!("note: everything finished before the cancel"),
    }
    let present = requests
        .iter()
        .filter(|r| resolve_managed_path(&root, &r.path).unwrap().exists())
        .count();
    println!(
        "after cancel: {present}/{} files committed; last progress {progress:?}",
        requests.len()
    );
    assert!(
        !root.downloads_dir().exists()
            || std::fs::read_dir(root.downloads_dir())
                .unwrap()
                .next()
                .is_none()
    );

    // Retry completes and reuses what was committed.
    let sink = Last::default();
    let report = executor
        .run(Uuid::new_v4(), requests.clone(), &CancelToken::new(), &sink)
        .await
        .unwrap();
    println!(
        "retry: {report:?}; progress events {}",
        sink.1.lock().unwrap()
    );
    assert_eq!(report.unique_files as usize, requests.len());
    assert_eq!(report.reused_files as usize, present);
    assert_eq!(
        report.reused_files + report.downloaded_files,
        report.unique_files
    );

    // A third run transfers nothing.
    let report = executor
        .run(Uuid::new_v4(), requests, &CancelToken::new(), &sink)
        .await
        .unwrap();
    println!("reuse: {report:?}");
    assert_eq!(report.network_bytes, 0);
}
