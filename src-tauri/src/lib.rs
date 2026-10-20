//! Rust core of the launcher. It owns all privileged work: networking, credentials,
//! disk access and child processes. The webview only renders state and sends typed
//! requests through `commands`.
//!
//! Module layout follows docs/spec/02-ARCHITECTURE-AND-STORAGE.md.

pub mod auth;
pub mod backup;
pub mod catalog;
pub mod commands;
pub mod connectivity;
pub mod console;
pub mod diagnostics;
pub mod download;
pub mod error;
pub mod external;
pub mod feed;
pub mod friends;
pub mod gallery;
pub mod install;
pub mod journal;
pub mod launch;
pub mod launcher_account;
pub mod lifecycle;
pub mod loader;
pub mod location;
pub mod mirror;
pub mod mods;
pub mod net;
pub mod reports;
pub mod reset;
pub mod runtime;
pub mod secrets;
pub mod skins;
pub mod skins_mod;
pub mod state;
pub mod storage;
pub mod sync;
pub mod traffic;
pub mod tunnel;
pub mod update;
pub mod validation;
pub mod worlds;

use std::path::PathBuf;
use std::sync::Arc;

use tauri::{Emitter, Manager, RunEvent};

use auth::service::{AccountHost, AuthService};
use error::{ErrorCode, LauncherError, LauncherResult};
use lifecycle::{ExitDecision, LifecycleController, RunHandles};
use state::dto::AuthStatus;
use state::dto::BackendEvent;
use state::{AppState, EventSink};
use storage::models::{AccountsFile, OperationKind};
use storage::paths::DataRoot;
use validation::MemoryPolicy;

/// Debug builds may relocate DATA_ROOT for tests and demonstrations. Release builds
/// ignore this variable; they choose the folder as described in [`location`] (D-043).
pub const DEV_DATA_ROOT_ENV: &str = "LAUNCHER_DATA_ROOT";

struct TauriSink(tauri::AppHandle);

impl EventSink for TauriSink {
    fn emit(&self, event: &BackendEvent) {
        // Delivery to a webview is best-effort; the snapshot stays authoritative.
        let _ = self.0.emit(event.name(), event.payload());
    }
}

/// Gives the install service access to the managed application state.
struct TauriStateAccess(tauri::AppHandle);

impl install::service::StateAccess for TauriStateAccess {
    fn app_state(&self) -> &AppState {
        self.0.state::<AppState>().inner()
    }

    /// A job that finished in the background brings back one window with its
    /// result, unless the user chose cancel-and-quit.
    fn operation_finished(&self) {
        // A failed download may mean the connection is gone: check it now (D-052).
        if let Some(monitor) = self.0.try_state::<Arc<connectivity::Monitor>>() {
            monitor.check_soon();
        }
        let controller = self.0.state::<LifecycleController>();
        if !controller.quit_intended()
            && self.0.get_webview_window(lifecycle::MAIN_WINDOW).is_none()
        {
            controller.show_main_window(&self.0);
        }
    }
}

/// Drives the main window around game runs exactly like the lifecycle harness.
struct TauriLaunchHost(tauri::AppHandle);

impl install::service::StateAccess for TauriLaunchHost {
    fn app_state(&self) -> &AppState {
        self.0.state::<AppState>().inner()
    }
}

impl launch::service::LaunchHost for TauriLaunchHost {
    fn store_handle(&self, run_id: &str, supervised: lifecycle::child::Supervised) {
        self.0.state::<RunHandles>().set(run_id, supervised);
    }
    fn clear_handle(&self, run_id: &str) {
        self.0.state::<RunHandles>().clear(run_id);
    }
    fn kill(&self, run_id: &str) -> bool {
        self.0.state::<RunHandles>().kill(run_id)
    }
    fn run_started(&self, run_id: &str) {
        let state = self.0.state::<AppState>();
        // Friends see what the player plays while the game runs.
        let playing = state
            .current_run()
            .filter(|r| r.run_id == run_id)
            .and_then(|r| r.instance_id)
            .and_then(|id| state.instance(id))
            .map(|instance| friends::playing_text(&instance));
        if let Some(playing) = playing {
            let handle = self.0.clone();
            tauri::async_runtime::spawn(async move {
                let friends = Arc::clone(&handle.state::<Arc<friends::Friends>>());
                friends.set_playing(&handle, Some(playing)).await;
            });
        }
        match state.during_game_mode() {
            storage::models::DuringGameMode::UnloadAndReturn => {}
            storage::models::DuringGameMode::KeepOpen => return,
            storage::models::DuringGameMode::CloseLauncher => {
                close_after_game_opens(self.0.clone(), run_id.to_owned());
                return;
            }
        }
        self.0.state::<LifecycleController>().arm_unload(run_id);
        let handle = self.0.clone();
        let run_id = run_id.to_owned();
        std::thread::spawn(move || {
            std::thread::sleep(lifecycle::SURVIVAL_INTERVAL);
            let running = handle
                .state::<AppState>()
                .current_run()
                .is_some_and(|r| r.run_id == run_id && r.state == state::dto::RunState::Running);
            if running {
                handle
                    .state::<LifecycleController>()
                    .unload_main_window(&handle, run_id);
            }
        });
    }
    fn process_exited(&self, run_id: &str) {
        self.0.state::<LifecycleController>().disarm_unload(run_id);
    }
    fn run_finished(&self, run_id: &str) {
        self.0
            .state::<LifecycleController>()
            .restore_after_run(&self.0, run_id);
        let handle = self.0.clone();
        tauri::async_runtime::spawn(async move {
            let friends = Arc::clone(&handle.state::<Arc<friends::Friends>>());
            friends.set_playing(&handle, None).await;
        });
        // Back up the worlds the session changed, if switched on (D-047).
        let instance = self
            .0
            .state::<AppState>()
            .snapshot()
            .last_run
            .filter(|r| r.run_id == run_id)
            .and_then(|r| r.instance_id);
        if let Some(instance) = instance {
            let handle = self.0.clone();
            std::thread::spawn(move || {
                backup::after_play(&handle.state::<AppState>(), instance);
            });
        }
    }
}

/// Longest wait for the game window before the launcher closes anyway.
const CLOSE_AFTER_START_LIMIT: std::time::Duration = std::time::Duration::from_secs(60);

/// "Close the launcher after the game opens": once the game's log shows its
/// window (or after [`CLOSE_AFTER_START_LIMIT`] while it still runs), the
/// launcher quits and leaves the game running. Its active-run record stays, so
/// a launcher started before the game exits waits for it (launch::orphan). A
/// game that ends first keeps the launcher open to show the result.
fn close_after_game_opens(handle: tauri::AppHandle, run_id: String) {
    std::thread::spawn(move || {
        let started = std::time::Instant::now();
        let log = handle.state::<AppState>().data_root().map(|root| {
            root.run_logs(uuid::Uuid::parse_str(&run_id).unwrap_or_default())
                .join("output.log")
        });
        loop {
            std::thread::sleep(std::time::Duration::from_millis(500));
            let state = handle.state::<AppState>();
            let running = state
                .current_run()
                .is_some_and(|r| r.run_id == run_id && r.state == state::dto::RunState::Running);
            if !running {
                return;
            }
            let window_up = log
                .as_ref()
                .and_then(|path| std::fs::read_to_string(path).ok())
                .is_some_and(|text| launch::logs::shows_game_window(&text));
            if window_up || started.elapsed() >= CLOSE_AFTER_START_LIMIT {
                state
                    .console()
                    .info("The game is running; the launcher closes as set in Settings");
                handle.state::<LifecycleController>().set_quit_intent();
                handle.exit(0);
                return;
            }
        }
    });
}

struct TauriAccountHost(tauri::AppHandle);

impl AccountHost for TauriAccountHost {
    fn auth_status(&self) -> AuthStatus {
        self.0.state::<AppState>().auth_status()
    }
    fn set_auth_status(&self, status: AuthStatus) {
        self.0.state::<AppState>().set_auth_status(status);
    }
    fn accounts_file(&self) -> AccountsFile {
        self.0.state::<AppState>().accounts_file()
    }
    fn update_accounts_file(
        &self,
        change: Box<dyn FnOnce(&mut AccountsFile) + Send + '_>,
    ) -> LauncherResult<()> {
        self.0.state::<AppState>().update_accounts_file(change)
    }
    fn launch_owns_account(&self) -> bool {
        self.0
            .state::<AppState>()
            .activity()
            .current()
            .is_some_and(|a| a.kind == OperationKind::Launch)
    }
}

fn create_auth_service(
    handle: &tauri::AppHandle,
) -> Result<AuthService, Box<dyn std::error::Error>> {
    let http = auth::http::AuthHttp::production(&handle.package_info().version.to_string())
        .map_err(|e| e.message)?;
    let opener_handle = handle.clone();
    Ok(AuthService::new(
        http,
        auth::client_id::build_time_client_id(),
        Arc::new(TauriAccountHost(handle.clone())),
        Box::new(move |url: &str| {
            use tauri_plugin_opener::OpenerExt;
            let url = external::check_allowed(url)?;
            opener_handle
                .opener()
                .open_url(url.as_str(), None::<&str>)
                .map_err(|_| LauncherError::internal("The system browser could not be opened."))
        }),
        Arc::new(auth::vault::KeyringStore),
    ))
}

fn resolve_data_root(app: &tauri::AppHandle) -> LauncherResult<DataRoot> {
    if cfg!(debug_assertions)
        && let Some(path) = std::env::var_os(DEV_DATA_ROOT_ENV)
    {
        return DataRoot::new(PathBuf::from(path));
    }
    let location = app.state::<commands::Location>();
    let path = location.current()?.path;
    DataRoot::new(path)
}

/// Where the data folder is and how it was chosen (D-043).
fn resolve_location(app: &tauri::AppHandle) -> LauncherResult<location::DataLocation> {
    let unavailable = || {
        LauncherError::new(
            ErrorCode::UnsupportedDataPath,
            "The operating system did not provide a per-user application data directory.",
        )
    };
    let default = app.path().app_local_data_dir().map_err(|_| unavailable())?;
    let config = app.path().app_config_dir().map_err(|_| unavailable())?;
    Ok(location::resolve(
        location::launcher_folder().as_deref(),
        &config,
        default,
    ))
}

/// Creates state and services; any failure stops the launcher before a window opens.
fn setup(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let handle = app.handle().clone();
    app.manage(
        commands::Location::new(resolve_location(&handle))
            .with_config_dir(handle.path().app_config_dir().ok()),
    );
    let data_root = resolve_data_root(&handle);
    // A new version that did not start goes back to the one before (D-063),
    // before its settings are read.
    let going_back = data_root
        .as_ref()
        .ok()
        .and_then(|root| update::at_start(&handle, root.path()));
    let state = AppState::initialize(
        data_root,
        app.package_info().version.to_string(),
        MemoryPolicy::detect(),
        Box::new(TauriSink(handle)),
    );
    let version = app.package_info().version.to_string();
    // The launcher's own log and failure records (D-061).
    if let Some(root) = state.data_root() {
        let logs = root.logs_dir();
        if journal::begin(&logs, &version).is_some() {
            state
                .console()
                .warn("The launcher did not close normally last time; a record was kept");
        }
        state.console().attach_file(journal::LogFile::open(&logs));
        if let Some(problem) = going_back {
            state.console().warn(problem);
        }
        state.console().info(format!(
            "Launcher {version} started ({} {})",
            std::env::consts::OS,
            std::env::consts::ARCH
        ));
    }
    let metadata = match state.data_root() {
        Some(root) => Some(Arc::new(catalog::source::MetadataService::new(
            net::PublicClient::new(
                net::SourcePolicy::MojangMetadata,
                &app.package_info().version.to_string(),
            )
            .map_err(|e| e.message)?,
            root,
        ))),
        None => None,
    };
    let source: Option<Arc<install::service::LiveSource>> = match &metadata {
        Some(metadata) => Some(Arc::new(
            install::service::LiveSource::new(Arc::clone(metadata), &version)
                .map_err(|e| e.message)?,
        )),
        None => None,
    };
    let installer = match &source {
        Some(source) => Some(Arc::new(install::service::InstallService::new(
            Arc::new(TauriStateAccess(app.handle().clone())),
            Arc::clone(source) as Arc<dyn install::service::PlanSource>,
            download::transport::HttpTransport::public(&version).map_err(|e| e.message)?,
            download::ExecutorConfig::default(),
        ))),
        None => None,
    };
    app.manage(commands::Metadata(metadata.clone()));
    app.manage(commands::Sources(source.clone()));
    // Queued installs and repairs start as soon as nothing else runs (D-049).
    if let Some(queue) = installer.clone() {
        tauri::async_runtime::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                queue.start_queued();
            }
        });
    }
    app.manage(commands::Installer(installer));
    // Counted traffic is written every few seconds (D-051).
    if let Some(root) = state.data_root() {
        tauri::async_runtime::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                let root = root.clone();
                let _ = tauri::async_runtime::spawn_blocking(move || traffic::flush(&root)).await;
            }
        });
    }
    app.manage(state);
    // Downloads fall back to the mirror (D-062); its hosts show in the interface.
    {
        let routing = mirror::shared();
        routing.set_mode(app.state::<AppState>().settings().download_source);
        let handle = app.handle().clone();
        let shown = std::sync::Mutex::new(Vec::<String>::new());
        routing.on_change(move || {
            let state = handle.state::<AppState>();
            let now = mirror::shared().mirrored();
            let mut before = shown
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            for host in now.iter().filter(|h| !before.contains(h)) {
                state.console().warn(format!(
                    "{host} is not reachable; its downloads come from the mirror ({})",
                    mirror::MIRROR_HOST
                ));
            }
            for host in before.iter().filter(|h| !now.contains(h)) {
                state.console().info(format!("{host} answers again"));
            }
            *before = now;
            drop(before);
            state.announce_runs();
        });
    }
    // Launcher updates (D-063): notice a new version, then look for the next.
    {
        let updates = Arc::new(update::Updates::default());
        app.manage(Arc::clone(&updates));
        update::Updates::after_start(&app.state::<AppState>());
        let handle = app.handle().clone();
        tauri::async_runtime::spawn(async move {
            // Let the start settle (network check, catalog) first.
            tokio::time::sleep(std::time::Duration::from_secs(30)).await;
            loop {
                updates.check(&handle, false).await;
                tokio::time::sleep(update::EVERY).await;
            }
        });
    }
    // Crash reports the player sends, and statistics they allowed (D-061).
    // After AppState is managed: the task below reads it at once.
    let reports = Arc::new(reports::Reports::new(Box::new(
        reports::HttpPoster::new(&version).map_err(|e| e.message)?,
    )));
    app.manage(Arc::clone(&reports));
    {
        let handle = app.handle().clone();
        tauri::async_runtime::spawn(async move {
            // Failures of earlier sessions count once in the statistics.
            if let Some(root) = handle.state::<AppState>().data_root() {
                let state = handle.state::<AppState>();
                let logs = root.logs_dir();
                for failure in journal::failures(&logs).into_iter().filter(|f| !f.counted) {
                    reports::count(
                        &state,
                        reports::Event {
                            source: "launcher".to_owned(),
                            kind: console::name(&failure.kind),
                            signature: failure.signature(),
                            launcher_version: failure.version.clone(),
                            os: failure.os.clone(),
                            game_version: String::new(),
                            loader: String::new(),
                            loader_version: String::new(),
                            analyzer_version: 0,
                            exit_code: None,
                        },
                    );
                    if state.settings().share_stats {
                        let _ = journal::update(&logs, &failure.id, |f| f.counted = true);
                    }
                }
            }
            // Reports left from earlier sessions go soon after start.
            tokio::time::sleep(std::time::Duration::from_secs(20)).await;
            loop {
                reports.flush(&handle.state::<AppState>()).await;
                reports.wait(std::time::Duration::from_secs(600)).await;
            }
        });
    }
    // The start page feed (D-067): asked only when the interface shows it.
    app.manage(Arc::new(
        feed::FeedService::new(&version).map_err(|e| e.message)?,
    ));
    // The launcher account (D-064): check the saved sign-in once the start settles.
    let launcher_account = Arc::new(launcher_account::LauncherAccount::new(
        &version,
        Arc::new(auth::vault::KeyringStore),
    )?);
    app.manage(Arc::clone(&launcher_account));
    {
        let handle = app.handle().clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            launcher_account.check(&handle.state::<AppState>()).await;
        });
    }
    // The mods catalog (D-066): search, projects and installing content.
    app.manage(Arc::new(mods::Mods::new(&version)?));
    // Friends and chats of the launcher account (D-064): one live connection.
    {
        let friends = Arc::new(friends::Friends::new(&version)?);
        app.manage(Arc::clone(&friends));
        let handle = app.handle().clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(6)).await;
            friends.run(handle).await;
        });
    }
    // The launcher account's skin and cape (D-065).
    app.manage(Arc::new(skins::Skins::new(&version)?));
    // Worlds with friends (D-068): hosting a room or being a guest in one.
    {
        let handle = app.handle().clone();
        let emit = Box::new(move |view: &worlds::app::WorldsView| {
            let _ = handle.emit(worlds::app::STATE_EVENT, view);
        });
        app.manage(Arc::new(worlds::app::Worlds::new(
            &version,
            app.state::<AppState>().data_root(),
            metadata.clone(),
            source.clone(),
            Arc::new(worlds::app::AccountToken(app.handle().clone())),
            emit,
        )?));
    }
    let auth = Arc::new(create_auth_service(app.handle())?);
    app.manage(Arc::clone(&auth));
    // Launcher Skins' jars come from GitHub, never through the mirror (D-065).
    let skins_mod =
        download::transport::HttpTransport::origin_only(net::SourcePolicy::SkinsMod, &version)
            .map_err(|e| e.message)?;
    let launcher = source.map(|source| {
        Arc::new(
            launch::service::LaunchService::new(
                Arc::new(TauriLaunchHost(app.handle().clone())),
                source as Arc<dyn install::service::PlanSource>,
                Arc::new(Arc::clone(&auth)),
                &version,
            )
            .with_skins_mod(skins_mod),
        )
    });
    // A game that outlived a previous launcher keeps everything blocked
    // until it exits.
    if let Some(launcher) = &launcher {
        launcher.watch_survivor();
    }
    // So does a world server, until it exits (D-068).
    if let Some(root) = app.state::<AppState>().data_root() {
        worlds::host::watch_survivor(&root, app.state::<AppState>().activity());
    }
    app.manage(commands::Launcher(launcher));
    // Finish a pending sign-out or refresh the saved account in the background.
    let reconnect_auth = Arc::clone(&auth);
    tauri::async_runtime::spawn(async move { auth.restore().await });
    // Whether the internet can be reached; when it returns, accounts are checked
    // again and queued downloads resume (D-052).
    let monitor = Arc::new(connectivity::Monitor::new(connectivity::https_probe(
        net::PublicClient::new(net::SourcePolicy::MojangMetadata, &version)
            .map_err(|e| e.message)?,
    )));
    app.manage(Arc::clone(&monitor));
    let handle = app.handle().clone();
    tauri::async_runtime::spawn(async move {
        loop {
            let state = handle.state::<AppState>();
            if monitor.check(&state).await {
                let auth = Arc::clone(&reconnect_auth);
                tauri::async_runtime::spawn(async move { auth.restore().await });
                if let Some(friends) = handle.try_state::<Arc<friends::Friends>>() {
                    friends.wake();
                }
            }
            monitor.next(state.network()).await;
        }
    });
    Ok(())
}

/// Answers a `gallery:` request with a screenshot or its thumbnail (D-050).
fn gallery_response<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    path: &str,
) -> tauri::http::Response<Vec<u8>> {
    use tauri::http::{Response, StatusCode, header};
    let status = |code: StatusCode| {
        let mut response = Response::new(Vec::new());
        *response.status_mut() = code;
        response
    };
    let Some((size, id, file)) = gallery::parse_request(path) else {
        return status(StatusCode::BAD_REQUEST);
    };
    let Some(state) = app.try_state::<AppState>() else {
        return status(StatusCode::SERVICE_UNAVAILABLE);
    };
    match gallery::serve(&state, size, id, &file) {
        Ok(bytes) => Response::builder()
            .header(header::CONTENT_TYPE, "image/png")
            // The page adds the file's time to the address, so a changed file
            // gets a new address.
            .header(header::CACHE_CONTROL, "private, max-age=3600")
            .body(bytes)
            .unwrap_or_else(|_| status(StatusCode::INTERNAL_SERVER_ERROR)),
        Err(_) => status(StatusCode::NOT_FOUND),
    }
}

/// Answers a `feed:` request with one of the feed's pictures (D-067): only
/// names the feed uses, fetched from feed.lapi.sh and kept on disk.
async fn feed_response<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    method: &tauri::http::Method,
    path: &str,
) -> tauri::http::Response<Vec<u8>> {
    use tauri::http::{Method, Response, StatusCode, header};
    let status = |code: StatusCode| {
        let mut response = Response::new(Vec::new());
        *response.status_mut() = code;
        response
    };
    if method != Method::GET {
        return status(StatusCode::METHOD_NOT_ALLOWED);
    }
    let Some((kind, name)) = feed::parse_picture_request(path) else {
        return status(StatusCode::BAD_REQUEST);
    };
    let (Some(state), Some(service)) = (
        app.try_state::<AppState>(),
        app.try_state::<Arc<feed::FeedService>>(),
    ) else {
        return status(StatusCode::SERVICE_UNAVAILABLE);
    };
    let service = Arc::clone(&service);
    let root = state.data_root();
    let enabled = state.settings().feed.enabled;
    let Some(root) = root.filter(|_| enabled) else {
        return status(StatusCode::NOT_FOUND);
    };
    match service.picture(&root, kind, &name).await {
        Some(bytes) => Response::builder()
            .header(header::CONTENT_TYPE, "image/jpeg")
            .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff")
            .header(header::CACHE_CONTROL, "private, max-age=3600")
            .body(bytes)
            .unwrap_or_else(|_| status(StatusCode::INTERNAL_SERVER_ERROR)),
        None => status(StatusCode::NOT_FOUND),
    }
}

/// Freezes `Object.prototype` in the launcher's own pages (D-006), before any of
/// their scripts run. Tauri's `freezePrototype` setting is off instead: it froze
/// the prototype in every frame of every window, and WebView2 runs it in
/// cross-origin frames too, so the player window's embedded YouTube and Twitch
/// players broke and stayed black (D-067).
pub const FREEZE_PROTOTYPE_SCRIPT: &str = "(function () {
  var l = window.location;
  if (l.protocol === 'tauri:' || l.hostname === 'tauri.localhost'
      || (l.protocol === 'http:' && l.hostname === 'localhost')) {
    Object.freeze(Object.prototype);
  }
})();";

/// Builds and runs the Tauri application.
pub fn run() {
    journal::install_panic_hook();
    let app = tauri::Builder::default()
        // Must be the first plugin: a second launch hands over to the running
        // supervisor and exits before any state or window is created. The DATA_ROOT
        // lock still protects storage if this notification cannot be delivered.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            app.state::<LifecycleController>().show_main_window(app);
        }))
        .plugin(
            tauri::plugin::Builder::<tauri::Wry>::new("own-pages")
                .js_init_script_on_all_frames(FREEZE_PROTOTYPE_SCRIPT)
                .build(),
        )
        // Links open through each window's own handlers (the player window's
        // on_new_window, the commands). The plugin's click script would call
        // IPC that the player window may not use, so its links did nothing.
        .plugin(
            tauri_plugin_opener::Builder::new()
                .open_js_links_on_click(false)
                .build(),
        )
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        // Updates (D-063); the key is given per check, so a build without it
        // never looks.
        .plugin(tauri_plugin_updater::Builder::new().build())
        .register_asynchronous_uri_scheme_protocol(gallery::PROTOCOL, |ctx, request, responder| {
            let app = ctx.app_handle().clone();
            let path = request.uri().path().to_owned();
            tauri::async_runtime::spawn_blocking(move || {
                responder.respond(gallery_response(&app, &path));
            });
        })
        .register_asynchronous_uri_scheme_protocol(feed::PROTOCOL, |ctx, request, responder| {
            let app = ctx.app_handle().clone();
            let method = request.method().clone();
            let path = request.uri().path().to_owned();
            tauri::async_runtime::spawn(async move {
                responder.respond(feed_response(&app, &method, &path).await);
            });
        })
        .manage(LifecycleController::new())
        .manage(RunHandles::default())
        .setup(|app| {
            if let Err(error) = setup(app) {
                // A readable message and exit code instead of a panic. The known
                // cause is a system without CA certificates, so HTTPS cannot work.
                eprintln!("The launcher cannot start: {error}");
                std::process::exit(1);
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() == lifecycle::MAIN_WINDOW
                && matches!(event, tauri::WindowEvent::CloseRequested { .. })
            {
                let app = window.app_handle();
                let supervising = app.state::<AppState>().activity().is_busy();
                app.state::<LifecycleController>()
                    .main_window_closed(app, supervising);
            }
        })
        .invoke_handler(handlers())
        .build(tauri::generate_context!())
        .expect("error while building the Tauri application");

    app.run(|handle, event| match event {
        RunEvent::ExitRequested { api, .. } => {
            let controller = handle.state::<LifecycleController>();
            let supervising = handle.state::<AppState>().activity().is_busy();
            if lifecycle::exit_decision(controller.quit_intended(), supervising)
                == ExitDecision::Prevent
            {
                api.prevent_exit();
                // With a window still present this was not "last window closed" but
                // an application Quit (menu, Cmd+Q). Ask the user instead of silently
                // ignoring it. Without a window, supervision simply continues and
                // reopening the app restores the UI.
                if handle.get_webview_window(lifecycle::MAIN_WINDOW).is_some() {
                    controller.show_main_window(handle);
                    let hosting = handle.state::<AppState>().activity().hosting().is_some();
                    let _ = handle.emit(
                        lifecycle::QUIT_REQUESTED_EVENT,
                        serde_json::json!({ "hosting": hosting }),
                    );
                }
            }
        }
        #[cfg(target_os = "macos")]
        RunEvent::Reopen { .. } => {
            handle
                .state::<LifecycleController>()
                .show_main_window(handle);
        }
        // Write the last counted traffic before the process ends (D-051).
        RunEvent::Exit => {
            // A hosted world saves and its room's session ends (D-068).
            let worlds = Arc::clone(&handle.state::<Arc<worlds::app::Worlds>>());
            tauri::async_runtime::block_on(worlds.shutdown());
            if let Some(root) = handle.state::<AppState>().data_root() {
                traffic::flush(&root);
                handle.state::<AppState>().console().info("Launcher closed");
                journal::end(&root.logs_dir());
            }
            // Last: on Windows the update's installer takes over from here (D-063).
            let updates = handle.state::<Arc<update::Updates>>();
            updates.at_exit(&handle.state::<AppState>());
            updates.install_on_close(&handle.state::<AppState>());
        }
        _ => {}
    });
}

/// Every command; the lifecycle test ones only in debug builds.
fn handlers<R: tauri::Runtime>() -> impl Fn(tauri::ipc::Invoke<R>) -> bool + Send + Sync + 'static {
    tauri::generate_handler![
        commands::get_app_info,
        commands::get_app_snapshot,
        commands::create_instance,
        commands::update_instance_settings,
        commands::set_instance_notes,
        commands::rename_instance,
        commands::set_app_settings,
        commands::show_main_window,
        commands::open_console,
        commands::get_data_location,
        commands::move_data_location,
        commands::pick_data_folder,
        commands::check_java,
        commands::set_instance_java,
        commands::set_instance_loader,
        commands::pick_java_file,
        commands::list_backups,
        commands::create_backup,
        commands::restore_backup,
        commands::delete_backup,
        commands::inspect_instance,
        commands::delete_instance,
        commands::reset_preview,
        commands::reset_launcher,
        commands::recommend_memory,
        commands::list_screenshots,
        commands::delete_screenshot,
        commands::open_screenshot,
        commands::read_console,
        commands::request_quit,
        commands::begin_login,
        commands::cancel_login,
        commands::sign_out,
        commands::verify_account,
        commands::add_notoken_profile,
        commands::remove_notoken_profile,
        commands::set_default_profile,
        commands::set_instance_profile,
        commands::open_external,
        commands::list_supported_versions,
        commands::preview_install_plan,
        commands::list_loader_game_versions,
        commands::list_loader_versions,
        commands::install_instance,
        commands::repair_instance,
        commands::cancel_operation,
        commands::answer_download,
        commands::check_network,
        commands::get_traffic,
        commands::reset_traffic,
        commands::launch_game,
        commands::stop_game,
        commands::open_instance_folder,
        commands::export_diagnostics,
        commands::report_frontend_error,
        commands::note_shown_error,
        commands::list_launcher_failures,
        commands::dismiss_launcher_failure,
        commands::preview_crash_report,
        commands::send_crash_report,
        commands::send_explanation_feedback,
        commands::check_for_update,
        commands::download_update,
        commands::restart_to_update,
        commands::dismiss_whats_new,
        commands::open_update_page,
        commands::interface_ready,
        commands::launcher_account_open,
        commands::launcher_account_sign_out,
        commands::launcher_account_set_nick,
        commands::launcher_account_resend_confirmation,
        commands::launcher_account_recheck,
        commands::friends_state,
        commands::friends_request,
        commands::friends_act,
        commands::friends_open_direct,
        commands::friends_create_group,
        commands::friends_change_room,
        commands::friends_messages,
        commands::friends_send,
        commands::friends_read,
        commands::friends_report,
        commands::friends_set_visible,
        commands::friends_decline_all,
        commands::friends_report_request,
        commands::launcher_account_export,
        commands::mods_search,
        commands::mods_project,
        commands::mods_versions,
        commands::mods_version,
        commands::mods_fetch_version,
        commands::mods_imports,
        commands::mods_import,
        commands::mods_import_state,
        commands::mods_choose,
        commands::mods_plan,
        commands::mods_install,
        commands::mods_cancel,
        commands::mods_content,
        commands::mods_identify,
        commands::mods_updates,
        commands::mods_set_enabled,
        commands::mods_remove,
        commands::mods_open_link,
        commands::skins_mine,
        commands::skins_texture,
        commands::skins_pick,
        commands::skins_upload,
        commands::skins_remove,
        commands::feed_get,
        commands::feed_note,
        commands::feed_play,
        commands::feed_open_link,
        commands::feed_channels,
        commands::feed_add_channel,
        commands::feed_remove_channel,
        commands::feed_own_videos,
        commands::worlds_state,
        commands::worlds_versions,
        commands::worlds_local,
        commands::worlds_rooms,
        commands::worlds_room,
        commands::worlds_create,
        commands::worlds_accept_eula,
        commands::worlds_moderate,
        commands::worlds_leave_room,
        commands::worlds_invite,
        commands::worlds_preview,
        commands::worlds_redeem,
        commands::worlds_host,
        commands::worlds_stop_hosting,
        commands::worlds_join,
        commands::worlds_disconnect,
        commands::worlds_cancel,
        commands::worlds_instance,
        commands::worlds_play,
        commands::worlds_devices,
        commands::worlds_revoke_device,
        #[cfg(debug_assertions)]
        commands::start_lifecycle_test,
        #[cfg(debug_assertions)]
        commands::stop_lifecycle_test,
    ]
}
