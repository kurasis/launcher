//! Window recreation, single-instance reopen and shutdown policy
//! (docs/spec/05 "Window and application states", "Background access").
//!
//! The Rust supervisor outlives the webview. While an owned run or job needs
//! supervision, closing or destroying the last window must not end the process;
//! an explicit Quit is the only path that may stop supervision. All window
//! operations run on the main event loop and are serialized there.

pub mod child;

/// How long a child must survive before the main window is unloaded. A survival
/// interval only gates the unload; it does not mean the game reached its menu.
pub const SURVIVAL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(1500);

#[cfg(debug_assertions)]
pub mod harness;

use std::sync::Mutex;
use std::sync::PoisonError;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use tauri::{AppHandle, Manager, Runtime, WebviewUrl, WebviewWindowBuilder};

/// Label of the single launcher window. Capabilities are bound to this label, so a
/// recreated window receives exactly the same permissions.
pub const MAIN_WINDOW: &str = "main";

/// Label of the console window (D-037). Its capability grants only reading the
/// console journal.
pub const CONSOLE_WINDOW: &str = "console";

/// The console window's native title in the interface language (D-038).
pub fn console_title(language: crate::storage::models::Language) -> &'static str {
    match language {
        crate::storage::models::Language::En => "Launcher console",
        crate::storage::models::Language::Ru => "Консоль лаунчера",
    }
}

/// Retitles an open console window after the language changed.
pub fn retitle_console<R: Runtime>(app: &AppHandle<R>, language: crate::storage::models::Language) {
    if let Some(console) = app.get_webview_window(CONSOLE_WINDOW) {
        let _ = console.set_title(console_title(language));
    }
}

/// Event asking the UI to show the Quit choice (an OS or menu Quit arrived while a
/// run needs supervision).
pub const QUIT_REQUESTED_EVENT: &str = "quit_requested";

/// What to do with an exit request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitDecision {
    Allow,
    Prevent,
}

/// Pure exit policy. Quit intent is explicit state, never inferred from the exit
/// code: closing the last window and some OS quit paths look identical otherwise.
pub fn exit_decision(explicit_quit: bool, supervision_required: bool) -> ExitDecision {
    if explicit_quit || !supervision_required {
        ExitDecision::Allow
    } else {
        ExitDecision::Prevent
    }
}

/// Holds the handle of the one supervised run so Stop and Quit can reach exactly
/// that process. Never used to find or kill unrelated processes.
#[derive(Default)]
pub struct RunHandles {
    active: Mutex<Option<(String, child::Supervised)>>,
}

impl RunHandles {
    pub fn set(&self, run_id: &str, supervised: child::Supervised) {
        *self.active.lock().unwrap_or_else(PoisonError::into_inner) =
            Some((run_id.to_owned(), supervised));
    }

    pub fn clear(&self, run_id: &str) {
        let mut active = self.active.lock().unwrap_or_else(PoisonError::into_inner);
        if active.as_ref().is_some_and(|(id, _)| id == run_id) {
            *active = None;
        }
    }

    /// Requests termination of the active run, if any. Returns its identifier.
    pub fn kill_active(&self) -> Option<String> {
        let active = self.active.lock().unwrap_or_else(PoisonError::into_inner);
        active.as_ref().map(|(id, supervised)| {
            supervised.kill();
            id.clone()
        })
    }

    /// Requests termination of `run_id` only.
    pub fn kill(&self, run_id: &str) -> bool {
        let active = self.active.lock().unwrap_or_else(PoisonError::into_inner);
        match active.as_ref() {
            Some((id, supervised)) if id == run_id => supervised.kill(),
            _ => false,
        }
    }
}

/// Tracks UI presence and the explicit quit intent.
#[derive(Debug, Default)]
pub struct LifecycleController {
    quit_intent: AtomicBool,
    /// Identifier of the run whose start may unload the window. Cleared when the run
    /// ends so a delayed unload can never act after restoration.
    unload_owner: Mutex<Option<String>>,
    window_operations: AtomicU64,
}

impl LifecycleController {
    pub fn new() -> Self {
        Self::default()
    }

    /// Records a deliberate quit (menu, tray or confirmed Quit command).
    pub fn set_quit_intent(&self) {
        self.quit_intent.store(true, Ordering::SeqCst);
    }

    pub fn quit_intended(&self) -> bool {
        self.quit_intent.load(Ordering::SeqCst)
    }

    /// Marks `run_id` as allowed to unload the window once it survives startup.
    pub fn arm_unload(&self, run_id: &str) {
        *self
            .unload_owner
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(run_id.into());
    }

    /// Invalidates any pending unload for `run_id`. Returns true if it was armed.
    pub fn disarm_unload(&self, run_id: &str) -> bool {
        let mut owner = self
            .unload_owner
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if owner.as_deref() == Some(run_id) {
            *owner = None;
            true
        } else {
            false
        }
    }

    fn unload_armed_for(&self, run_id: &str) -> bool {
        self.unload_owner
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_deref()
            == Some(run_id)
    }

    /// Number of window create/destroy operations performed (diagnostics).
    pub fn window_operations(&self) -> u64 {
        self.window_operations.load(Ordering::SeqCst)
    }

    /// Shows the existing main window or creates exactly one new one.
    pub fn show_main_window<R: Runtime>(&self, app: &AppHandle<R>) {
        let handle = app.clone();
        let _ = app.run_on_main_thread(move || {
            let controller = handle.state::<LifecycleController>();
            if let Some(window) = handle.get_webview_window(MAIN_WINDOW) {
                let _ = window.unminimize();
                let _ = window.show();
                let _ = window.set_focus();
                return;
            }
            let Some(config) = handle
                .config()
                .app
                .windows
                .iter()
                .find(|w| w.label == MAIN_WINDOW)
                .cloned()
            else {
                return;
            };
            if let Ok(builder) = WebviewWindowBuilder::from_config(&handle, &config)
                && let Ok(window) = builder.build()
            {
                controller.window_operations.fetch_add(1, Ordering::SeqCst);
                let _ = window.set_focus();
            }
        });
    }

    /// Shows the console window, creating it on first use. It loads the same
    /// packaged UI, which renders the console for this window label.
    pub fn show_console_window<R: Runtime>(&self, app: &AppHandle<R>) {
        let handle = app.clone();
        let _ = app.run_on_main_thread(move || {
            if let Some(window) = handle.get_webview_window(CONSOLE_WINDOW) {
                let _ = window.unminimize();
                let _ = window.show();
                let _ = window.set_focus();
                return;
            }
            let language = handle
                .try_state::<crate::state::AppState>()
                .map(|state| state.language())
                .unwrap_or_default();
            if let Ok(window) = WebviewWindowBuilder::new(
                &handle,
                CONSOLE_WINDOW,
                WebviewUrl::App("index.html".into()),
            )
            .title(console_title(language))
            .inner_size(980.0, 620.0)
            .min_inner_size(560.0, 320.0)
            .build()
            {
                let _ = window.set_focus();
            }
        });
    }

    /// The user closed the main window. Without a run to supervise the launcher
    /// is ending, so the console closes too instead of keeping the process alive.
    pub fn main_window_closed<R: Runtime>(&self, app: &AppHandle<R>, supervising: bool) {
        if supervising || self.quit_intended() {
            return;
        }
        if let Some(console) = app.get_webview_window(CONSOLE_WINDOW) {
            let _ = console.destroy();
        }
    }

    /// Destroys the main window if `run_id` still owns the unload. Runs on the main
    /// thread, where it is ordered with any restoration request.
    pub fn unload_main_window<R: Runtime>(&self, app: &AppHandle<R>, run_id: String) {
        let handle = app.clone();
        let _ = app.run_on_main_thread(move || {
            let controller = handle.state::<LifecycleController>();
            if !controller.unload_armed_for(&run_id) {
                return;
            }
            if let Some(window) = handle.get_webview_window(MAIN_WINDOW)
                && window.destroy().is_ok()
            {
                controller.window_operations.fetch_add(1, Ordering::SeqCst);
            }
        });
    }

    /// Destroys the main window unconditionally (the user chose to keep the
    /// launcher in the background). Supervision continues.
    pub fn close_main_window<R: Runtime>(&self, app: &AppHandle<R>) {
        let handle = app.clone();
        let _ = app.run_on_main_thread(move || {
            if let Some(window) = handle.get_webview_window(MAIN_WINDOW)
                && window.destroy().is_ok()
            {
                handle
                    .state::<LifecycleController>()
                    .window_operations
                    .fetch_add(1, Ordering::SeqCst);
            }
        });
    }

    /// Called when a supervised run ends: cancels any pending unload and ensures one
    /// window shows the result.
    pub fn restore_after_run<R: Runtime>(&self, app: &AppHandle<R>, run_id: &str) {
        self.disarm_unload(run_id);
        self.show_main_window(app);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_policy() {
        assert_eq!(exit_decision(false, false), ExitDecision::Allow);
        assert_eq!(exit_decision(false, true), ExitDecision::Prevent);
        assert_eq!(exit_decision(true, true), ExitDecision::Allow);
        assert_eq!(exit_decision(true, false), ExitDecision::Allow);
    }

    #[test]
    fn delayed_unload_is_invalidated_by_run_end() {
        let controller = LifecycleController::new();
        controller.arm_unload("run-1");
        assert!(controller.unload_armed_for("run-1"));
        assert!(controller.disarm_unload("run-1"));
        assert!(!controller.unload_armed_for("run-1"));
        assert!(!controller.disarm_unload("run-1"));
    }

    #[test]
    fn a_new_run_does_not_inherit_an_old_unload() {
        let controller = LifecycleController::new();
        controller.arm_unload("run-1");
        controller.arm_unload("run-2");
        assert!(!controller.unload_armed_for("run-1"));
        assert!(!controller.disarm_unload("run-1"));
        assert!(controller.unload_armed_for("run-2"));
    }

    #[test]
    fn quit_intent_is_sticky() {
        let controller = LifecycleController::new();
        assert!(!controller.quit_intended());
        controller.set_quit_intent();
        assert!(controller.quit_intended());
    }
}
