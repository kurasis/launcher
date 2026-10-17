//! Recovery after an unexpected supervisor restart (docs/spec/05 "Background
//! access and recovery", doc 10 rule 6).
//!
//! While a game runs, `locks/active-run.json` names its PID and the process
//! start time reported by the OS. At startup a record whose PID still exists
//! with the same start time and executable is a surviving game: it is not
//! adopted (its pipes are gone), but it holds the global activity permit until it
//! exits, so no launch, install or repair can touch shared files meanwhile. A
//! record whose PID is gone or now belongs to another process is stale and is
//! removed; that other process is never signalled.

use std::path::{Path, PathBuf};
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

use crate::error::{ErrorCode, LauncherError};
use crate::storage::atomic;
use crate::storage::document::SCHEMA_VERSION;
use crate::storage::paths::{DataRoot, InstanceId};

/// Poll interval while waiting for a surviving game to exit.
pub const WATCH_INTERVAL: Duration = Duration::from_secs(2);

/// Allowed difference between recorded and observed process start times. Linux
/// derives them from the kernel's boot time, which is wall clock minus uptime and
/// moves when the clock is corrected (NTP), so the same process can be reported
/// a second or more apart. A reused PID would also need the same executable and
/// a start within this window of a game that had to exit first.
pub const START_TOLERANCE_SECS: u64 = 5;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActiveRunRecord {
    pub schema_version: u64,
    pub run_id: String,
    pub instance_id: InstanceId,
    pub pid: u32,
    /// Process start time in seconds since the epoch, as reported by the OS.
    pub process_start: u64,
    pub executable: PathBuf,
    pub started_at: DateTime<Utc>,
}

fn path(root: &DataRoot) -> PathBuf {
    root.locks_dir().join("active-run.json")
}

/// The OS view of a process: start time and executable, if it exists.
pub fn process_identity(pid: u32) -> Option<(u64, Option<PathBuf>)> {
    let mut system = System::new();
    let pid = Pid::from_u32(pid);
    system.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[pid]),
        true,
        ProcessRefreshKind::nothing().with_exe(UpdateKind::Always),
    );
    let process = system.process(pid)?;
    Some((process.start_time(), process.exe().map(Path::to_path_buf)))
}

/// Records the running game. Best effort: a failure only weakens recovery.
pub fn record(
    root: &DataRoot,
    run_id: &str,
    instance_id: InstanceId,
    pid: u32,
    executable: &Path,
) -> bool {
    let Some((process_start, _)) = process_identity(pid) else {
        return false;
    };
    let record = ActiveRunRecord {
        schema_version: SCHEMA_VERSION,
        run_id: run_id.to_owned(),
        instance_id,
        pid,
        process_start,
        executable: executable.to_path_buf(),
        started_at: Utc::now(),
    };
    let Ok(mut bytes) = serde_json::to_vec_pretty(&record) else {
        return false;
    };
    bytes.push(b'\n');
    std::fs::create_dir_all(root.locks_dir()).is_ok() && atomic::write(&path(root), &bytes).is_ok()
}

/// Removes the record if it belongs to `run_id`.
pub fn clear(root: &DataRoot, run_id: &str) {
    if read(root).is_some_and(|r| r.run_id == run_id) {
        let _ = std::fs::remove_file(path(root));
    }
}

pub fn read(root: &DataRoot) -> Option<ActiveRunRecord> {
    let bytes = std::fs::read(path(root)).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// True while the recorded process is still the same process.
pub fn is_alive(record: &ActiveRunRecord) -> bool {
    process_identity(record.pid).is_some_and(|(start, exe)| {
        start.abs_diff(record.process_start) <= START_TOLERANCE_SECS
            && exe.is_none_or(|exe| same_executable(&exe, &record.executable))
    })
}

fn same_executable(seen: &Path, recorded: &Path) -> bool {
    let canonical = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    canonical(seen) == canonical(recorded)
}

/// At startup: the surviving game, or `None` after removing a stale record.
pub fn survivor(root: &DataRoot) -> Option<ActiveRunRecord> {
    let record = read(root)?;
    if is_alive(&record) {
        Some(record)
    } else {
        let _ = std::fs::remove_file(path(root));
        None
    }
}

/// Terminates a surviving game after re-checking its identity. Returns false if
/// it is gone or no longer the recorded process.
pub fn stop(record: &ActiveRunRecord) -> bool {
    if !is_alive(record) {
        return false;
    }
    let mut system = System::new();
    let pid = Pid::from_u32(record.pid);
    system.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[pid]),
        true,
        ProcessRefreshKind::nothing(),
    );
    system.process(pid).is_some_and(sysinfo::Process::kill)
}

pub fn interrupted_error() -> LauncherError {
    LauncherError::new(
        ErrorCode::OperationInterrupted,
        "The launcher restarted while this game was running, so its output and exit code \
         are not available. Installing and playing were blocked until it ended.",
    )
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    use super::*;

    fn root() -> (tempfile::TempDir, DataRoot) {
        let dir = tempfile::tempdir().unwrap();
        let root = DataRoot::new(dir.path().to_path_buf()).unwrap();
        (dir, root)
    }

    fn sleeper() -> (std::process::Child, PathBuf) {
        let (program, args): (&str, &[&str]) = if cfg!(windows) {
            (
                "powershell",
                &["-NoProfile", "-Command", "Start-Sleep -Seconds 30"],
            )
        } else {
            ("sleep", &["30"])
        };
        let child = Command::new(program).args(args).spawn().unwrap();
        let exe = process_identity(child.id())
            .and_then(|(_, e)| e)
            .unwrap_or_default();
        (child, exe)
    }

    #[test]
    fn a_live_recorded_process_is_a_survivor_until_it_exits() {
        let (_dir, root) = root();
        let (mut child, exe) = sleeper();
        let id = InstanceId::generate();
        assert!(record(&root, "run-1", id, child.id(), &exe));
        let found = survivor(&root).expect("survivor");
        assert_eq!((found.run_id.as_str(), found.instance_id), ("run-1", id));
        assert!(stop(&found), "identity re-checked, then killed");
        let _ = child.wait();
        assert!(!is_alive(&found));
        assert!(survivor(&root).is_none(), "gone: record removed");
        assert!(read(&root).is_none());
    }

    #[test]
    fn a_reused_pid_is_never_treated_as_the_game() {
        let (_dir, root) = root();
        let (mut child, exe) = sleeper();
        assert!(record(
            &root,
            "run-2",
            InstanceId::generate(),
            child.id(),
            &exe
        ));
        // A start time reported slightly differently is still the same process.
        let mut drifted = read(&root).unwrap();
        drifted.process_start += 1;
        assert!(is_alive(&drifted));
        drifted.process_start -= 2;
        assert!(is_alive(&drifted));
        // Same PID, different start time: another process now.
        let mut stale = read(&root).unwrap();
        stale.process_start += 3600;
        assert!(!is_alive(&stale));
        assert!(!stop(&stale), "an unrelated process is not signalled");
        // Same PID and start, different executable.
        let mut other = read(&root).unwrap();
        other.executable = PathBuf::from("/definitely/not/java");
        assert!(!is_alive(&other));
        let _ = child.kill();
        let _ = child.wait();
    }

    #[test]
    fn clearing_only_removes_the_own_record() {
        let (_dir, root) = root();
        let (mut child, exe) = sleeper();
        assert!(record(
            &root,
            "run-3",
            InstanceId::generate(),
            child.id(),
            &exe
        ));
        clear(&root, "another-run");
        assert!(read(&root).is_some());
        clear(&root, "run-3");
        assert!(read(&root).is_none());
        let _ = child.kill();
        let _ = child.wait();
    }
}
