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
pub mod install;
pub mod launch;
pub mod lifecycle;
pub mod runtime;
pub mod secrets;
pub mod state;
pub mod storage;
pub mod validation;

use std::path::PathBuf;

use tauri::{Emitter, Manager, RunEvent};

use error::{ErrorCode, LauncherError, LauncherResult};
use lifecycle::{ExitDecision, LifecycleController, RunHandles};
use state::dto::BackendEvent;
use state::{AppState, EventSink};
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
    ]
}
