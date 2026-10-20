//! The game test on the test machine (docs/GAMETEST.md): installs a game
//! version with a mod loader through the launcher, adds the test bot, the
//! skins mod under test and optionally a medium set of popular mods, serves
//! test skins from this computer, starts the game through the launch service
//! and lets the bot play. Ignored by default; the Game test workflow runs it:
//!
//! `LAUNCHER_LIVE_ROOT=<data dir> GAMETEST_VERSION=26.3 GAMETEST_LOADER=fabric
//!  GAMETEST_JARS=<built jars> GAMETEST_OUT=<results> cargo test --test live_gametest -- --ignored --nocapture`
//!
//! Optional: `GAMETEST_MODS=medium`, `GAMETEST_MINUTES=5`, `GAMETEST_SKIN=hd512`
//! (see `gametest::skins::VARIANTS`). The run's results (the bot's report,
//! screenshots, cleaned logs, a Markdown summary) go to
//! `GAMETEST_OUT/<scenario>/`; the test fails when a check fails.

#![allow(clippy::print_stdout, clippy::unwrap_used, clippy::expect_used)] // a harness: prints its evidence

mod gametest;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use gametest::report::{self, Redactor};
use gametest::{mods, skins};
use launcher_lib::catalog::source::MetadataService;
use launcher_lib::download::ExecutorConfig;
use launcher_lib::download::transport::HttpTransport;
use launcher_lib::error::LauncherResult;
use launcher_lib::install::service::{
    BoxFuture, InstallService, LiveSource, PlanSource, StateAccess,
};
use launcher_lib::launch::compile::LaunchIdentity;
use launcher_lib::launch::notoken::NoTokenProfile;
use launcher_lib::launch::service::{Authorizer, LaunchHost, LaunchMode, LaunchService};
use launcher_lib::lifecycle::child::Supervised;
use launcher_lib::loader::{LoaderChoice, LoaderKind};
use launcher_lib::net::{PublicClient, SourcePolicy};
use launcher_lib::secrets::SecretString;
use launcher_lib::state::dto::{
    CreateInstancePayload, JavaChoice, MutationRequest, SetInstanceJavaPayload,
    UpdateInstanceSettingsPayload,
};
use launcher_lib::state::{AppState, NoopSink};
use launcher_lib::storage::models::{OperationKind, Outcome, Resolution};
use launcher_lib::storage::paths::DataRoot;
use launcher_lib::validation::MemoryPolicy;
use serde_json::Value;

const VERSION: &str = "0.1.0";
/// Beyond the play time: starting, the world, closing.
const SLACK: Duration = Duration::from_secs(15 * 60);

/// The test player, offline in the full game: its nick picks the skin variant.
/// It exists only in this test binary, like `live_play`'s harness identity.
struct TestPlayer(String);

impl Authorizer for TestPlayer {
    fn authorize<'a>(
        &'a self,
        _profile_id: &'a str,
    ) -> BoxFuture<'a, LauncherResult<LaunchIdentity>> {
        Box::pin(async {
            let profile = NoTokenProfile::new(&self.0)?;
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
    state: Arc<AppState>,
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

fn env(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|v| v.trim().to_owned())
        .filter(|v| !v.is_empty())
}

fn request<P>(payload: P) -> MutationRequest<P> {
    MutationRequest {
        request_id: uuid::Uuid::new_v4().to_string(),
        payload,
    }
}

/// The one jar in `dir` named `<prefix><loader>-<game>-<version>.jar`.
fn built_jar(dir: &Path, prefix: &str, loader: &str, game: &str) -> Option<PathBuf> {
    let start = format!("{prefix}{loader}-{game}-");
    report::files_starting(dir, &start).into_iter().find(|p| {
        let name = p.file_name().unwrap().to_string_lossy().to_string();
        name.ends_with(".jar")
            && !["-sources.jar", "-dev.jar", "-named.jar"]
                .iter()
                .any(|s| name.ends_with(s))
    })
}

/// Sets these keys in the game's options.txt, keeping every other line.
fn write_options(game_dir: &Path, wanted: &[(&str, &str)]) {
    let path = game_dir.join("options.txt");
    let old = std::fs::read_to_string(&path).unwrap_or_default();
    let mut lines: Vec<String> = old
        .lines()
        .filter(|l| !wanted.iter().any(|(k, _)| l.split(':').next() == Some(*k)))
        .map(str::to_owned)
        .collect();
    lines.extend(wanted.iter().map(|(k, v)| format!("{k}:{v}")));
    std::fs::create_dir_all(game_dir).unwrap();
    std::fs::write(path, lines.join("\n") + "\n").unwrap();
}

/// Empties what an earlier run left in the game folder.
fn clean_game_dir(game_dir: &Path) {
    let mods = game_dir.join("mods");
    for file in report::files_starting(&mods, "") {
        std::fs::remove_file(file).unwrap();
    }
    for dir in report::files_starting(&game_dir.join("saves"), "gametest") {
        let _ = std::fs::remove_file(dir);
    }
    if let Ok(entries) = std::fs::read_dir(game_dir.join("saves")) {
        for e in entries.flatten() {
            if e.file_name().to_string_lossy().starts_with("gametest") {
                std::fs::remove_dir_all(e.path()).unwrap();
            }
        }
    }
    for shot in report::files_starting(&game_dir.join("screenshots"), "gametest-") {
        std::fs::remove_file(shot).unwrap();
    }
    let _ = std::fs::remove_dir_all(game_dir.join("crash-reports"));
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "installs and plays the real game; needs a display, the built jars and the network"]
async fn the_game_is_played_with_mods_and_skins() {
    let root_path = PathBuf::from(env("LAUNCHER_LIVE_ROOT").expect("set LAUNCHER_LIVE_ROOT"));
    std::fs::create_dir_all(&root_path).unwrap();
    let root = DataRoot::new(root_path.clone()).unwrap();
    let version = env("GAMETEST_VERSION").expect("set GAMETEST_VERSION");
    let loader_name = env("GAMETEST_LOADER").expect("set GAMETEST_LOADER");
    let mod_set = env("GAMETEST_MODS").unwrap_or_else(|| "none".to_owned());
    assert!(
        ["none", "medium"].contains(&mod_set.as_str()),
        "GAMETEST_MODS is none or medium"
    );
    let minutes: f64 = env("GAMETEST_MINUTES")
        .and_then(|m| m.parse().ok())
        .unwrap_or(5.0);
    assert!(
        (0.5..=240.0).contains(&minutes),
        "GAMETEST_MINUTES is 0.5 to 240"
    );
    let variant_name = env("GAMETEST_SKIN").unwrap_or_else(|| "hd256".to_owned());
    let variant = skins::variant(&variant_name).expect("GAMETEST_SKIN names a variant");
    let jars = PathBuf::from(env("GAMETEST_JARS").expect("set GAMETEST_JARS"));
    let scenario = format!("{version}-{loader_name}-{mod_set}-{variant_name}");
    let out = PathBuf::from(env("GAMETEST_OUT").expect("set GAMETEST_OUT")).join(&scenario);
    let _ = std::fs::remove_dir_all(&out);
    std::fs::create_dir_all(&out).unwrap();
    let out = out.canonicalize().unwrap();
    let redactor = Redactor::new(&root_path);
    let mut notes: Vec<String> = Vec::new();
    println!("scenario {scenario}: {}", variant.describe());

    let kind: LoaderKind = serde_json::from_value(serde_json::json!(loader_name))
        .expect("GAMETEST_LOADER is fabric, neoforge or forge");
    let bot_jar = built_jar(&jars, "launchergametest-", &loader_name, &version)
        .expect("the test bot's jar for this version and loader in GAMETEST_JARS");
    let skins_jar = built_jar(&jars, "launcherskins-", &loader_name, &version);

    // The installation: one per version, loader and mod set, kept between runs.
    let memory = MemoryPolicy::detect();
    // The most this computer allows: an 8 GiB machine gives the game about 5.6 GiB.
    let max_heap = memory.as_ref().map_or(3072, |m| m.max_heap_mib);
    let state = Arc::new(AppState::initialize(
        Ok(root.clone()),
        VERSION,
        memory,
        Box::new(NoopSink),
    ));
    let metadata = Arc::new(MetadataService::new(
        PublicClient::new(SourcePolicy::MojangMetadata, VERSION).unwrap(),
        root.clone(),
    ));
    let live = Arc::new(LiveSource::new(metadata, VERSION).unwrap());
    let display_name = format!("Game test {version} {loader_name} {mod_set}");
    let existing = state
        .snapshot()
        .instances
        .into_iter()
        .find(|i| i.display_name == display_name);
    let instance = match existing {
        Some(instance) => instance,
        None => {
            let versions = live.loader_versions(kind, &version).await.unwrap();
            let chosen = versions
                .iter()
                .find(|v| v.recommended)
                .or_else(|| versions.first())
                .expect("a loader version for this game version")
                .id
                .clone();
            state
                .create_instance(request(CreateInstancePayload {
                    display_name: display_name.clone(),
                    version_id: version.clone(),
                    loader: Some(LoaderChoice {
                        kind,
                        version: chosen,
                    }),
                }))
                .unwrap()
        }
    };
    let loader = instance.loader.clone().unwrap();
    notes.push(format!("- {} {}", loader.kind.name(), loader.version));
    let source: Arc<dyn PlanSource> = live;
    let installer = InstallService::new(
        Arc::new(Arc::clone(&state)) as Arc<dyn StateAccess>,
        Arc::clone(&source),
        HttpTransport::public(VERSION).unwrap(),
        ExecutorConfig::default(),
    );
    let started = Instant::now();
    let operation = installer
        .start(
            &uuid::Uuid::new_v4().to_string(),
            OperationKind::Install,
            instance.id,
        )
        .unwrap();
    let result = loop {
        if let Some(result) = state.operation_result(&operation.operation_id) {
            break result;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    };
    assert_eq!(
        result.outcome,
        Outcome::Succeeded,
        "install: {:?}",
        result.error
    );
    println!(
        "installed in {:.0} s ({} files downloaded)",
        started.elapsed().as_secs_f64(),
        result.downloaded_files
    );

    // The game folder: the bot, the skins mod under test, the mod set, options.
    let game_dir = root.game_dir(instance.id);
    clean_game_dir(&game_dir);
    let mods_dir = game_dir.join("mods");
    std::fs::create_dir_all(&mods_dir).unwrap();
    std::fs::copy(&bot_jar, mods_dir.join(bot_jar.file_name().unwrap())).unwrap();
    // Our own build of the skins mod goes in under a name the launcher leaves
    // alone; without one the launcher adds the released mod as for a player.
    let own_skins = skins_jar.is_some();
    if let Some(jar) = &skins_jar {
        let name = format!("gametest-{}", jar.file_name().unwrap().to_string_lossy());
        std::fs::copy(jar, mods_dir.join(name)).unwrap();
        notes.push(format!(
            "- skins mod: this commit's build ({})",
            jar.file_name().unwrap().to_string_lossy()
        ));
    } else {
        notes.push("- skins mod: the released version the launcher pins".to_owned());
    }
    if mod_set == "medium" {
        let resolved = mods::resolve(&version, &loader_name).await;
        mods::install(
            &resolved.files,
            &root_path.join("gametest-cache").join("mods"),
            &mods_dir,
        )
        .await
        .expect("the mod set downloads");
        notes.push(format!(
            "- mods ({}): {}",
            resolved.files.len(),
            resolved
                .files
                .iter()
                .map(|f| format!("{} {}", f.file_name, f.version))
                .collect::<Vec<_>>()
                .join(", ")
        ));
        if !resolved.skipped.is_empty() {
            notes.push(format!("- not in the set: {}", resolved.skipped.join("; ")));
        }
        println!("mod set: {} files", resolved.files.len());
    }
    write_options(
        &game_dir,
        &[
            ("onboardAccessibility", "false"),
            ("pauseOnLostFocus", "false"),
            ("tutorialStep", "none"),
            ("skipMultiplayerWarning", "true"),
            ("joinedFirstServer", "true"),
            ("narrator", "0"),
            ("soundCategory_master", "0.0"),
            ("enableVsync", "false"),
            ("maxFps", "260"),
            ("renderDistance", "10"),
            ("simulationDistance", "8"),
            ("fullscreen", "false"),
            ("inactivityFpsLimit", "\"minimized\""),
        ],
    );

    // The skins this run's player wears, served from this computer.
    let server = skins::start().await;
    let bot_out = out.join("bot");
    let jvm = format!(
        "\"-Dlaunchergametest.out={}\" -Dlaunchergametest.minutes={minutes} \
         -Dlaunchergametest.skin={} -Dlaunchergametest.skinFrames={} \
         -Dlaunchergametest.capeFrames={} -Dlauncherskins.api={}",
        bot_out.display(),
        variant.expect_skin(),
        variant.skin_frames(),
        variant.cape_frames(),
        server.base,
    );
    let wanted_heap = if mod_set == "medium" { 6144 } else { 3072 };
    let heap = wanted_heap.min(max_heap);
    notes.push(format!("- memory for the game: {heap} MiB"));
    let summary = state
        .update_instance_settings(request(UpdateInstanceSettingsPayload {
            instance_id: instance.id,
            max_heap_mib: heap,
            resolution: Resolution {
                width: 1280,
                height: 720,
            },
            fullscreen: Some(false),
            game_arguments: Some(String::new()),
            launcher_skins: Some(!own_skins),
        }))
        .unwrap();
    let provider = summary
        .java_provider
        .or(summary.java.as_ref().map(|j| j.provider))
        .unwrap_or_default();
    state
        .set_instance_java(
            request(SetInstanceJavaPayload {
                instance_id: instance.id,
                java: JavaChoice::Managed { provider },
                jvm_arguments: jvm,
                accept_issues: false,
            }),
            None,
        )
        .unwrap();

    // Play.
    let host = Arc::new(Host {
        state: Arc::clone(&state),
        handles: Mutex::default(),
        finished: Mutex::default(),
    });
    let service = LaunchService::new(
        Arc::clone(&host) as Arc<dyn LaunchHost>,
        source,
        Arc::new(TestPlayer(variant.nick())),
        VERSION,
    );
    let started = Instant::now();
    let run = service
        .launch_as(
            &uuid::Uuid::new_v4().to_string(),
            instance.id,
            LaunchMode::Account("game-test".into()),
        )
        .await
        .expect("preflight passes and the game starts");
    let run_id = uuid::Uuid::parse_str(&run.run_id).unwrap();
    println!("game started");
    let limit = Duration::from_secs_f64(minutes * 60.0) + SLACK;
    let report_path = bot_out.join("report.json");
    let read_report =
        || -> Option<Value> { serde_json::from_slice(&std::fs::read(&report_path).ok()?).ok() };
    let mut timed_out = false;
    while !host.finished.lock().unwrap().contains(&run.run_id) {
        if started.elapsed() > limit {
            timed_out = true;
            println!("no end after {} s: stopping the game", limit.as_secs());
            service.stop(&run.run_id);
            let deadline = Instant::now() + Duration::from_secs(60);
            while !host.finished.lock().unwrap().contains(&run.run_id) && Instant::now() < deadline
            {
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
            break;
        }
        // The bot closes the game itself; give it a minute to do so after its report.
        if read_report().is_some_and(|r| r["finished"] == true) {
            let deadline = Instant::now() + Duration::from_secs(90);
            while !host.finished.lock().unwrap().contains(&run.run_id) && Instant::now() < deadline
            {
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
            if !host.finished.lock().unwrap().contains(&run.run_id) {
                notes
                    .push("- the game did not close after the bot asked it to; stopped".to_owned());
                service.stop(&run.run_id);
                tokio::time::sleep(Duration::from_secs(10)).await;
            }
            break;
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
    let last = state.snapshot().last_run;
    let exit_code = last.as_ref().and_then(|r| r.exit_code);
    println!(
        "game ended after {:.0} s, exit code {exit_code:?}",
        started.elapsed().as_secs_f64()
    );

    // Collect: report, screenshots, cleaned logs and crash reports.
    let bot_report = read_report();
    if bot_report.is_some() {
        report::copy_clean(&report_path, &out.join("report.json"), &redactor);
    }
    let _ = std::fs::remove_dir_all(&bot_out);
    let shots = out.join("screenshots");
    std::fs::create_dir_all(&shots).unwrap();
    for shot in report::files_starting(&game_dir.join("screenshots"), "gametest-") {
        std::fs::copy(&shot, shots.join(shot.file_name().unwrap())).unwrap();
    }
    let latest = game_dir.join("logs").join("latest.log");
    let log_text = std::fs::read_to_string(&latest).unwrap_or_default();
    let scan = report::scan_log(&log_text, &redactor);
    report::copy_clean(&latest, &out.join("logs").join("latest.log"), &redactor);
    report::copy_clean(
        &root.run_logs(run_id).join("output.log"),
        &out.join("logs").join("output.log"),
        &redactor,
    );
    let crashes = report::files_starting(&game_dir.join("crash-reports"), "crash-");
    for crash in &crashes {
        report::copy_clean(
            crash,
            &out.join("crash-reports").join(crash.file_name().unwrap()),
            &redactor,
        );
    }
    let diagnosis = last
        .as_ref()
        .map(|r| serde_json::to_value(&r.diagnosis).unwrap_or(Value::Null))
        .unwrap_or(Value::Null);

    let bot_passed = bot_report.as_ref().is_some_and(|r| r["passed"] == true);
    let clean_exit = exit_code == Some(0) && !timed_out;
    let passed = bot_passed && clean_exit && crashes.is_empty();
    let mut lines = vec![format!(
        "Skins: {} (nick {})",
        variant.describe(),
        variant.nick()
    )];
    lines.extend(notes);
    lines.push(format!(
        "- skin server: {} profile requests, {} textures sent",
        server.profiles_asked.load(Ordering::Relaxed),
        server.textures_sent.load(Ordering::Relaxed)
    ));
    lines.push(format!(
        "- game: exit code {}, {} crash reports{}",
        exit_code.map_or("none".to_owned(), |c| c.to_string()),
        crashes.len(),
        if timed_out {
            ", stopped after the time limit"
        } else {
            ""
        }
    ));
    lines.push(format!(
        "- log: {} errors, {} warnings, {} exception lines",
        scan.errors, scan.warnings, scan.exceptions
    ));
    if !diagnosis.is_null() {
        lines.push(format!(
            "- launcher's diagnosis: {}",
            redactor
                .clean(&diagnosis.to_string())
                .chars()
                .take(400)
                .collect::<String>()
        ));
    }
    if !scan.samples.is_empty() {
        lines.push(String::new());
        lines.push("<details><summary>First error lines</summary>\n\n```".to_owned());
        lines.extend(scan.samples.iter().cloned());
        lines.push("```\n</details>".to_owned());
    }
    let text = report::summary(&scenario, passed, bot_report.as_ref(), &lines);
    std::fs::write(out.join("summary.md"), &text).unwrap();
    std::fs::write(
        out.join("result.json"),
        serde_json::to_string_pretty(&serde_json::json!({
            "scenario": scenario,
            "passed": passed,
            "botPassed": bot_passed,
            "exitCode": exit_code,
            "timedOut": timed_out,
            "crashReports": crashes.len(),
            "logErrors": scan.errors,
        }))
        .unwrap(),
    )
    .unwrap();
    println!("{text}");
    assert!(
        passed,
        "the game test failed; see {}",
        out.join("summary.md").display()
    );
}
