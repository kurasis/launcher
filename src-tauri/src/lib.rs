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

use tauri::{Emitter, Manager};

use error::{ErrorCode, LauncherError, LauncherResult};
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
    tauri::Builder::default()
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
        .invoke_handler(tauri::generate_handler![
            commands::get_app_info,
            commands::get_app_snapshot,
            commands::create_instance,
            commands::update_instance_settings,
            commands::set_app_settings,
        ])
        .run(tauri::generate_context!())
        .expect("error while running the Tauri application");
}
