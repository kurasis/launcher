//! Live game launch with the debug-build test profile (D-030). Ignored by default;
//! run on a machine with a display (Xvfb works) and an installed version:
//! `LAUNCHER_LIVE_ROOT=<data dir> LAUNCHER_LIVE_VERSION=26.3 cargo test --test live_play -- --ignored --nocapture`
//!
//! Starts the real Minecraft client through the launch service with the no-token
//! profile (Minecraft's demo mode), waits
//! until its log shows that the client finished loading its resources (the title
//! screen follows), optionally keeps it running for `LAUNCHER_LIVE_HOLD_SECS` (for
//! screenshots or manual steps), then stops it through the service and checks the
//! recorded run. This is a "test profile, demo" result, never account evidence.
//!
//! `LAUNCHER_LIVE_LOADER=<kind>` (`fabric`, `quilt`, `forge`, `neoforge`) picks the
//! installation of the version with that mod loader (D-039); without it the
//! vanilla one.

#![allow(clippy::print_stdout, clippy::unwrap_used, clippy::expect_used)] // prints the evidence summary

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use launcher_lib::catalog::source::MetadataService;
use launcher_lib::error::LauncherResult;
use launcher_lib::install::service::BoxFuture;
use launcher_lib::install::service::{LiveSource, PlanSource, StateAccess};
use launcher_lib::launch::compile::LaunchIdentity;
use launcher_lib::launch::notoken::NoTokenProfile;
use launcher_lib::launch::service::{Authorizer, LaunchHost, LaunchMode, LaunchService};
use launcher_lib::lifecycle::child::Supervised;
use launcher_lib::net::{PublicClient, SourcePolicy};
use launcher_lib::secrets::SecretString;
use launcher_lib::state::dto::RunState;
use launcher_lib::state::{AppState, NoopSink};
use launcher_lib::storage::paths::DataRoot;
use launcher_lib::validation::MemoryPolicy;

const VERSION: &str = "0.1.0";
/// After a marker, the game must keep running this long to count as started.
const SETTLE: Duration = Duration::from_secs(15);

/// Test-only identity without `--demo`, for versions that have no demo mode
/// (`LAUNCHER_LIVE_FULL=1`). It exists only in this test binary; the product
/// refuses no-token launches of such versions (D-035).
struct FullGameHarness;

impl Authorizer for FullGameHarness {
    fn authorize<'a>(
        &'a self,
        _profile_id: &'a str,
    ) -> BoxFuture<'a, LauncherResult<LaunchIdentity>> {
        Box::pin(async {
            let profile = NoTokenProfile::new("TestPlayer").unwrap();
            Ok(LaunchIdentity {
                player_name: profile.name().to_owned(),
                uuid: profile.uuid(),
                access_token: SecretString::new("notoken-profile"),
                xuid: None,
                client_id: String::new(),
                notoken: false,
                offline: false,
            })
        })
    }
}

/// Headless host: keeps process handles, no window to unload.
struct Host {
    state: AppState,
    handles: Mutex<HashMap<String, Supervised>>,
    finished: Mutex<Vec<String>>,
}

impl StateAccess for Host {
    fn app_state(&self) -> &AppState {
        &self.state
    }
}

impl LaunchHost for Host {
    fn store_handle(&self, run_id: &str, supervised: Supervised) {
        self.handles
            .lock()
            .unwrap()
            .insert(run_id.to_owned(), supervised);
    }
    fn clear_handle(&self, run_id: &str) {
        self.handles.lock().unwrap().remove(run_id);
    }
    fn kill(&self, run_id: &str) -> bool {
        self.handles
            .lock()
            .unwrap()
            .get(run_id)
            .is_some_and(Supervised::kill)
    }
    fn run_started(&self, _run_id: &str) {}
    fn process_exited(&self, _run_id: &str) {}
    fn run_finished(&self, run_id: &str) {
        self.finished.lock().unwrap().push(run_id.to_owned());
    }
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "starts the real game; needs a display and an installed version"]
async fn the_game_starts_without_a_token_and_stops_cleanly() {
    let root = DataRoot::new(PathBuf::from(
        std::env::var("LAUNCHER_LIVE_ROOT").expect("set LAUNCHER_LIVE_ROOT"),
    ))
    .unwrap();
    let version = std::env::var("LAUNCHER_LIVE_VERSION").expect("set LAUNCHER_LIVE_VERSION");
    let loader = std::env::var("LAUNCHER_LIVE_LOADER").ok();
    let hold = std::env::var("LAUNCHER_LIVE_HOLD_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0u64);
    let host = Arc::new(Host {
        state: AppState::initialize(
            Ok(root.clone()),
            VERSION,
            MemoryPolicy::detect(),
            Box::new(NoopSink),
        ),
        handles: Mutex::default(),
        finished: Mutex::default(),
    });
    let instance = host
        .state
        .snapshot()
        .instances
        .into_iter()
        .find(|i| {
            i.version_id == version
                && i.loader.as_ref().map(|l| l.kind.as_str()) == loader.as_deref()
        })
        .expect("an installation of the version");
    let metadata = Arc::new(MetadataService::new(
        PublicClient::new(SourcePolicy::MojangMetadata, VERSION).unwrap(),
        root.clone(),
    ));
    let source: Arc<dyn PlanSource> = Arc::new(LiveSource::new(metadata, VERSION).unwrap());
    let full = std::env::var_os("LAUNCHER_LIVE_FULL").is_some();
    let service = LaunchService::new(
        Arc::clone(&host) as Arc<dyn LaunchHost>,
        source,
        Arc::new(FullGameHarness),
        VERSION,
    );
    let mode = if full {
        LaunchMode::Account("full-game-harness".into())
    } else {
        LaunchMode::NoToken(NoTokenProfile::new("TestPlayer").unwrap())
    };

    let started = Instant::now();
    let run = service
        .launch_as(&uuid::Uuid::new_v4().to_string(), instance.id, mode)
        .await
        .expect("preflight passes and the game starts");
    println!(
        "{version}: game process started after {:.1} s",
        started.elapsed().as_secs_f64()
    );
    let log = root
        .run_logs(uuid::Uuid::parse_str(&run.run_id).unwrap())
        .join("output.log");

    // Wait for the client to finish loading while it keeps running.
    let deadline = Instant::now() + Duration::from_secs(300);
    let mut ready_at: Option<Instant> = None;
    let ready_after = std::env::var("LAUNCHER_LIVE_READY_AFTER_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .map(Duration::from_secs);
    loop {
        let text = std::fs::read_to_string(&log).unwrap_or_default();
        // The 2009 pre-Classic versions log nothing on start; for them
        // LAUNCHER_LIVE_READY_AFTER_SECS counts running that long as started.
        let alive_long_enough = ready_after.is_some_and(|after| started.elapsed() >= after);
        if ready_at.is_none()
            && (alive_long_enough || launcher_lib::launch::logs::shows_game_window(&text))
        {
            ready_at = Some(Instant::now());
        }
        if ready_at.is_some_and(|at| at.elapsed() >= SETTLE) {
            break;
        }
        let current = host.state.current_run();
        assert!(
            current
                .as_ref()
                .is_some_and(|r| r.state == RunState::Running),
            "the game ended before loading: {:?}",
            host.state.snapshot().last_run.map(|r| r.output_tail)
        );
        assert!(
            Instant::now() < deadline,
            "no resource load within 5 minutes"
        );
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    println!(
        "{version}: client loaded its resources after {:.1} s",
        started.elapsed().as_secs_f64()
    );
    let text = std::fs::read_to_string(&log).unwrap();
    // Versions from 1.6 log the user; older ones print nothing about it.
    println!(
        "{version}: profile name in log: {}, notoken: {}",
        text.contains("TestPlayer"),
        !full
    );
    if let Some(loader) = &instance.loader {
        let name = loader.kind.name();
        println!(
            "{version}: {name} {} named in log: {}",
            loader.version,
            text.contains(name) || text.to_lowercase().contains(loader.kind.as_str())
        );
    }
    assert!(
        !text.contains("notoken-profile"),
        "the placeholder token is redacted in the run log"
    );
    if hold > 0 {
        println!("{version}: holding for {hold} s");
        tokio::time::sleep(Duration::from_secs(hold)).await;
    }

    assert!(service.stop(&run.run_id), "the running game accepts Stop");
    let deadline = Instant::now() + Duration::from_secs(60);
    while !host.finished.lock().unwrap().contains(&run.run_id) {
        assert!(
            Instant::now() < deadline,
            "the run did not finish after Stop"
        );
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    let last = host.state.snapshot().last_run.expect("a recorded run");
    assert_eq!(last.run_id, run.run_id);
    assert!(host.state.current_run().is_none());
    println!(
        "{version}: stopped; state {:?}, exit code {:?}, {} output lines kept, log {} bytes",
        last.state,
        last.exit_code,
        last.output_tail.len(),
        std::fs::metadata(&log).map(|m| m.len()).unwrap_or(0)
    );
}

/// Starts an installation set up to fail (a missing or wrong mod, a damaged file)
/// and prints the explanation the launcher records when the game ends by itself
/// (D-059). Error windows are closed from outside, as a player would.
/// `LAUNCHER_LIVE_WAIT_SECS` bounds the wait (default 600).
#[tokio::test(flavor = "multi_thread")]
#[ignore = "starts the real game; needs a display and a prepared installation"]
async fn a_crashed_game_is_explained() {
    let root = DataRoot::new(PathBuf::from(
        std::env::var("LAUNCHER_LIVE_ROOT").expect("set LAUNCHER_LIVE_ROOT"),
    ))
    .unwrap();
    let version = std::env::var("LAUNCHER_LIVE_VERSION").expect("set LAUNCHER_LIVE_VERSION");
    let loader = std::env::var("LAUNCHER_LIVE_LOADER").ok();
    // Two installations of one version and loader: `LAUNCHER_LIVE_INSTANCE=<id>`.
    let wanted = std::env::var("LAUNCHER_LIVE_INSTANCE").ok();
    let wait = std::env::var("LAUNCHER_LIVE_WAIT_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(600u64);
    let host = Arc::new(Host {
        state: AppState::initialize(
            Ok(root.clone()),
            VERSION,
            MemoryPolicy::detect(),
            Box::new(NoopSink),
        ),
        handles: Mutex::default(),
        finished: Mutex::default(),
    });
    let instance = host
        .state
        .snapshot()
        .instances
        .into_iter()
        .find(|i| {
            i.version_id == version
                && i.loader.as_ref().map(|l| l.kind.as_str()) == loader.as_deref()
                && wanted.as_ref().is_none_or(|id| i.id.to_string() == *id)
        })
        .expect("an installation of the version");
    let metadata = Arc::new(MetadataService::new(
        PublicClient::new(SourcePolicy::MojangMetadata, VERSION).unwrap(),
        root.clone(),
    ));
    let source: Arc<dyn PlanSource> = Arc::new(LiveSource::new(metadata, VERSION).unwrap());
    let service = LaunchService::new(
        Arc::clone(&host) as Arc<dyn LaunchHost>,
        source,
        Arc::new(FullGameHarness),
        VERSION,
    );
    let mode = LaunchMode::Account("full-game-harness".into());
    let run = service
        .launch_as(&uuid::Uuid::new_v4().to_string(), instance.id, mode)
        .await
        .expect("preflight passes and the game starts");
    println!("{version}: game process started; waiting for it to end");
    let deadline = Instant::now() + Duration::from_secs(wait);
    while !host.finished.lock().unwrap().contains(&run.run_id) {
        assert!(Instant::now() < deadline, "the game did not end");
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    let last = host.state.snapshot().last_run.expect("a recorded run");
    println!(
        "{version}: ended; state {:?}, exit code {:?}, signal {:?}, run {}",
        last.state, last.exit_code, last.signal, last.run_id
    );
    println!(
        "DIAGNOSIS {}",
        serde_json::to_string_pretty(&last.diagnosis).unwrap()
    );
}
