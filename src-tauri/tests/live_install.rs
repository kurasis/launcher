//! Live installation through the real install service (Mojang and Adoptium).
//! Ignored by default; run with
//! `LAUNCHER_LIVE_ROOT=<data dir> LAUNCHER_LIVE_VERSION=26.3 cargo test --test live_install -- --ignored --nocapture`
//!
//! Installs the version into the data directory, reusing an existing installation
//! of it (`LAUNCHER_LIVE_REPAIR=1` repairs instead), waits for the result and prints
//! what was downloaded and what was reused from the shared cache, so alternating
//! versions in one data directory show their cache sharing (docs/spec/10).

#![allow(clippy::print_stdout, clippy::unwrap_used, clippy::expect_used)] // prints the evidence summary

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use launcher_lib::catalog::source::MetadataService;
use launcher_lib::download::ExecutorConfig;
use launcher_lib::download::transport::HttpTransport;
use launcher_lib::install::service::{InstallService, LiveSource, PlanSource, StateAccess};
use launcher_lib::net::{PublicClient, SourcePolicy};
use launcher_lib::state::dto::{CreateInstancePayload, MutationRequest};
use launcher_lib::state::{AppState, NoopSink};
use launcher_lib::storage::models::{OperationKind, Outcome};
use launcher_lib::storage::paths::DataRoot;
use launcher_lib::validation::MemoryPolicy;

const VERSION: &str = "0.1.0";

fn bytes_under(path: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(path) else {
        return 0;
    };
    entries
        .flatten()
        .map(|e| match e.file_type() {
            Ok(t) if t.is_dir() => bytes_under(&e.path()),
            Ok(t) if t.is_file() => e.metadata().map_or(0, |m| m.len()),
            _ => 0,
        })
        .sum()
}

fn usage(root: &DataRoot) -> String {
    let gib = |p: PathBuf| bytes_under(&p) as f64 / 1_073_741_824.0;
    format!(
        "shared {:.3} GiB, runtimes {:.3} GiB",
        gib(root.path().join("shared")),
        gib(root.path().join("runtimes")),
    )
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "downloads Minecraft and Java from the official sources"]
async fn install_a_version_into_a_data_directory() {
    let root = DataRoot::new(PathBuf::from(
        std::env::var("LAUNCHER_LIVE_ROOT").expect("set LAUNCHER_LIVE_ROOT"),
    ))
    .unwrap();
    let version = std::env::var("LAUNCHER_LIVE_VERSION").expect("set LAUNCHER_LIVE_VERSION");
    let repair = std::env::var_os("LAUNCHER_LIVE_REPAIR").is_some();
    let state = Arc::new(AppState::initialize(
        Ok(root.clone()),
        VERSION,
        MemoryPolicy::detect(),
        Box::new(NoopSink),
    ));
    let metadata = Arc::new(MetadataService::new(
        PublicClient::new(SourcePolicy::MojangMetadata, VERSION).unwrap(),
        root.clone(),
    ));
    let source: Arc<dyn PlanSource> = Arc::new(LiveSource::new(metadata, VERSION).unwrap());
    let service = InstallService::new(
        Arc::new(Arc::clone(&state)) as Arc<dyn StateAccess>,
        source,
        HttpTransport::public(VERSION).unwrap(),
        ExecutorConfig::default(),
    );
    // LAUNCHER_LIVE_NEW adds another installation of the same version (MV01).
    let existing = state
        .snapshot()
        .instances
        .into_iter()
        .find(|i| i.version_id == version)
        .filter(|_| std::env::var_os("LAUNCHER_LIVE_NEW").is_none());
    // An existing installation of the version is installed again (resuming an
    // interrupted install) or, with LAUNCHER_LIVE_REPAIR, repaired.
    let kind = if repair {
        OperationKind::Repair
    } else {
        OperationKind::Install
    };
    let instance = match existing {
        Some(instance) => instance.id,
        None => {
            state
                .create_instance(MutationRequest {
                    request_id: uuid::Uuid::new_v4().to_string(),
                    payload: CreateInstancePayload {
                        display_name: format!("Live {version}"),
                        version_id: version.clone(),
                    },
                })
                .unwrap()
                .id
        }
    };
    println!("before: {}", usage(&root));
    let started = Instant::now();
    let operation = service
        .start(&uuid::Uuid::new_v4().to_string(), kind, instance)
        .unwrap();
    let result = loop {
        if let Some(result) = state.operation_result(&operation.operation_id) {
            break result;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    };
    println!(
        "{kind:?} {version}: {:?} in {:.1} s; downloaded {} files ({} network bytes), reused {} files",
        result.outcome,
        started.elapsed().as_secs_f64(),
        result.downloaded_files,
        result.network_bytes,
        result.reused_files,
    );
    println!("after: {}", usage(&root));
    assert_eq!(result.outcome, Outcome::Succeeded, "{:?}", result.error);
}
