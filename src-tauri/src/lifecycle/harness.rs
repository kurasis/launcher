//! Lifecycle test harness (docs/spec/phases/P03). Compiled into debug builds only.
//!
//! The "test child" is this launcher executable started with a fixed flag and one
//! of a few named scenarios. Nothing here accepts a path, a command line or free
//! text, so the harness cannot launch arbitrary processes, and release builds do
//! not contain it at all.

use std::io::Write;
use std::process::Command;
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::Duration;

use chrono::Utc;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, Runtime};
use uuid::Uuid;

use super::SURVIVAL_INTERVAL;
use super::child::{self, OutputLine, OutputTail};
use super::{LifecycleController, RunHandles};
use crate::error::{ErrorCode, LauncherError, LauncherResult};
use crate::state::AppState;
use crate::state::dto::{RunKind, RunState, RunSummary};
use crate::storage::models::{DuringGameMode, OperationKind};

/// First argument that turns the executable into a test child.
pub const CHILD_FLAG: &str = "--launcher-lifecycle-test-child";

/// Lines from the end of the output kept in the run summary shown to the UI.
const SUMMARY_TAIL_LINES: usize = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Scenario {
    /// Exits with code 0 right away (before the survival interval).
    ImmediateExit,
    /// Runs about 4 seconds, then exits with code 0.
    Short,
    /// Runs about 12 seconds, then exits with code 0.
    Long,
    /// Runs about 4 seconds, then exits with code 3 after writing to stderr.
    Crash,
    /// Starts a grandchild that keeps the output pipes open, then exits.
    InheritedPipe,
    /// Writes about 32 MiB to both pipes as fast as possible, then exits.
    Noisy,
}

impl Scenario {
    pub fn as_arg(self) -> &'static str {
        match self {
            Self::ImmediateExit => "immediate-exit",
            Self::Short => "short",
            Self::Long => "long",
            Self::Crash => "crash",
            Self::InheritedPipe => "inherited-pipe",
            Self::Noisy => "noisy",
        }
    }
}

/// Entry point for the child side. Returns the exit code when `args` select a
/// test child; `None` means "start the launcher normally".
pub fn child_main(args: &[String]) -> Option<i32> {
    if args.get(1).map(String::as_str) != Some(CHILD_FLAG) {
        return None;
    }
    let scenario = args.get(2).map(String::as_str).unwrap_or_default();
    let out = &mut std::io::stdout();
    let err = &mut std::io::stderr();
    let ticks = |out: &mut std::io::Stdout, seconds: u64| {
        for i in 0..(seconds * 4) {
            let _ = writeln!(out, "test child tick {i}");
            let _ = out.flush();
            thread::sleep(Duration::from_millis(250));
        }
    };
    Some(match scenario {
        "immediate-exit" => {
            let _ = writeln!(out, "test child: exiting immediately");
            0
        }
        "short" => {
            ticks(out, 4);
            0
        }
        "long" => {
            ticks(out, 12);
            0
        }
        "crash" => {
            ticks(out, 4);
            let _ = writeln!(err, "test child: simulated crash");
            3
        }
        "inherited-pipe" => {
            // The grandchild inherits stdout/stderr and keeps them open.
            let spawned = std::env::current_exe()
                .map(|exe| Command::new(exe).args([CHILD_FLAG, "pipe-holder"]).spawn());
            let _ = writeln!(out, "test child: started pipe holder: {}", spawned.is_ok());
            0
        }
        "pipe-holder" => {
            thread::sleep(Duration::from_secs(8));
            0
        }
        "noisy" => {
            let line = "n".repeat(1023);
            for _ in 0..(16 * 1024) {
                let _ = writeln!(out, "{line}");
                let _ = writeln!(err, "{line}");
            }
            0
        }
        _ => 64,
    })
}

/// Starts a test child under supervision and drives the window lifecycle exactly
/// as a game run will: unload after the survival interval in the default mode,
/// restore one window when it exits.
pub fn start<R: Runtime>(app: &AppHandle<R>, scenario: Scenario) -> LauncherResult<RunSummary> {
    let state = app.state::<AppState>();
    let run_id = Uuid::new_v4().to_string();
    let permit = state
        .activity()
        .try_acquire(OperationKind::Launch, None, run_id.clone())?;
    let exe =
        std::env::current_exe().map_err(|e| LauncherError::from_io(&e, "Locating the launcher"))?;
    let mut command = Command::new(exe);
    command.args([CHILD_FLAG, scenario.as_arg()]);

    let run = RunSummary {
        run_id: run_id.clone(),
        kind: RunKind::LifecycleTest,
        instance_id: None,
        state: RunState::Spawning,
        started_at: Utc::now(),
        ended_at: None,
        exit_code: None,
        duration_ms: None,
        drain_timed_out: false,
        output_tail: Vec::new(),
        output_lines_dropped: 0,
        error: None,
        logging_degraded: false,
        signal: None,
        diagnosis: None,
        server: None,
    };
    state.begin_run(run);

    let tail = Arc::new(Mutex::new(OutputTail::default()));
    let sink_tail = Arc::clone(&tail);
    let handle = app.clone();
    let exit_run_id = run_id.clone();
    let exited_handle = app.clone();
    let exited_run_id = run_id.clone();
    let spawned = child::spawn(
        command,
        Box::new(move |line: OutputLine| {
            sink_tail
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(line);
        }),
        // The process is gone; a pending window unload must not happen during the
        // bounded output drain that follows.
        move || {
            exited_handle
                .state::<LifecycleController>()
                .disarm_unload(&exited_run_id);
        },
        move |outcome| {
            let state = handle.state::<AppState>();
            let tail = tail.lock().unwrap_or_else(PoisonError::into_inner);
            let lines: Vec<String> = tail.lines().map(|l| l.text.clone()).collect();
            let start = lines.len().saturating_sub(SUMMARY_TAIL_LINES);
            state.finish_run(&exit_run_id, |run| {
                run.ended_at = Some(Utc::now());
                run.duration_ms = u64::try_from(outcome.duration.as_millis()).ok();
                run.drain_timed_out = outcome.drain_timed_out;
                run.output_tail = lines[start..].to_vec();
                run.output_lines_dropped = tail.dropped() + start as u64;
                match &outcome.status {
                    Ok(status) => {
                        run.exit_code = status.code();
                        run.state = if status.success() {
                            RunState::Exited
                        } else {
                            RunState::Failed
                        };
                    }
                    Err(kind) => {
                        run.state = RunState::Failed;
                        run.error = Some(
                            LauncherError::new(
                                ErrorCode::GameExitedWithError,
                                "Waiting for the test child failed.",
                            )
                            .with_detail("ioKind", kind.clone()),
                        );
                    }
                }
            });
            handle.state::<RunHandles>().clear(&exit_run_id);
            // Release the permit only after exit, bounded drain and cleanup.
            drop(permit);
            state.announce_runs();
            handle
                .state::<LifecycleController>()
                .restore_after_run(&handle, &exit_run_id);
        },
    );

    let supervised = match spawned {
        Ok(supervised) => supervised,
        Err(error) => {
            let launcher_error = LauncherError::new(
                ErrorCode::GameSpawnFailed,
                "The test child could not be started.",
            )
            .with_detail("ioKind", format!("{:?}", error.kind()));
            state.finish_run(&run_id, |run| {
                run.state = RunState::Failed;
                run.ended_at = Some(Utc::now());
                run.error = Some(launcher_error.clone());
            });
            state.announce_runs();
            return Err(launcher_error);
        }
    };
    app.state::<RunHandles>().set(&run_id, supervised);
    state.update_run(&run_id, |run| run.state = RunState::Running);

    if state.during_game_mode() == DuringGameMode::UnloadAndReturn {
        let controller = app.state::<LifecycleController>();
        controller.arm_unload(&run_id);
        let handle = app.clone();
        let unload_run_id = run_id.clone();
        thread::spawn(move || {
            thread::sleep(SURVIVAL_INTERVAL);
            let state = handle.state::<AppState>();
            if state
                .current_run()
                .is_some_and(|r| r.run_id == unload_run_id && r.state == RunState::Running)
            {
                handle
                    .state::<LifecycleController>()
                    .unload_main_window(&handle, unload_run_id);
            }
        });
    }

    // A very short child may already have finished; report whichever is recorded.
    state
        .current_run()
        .filter(|r| r.run_id == run_id)
        .or_else(|| state.snapshot().last_run.filter(|r| r.run_id == run_id))
        .ok_or_else(|| LauncherError::internal("The run disappeared."))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_child_arguments_start_the_launcher() {
        assert_eq!(child_main(&["app".into()]), None);
        assert_eq!(child_main(&["app".into(), "--other".into()]), None);
    }

    #[test]
    fn child_scenarios_have_fixed_exit_codes() {
        let run = |name: &str| child_main(&["app".into(), CHILD_FLAG.into(), name.into()]);
        assert_eq!(run("immediate-exit"), Some(0));
        assert_eq!(run("unknown"), Some(64));
    }

    #[test]
    fn scenarios_deserialize_from_snake_case_only() {
        assert_eq!(
            serde_json::from_str::<Scenario>("\"inherited_pipe\"").unwrap(),
            Scenario::InheritedPipe
        );
        assert!(serde_json::from_str::<Scenario>("\"/bin/sh\"").is_err());
    }
}
