//! Live world server (Worlds with Friends W3). Ignored by default; CI runs it on
//! Linux, and it runs anywhere with network access:
//! `cargo test --test live_server -- --ignored --nocapture --test-threads=1`
//!
//! Creates a world, downloads Mojang's 1.21.1 server JAR and its Java through
//! the launcher's verified paths, starts the server in offline mode on
//! loopback, waits for the `Done (` line and a status ping, checks RCON with the
//! per-run password, stops it through stdin and checks that the run was clean,
//! the receipt removed and the hosting permit released. A second start is force
//! stopped and must be recorded as unclean. Prints fixed words and numbers only.

#![allow(clippy::print_stdout, clippy::unwrap_used)] // prints the evidence summary

use std::sync::Arc;
use std::time::{Duration, Instant};

use launcher_lib::catalog::source::MetadataService;
use launcher_lib::download::transport::HttpTransport;
use launcher_lib::download::{CancelToken, Executor, ExecutorConfig, NoProgress};
use launcher_lib::install::service::LiveSource;
use launcher_lib::net::{PublicClient, SourcePolicy};
use launcher_lib::runtime::RuntimeInstaller;
use launcher_lib::state::activity::ActivityRegistry;
use launcher_lib::storage::models::OperationKind;
use launcher_lib::storage::paths::{DataRoot, JavaProvider};
use launcher_lib::worlds::host::{self, Launch, Phase, STARTUP_DEADLINE, STOP_GRACE};
use launcher_lib::worlds::identity::Nick;
use launcher_lib::worlds::properties::WorldSettings;
use launcher_lib::worlds::{install, ping, rcon, receipt, world};

const VERSION: &str = "live-test";

fn physical_memory() -> u64 {
    let mut system = sysinfo::System::new();
    system.refresh_memory();
    system.total_memory()
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "network: downloads the 1.21.1 server and Java 21, runs the server"]
async fn host_a_1_21_1_world() {
    let dir = tempfile::tempdir().unwrap();
    let root = DataRoot::new(dir.path().to_path_buf()).unwrap();
    let created = world::create(&root, "Live world", "1.21.1", WorldSettings::default()).unwrap();
    world::accept_eula(&root, created.id).unwrap();

    let metadata = Arc::new(MetadataService::new(
        PublicClient::new(SourcePolicy::MojangMetadata, VERSION).unwrap(),
        root.clone(),
    ));
    let source = LiveSource::new(Arc::clone(&metadata), VERSION).unwrap();
    let transport = HttpTransport::public(VERSION).unwrap();
    let runtimes =
        RuntimeInstaller::new(root.clone(), transport.clone(), ExecutorConfig::default());
    let executor = Executor::new(transport, root.clone(), ExecutorConfig::default());
    let started = Instant::now();
    let (prepared, java) = install::prepare(
        &root,
        &metadata,
        &source,
        &runtimes,
        &executor,
        created.id,
        JavaProvider::Mojang,
        &CancelToken::new(),
        &NoProgress,
    )
    .await
    .unwrap();
    let pin = prepared.server.clone().expect("pinned server");
    assert!(install::verify(&root, &pin));
    println!(
        "server jar {} bytes, java {}, prepared in {:.1?}",
        pin.jar_size,
        pin.java_major,
        started.elapsed()
    );

    let activity = ActivityRegistry::new();
    let members = [Nick::parse("Host_Player").unwrap()];
    let run = |operation: &str| {
        let permit = activity
            .try_acquire_hosting(created.id.uuid(), operation.to_owned())
            .unwrap();
        host::start(
            &Launch {
                root: &root,
                world_id: created.id,
                java: &java,
                server_heap_mib: 1024,
                game_heap_mib: 0,
                physical_memory_bytes: physical_memory(),
                members: &members,
                operation_id: operation.to_owned(),
            },
            permit,
            Arc::new(|| {}),
        )
        .unwrap()
    };

    // First run: start, check, stop cleanly.
    let server = run("live-1");
    assert!(activity.is_busy());
    assert!(
        activity
            .try_acquire(OperationKind::Install, None, "install")
            .is_err(),
        "an install waits for the server"
    );
    let started = Instant::now();
    tokio::task::block_in_place(|| server.wait_ready(STARTUP_DEADLINE)).unwrap();
    println!("ready in {:.1?}", started.elapsed());
    assert_eq!(server.status().phase, Phase::Ready);
    let status = ping::status(server.port(), 767, Duration::from_secs(5)).unwrap();
    assert_eq!((status.protocol, status.online), (767, 0));
    assert_eq!(status.max, 8);

    let recorded = receipt::read(&root).expect("run receipt");
    assert!(recorded.is_alive());
    let mut console = rcon::Rcon::connect(
        recorded.rcon_port,
        &recorded.rcon_password(),
        Duration::from_secs(5),
    )
    .unwrap();
    let answer = console.command("list").unwrap();
    assert!(answer.contains("0 of a max of 8"), "rcon list answers");
    let wrong = rcon::Rcon::connect(
        recorded.rcon_port,
        &launcher_lib::secrets::SecretString::new("wrong"),
        Duration::from_secs(5),
    );
    assert_eq!(wrong.err(), Some(rcon::RconError::Refused));
    drop(console);

    let world_now = world::load(&root, created.id).unwrap();
    assert!(
        world_now.mutation.is_some(),
        "hosting recorded before start"
    );
    let properties =
        std::fs::read_to_string(root.world_server_dir(created.id).join("server.properties"))
            .unwrap();
    assert!(properties.lines().any(|l| l == "online-mode=false"));
    assert!(properties.lines().any(|l| l == "server-ip=127.0.0.1"));

    let stopping = Instant::now();
    let clean = tokio::task::block_in_place(|| server.stop(STOP_GRACE));
    println!("stopped in {:.1?}, clean {clean}", stopping.elapsed());
    assert!(clean);
    assert_eq!(server.status().phase, Phase::Exited);
    assert!(!activity.is_busy(), "permit released after exit");
    assert!(receipt::read(&root).is_none(), "receipt removed");
    let after = world::load(&root, created.id).unwrap();
    assert!(after.mutation.is_none());
    assert!(after.last_run.unwrap().clean);
    assert!(
        root.world_server_dir(created.id)
            .join("world")
            .join("level.dat")
            .is_file(),
        "the world was saved"
    );
    assert!(
        !root.world_server_dir(created.id).join("logs").exists(),
        "the server keeps no log files"
    );

    // Second run: a force stop is recorded as unclean.
    let server = run("live-2");
    tokio::task::block_in_place(|| server.wait_ready(STARTUP_DEADLINE)).unwrap();
    server.force_stop();
    let clean = tokio::task::block_in_place(|| server.wait_exit(Duration::from_secs(30)));
    assert_eq!(clean, Some(false));
    assert!(!activity.is_busy());
    assert!(
        !world::load(&root, created.id)
            .unwrap()
            .last_run
            .unwrap()
            .clean
    );
    println!("force stop recorded as unclean");
}
