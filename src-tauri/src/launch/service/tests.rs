//! Launch supervision with an installed synthetic version and a fake `java`
//! script that echoes its arguments (Unix only). The test authorizer supplies a
//! placeholder identity to exercise the supervisor; production uses the account
//! service and never starts Minecraft without real verified access.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use super::*;
use crate::install::service::tests::Fixture;
use crate::secrets::SecretString;
use crate::state::AppState;

const TOKEN: &str = "TEST-ONLY-TOKEN-7f3a9c2e51";

struct TestHost {
    state: Arc<AppState>,
    handles: Mutex<HashMap<String, Supervised>>,
    finished: Mutex<Vec<String>>,
}

impl StateAccess for TestHost {
    fn app_state(&self) -> &AppState {
        &self.state
    }
}

impl LaunchHost for TestHost {
    fn store_handle(&self, run_id: &str, supervised: Supervised) {
        lock_or(&self.handles).insert(run_id.to_owned(), supervised);
    }
    fn clear_handle(&self, run_id: &str) {
        lock_or(&self.handles).remove(run_id);
    }
    fn kill(&self, run_id: &str) -> bool {
        lock_or(&self.handles)
            .get(run_id)
            .is_some_and(Supervised::kill)
    }
    fn run_started(&self, _run_id: &str) {}
    fn process_exited(&self, _run_id: &str) {}
    fn run_finished(&self, run_id: &str) {
        lock_or(&self.finished).push(run_id.to_owned());
    }
}

/// Test double: a fixed identity, an error, or a slow answer.
struct TestAuth {
    name: &'static str,
    fail: Option<ErrorCode>,
    delay: Duration,
}

impl Authorizer for TestAuth {
    fn authorize(&self) -> BoxFuture<'_, LauncherResult<LaunchIdentity>> {
        Box::pin(async move {
            tokio::time::sleep(self.delay).await;
            if let Some(code) = self.fail {
                return Err(LauncherError::new(code, "Sign in to play."));
            }
            Ok(LaunchIdentity {
                player_name: self.name.into(),
                uuid: "0123456789abcdef0123456789abcdef".into(),
                access_token: SecretString::new(TOKEN),
                xuid: None,
                client_id: "test-client".into(),
                notoken: false,
            })
        })
    }
}

struct Setup {
    f: Fixture,
    host: Arc<TestHost>,
    instance: InstanceId,
}

impl Setup {
    async fn new() -> Self {
        let f = Fixture::new().await;
        let instance = f.create("A");
        assert_eq!(
            f.run(OperationKind::Install, instance).await.outcome,
            Outcome::Succeeded
        );
        let _ = std::fs::remove_file(self::marker(&f));
        let host = Arc::new(TestHost {
            state: Arc::clone(&f.state),
            handles: Mutex::default(),
            finished: Mutex::default(),
        });
        Self { f, host, instance }
    }

    fn service(&self, auth: TestAuth) -> LaunchService {
        LaunchService::new(
            Arc::clone(&self.host) as Arc<dyn LaunchHost>,
            Arc::clone(&self.f.source) as Arc<dyn PlanSource>,
            Arc::new(auth),
            "0.1.0",
        )
    }

    async fn finished(&self, run_id: &str) -> RunSummary {
        for _ in 0..500 {
            if let Some(run) = self
                .f
                .state
                .snapshot()
                .last_run
                .filter(|r| r.run_id == run_id)
            {
                return run;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("run did not finish");
    }
}

fn marker(f: &Fixture) -> PathBuf {
    f._dir.path().join("java-ran")
}

fn auth(name: &'static str) -> TestAuth {
    TestAuth {
        name,
        fail: None,
        delay: Duration::ZERO,
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_launch_is_supervised_redacted_and_recorded() {
    let s = Setup::new().await;
    let service = s.service(auth("Player"));
    let run = service
        .launch(&Uuid::new_v4().to_string(), s.instance)
        .await
        .unwrap();
    assert_eq!(run.kind, RunKind::Game);
    let done = s.finished(&run.run_id).await;
    assert_eq!(done.state, RunState::Exited, "{:?}", done.error);
    assert_eq!(done.exit_code, Some(0));
    let tail = done.output_tail.join("\n");
    assert!(tail.contains("ARG:--accessToken"));
    assert!(tail.contains("ARG:<redacted>"));
    assert!(!tail.contains(TOKEN));
    assert!(tail.contains("ARG:-Xmx4096M"));
    assert!(tail.contains("ARG:Player"));
    let log = std::fs::read_to_string(
        s.f.root
            .run_logs(Uuid::parse_str(&run.run_id).unwrap())
            .join("output.log"),
    )
    .unwrap();
    assert!(log.contains("ARG:<redacted>") && !log.contains(TOKEN));
    assert!(!done.logging_degraded);
    // The console journal (D-037) has the command and the game's lines, never
    // the token.
    let journal: Vec<_> = s.f.state.console().read(0).entries;
    let text: String = journal.iter().map(|e| format!("{}\n", e.text)).collect();
    assert!(!text.contains(TOKEN), "{text}");
    assert!(text.contains("Arguments ("));
    assert!(
        text.contains("  --accessToken <redacted>"),
        "the token argument is redacted: {text}"
    );
    assert!(
        journal
            .iter()
            .any(|e| e.source == crate::console::Source::Game && e.text == "ARG:<redacted>")
    );
    assert!(text.contains(": exited, exit code 0"), "{text}");
    // Run folder (natives, argfile) removed; permit released; result recorded.
    assert!(
        !s.f.root
            .run_dir(s.instance, Uuid::parse_str(&run.run_id).unwrap())
            .exists()
    );
    assert!(s.f.state.activity().current().is_none());
    let instance = s.f.state.instance(s.instance).unwrap();
    let last = instance.last_result.unwrap();
    assert_eq!(
        (last.kind, last.outcome),
        (OperationKind::Launch, Outcome::Succeeded)
    );
    assert!(instance.last_played_at.is_some());
    assert_eq!(
        lock_or(&s.host.finished).as_slice(),
        std::slice::from_ref(&run.run_id)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn jvm_arguments_can_go_through_an_argfile() {
    let s = Setup::new().await;
    let service = s.service(auth("Player")).with_argfile();
    let run = service
        .launch(&Uuid::new_v4().to_string(), s.instance)
        .await
        .unwrap();
    let done = s.finished(&run.run_id).await;
    let tail = done.output_tail.join("\n");
    assert!(tail.contains("ARG:@"), "JVM arguments passed by file");
    assert!(!tail.contains("ARG:-Xmx4096M"));
    assert!(
        tail.contains("ARG:<redacted>"),
        "the token stays a direct argument"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn without_verified_access_java_never_starts() {
    let s = Setup::new().await;
    for code in [
        ErrorCode::AuthNotConfigured,
        ErrorCode::NoGameAccess,
        ErrorCode::ServiceUnavailable,
    ] {
        let service = s.service(TestAuth {
            name: "Player",
            fail: Some(code),
            delay: Duration::ZERO,
        });
        let error = service
            .launch(&Uuid::new_v4().to_string(), s.instance)
            .await
            .unwrap_err();
        assert_eq!(error.code, code);
        assert!(!marker(&s.f).exists(), "no process was started");
        assert!(s.f.state.activity().current().is_none(), "permit released");
        let last = s.f.state.snapshot().last_run.unwrap();
        assert_eq!(last.state, RunState::Failed);
    }
    assert_eq!(
        s.f.state.instance(s.instance).unwrap().health,
        InstallationHealth::Ready
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn notoken_launches_need_no_account_and_always_run_the_demo() {
    let s = Setup::new().await;
    // The account cannot play, but the no-token profile does not ask it.
    let service = s.service(TestAuth {
        name: "Player",
        fail: Some(ErrorCode::AuthNotConfigured),
        delay: Duration::ZERO,
    });
    let run = service
        .launch_as(&Uuid::new_v4().to_string(), s.instance, LaunchMode::NoToken)
        .await
        .unwrap();
    let done = s.finished(&run.run_id).await;
    assert_eq!(done.state, RunState::Exited, "{:?}", done.error);
    let tail = done.output_tail.join("\n");
    assert!(tail.contains("ARG:--demo"));
    assert!(tail.contains(&format!("ARG:{}", crate::launch::notoken::DEFAULT_NAME)));
    assert!(
        !tail.contains("notoken-profile"),
        "placeholder token redacted"
    );
    // A normal launch still requires the account.
    let error = service
        .launch_as(&Uuid::new_v4().to_string(), s.instance, LaunchMode::Account)
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::AuthNotConfigured);
}

#[tokio::test(flavor = "multi_thread")]
async fn missing_files_require_repair_before_any_spawn() {
    let s = Setup::new().await;
    std::fs::remove_file(s.f.library_path()).unwrap();
    let service = s.service(auth("Player"));
    let error = service
        .launch(&Uuid::new_v4().to_string(), s.instance)
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::RepairRequired);
    assert!(!marker(&s.f).exists());
    assert_eq!(
        s.f.state.instance(s.instance).unwrap().health,
        InstallationHealth::RepairRequired
    );
    assert!(
        s.f.state.activity().current().is_none(),
        "no deadlock: permit released"
    );
    // Repair is possible right away and makes the instance playable again.
    assert_eq!(
        s.f.run(OperationKind::Repair, s.instance).await.outcome,
        Outcome::Succeeded
    );
    let run = service
        .launch(&Uuid::new_v4().to_string(), s.instance)
        .await
        .unwrap();
    assert_eq!(s.finished(&run.run_id).await.state, RunState::Exited);
}

#[tokio::test(flavor = "multi_thread")]
async fn one_game_at_a_time_and_stop_targets_the_owned_process() {
    let s = Setup::new().await;
    let service = s.service(auth("Sleeper"));
    let request = Uuid::new_v4().to_string();
    let run = service.launch(&request, s.instance).await.unwrap();
    assert_eq!(run.state, RunState::Running);
    // Same request: same run. Another request: refused, nothing spawned.
    assert_eq!(
        service.launch(&request, s.instance).await.unwrap().run_id,
        run.run_id
    );
    let second = service
        .launch(&Uuid::new_v4().to_string(), s.instance)
        .await
        .unwrap_err();
    assert_eq!(second.code, ErrorCode::GameAlreadyRunning);
    // Install/repair are refused while the game runs.
    let busy =
        s.f.service
            .start(
                &Uuid::new_v4().to_string(),
                OperationKind::Repair,
                s.instance,
            )
            .unwrap_err();
    assert_eq!(busy.code, ErrorCode::AppBusy);
    let record = crate::launch::orphan::read(&s.f.root).expect("active run recorded");
    assert_eq!(record.run_id, run.run_id);
    assert!(service.stop(&run.run_id));
    let done = s.finished(&run.run_id).await;
    assert!(
        crate::launch::orphan::read(&s.f.root).is_none(),
        "record cleared after exit"
    );
    assert_eq!(done.state, RunState::Stopped, "ended by the user");
    assert!(done.error.is_none(), "a stop is not a game error");
    assert!(s.f.state.activity().current().is_none());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_nonzero_exit_is_reported_as_failure() {
    let s = Setup::new().await;
    let service = s.service(auth("Failing"));
    let run = service
        .launch(&Uuid::new_v4().to_string(), s.instance)
        .await
        .unwrap();
    let done = s.finished(&run.run_id).await;
    assert_eq!((done.state, done.exit_code), (RunState::Failed, Some(3)));
    assert_eq!(done.error.unwrap().code, ErrorCode::GameExitedWithError);
    let last = s.f.state.instance(s.instance).unwrap().last_result.unwrap();
    assert_eq!((last.outcome, last.exit_code), (Outcome::Failed, Some(3)));
}

#[tokio::test(flavor = "multi_thread")]
async fn cancel_before_spawn_prevents_the_process() {
    let s = Setup::new().await;
    let service = Arc::new(s.service(TestAuth {
        name: "Player",
        fail: None,
        delay: Duration::from_secs(3),
    }));
    let task = tokio::spawn({
        let (service, instance) = (Arc::clone(&service), s.instance);
        async move { service.launch(&Uuid::new_v4().to_string(), instance).await }
    });
    let run_id = loop {
        if let Some(run) = s.f.state.current_run() {
            break run.run_id;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    };
    assert!(service.cancel_pending(&run_id).is_some());
    let error = tokio::time::timeout(Duration::from_secs(2), task)
        .await
        .expect("cancellation is prompt")
        .unwrap()
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::OperationCancelled);
    assert!(!marker(&s.f).exists());
    assert_eq!(
        s.f.state.snapshot().last_run.unwrap().state,
        RunState::Cancelled
    );
    assert!(service.cancel_pending("unknown").is_none());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_game_that_survived_a_restart_blocks_everything_until_it_exits() {
    let s = Setup::new().await;
    // A game left behind by a previous supervisor: a real process with a record.
    let mut child = std::process::Command::new("sleep")
        .arg("30")
        .spawn()
        .unwrap();
    let exe = crate::launch::orphan::exe_after_exec(child.id());
    assert!(crate::launch::orphan::record(
        &s.f.root,
        "old-run",
        s.instance,
        child.id(),
        &exe
    ));
    let service = s.service(auth("Player"));
    assert_eq!(
        service.watch_survivor().as_deref(),
        Some("old-run"),
        "recorded {exe:?}, now {:?}",
        crate::launch::orphan::process_identity(child.id())
    );
    let current = s.f.state.current_run().unwrap();
    assert_eq!(
        (current.run_id.as_str(), current.state),
        ("old-run", RunState::Running)
    );
    // No second game and no shared-file changes while it lives.
    let launch = service
        .launch(&Uuid::new_v4().to_string(), s.instance)
        .await
        .unwrap_err();
    assert_eq!(launch.code, ErrorCode::GameAlreadyRunning);
    let repair =
        s.f.service
            .start(
                &Uuid::new_v4().to_string(),
                OperationKind::Repair,
                s.instance,
            )
            .unwrap_err();
    assert_eq!(repair.code, ErrorCode::AppBusy);
    // User-directed recovery: Stop re-checks the identity, then ends it.
    assert!(service.stop("old-run"));
    let _ = child.wait();
    let done = s.finished("old-run").await;
    assert_eq!(done.error.unwrap().code, ErrorCode::OperationInterrupted);
    assert!(s.f.state.activity().current().is_none());
    assert!(crate::launch::orphan::read(&s.f.root).is_none());
    assert!(service.watch_survivor().is_none(), "nothing left to watch");
}
