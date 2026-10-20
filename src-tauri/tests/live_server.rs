//! Live world server (Worlds with Friends W3, A8). Ignored by default; CI runs
//! it on Linux for several Minecraft versions, and it runs anywhere with
//! network access:
//! `WORLD_VERSION=1.12.2 WORLD_PROTOCOL=340 cargo test --test live_server -- --ignored --nocapture --test-threads=1`
//!
//! `WORLD_VERSION` is a release (1.21.1 when unset, `latest` for the newest
//! release in the catalog); `WORLD_PROTOCOL`, when set, is the protocol its
//! server must report.
//!
//! Creates a world, downloads Mojang's server JAR and the Java its descriptor
//! names through the launcher's verified paths, starts the server in offline
//! mode on loopback, waits for the `Done (` line and a status ping, checks RCON
//! with the per-run password, logs in with the Login Start the tunnel reads for
//! that protocol (a member is let in, anyone else is not), checks that a
//! Log4Shell lookup in chat goes nowhere, stops the server through stdin and
//! checks that the run was clean, the receipt removed and the hosting permit
//! released. A second start is force stopped and must be recorded as unclean.
//! Prints fixed words and numbers only.

#![allow(clippy::print_stdout, clippy::unwrap_used)] // prints the evidence summary

use std::io::{Read as _, Write as _};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
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
use launcher_lib::tunnel::handshake::{self, Classified};
use launcher_lib::worlds::host::{self, Launch, Phase, STARTUP_DEADLINE, STOP_GRACE};
use launcher_lib::worlds::identity::Nick;
use launcher_lib::worlds::properties::WorldSettings;
use launcher_lib::worlds::protocol::{read_packet, write_packet, write_string, write_varint};
use launcher_lib::worlds::{install, ping, rcon, receipt, world};

const VERSION: &str = "live-test";

/// A handshake and the Login Start a client of `protocol` sends for `nick`:
/// no signing key, the offline UUID where the layout has one.
fn login(protocol: i32, port: u16, nick: &Nick) -> Vec<u8> {
    let mut body = Vec::new();
    write_varint(&mut body, protocol);
    write_string(&mut body, "127.0.0.1");
    body.extend_from_slice(&port.to_be_bytes());
    write_varint(&mut body, 2);
    let mut wire = Vec::new();
    write_packet(&mut wire, 0x00, &body).unwrap();
    let uuid = *nick.offline_uuid().as_bytes();
    let mut body = Vec::new();
    write_string(&mut body, nick.as_str());
    match protocol {
        ..=758 => {}
        759 => body.push(0),
        760 => {
            body.extend_from_slice(&[0, 1]);
            body.extend_from_slice(&uuid);
        }
        761..=763 => {
            body.push(1);
            body.extend_from_slice(&uuid);
        }
        _ => body.extend_from_slice(&uuid),
    }
    write_packet(&mut wire, 0x00, &body).unwrap();
    wire
}

/// The id of the first packet the server answers a login with: 0x00 is a
/// refusal, 0x02 (Login Success) or 0x03 (Set Compression) lets the player in.
fn first_answer(port: u16, wire: &[u8]) -> i32 {
    let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    stream.write_all(wire).unwrap();
    read_packet(&mut stream, 1 << 20).unwrap().0
}

/// The launcher's own reading of the same bytes: a login of `nick`.
fn tunnel_reads(wire: &[u8], protocol: i32, nick: &Nick) {
    match handshake::classify(wire, Some(protocol)) {
        Ok(Classified::Login {
            nick: read,
            consumed,
        }) => {
            assert_eq!(&read, nick);
            assert_eq!(consumed, wire.len());
        }
        other => panic!("the tunnel does not read this login: {other:?}"),
    }
}

/// Whether anything connected to `listener` within `wait`.
fn contacted(listener: &TcpListener, wait: Duration) -> bool {
    listener.set_nonblocking(true).unwrap();
    let until = Instant::now() + wait;
    while Instant::now() < until {
        if let Ok((mut stream, _)) = listener.accept() {
            let _ = stream.read(&mut [0u8; 1]);
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    false
}

fn physical_memory() -> u64 {
    let mut system = sysinfo::System::new();
    system.refresh_memory();
    system.total_memory()
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "network: downloads a server and its Java, runs the server"]
async fn host_a_world() {
    let dir = tempfile::tempdir().unwrap();
    let root = DataRoot::new(dir.path().to_path_buf()).unwrap();
    let metadata = Arc::new(MetadataService::new(
        PublicClient::new(SourcePolicy::MojangMetadata, VERSION).unwrap(),
        root.clone(),
    ));
    let minecraft = match std::env::var("WORLD_VERSION")
        .ok()
        .filter(|v| !v.is_empty())
    {
        Some(v) if v == "latest" => {
            let (catalog, ..) = metadata.catalog(false).await;
            catalog.unwrap().latest_release.unwrap()
        }
        Some(v) => v,
        None => "1.21.1".to_owned(),
    };
    let expected: Option<i64> = std::env::var("WORLD_PROTOCOL")
        .ok()
        .filter(|v| !v.is_empty())
        .map(|v| v.parse().unwrap());
    println!("minecraft {minecraft}");
    let created = world::create(&root, "Live world", &minecraft, WorldSettings::default()).unwrap();
    world::accept_eula(&root, created.id).unwrap();

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
    let status = ping::status(server.port(), -1, Duration::from_secs(5)).unwrap();
    println!("protocol {}", status.protocol);
    if let Some(expected) = expected {
        assert_eq!(status.protocol, expected);
    }
    assert_eq!(
        i64::from(server.protocol()),
        status.protocol,
        "learned when ready"
    );
    assert_eq!((status.online, status.max), (0, 8));

    let recorded = receipt::read(&root).expect("run receipt");
    assert!(recorded.is_alive());
    // Servers before 1.13 open RCON just after the `Done (` line.
    let rcon_until = Instant::now() + Duration::from_secs(20);
    let mut console = loop {
        match rcon::Rcon::connect(
            recorded.rcon_port,
            &recorded.rcon_password(),
            Duration::from_secs(5),
        ) {
            Err(rcon::RconError::Unreachable) if Instant::now() < rcon_until => {
                std::thread::sleep(Duration::from_millis(250));
            }
            other => break other.unwrap(),
        }
    };
    let answer = console.command("list").unwrap();
    assert!(answer.contains('8'), "rcon list answers");
    let wrong = rcon::Rcon::connect(
        recorded.rcon_port,
        &launcher_lib::secrets::SecretString::new("wrong"),
        Duration::from_secs(5),
    );
    assert_eq!(wrong.err(), Some(rcon::RconError::Refused));

    // Logins in this version's layout, read the same way by the tunnel.
    let protocol = server.protocol();
    let member = login(protocol, server.port(), &members[0]);
    tunnel_reads(&member, protocol, &members[0]);
    assert_ne!(
        first_answer(server.port(), &member),
        0x00,
        "a member gets in"
    );
    let stranger_nick = Nick::parse("Not_A_Member").unwrap();
    let stranger = login(protocol, server.port(), &stranger_nick);
    tunnel_reads(&stranger, protocol, &stranger_nick);
    assert_eq!(
        first_answer(server.port(), &stranger),
        0x00,
        "the whitelist refuses"
    );
    println!("member in, stranger refused");

    // A lookup in chat is never resolved: nothing connects to the address.
    let trap = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let port = trap.local_addr().unwrap().port();
    console
        .command(&format!("say ${{jndi:ldap://127.0.0.1:{port}/a}}"))
        .unwrap();
    console
        .command(&format!(
            "say ${{${{lower:j}}ndi:ldap://127.0.0.1:{port}/b}}"
        ))
        .unwrap();
    assert!(
        !contacted(&trap, Duration::from_secs(5)),
        "Log4Shell lookup"
    );
    println!("no lookup");
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
