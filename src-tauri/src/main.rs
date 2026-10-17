// Prevents an additional console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Debug builds can act as the harmless lifecycle test child (P03 harness).
    #[cfg(debug_assertions)]
    {
        let args: Vec<String> = std::env::args().collect();
        if let Some(code) = launcher_lib::lifecycle::harness::child_main(&args) {
            std::process::exit(code);
        }
    }
    launcher_lib::run();
}
