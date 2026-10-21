//! Live world server (Worlds with Friends W3, A8). Ignored by default; CI runs
//! it on Linux for several Minecraft versions, and it runs anywhere with
//! network access:
//! `WORLD_VERSION=1.12.2 WORLD_PROTOCOL=340 cargo test --test live_server -- --ignored --nocapture --test-threads=1`
//!
//! `WORLD_VERSION` is a world version (1.21.1 when unset, `latest` for the
//! newest release in the catalog, `snapshot` for its newest snapshot); `WORLD_PROTOCOL`, when set, is the protocol its
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
//!
//! `host_a_world_with_mods` (W12) does the same for a world with mods: the
//! newest stable Fabric loader for the version, Fabric API (both sides) and
//! Sodium (only the game) from the catalog through its verified downloads, a
//! generation built and stored by the launcher, the loader's server
//! installed from its meta service. The server must load Fabric API and
//! never see Sodium; a member gets in, a stranger does not.
//! `WORLD_LOADER=fabric WORLD_VERSION=1.21.1 cargo test --test live_server host_a_world_with_mods -- --ignored --exact --nocapture`

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

/// How a login ends with a modded server, which may compress and ask the
/// client login questions (Fabric API does both) before it decides: true
/// for Login Success, false for a Disconnect. Questions get "not understood".
fn logged_in(port: u16, wire: &[u8]) -> bool {
    use launcher_lib::worlds::protocol::read_varint;
    let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    stream.write_all(wire).unwrap();
    let mut compressed = false;
    loop {
        let (id, body) = if compressed {
            let length = usize::try_from(read_varint(&mut stream).unwrap()).unwrap();
            let mut frame = vec![0u8; length];
            stream.read_exact(&mut frame).unwrap();
            let mut frame = std::io::Cursor::new(frame);
            let inflated = read_varint(&mut frame).unwrap();
            let mut data = Vec::new();
            if inflated == 0 {
                frame.read_to_end(&mut data).unwrap();
            } else {
                flate2::read::ZlibDecoder::new(frame)
                    .read_to_end(&mut data)
                    .unwrap();
            }
            let mut data = std::io::Cursor::new(data);
            let id = read_varint(&mut data).unwrap();
            let mut body = Vec::new();
            data.read_to_end(&mut body).unwrap();
            (id, body)
        } else {
            read_packet(&mut stream, 1 << 20).unwrap()
        };
        match id {
            0x00 => return false,
            0x02 => return true,
            0x03 => compressed = true,
            0x04 => {
                let message = read_varint(&mut std::io::Cursor::new(body)).unwrap();
                let mut answer = Vec::new();
                write_varint(&mut answer, message);
                answer.push(0);
                let mut packet = Vec::new();
                write_varint(&mut packet, 0x02);
                packet.extend_from_slice(&answer);
                let mut framed = Vec::new();
                if compressed {
                    write_varint(&mut framed, i32::try_from(packet.len() + 1).unwrap());
                    write_varint(&mut framed, 0);
                } else {
                    write_varint(&mut framed, i32::try_from(packet.len()).unwrap());
                }
                framed.extend_from_slice(&packet);
                stream.write_all(&framed).unwrap();
            }
            other => panic!("unexpected login packet {other}"),
        }
    }
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

/// `WORLD_VERSION` as a world version.
async fn world_version(metadata: &MetadataService<PublicClient>) -> String {
    match std::env::var("WORLD_VERSION")
        .ok()
        .filter(|v| !v.is_empty())
    {
        Some(v) if v == "latest" => {
            let (catalog, ..) = metadata.catalog(false).await;
            catalog.unwrap().latest_release.unwrap()
        }
        Some(v) if v == "snapshot" => {
            let (catalog, ..) = metadata.catalog(false).await;
            catalog.unwrap().latest_snapshot.unwrap()
        }
        Some(v) => v,
        None => "1.21.1".to_owned(),
    }
}

/// The protocol of 1.16, whose servers mark the RCON listener running before
/// starting its thread.
const FIRST_STEADY_RCON: i32 = 735;
/// Starts of a server before 1.16 until its RCON listener survives its start.
const RCON_STARTS: usize = 3;

/// Logs in to a ready server's RCON. Servers before 1.13 open it just after the
/// `Done (` line.
fn open_console(recorded: &receipt::ServerReceipt) -> Result<rcon::Rcon, rcon::RconError> {
    let until = Instant::now() + Duration::from_secs(20);
    loop {
        match rcon::Rcon::connect(
            recorded.rcon_port,
            &recorded.rcon_password(),
            Duration::from_secs(5),
        ) {
            Err(rcon::RconError::Unreachable) if Instant::now() < until => {
                std::thread::sleep(Duration::from_millis(250));
            }
            other => return other,
        }
    }
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
    let minecraft = world_version(&metadata).await;
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

    // First run: start, check, stop cleanly. A server before 1.16 starts its
    // RCON listener thread before marking it running, and a thread that wins
    // that race stops at once, leaving the run without a console; such a run
    // is stopped and started again.
    let mut starts = 0;
    let (server, recorded, mut console) = loop {
        starts += 1;
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
        match open_console(&recorded) {
            Ok(console) => break (server, recorded, console),
            Err(rcon::RconError::Unreachable)
                if server.protocol() < FIRST_STEADY_RCON && starts < RCON_STARTS =>
            {
                println!("rcon listener lost its start race, starting again");
                assert!(tokio::task::block_in_place(|| server.stop(STOP_GRACE)));
            }
            Err(error) => panic!("rcon answers: {error:?}"),
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

/// Whether a catalog slug names the project `slug`: itself, or the name the
/// catalog gives a twin from another site when the slug is taken
/// (`fabric-api-modrinth-20953b`).
fn names_project(found: &str, slug: &str) -> bool {
    found == slug
        || found
            .strip_prefix(slug)
            .and_then(|rest| rest.strip_prefix('-'))
            .and_then(|rest| rest.split_once('-'))
            .is_some_and(|(site, id)| {
                site.bytes().all(|b| b.is_ascii_lowercase())
                    && id.len() == 6
                    && id.bytes().all(|b| b.is_ascii_hexdigit())
            })
}

/// The newest release of a catalog project for the version and the loader,
/// from whichever of its sites has one.
async fn catalog_release(
    catalog: &launcher_lib::mods::Client,
    slug: &str,
    minecraft: &str,
    loader: &str,
) -> Option<launcher_lib::mods::Version> {
    use launcher_lib::mods::{Catalog as _, SearchQuery};
    let page = catalog
        .search(&SearchQuery {
            text: slug.replace('-', " "),
            kind: Some("mod".to_owned()),
            ..SearchQuery::default()
        })
        .await
        .unwrap();
    for project in page.hits.iter().filter(|h| names_project(&h.slug, slug)) {
        let release = catalog
            .versions(&project.id, Some(minecraft), Some(loader))
            .await
            .unwrap()
            .into_iter()
            .find(|v| v.channel == "release");
        if release.is_some() {
            return release;
        }
    }
    None
}

/// The newest catalog file of a project, for the version and the loader,
/// downloaded through the launcher's verified path into `mods/`.
async fn catalog_mod(
    catalog: &launcher_lib::mods::Client,
    executor: &Executor,
    instance: &str,
    slug: &str,
    minecraft: &str,
    loader: &str,
) -> String {
    let newest = catalog_release(catalog, slug, minecraft, loader)
        .await
        .unwrap_or_else(|| panic!("{slug} has a release for this version"));
    keep_mod(catalog, executor, instance, slug, &newest).await
}

/// A catalog version's file, downloaded into the installation's `mods/` as
/// `<slug>.jar`.
async fn keep_mod(
    catalog: &launcher_lib::mods::Client,
    executor: &Executor,
    instance: &str,
    slug: &str,
    newest: &launcher_lib::mods::Version,
) -> String {
    use launcher_lib::mods::Catalog as _;
    // Files the catalog does not keep yet are fetched on the first ask, and
    // scanned: a few minutes when many are asked at once.
    let mut file = None;
    for _ in 0..150 {
        catalog.fetch(&newest.id).await.unwrap();
        let version = catalog.version(&newest.id).await.unwrap();
        // The file an installation takes, once the catalog keeps it.
        file = version
            .main_file()
            .filter(|f| f.sha256.is_some() && f.url.is_some() && !f.held)
            .cloned();
        if file.is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
    let file = file.unwrap_or_else(|| panic!("{slug} file kept by the catalog"));
    let name = format!("{slug}.jar");
    executor
        .run(
            uuid::Uuid::new_v4(),
            vec![launcher_lib::download::DownloadRequest {
                source: SourcePolicy::ModsCatalog,
                url: SourcePolicy::ModsCatalog
                    .check(file.url.as_deref().unwrap())
                    .unwrap(),
                path: format!("instances/{instance}/game/mods/{name}"),
                size: file.size,
                digest: launcher_lib::download::Digest::Sha256(
                    launcher_lib::storage::paths::Sha256::parse(file.sha256.as_deref().unwrap())
                        .unwrap(),
                ),
                label: None,
            }],
            &CancelToken::new(),
            &NoProgress,
        )
        .await
        .unwrap();
    name
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "network: downloads a loader server, mods and Java, runs the server"]
async fn host_a_world_with_mods() {
    use launcher_lib::loader::{LoaderChoice, LoaderKind, meta};
    use launcher_lib::worlds::pack::{self, Side};

    let dir = tempfile::tempdir().unwrap();
    let root = DataRoot::new(dir.path().to_path_buf()).unwrap();
    let metadata = Arc::new(MetadataService::new(
        PublicClient::new(SourcePolicy::MojangMetadata, VERSION).unwrap(),
        root.clone(),
    ));
    let minecraft = world_version(&metadata).await;
    // Quilt runs Fabric mods: Fabric API serves both. On Forge and NeoForge the
    // first of these mods with a release for the version proves the loader
    // loads mods (by its mod id); Sodium, where it has one, stays on the
    // client.
    const FORGE_PROOFS: &[(&str, &str)] = &[
        ("jei", "jei"),
        ("appleskin", "appleskin"),
        ("clumps", "clumps"),
        ("ferrite-core", "ferritecore"),
        ("spark", "spark"),
        ("bookshelf-lib", "bookshelf"),
    ];
    let (kind, catalog_loader, proofs): (_, _, &[(&str, &str)]) =
        match std::env::var("WORLD_LOADER").as_deref().unwrap_or("fabric") {
            "fabric" => (LoaderKind::Fabric, "fabric", &[("fabric-api", "fabric")]),
            "quilt" => (LoaderKind::Quilt, "fabric", &[("fabric-api", "fabric")]),
            "neoforge" => (LoaderKind::NeoForge, "neoforge", FORGE_PROOFS),
            "forge" => (LoaderKind::Forge, "forge", FORGE_PROOFS),
            other => panic!("WORLD_LOADER {other} has no live test yet"),
        };
    println!("minecraft {minecraft}, {}", kind.name());
    let loaders = PublicClient::new(SourcePolicy::ModLoaders, VERSION).unwrap();
    let loader = LoaderChoice {
        kind,
        version: meta::loader_versions(&loaders, kind, &minecraft)
            .await
            .unwrap()
            .into_iter()
            .find(|v| v.stable)
            .expect("a stable loader")
            .id,
    };
    println!("loader {}", loader.version);
    pack::check_supported(&loader, &minecraft).expect("a tested release");

    // An installation's game folder: mods from the catalog, a setting.
    let transport = HttpTransport::public(VERSION).unwrap();
    let executor = Executor::new(transport.clone(), root.clone(), ExecutorConfig::default());
    let catalog = launcher_lib::mods::Client::new(VERSION).unwrap();
    let instance = launcher_lib::storage::paths::InstanceId::generate();
    let mut proof = None;
    for (slug, id) in proofs {
        if let Some(release) = catalog_release(&catalog, slug, &minecraft, catalog_loader).await {
            proof = Some((*slug, *id, release));
            break;
        }
    }
    let (proof_slug, proof_id, release) = proof.unwrap_or_else(|| {
        panic!("the catalog has none of {proofs:?} for {catalog_loader} {minecraft}")
    });
    let mut installed = vec![(
        keep_mod(
            &catalog,
            &executor,
            &instance.to_string(),
            proof_slug,
            &release,
        )
        .await,
        Side::Both,
    )];
    if let Some(release) = catalog_release(&catalog, "sodium", &minecraft, catalog_loader).await {
        installed.push((
            keep_mod(
                &catalog,
                &executor,
                &instance.to_string(),
                "sodium",
                &release,
            )
            .await,
            Side::Client,
        ));
    }
    println!("mods {installed:?}");
    let game = root.game_dir(instance);
    std::fs::create_dir_all(game.join("config")).unwrap();
    std::fs::write(game.join("config/live-test.json"), b"{\"kept\":true}").unwrap();
    std::fs::write(game.join("options.txt"), b"fov:1").unwrap();

    let harmful = catalog.harmful().await.unwrap();
    let built = pack::build(&catalog, &harmful, &game, &loader)
        .await
        .unwrap();
    let mut created = world::create(
        &root,
        "Live modded world",
        &minecraft,
        WorldSettings::default(),
    )
    .unwrap();
    let generation = pack::store(&root, &created, built).unwrap();
    let side = |name: &str| {
        generation
            .files
            .iter()
            .find(|f| f.path == format!("mods/{name}"))
            .unwrap()
            .side
    };
    // The proof runs on the server (on both sides, or the server's alone by
    // the catalog's word); Sodium on the client only.
    for (name, expected) in &installed {
        match expected {
            Side::Client => assert_eq!(side(name), Side::Client, "{name}"),
            _ => assert!(side(name).on_server(), "{name}"),
        }
    }
    assert!(
        generation.files.iter().all(|f| f.path != "options.txt"),
        "the player's options stay"
    );
    println!("generation of {} files", generation.files.len());
    created.pack = Some(generation);
    world::save(&root, &created).unwrap();
    world::accept_eula(&root, created.id).unwrap();

    let source = LiveSource::new(Arc::clone(&metadata), VERSION).unwrap();
    let runtimes = RuntimeInstaller::new(root.clone(), transport, ExecutorConfig::default());
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
    assert!(
        prepared.pack.as_ref().unwrap().server_manifest.is_some(),
        "the loader's server is pinned"
    );
    println!("prepared in {:.1?}", started.elapsed());

    let activity = ActivityRegistry::new();
    let members = [Nick::parse("Host_Player").unwrap()];
    let permit = activity
        .try_acquire_hosting(created.id.uuid(), "live-mods".to_owned())
        .unwrap();
    let server = host::start(
        &Launch {
            root: &root,
            world_id: created.id,
            java: &java,
            server_heap_mib: 2048,
            game_heap_mib: 0,
            physical_memory_bytes: physical_memory(),
            members: &members,
            operation_id: "live-mods".to_owned(),
        },
        permit,
        Arc::new(|| {}),
    )
    .unwrap();
    let started = Instant::now();
    tokio::task::block_in_place(|| server.wait_ready(STARTUP_DEADLINE)).unwrap();
    println!("ready in {:.1?}", started.elapsed());

    let server_dir = root.world_server_dir(created.id);
    let mut mods: Vec<String> = std::fs::read_dir(server_dir.join("mods"))
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    mods.sort();
    let server_mods: Vec<&String> = installed
        .iter()
        .filter(|(_, side)| side.on_server())
        .map(|(name, _)| name)
        .collect();
    assert_eq!(
        mods.iter().collect::<Vec<_>>(),
        server_mods,
        "only the server's mods"
    );
    // Fabric Loader unpacks the modules nested in the mods it loads (Fabric
    // API of 1.14 and 1.15 nests none).
    if kind == LoaderKind::Fabric {
        let processed: Vec<String> = std::fs::read_dir(server_dir.join(".fabric/processedMods"))
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .map(|e| e.file_name().to_string_lossy().to_lowercase())
            .collect();
        assert!(!processed.iter().any(|n| n.contains("sodium")));
        println!("{} nested modules unpacked", processed.len());
    }
    assert_eq!(
        std::fs::read(server_dir.join("config/live-test.json")).unwrap(),
        b"{\"kept\":true}"
    );

    let protocol = server.protocol();
    let member = login(protocol, server.port(), &members[0]);
    tunnel_reads(&member, protocol, &members[0]);
    assert!(logged_in(server.port(), &member), "a member gets in");
    let stranger_nick = Nick::parse("Not_A_Member").unwrap();
    let stranger = login(protocol, server.port(), &stranger_nick);
    assert!(
        !logged_in(server.port(), &stranger),
        "the whitelist refuses"
    );
    println!("member in, stranger refused");

    if matches!(kind, LoaderKind::Fabric | LoaderKind::Quilt) {
        // Fabric API brings data, which it lists as mods' data packs
        // (`fabric/<mod id>` on 1.14, `Fabric Mods (Fabric mod)` on 1.19.2);
        // without a mod with data only `vanilla` shows.
        let packs = data_packs(&server_dir).to_lowercase();
        println!("data packs: {packs}");
        assert!(packs.contains("fabric"), "Fabric API loaded");
        assert!(!packs.contains("sodium"));
        println!("fabric api data loaded");
    }
    let clean = tokio::task::block_in_place(|| server.stop(STOP_GRACE));
    assert!(clean);
    assert!(!activity.is_busy());
    assert!(
        server_dir.join("world").join("level.dat").is_file(),
        "the world was saved"
    );
    if matches!(kind, LoaderKind::Forge | LoaderKind::NeoForge) {
        // FML records the mods it loaded in the save's `level.dat`, from
        // 1.7.10's Forge to now, and NeoForge's.
        use std::io::Read as _;
        let mut level = Vec::new();
        flate2::read::GzDecoder::new(
            std::fs::File::open(server_dir.join("world/level.dat")).unwrap(),
        )
        .read_to_end(&mut level)
        .unwrap();
        // Ids by any case: 1.7.10 and 1.8.9 write `Forge` and `FML`.
        level.make_ascii_lowercase();
        let recorded = |id: &str| level.windows(id.len()).any(|w| w == id.as_bytes());
        // `neoforge`, or `forge` for NeoForge 1.20.1, which kept Forge's id.
        assert!(recorded("forge"), "the loader recorded");
        assert!(recorded(proof_id), "{proof_id} loaded");
        assert!(!recorded("sodium"));
        println!("{} loaded {proof_id}", kind.name());
    }
    // No chat or nicks in files: a loader that writes its own logs writes
    // nothing into them.
    for entry in std::fs::read_dir(server_dir.join("logs"))
        .into_iter()
        .flatten()
        .flatten()
    {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.ends_with(".log") {
            assert_eq!(entry.metadata().unwrap().len(), 0, "{name} is empty");
        }
    }
    println!("stopped clean");
}

/// The server's `datapack list` through its RCON: the mods' data shows there
/// whichever loader loaded them.
fn data_packs(server_dir: &std::path::Path) -> String {
    let properties = std::fs::read_to_string(server_dir.join("server.properties")).unwrap();
    let value = |key: &str| {
        properties
            .lines()
            .find_map(|l| l.strip_prefix(key)?.strip_prefix('='))
            .unwrap_or_else(|| panic!("{key} is set"))
            .trim()
            .to_owned()
    };
    let mut rcon = launcher_lib::worlds::rcon::Rcon::connect(
        value("rcon.port").parse().unwrap(),
        &launcher_lib::secrets::SecretString::new(value("rcon.password")),
        Duration::from_secs(10),
    )
    .unwrap_or_else(|_| panic!("rcon answers"));
    rcon.command("datapack list").unwrap()
}

/// Starts a world's server, waits until it is ready or fails, and stops it.
async fn start_world(
    root: &DataRoot,
    world_id: launcher_lib::storage::paths::WorldId,
    java: &std::path::Path,
    activity: &ActivityRegistry,
) -> launcher_lib::error::LauncherResult<()> {
    let members = [Nick::parse("Host_Player").unwrap()];
    let permit = activity
        .try_acquire_hosting(world_id.uuid(), "live-update".to_owned())
        .unwrap();
    let server = host::start(
        &Launch {
            root,
            world_id,
            java,
            server_heap_mib: 2048,
            game_heap_mib: 0,
            physical_memory_bytes: physical_memory(),
            members: &members,
            operation_id: "live-update".to_owned(),
        },
        permit,
        Arc::new(|| {}),
    )?;
    tokio::task::block_in_place(|| {
        let ready = server.wait_ready(STARTUP_DEADLINE);
        let clean = server.stop(STOP_GRACE);
        ready.map(|()| assert!(clean, "stopped clean"))
    })
}

/// Whether `save` holds exactly the files of `snapshot`, by content.
fn holds_backup(save: &std::path::Path, snapshot: &launcher_lib::backup::store::Snapshot) -> bool {
    use sha2::Digest as _;
    let mut count = 0usize;
    let mut stack = vec![save.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(dir).unwrap().filter_map(Result::ok) {
            if entry.file_type().unwrap().is_dir() {
                stack.push(entry.path());
            } else {
                count += 1;
            }
        }
    }
    count == snapshot.files.len()
        && snapshot.files.iter().all(|entry| {
            std::fs::read(save.join(&entry.path)).is_ok_and(|bytes| {
                bytes.len() as u64 == entry.size
                    && hex::encode(sha2::Sha256::digest(&bytes)) == entry.sha256
            })
        })
}

/// The mods in a world server's `mods/`, by name.
fn server_mods(root: &DataRoot, world_id: launcher_lib::storage::paths::WorldId) -> Vec<String> {
    let mut mods: Vec<String> = std::fs::read_dir(root.world_server_dir(world_id).join("mods"))
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    mods.sort();
    mods
}

/// W12 update and rollback (A45): a played world with Fabric API gets
/// JEI as an update, tried on a copy of its backup first and confirmed by
/// a start. An update whose Fabric API is built for another release does not
/// start: on the copy it leaves the world as it was, and on the world itself
/// the backup and the earlier mods come back together.
/// `WORLD_VERSION=1.21.1 cargo test --test live_server update_a_world_with_mods -- --ignored --exact --nocapture`
#[tokio::test(flavor = "multi_thread")]
#[ignore = "network: downloads a loader server, mods and Java, runs the server"]
async fn update_a_world_with_mods() {
    use launcher_lib::backup::store::Reason;
    use launcher_lib::loader::{LoaderChoice, LoaderKind, meta};
    use launcher_lib::storage::paths::InstanceId;
    use launcher_lib::worlds::{pack, safety, update};

    let dir = tempfile::tempdir().unwrap();
    let root = DataRoot::new(dir.path().to_path_buf()).unwrap();
    let metadata = Arc::new(MetadataService::new(
        PublicClient::new(SourcePolicy::MojangMetadata, VERSION).unwrap(),
        root.clone(),
    ));
    let minecraft = world_version(&metadata).await;
    // Another release, whose Fabric API this one refuses.
    let other = if minecraft == "1.20.1" {
        "1.21.1"
    } else {
        "1.20.1"
    };
    println!("minecraft {minecraft}, fabric, broken by mods of {other}");
    let loaders = PublicClient::new(SourcePolicy::ModLoaders, VERSION).unwrap();
    let loader = LoaderChoice {
        kind: LoaderKind::Fabric,
        version: meta::loader_versions(&loaders, LoaderKind::Fabric, &minecraft)
            .await
            .unwrap()
            .into_iter()
            .find(|v| v.stable)
            .expect("a stable Fabric loader")
            .id,
    };
    pack::check_supported(&loader, &minecraft).expect("a tested release");

    let transport = HttpTransport::public(VERSION).unwrap();
    let executor = Executor::new(transport.clone(), root.clone(), ExecutorConfig::default());
    let catalog = launcher_lib::mods::Client::new(VERSION).unwrap();
    let harmful = catalog.harmful().await.unwrap();
    let source = LiveSource::new(Arc::clone(&metadata), VERSION).unwrap();
    let runtimes = RuntimeInstaller::new(root.clone(), transport, ExecutorConfig::default());
    let activity = ActivityRegistry::new();
    let installation = |mods: &[(&str, &str)], setting: &'static [u8]| {
        let (catalog, executor, root) = (&catalog, &executor, &root);
        let mods: Vec<(String, String)> = mods
            .iter()
            .map(|(s, v)| ((*s).to_owned(), (*v).to_owned()))
            .collect();
        async move {
            let instance = InstanceId::generate();
            for (slug, release) in &mods {
                catalog_mod(
                    catalog,
                    executor,
                    &instance.to_string(),
                    slug,
                    release,
                    "fabric",
                )
                .await;
            }
            let game = root.game_dir(instance);
            std::fs::create_dir_all(game.join("config")).unwrap();
            std::fs::write(game.join("config/live-test.json"), setting).unwrap();
            game
        }
    };

    // Generation 1, played once.
    let game = installation(&[("fabric-api", &minecraft)], b"{\"rate\":1}").await;
    let built = pack::build(&catalog, &harmful, &game, &loader)
        .await
        .unwrap();
    let mut created = world::create(
        &root,
        "Live updated world",
        &minecraft,
        WorldSettings::default(),
    )
    .unwrap();
    created.pack = Some(pack::store(&root, &created, built).unwrap());
    world::save(&root, &created).unwrap();
    world::accept_eula(&root, created.id).unwrap();
    let id = created.id;
    let cancel = CancelToken::new();
    let prepare = |world_id| {
        install::prepare(
            &root,
            &metadata,
            &source,
            &runtimes,
            &executor,
            world_id,
            JavaProvider::Mojang,
            &cancel,
            &NoProgress,
        )
    };
    let (_, java) = prepare(id).await.unwrap();
    start_world(&root, id, &java, &activity).await.unwrap();
    println!("generation 1 played");

    let trial = update::Trial {
        root: &root,
        metadata: &metadata,
        source: &source,
        runtimes: &runtimes,
        executor: &executor,
        activity: &activity,
        java: JavaProvider::Mojang,
        game_heap_mib: 0,
        physical_memory_bytes: physical_memory(),
        cancel: &cancel,
        sink: &NoProgress,
        starting: &|| {},
    };
    let no_choices = std::collections::HashMap::new();

    // Generation 2 adds JEI: tried on a copy, kept, confirmed by a start.
    let game = installation(
        &[("fabric-api", &minecraft), ("jei", &minecraft)],
        b"{\"rate\":2}",
    )
    .await;
    let built = pack::build(&catalog, &harmful, &game, &loader)
        .await
        .unwrap();
    let world = world::load(&root, id).unwrap();
    let shown = update::preview(&root, &world, &built).unwrap();
    assert_eq!(shown.added.len(), 1, "jei added");
    assert!(shown.conflicts.is_empty(), "the host changed no setting");
    assert_eq!(shown.settings, 1);
    let staged = update::stage(&root, &world, built, &shown.fingerprint, &no_choices).unwrap();
    let backup = safety::backup(&root, id, Reason::BeforeUpdate)
        .unwrap()
        .unwrap()
        .id;
    update::begin(&root, id).unwrap();
    let started = Instant::now();
    let manifest = update::try_on_copy(&trial, id, &staged, Some(&backup))
        .await
        .unwrap();
    println!("generation 2 tried on a copy in {:.1?}", started.elapsed());
    assert!(world::trials(&root).is_empty(), "the copy is gone");
    update::commit(&root, id, staged, manifest, Some(backup)).unwrap();
    let (prepared, java) = prepare(id).await.unwrap();
    assert!(update::pending(&prepared));
    start_world(&root, id, &java, &activity).await.unwrap();
    update::confirm(&root, id).unwrap();
    assert_eq!(server_mods(&root, id).len(), 2, "fabric api and jei");
    assert_eq!(
        std::fs::read(root.world_server_dir(id).join("config/live-test.json")).unwrap(),
        b"{\"rate\":2}"
    );
    println!("generation 2 confirmed");

    // Generation 3 brings Fabric API of another release.
    let game = installation(&[("fabric-api", other)], b"{\"rate\":3}").await;
    let broken = || pack::build(&catalog, &harmful, &game, &loader);
    let world = world::load(&root, id).unwrap();
    let shown = update::preview(&root, &world, &broken().await.unwrap()).unwrap();
    let staged = update::stage(
        &root,
        &world,
        broken().await.unwrap(),
        &shown.fingerprint,
        &no_choices,
    )
    .unwrap();
    let backup = safety::backup(&root, id, Reason::BeforeUpdate)
        .unwrap()
        .unwrap()
        .id;
    update::begin(&root, id).unwrap();
    let tried = update::try_on_copy(&trial, id, &staged, Some(&backup)).await;
    assert!(tried.is_err(), "the copy does not start");
    update::abandon(&root, id).unwrap();
    let world = world::load(&root, id).unwrap();
    assert_eq!(
        world.pack.as_ref().unwrap().generation,
        2,
        "the world as it was"
    );
    assert!(world::trials(&root).is_empty());
    println!("generation 3 refused on the copy");

    // The same generation kept without its trial: the world's own start
    // fails, and its backup and generation 2 come back.
    let world = world::load(&root, id).unwrap();
    let shown = update::preview(&root, &world, &broken().await.unwrap()).unwrap();
    let staged = update::stage(
        &root,
        &world,
        broken().await.unwrap(),
        &shown.fingerprint,
        &no_choices,
    )
    .unwrap();
    update::commit(&root, id, staged, None, Some(backup.clone())).unwrap();
    let (_, java) = prepare(id).await.unwrap();
    assert!(start_world(&root, id, &java, &activity).await.is_err());
    assert!(update::roll_back(&root, id).unwrap());
    let world = world::load(&root, id).unwrap();
    assert_eq!(world.pack.as_ref().unwrap().generation, 2);
    let snapshot = safety::store(&root).load(&backup).unwrap();
    assert!(
        holds_backup(&safety::save_dir(&root, id), &snapshot),
        "the save is the backup"
    );
    let (_, java) = prepare(id).await.unwrap();
    start_world(&root, id, &java, &activity).await.unwrap();
    assert_eq!(server_mods(&root, id).len(), 2);
    println!("generation 3 rolled back, generation 2 starts again");
}
