//! Rust core of the launcher. It owns all privileged work: networking, credentials,
//! disk access and child processes. The webview only renders state and sends typed
//! requests through `commands`.
//!
//! Module layout follows docs/spec/02-ARCHITECTURE-AND-STORAGE.md. Modules that are
//! still empty are filled by the phase named in their documentation.

pub mod auth;
pub mod catalog;
pub mod commands;
pub mod install;
pub mod launch;
pub mod lifecycle;
pub mod runtime;
pub mod storage;

/// Builds and runs the Tauri application.
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![commands::get_app_info])
        .run(tauri::generate_context!())
        .expect("error while running the Tauri application");
}
