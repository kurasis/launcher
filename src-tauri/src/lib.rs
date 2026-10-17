//! Rust core of the launcher. It owns all privileged work: networking, credentials,
//! disk access and child processes. The webview only renders state and sends typed
//! requests through `commands`.
//!
//! Module layout follows docs/spec/02-ARCHITECTURE-AND-STORAGE.md. Modules that are
//! still empty are filled by the phase named in their documentation.

pub mod auth;
pub mod catalog;
pub mod commands;
pub mod error;
pub mod external;
pub mod install;
pub mod launch;
pub mod lifecycle;
pub mod runtime;
pub mod secrets;
pub mod state;
pub mod storage;
pub mod validation;

use std::path::PathBuf;
use std::sync::Arc;

use tauri::{Emitter, Manager, RunEvent};

use auth::service::{AccountHost, AuthService};
use error::{ErrorCode, LauncherError, LauncherResult};
use lifecycle::{ExitDecision, LifecycleController, RunHandles};
use state::dto::AccountSummary;
use state::dto::BackendEvent;
use state::{AppState, EventSink};
use storage::models::{AccountsFile, OperationKind};
use storage::paths::DataRoot;
use validation::MemoryPolicy;

/// Debug builds may relocate DATA_ROOT for tests and demonstrations. Release builds
/// ignore this variable: data-directory relocation is not an MVP feature.
pub const DEV_DATA_ROOT_ENV: &str = "KURASIS_DATA_ROOT";

struct TauriSink(tauri::AppHandle);

impl EventSink for TauriSink {
    fn emit(&self, event: &BackendEvent) {
        // Delivery to a webview is best-effort; the snapshot stays authoritative.
        let _ = self.0.emit(event.name(), event.payload());
    }
}

/// Gives the auth service access to the managed application state.
struct TauriAccountHost(tauri::AppHandle);

impl AccountHost for TauriAccountHost {
    fn account(&self) -> AccountSummary {
        self.0.state::<AppState>().account()
    }
    fn set_account(&self, account: AccountSummary) {
        self.0.state::<AppState>().set_account(account);
    }
    fn accounts_file(&self) -> AccountsFile {
        self.0.state::<AppState>().accounts_file()
    }
    fn save_accounts_file(&self, accounts: AccountsFile) -> LauncherResult<()> {
        self.0.state::<AppState>().save_accounts_file(accounts)
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
    let path = app.path().app_local_data_dir().map_err(|_| {
        LauncherError::new(
            ErrorCode::UnsupportedDataPath,
            "The operating system did not provide a per-user application data directory.",
        )
    })?;
    DataRoot::new(path)
}

/// Builds and runs the Tauri application.
pub fn run() {
    let app = tauri::Builder::default()
        // Must be the first plugin: a second launch hands over to the running
        // supervisor and exits before any state or window is created. The DATA_ROOT
        // lock still protects storage if this notification cannot be delivered.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            app.state::<LifecycleController>().show_main_window(app);
        }))
        .plugin(tauri_plugin_opener::init())
        .manage(LifecycleController::new())
        .manage(RunHandles::default())
        .setup(|app| {
            let handle = app.handle().clone();
            let state = AppState::initialize(
                resolve_data_root(&handle),
                app.package_info().version.to_string(),
                MemoryPolicy::detect(),
                Box::new(TauriSink(handle)),
            );
            app.manage(state);
            let auth = Arc::new(create_auth_service(app.handle())?);
            app.manage(Arc::clone(&auth));
            // Finish a pending sign-out or refresh the saved account in the background.
            tauri::async_runtime::spawn(async move { auth.restore().await });
            Ok(())
        })
        .invoke_handler(handlers())
        .build(tauri::generate_context!())
        .expect("error while building the Tauri application");

    app.run(|handle, event| match event {
        RunEvent::ExitRequested { api, .. } => {
            let controller = handle.state::<LifecycleController>();
            let supervising = handle.state::<AppState>().activity().current().is_some();
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
                    let _ = handle.emit(lifecycle::QUIT_REQUESTED_EVENT, ());
                }
            }
        }
        #[cfg(target_os = "macos")]
        RunEvent::Reopen { .. } => {
            handle
                .state::<LifecycleController>()
                .show_main_window(handle);
        }
        _ => {}
    });
}

#[cfg(debug_assertions)]
fn handlers<R: tauri::Runtime>() -> impl Fn(tauri::ipc::Invoke<R>) -> bool + Send + Sync + 'static {
    tauri::generate_handler![
        commands::get_app_info,
        commands::get_app_snapshot,
        commands::create_instance,
        commands::update_instance_settings,
        commands::set_app_settings,
        commands::show_main_window,
        commands::request_quit,
        commands::begin_login,
        commands::cancel_login,
        commands::sign_out,
        commands::verify_account,
        commands::open_external,
        commands::start_lifecycle_test,
        commands::stop_lifecycle_test,
    ]
}

/// Release builds register no lifecycle test commands.
#[cfg(not(debug_assertions))]
fn handlers<R: tauri::Runtime>() -> impl Fn(tauri::ipc::Invoke<R>) -> bool + Send + Sync + 'static {
    tauri::generate_handler![
        commands::get_app_info,
        commands::get_app_snapshot,
        commands::create_instance,
        commands::update_instance_settings,
        commands::set_app_settings,
        commands::show_main_window,
        commands::request_quit,
        commands::begin_login,
        commands::cancel_login,
        commands::sign_out,
        commands::verify_account,
        commands::open_external,
    ]
}
