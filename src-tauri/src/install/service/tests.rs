//! Install/repair orchestration against the local fixture server, a synthetic
//! game plan and a fake `java` (Unix only).

use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use sha1::Digest as _;

use super::*;
use crate::download::tests::{Reply, Server, content, fast_config};
use crate::install::plan::{AssetIndexPlan, LoggingPlan, PlannedArtifact, RuntimeRequirement};
use crate::runtime::adoptium::ArchiveFormat;
use crate::runtime::extract::tests::{E, tar_gz};
use crate::state::dto::BackendEvent;
use crate::state::{EventSink, dto::MutationRequest};
use crate::storage::models::ArtifactRole;
use crate::storage::paths::{DataRoot, ImageType, RuntimeKey, Sha256, TargetArch, TargetOs};
use crate::validation::MemoryPolicy;

#[derive(Clone, Default)]
pub(crate) struct Events(Arc<Mutex<Vec<(String, serde_json::Value)>>>);

impl EventSink for Events {
    fn emit(&self, event: &BackendEvent) {
        lock_or(&self.0).push((event.name().to_owned(), event.payload()));
    }
}

impl Events {
    fn named(&self, name: &str) -> Vec<serde_json::Value> {
        lock_or(&self.0)
            .iter()
            .filter(|(n, _)| n == name)
            .map(|(_, p)| p.clone())
            .collect()
    }
}

pub(crate) struct FakeSource {
    pub(crate) plan: Mutex<(GamePlan, RuntimePackage)>,
    pub(crate) resolves: AtomicUsize,
}

impl PlanSource for FakeSource {
    fn resolve<'a>(
        &'a self,
        _version: &'a str,
        _java: crate::storage::paths::JavaProvider,
    ) -> BoxFuture<'a, LauncherResult<(GamePlan, RuntimePackage)>> {
        self.resolves.fetch_add(1, Ordering::SeqCst);
        let plan = lock_or(&self.plan).clone();
        Box::pin(async move { Ok(plan) })
    }

    fn locked<'a>(&'a self, _lock: &'a InstallLock) -> BoxFuture<'a, LauncherResult<GamePlan>> {
        let plan = lock_or(&self.plan).0.clone();
        Box::pin(async move { Ok(plan) })
    }
}

pub(crate) fn sha1(bytes: &[u8]) -> Sha1 {
    Sha1::parse(&hex::encode(sha1::Sha1::digest(bytes))).unwrap()
}

type FinalInstallPlanParts = (GamePlan, RuntimePackage);

pub(crate) fn state_at(root: &DataRoot, events: Events) -> AppState {
    AppState::initialize(
        Ok(root.clone()),
        "test",
        MemoryPolicy::from_physical_bytes(16 * 1024 * 1024 * 1024),
        Box::new(events),
    )
}

pub(crate) fn create_in(state: &AppState, name: &str) -> InstanceId {
    state
        .create_instance(MutationRequest {
            request_id: Uuid::new_v4().to_string(),
            payload: crate::state::dto::CreateInstancePayload {
                display_name: name.into(),
                version_id: "1.21.1".into(),
            },
        })
        .unwrap()
        .id
}

pub(crate) struct Fixture {
    pub(crate) _dir: tempfile::TempDir,
    pub(crate) root: DataRoot,
    pub(crate) server: Server,
    pub(crate) state: Arc<AppState>,
    pub(crate) events: Events,
    pub(crate) source: Arc<FakeSource>,
    pub(crate) service: InstallService,
    pub(crate) library: Vec<u8>,
}

fn artifact(
    server: &Server,
    role: ArtifactRole,
    path: String,
    url_path: &str,
    body: &[u8],
) -> PlannedArtifact {
    server.serve(url_path, body);
    PlannedArtifact {
        role,
        path,
        url: server.url(url_path).to_string(),
        sha1: sha1(body),
        size: body.len() as u64,
        provenance: None,
    }
}

fn java_package(server: &Server, marker: &Path) -> RuntimePackage {
    let script = format!(
        "#!/bin/sh\ntouch '{}'\ncat >&2 <<'X'\n    java.specification.version = 21\n    java.version = 21.0.1\n    os.arch = amd64\nX\nfor a in \"$@\"; do echo \"ARG:$a\"; done\ncase \"$*\" in *Sleeper*) exec sleep 30;; *Failing*) exit 3;; esac\n",
        marker.display()
    );
    let bytes = tar_gz(&[
        E::Dir("jre/"),
        E::File("jre/bin/java", script.as_bytes(), 0o755),
        E::File("jre/lib/modules", b"modules", 0o644),
    ]);
    server.serve("/jre.tar.gz", &bytes);
    RuntimePackage {
        provider: "temurin".into(),
        key: RuntimeKey {
            provider: crate::storage::paths::JavaProvider::Temurin,
            os: TargetOs::Linux,
            arch: TargetArch::X64,
            major: 21,
            image_type: ImageType::Jre,
            archive_sha256: Sha256::parse(&hex::encode(sha2::Sha256::digest(&bytes))).unwrap(),
        },
        release_name: "jdk-21.0.1+1".into(),
        openjdk_version: "21.0.1+1".into(),
        archive_name: "jre.tar.gz".into(),
        archive_url: server.url("/jre.tar.gz").to_string(),
        archive_size: bytes.len() as u64,
        archive_format: ArchiveFormat::TarGz,
        jdk_fallback: false,
    }
}

impl Fixture {
    pub(crate) async fn new() -> Self {
        Self::new_after(|_, _| {}).await
    }

    /// `before` runs on the data directory before this fixture's app state
    /// takes the DATA_ROOT lock, e.g. to simulate an earlier process.
    pub(crate) async fn new_after(before: impl FnOnce(&DataRoot, FinalInstallPlanParts)) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = DataRoot::new(dir.path().join("data")).unwrap();
        let server = Server::start().await;
        let library = content(1, 50_000);
        let client = content(2, 80_000);
        let (a1, a2) = (content(3, 300), content(4, 400));
        let index = serde_json::json!({"objects": {
            "minecraft/sounds/a.ogg": {"hash": sha1(&a1).to_string(), "size": a1.len()},
            "minecraft/lang/b.json": {"hash": sha1(&a2).to_string(), "size": a2.len()},
        }})
        .to_string()
        .into_bytes();
        let log = b"<Configuration/>".to_vec();
        let lib = artifact(
            &server,
            ArtifactRole::Library,
            format!("shared/libraries/{}/artifact.jar", sha1(&library)),
            "/lib.jar",
            &library,
        );
        let client_a = artifact(
            &server,
            ArtifactRole::Client,
            format!("shared/clients/{}/client.jar", sha1(&client)),
            "/client.jar",
            &client,
        );
        let index_a = artifact(
            &server,
            ArtifactRole::AssetIndex,
            format!("metadata/asset-indexes/{}.json", sha1(&index)),
            "/index.json",
            &index,
        );
        let log_a = artifact(
            &server,
            ArtifactRole::LoggingConfig,
            format!("shared/logging/{}/client-1.21.2.xml", sha1(&log)),
            "/log.xml",
            &log,
        );
        let assets = [a1, a2]
            .iter()
            .enumerate()
            .map(|(i, body)| {
                let hash = sha1(body).to_string();
                artifact(
                    &server,
                    ArtifactRole::AssetObject,
                    format!("shared/assets/objects/{}/{hash}", &hash[..2]),
                    &format!("/asset{i}"),
                    body,
                )
            })
            .collect();
        let marker = dir.path().join("java-ran");
        let runtime = java_package(&server, &marker);
        let game = GamePlan {
            version_id: "1.21.1".into(),
            version_type: "release".into(),
            descriptor_url: "https://piston-meta.mojang.com/v1/packages/x/1.21.1.json".into(),
            descriptor_sha1: Sha1::parse("499680d659b7a0ccb2fa9a809c345c5baf2ace7d").unwrap(),
            main_class: "net.minecraft.client.main.Main".into(),
            classpath: vec![lib.path.clone(), client_a.path.clone()],
            client: client_a,
            libraries: vec![lib],
            asset_index: AssetIndexPlan {
                id: SafeName::parse("17").unwrap(),
                artifact: index_a,
                declared_total_size: 700,
            },
            assets,
            asset_layout: crate::install::plan::AssetLayout::Modern,
            natives: Vec::new(),
            logging: Some(LoggingPlan {
                id: SafeName::parse("client-1.21.2.xml").unwrap(),
                argument: "-Dlog4j.configurationFile=${path}".into(),
                artifact: log_a,
            }),
            jvm_arguments: vec![
                "-Djava.library.path=${natives_directory}".into(),
                "-Djna.tmpdir=${natives_directory}/jna".into(),
                "-cp".into(),
                "${classpath}".into(),
            ],
            game_arguments: [
                "--username",
                "${auth_player_name}",
                "--version",
                "${version_name}",
                "--gameDir",
                "${game_directory}",
                "--accessToken",
                "${auth_access_token}",
                "--width",
                "${resolution_width}",
            ]
            .map(String::from)
            .to_vec(),
            runtime: RuntimeRequirement {
                major: 21,
                os: TargetOs::Linux,
                arch: TargetArch::X64,
                component: None,
            },
        };
        before(&root, (game.clone(), runtime.clone()));
        let events = Events::default();
        let state = Arc::new(state_at(&root, events.clone()));
        let source = Arc::new(FakeSource {
            plan: Mutex::new((game, runtime)),
            resolves: AtomicUsize::new(0),
        });
        let service = InstallService::new(
            Arc::new(Arc::clone(&state)),
            Arc::clone(&source) as Arc<dyn PlanSource>,
            HttpTransport::loopback(),
            fast_config(),
        );
        Self {
            _dir: dir,
            root,
            server,
            state,
            events,
            source,
            service,
            library,
        }
    }

    pub(crate) fn create(&self, name: &str) -> InstanceId {
        create_in(&self.state, name)
    }

    pub(crate) async fn wait(&self, operation_id: &str) -> OperationResult {
        for _ in 0..500 {
            if let Some(result) = self.state.operation_result(operation_id) {
                return result;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("operation did not finish");
    }

    pub(crate) async fn run(&self, kind: OperationKind, id: InstanceId) -> OperationResult {
        let started = self
            .service
            .start(&Uuid::new_v4().to_string(), kind, id)
            .unwrap();
        self.wait(&started.operation_id).await
    }

    pub(crate) fn library_path(&self) -> std::path::PathBuf {
        self.root.path().join(format!(
            "shared/libraries/{}/artifact.jar",
            sha1(&self.library)
        ))
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn fresh_install_commits_lock_then_summary_and_second_instance_reuses() {
    let f = Fixture::new().await;
    let a = f.create("A");
    let result = f.run(OperationKind::Install, a).await;
    assert_eq!(result.outcome, Outcome::Succeeded, "{:?}", result.error);
    assert_eq!(
        result.downloaded_files, 7,
        "6 game files and the Java archive"
    );

    let lock = match reconcile::read_lock(&f.root.install_lock(a)).unwrap() {
        LockState::Complete(lock) => lock,
        other => panic!("{other:?}"),
    };
    assert_eq!(lock.artifacts.len(), 6);
    assert_eq!(lock.runtime.major, 21);
    let instance = f.state.instance(a).unwrap();
    assert_eq!(instance.health, InstallationHealth::Ready);
    assert_eq!(instance.manifest_sha1, Some(lock.descriptor.sha1.clone()));
    assert!(
        f.root.path().join("shared/assets/indexes/17.json").exists(),
        "index projected"
    );
    assert!(f.state.activity().current().is_none(), "permit released");

    // Progress was published; exactly one terminal event.
    let progress = f.events.named("operation_progress");
    assert!(progress.iter().any(|p| p["phase"] == "downloading"));
    assert!(progress.iter().any(|p| p["phase"] == "extracting_runtime"));
    assert!(progress.iter().any(|p| p["phase"] == "finalizing"));
    assert_eq!(f.events.named("operation_finished").len(), 1);
    assert!(f.state.snapshot().operation_progress.is_none());

    // MV01: a second instance of the same version transfers nothing.
    let b = f.create("B");
    let result = f.run(OperationKind::Install, b).await;
    assert_eq!(result.outcome, Outcome::Succeeded);
    assert_eq!((result.downloaded_files, result.network_bytes), (0, 0));
    assert_eq!(f.server.hits("/jre.tar.gz"), 1, "verified runtime reused");
    assert_ne!(f.root.game_dir(a), f.root.game_dir(b));
}

#[tokio::test(flavor = "multi_thread")]
async fn cancelled_install_resumes_with_the_frozen_plan_and_kept_files() {
    let f = Fixture::new().await;
    let library = f.library.clone();
    f.server.route("/lib.jar", move |n| {
        if n == 0 {
            Reply::Stall {
                body: library.clone(),
                first: 100,
                wait: Duration::from_secs(30),
            }
        } else {
            Reply::Body(library.clone())
        }
    });
    let a = f.create("A");
    let started = f
        .service
        .start(&Uuid::new_v4().to_string(), OperationKind::Install, a)
        .unwrap();
    while f.server.hits("/lib.jar") == 0 {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    tokio::time::sleep(Duration::from_millis(200)).await;
    let ack = f.service.cancel(&started.operation_id).unwrap();
    assert_eq!(ack.state, "cancel_requested");
    let result = f.wait(&started.operation_id).await;
    assert_eq!(result.outcome, Outcome::Cancelled);
    assert_eq!(
        f.service.cancel(&started.operation_id).unwrap().state,
        "cancelled"
    );
    let instance = f.state.instance(a).unwrap();
    assert_eq!(
        instance.health,
        InstallationHealth::NotInstalled,
        "never ready"
    );
    assert_eq!(instance.last_result.unwrap().outcome, Outcome::Cancelled);
    assert!(!f.root.install_lock(a).exists());
    let client_hits = f.server.hits("/client.jar");
    assert_eq!(client_hits, 1, "client finished before the cancel");

    // The catalog would now give another plan; the retry keeps the frozen one.
    lock_or(&f.source.plan).0.main_class = "changed.Main".into();
    let result = f.run(OperationKind::Install, a).await;
    assert_eq!(result.outcome, Outcome::Succeeded, "{:?}", result.error);
    assert_eq!(f.source.resolves.load(Ordering::SeqCst), 1);
    assert_eq!(
        f.server.hits("/client.jar"),
        client_hits,
        "verified file not fetched again"
    );
    assert!(result.reused_files >= 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn repair_replaces_one_file_and_keeps_user_data_identical() {
    let f = Fixture::new().await;
    let a = f.create("A");
    assert_eq!(
        f.run(OperationKind::Install, a).await.outcome,
        Outcome::Succeeded
    );
    let game = f.root.game_dir(a);
    std::fs::create_dir_all(game.join("saves/World/region")).unwrap();
    let sentinels = [
        (game.join("saves/World/level.dat"), content(9, 4096)),
        (game.join("saves/World/region/r.0.0.mca"), content(10, 8192)),
        (game.join("options.txt"), b"fov:90\n".to_vec()),
    ];
    for (path, bytes) in &sentinels {
        std::fs::write(path, bytes).unwrap();
    }
    std::fs::write(f.library_path(), b"corrupted").unwrap();

    let result = f.run(OperationKind::Repair, a).await;
    assert_eq!(result.outcome, Outcome::Succeeded, "{:?}", result.error);
    assert_eq!(result.downloaded_files, 1, "only the damaged file");
    assert_eq!(
        f.server.hits("/jre.tar.gz"),
        1,
        "verified runtime not rebuilt"
    );
    assert_eq!(std::fs::read(f.library_path()).unwrap(), f.library);
    for (path, bytes) in &sentinels {
        assert_eq!(&std::fs::read(path).unwrap(), bytes, "{}", path.display());
    }
    assert_eq!(
        f.state.instance(a).unwrap().health,
        InstallationHealth::Ready
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn failed_repair_of_shared_corruption_invalidates_every_user() {
    let f = Fixture::new().await;
    let (a, b) = (f.create("A"), f.create("B"));
    assert_eq!(
        f.run(OperationKind::Install, a).await.outcome,
        Outcome::Succeeded
    );
    assert_eq!(
        f.run(OperationKind::Install, b).await.outcome,
        Outcome::Succeeded
    );
    std::fs::write(f.library_path(), b"corrupted").unwrap();
    f.server.route("/lib.jar", |_| Reply::Status(404, vec![]));

    let result = f.run(OperationKind::Repair, a).await;
    assert_eq!(result.outcome, Outcome::Failed);
    assert_eq!(
        f.state.instance(a).unwrap().health,
        InstallationHealth::RepairRequired
    );
    assert_eq!(
        f.state.instance(b).unwrap().health,
        InstallationHealth::RepairRequired
    );
    assert!(f.root.install_lock(b).exists(), "locks stay immutable");
}

#[tokio::test(flavor = "multi_thread")]
async fn permits_requests_and_cancellation_are_explicit() {
    let f = Fixture::new().await;
    let library = f.library.clone();
    f.server.route("/lib.jar", move |_| Reply::Stall {
        body: library.clone(),
        first: 10,
        wait: Duration::from_secs(30),
    });
    let (a, b) = (f.create("A"), f.create("B"));
    let request_id = Uuid::new_v4().to_string();
    let first = f
        .service
        .start(&request_id, OperationKind::Install, a)
        .unwrap();
    // The same request again is the same operation; reuse for another is refused.
    assert_eq!(
        f.service
            .start(&request_id, OperationKind::Install, a)
            .unwrap(),
        first
    );
    assert_eq!(
        f.service
            .start(&request_id, OperationKind::Install, b)
            .unwrap_err()
            .code,
        ErrorCode::RequestIdConflict
    );
    let busy = f
        .service
        .start(&Uuid::new_v4().to_string(), OperationKind::Install, b)
        .unwrap_err();
    assert_eq!(busy.code, ErrorCode::AppBusy);
    assert_eq!(
        busy.operation_id.as_deref(),
        Some(first.operation_id.as_str())
    );
    assert_eq!(
        f.service.cancel("unknown").unwrap_err().code,
        ErrorCode::OperationNotFound
    );
    f.service.cancel(&first.operation_id).unwrap();
    assert_eq!(
        f.wait(&first.operation_id).await.outcome,
        Outcome::Cancelled
    );
    assert_eq!(f.events.named("operation_finished").len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn crash_during_install_is_reported_and_resumed() {
    let instance = Arc::new(Mutex::new(None));
    let slot = Arc::clone(&instance);
    let f = Fixture::new_after(move |root, (game, runtime)| {
        // An earlier process created the instance, froze a plan and died
        // mid-download, leaving a running receipt and a partial file.
        let earlier = state_at(root, Events::default());
        let a = create_in(&earlier, "A");
        drop(earlier);
        let crashed = Uuid::new_v4();
        let plan = FinalInstallPlan {
            instance_id: a,
            game,
            runtime,
        };
        JobReceipt::start(root, crashed, OperationKind::Install, Some(a), Some(plan)).unwrap();
        let part = root.download_part(crashed, "sha1", "00");
        std::fs::create_dir_all(part.parent().unwrap()).unwrap();
        std::fs::write(&part, b"partial").unwrap();
        *lock_or(&slot) = Some(a);
    })
    .await;
    let a = lock_or(&instance).unwrap();
    let snapshot = f.state.snapshot();
    assert_eq!(snapshot.recent_operations[0].outcome, Outcome::Interrupted);
    let recorded = f.state.instance(a).unwrap();
    assert_eq!(recorded.last_result.unwrap().outcome, Outcome::Interrupted);
    assert_eq!(recorded.health, InstallationHealth::NotInstalled);
    assert!(
        !f.root.downloads_dir().exists(),
        "staging removed at startup"
    );

    // The frozen plan is used; nothing is resolved again.
    let result = f.run(OperationKind::Install, a).await;
    assert_eq!(result.outcome, Outcome::Succeeded, "{:?}", result.error);
    assert_eq!(f.source.resolves.load(Ordering::SeqCst), 0);
}

#[test]
fn asset_index_projection_follows_the_selected_hash() {
    let dir = tempfile::tempdir().unwrap();
    let root = DataRoot::new(dir.path().to_path_buf()).unwrap();
    let id = SafeName::parse("17").unwrap();
    let (a, b) = (
        b"{\"objects\":{\"a\":1}}".to_vec(),
        b"{\"objects\":{\"b\":2}}".to_vec(),
    );
    for bytes in [&a, &b] {
        let path = root.asset_index_source(&sha1(bytes));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    }
    for bytes in [&a, &b, &a] {
        lock::project_asset_index(&root, &id, &sha1(bytes)).unwrap();
        assert_eq!(
            &std::fs::read(root.asset_index_projection(&id)).unwrap(),
            bytes
        );
    }
    // Both immutable sources are untouched.
    assert_eq!(
        std::fs::read(root.asset_index_source(&sha1(&a))).unwrap(),
        a
    );
    assert_eq!(
        std::fs::read(root.asset_index_source(&sha1(&b))).unwrap(),
        b
    );
    // A damaged source is not projected.
    std::fs::write(root.asset_index_source(&sha1(&b)), b"x").unwrap();
    assert_eq!(
        lock::project_asset_index(&root, &id, &sha1(&b))
            .unwrap_err()
            .code,
        ErrorCode::RepairRequired
    );
}

/// Regression: a frozen plan for a real release is about 1.6 MB and was once set
/// aside as corrupt by the 1 MiB app-document limit, losing the frozen plan.
#[tokio::test]
async fn large_frozen_plans_survive_a_reload() {
    let f = Fixture::new().await;
    let a = f.create("A");
    let (mut game, runtime) = lock_or(&f.source.plan).clone();
    let template = game.assets[0].clone();
    game.assets = (0..6000)
        .map(|i| PlannedArtifact {
            path: format!("shared/assets/objects/ab/{i:040}"),
            provenance: Some(format!("minecraft/sounds/ambient/cave/cave{i}.ogg")),
            ..template.clone()
        })
        .collect();
    let plan = FinalInstallPlan {
        instance_id: a,
        game,
        runtime,
    };
    JobReceipt::start(
        &f.root,
        Uuid::new_v4(),
        OperationKind::Install,
        Some(a),
        Some(plan.clone()),
    )
    .unwrap();
    let file = std::fs::read_dir(f.root.jobs_dir())
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    assert!(std::fs::metadata(&file).unwrap().len() > 1024 * 1024);
    assert_eq!(receipt::resumable_plan(&f.root, a).unwrap(), Some(plan));
    assert!(file.exists(), "not set aside");
}
