//! Launcher-captured game output on disk (docs/spec/05 "Process supervision").
//!
//! Each run writes redacted lines to `logs/runs/<run>/output.log`, rotated at
//! 10 MiB with at most 5 files. A write failure (disk full, permissions) switches
//! the run to "logging degraded": output keeps being drained and kept in the
//! bounded memory tail, nothing raw is written anywhere, and the run records the
//! condition. Completed runs are pruned to the newest 20 and 200 MiB in total.
//! Game-owned logs and worlds are never touched.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

/// Log lines that appear once the client window is up, newest era first:
/// texture atlases (1.13+), the 1.7–1.12 atlas and sound engine, the 1.6 resource
/// manager, and the launch wrapper of older versions (1.5.2 and before).
pub const READY_MARKERS: &[&str] = &[
    "minecraft:textures/atlas/",
    "textures-atlas",
    "Sound engine started",
    "Reloading ResourceManager",
    "Launching wrapped minecraft",
];

/// Whether a run's log shows that the game window is up.
pub fn shows_game_window(text: &str) -> bool {
    READY_MARKERS.iter().any(|m| text.contains(m))
}

pub const MAX_FILE_BYTES: u64 = 10 * 1024 * 1024;
pub const MAX_FILES: usize = 5;
pub const MAX_RUNS: usize = 20;
pub const MAX_TOTAL_BYTES: u64 = 200 * 1024 * 1024;

pub struct RunLog {
    dir: PathBuf,
    file: Option<File>,
    index: usize,
    written: u64,
    degraded: bool,
    max_file_bytes: u64,
}

fn file_name(index: usize) -> String {
    if index == 0 {
        "output.log".into()
    } else {
        format!("output.{index}.log")
    }
}

impl RunLog {
    pub fn open(dir: &Path) -> Self {
        Self::with_limit(dir, MAX_FILE_BYTES)
    }

    pub fn with_limit(dir: &Path, max_file_bytes: u64) -> Self {
        let mut log = Self {
            dir: dir.to_path_buf(),
            file: None,
            index: 0,
            written: 0,
            degraded: false,
            max_file_bytes,
        };
        if fs::create_dir_all(dir).is_err() {
            log.degraded = true;
        } else {
            log.file = log.create(0);
            log.degraded = log.file.is_none();
        }
        log
    }

    fn create(&self, index: usize) -> Option<File> {
        OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(self.dir.join(file_name(index)))
            .ok()
    }

    /// True once writing failed; capture continues in memory only.
    pub fn degraded(&self) -> bool {
        self.degraded
    }

    /// Writes one already-redacted line. Never blocks on repeated failures.
    pub fn write_line(&mut self, line: &str) {
        if self.degraded {
            return;
        }
        let bytes = line.len() as u64 + 1;
        if self.written + bytes > self.max_file_bytes && self.written > 0 {
            // Rotate: output.log is the newest; older parts shift up, oldest dropped.
            self.file = None;
            let _ = fs::remove_file(self.dir.join(file_name(MAX_FILES - 1)));
            for index in (0..MAX_FILES - 1).rev() {
                let _ = fs::rename(
                    self.dir.join(file_name(index)),
                    self.dir.join(file_name(index + 1)),
                );
            }
            self.index += 1;
            self.written = 0;
            self.file = self.create(0);
        }
        let ok = match self.file.as_mut() {
            Some(file) => file
                .write_all(line.as_bytes())
                .and_then(|()| file.write_all(b"\n"))
                .is_ok(),
            None => false,
        };
        if ok {
            self.written += bytes;
        } else {
            self.degraded = true;
            self.file = None;
        }
    }
}

fn dir_size(dir: &Path) -> u64 {
    fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .filter_map(|e| e.metadata().ok())
                .filter(|m| m.is_file())
                .map(|m| m.len())
                .sum()
        })
        .unwrap_or(0)
}

/// Removes the oldest completed run logs beyond the count and size limits.
/// `active` is never removed.
pub fn prune(runs_dir: &Path, active: Option<&str>, max_runs: usize, max_bytes: u64) {
    let Ok(entries) = fs::read_dir(runs_dir) else {
        return;
    };
    let mut runs: Vec<(std::time::SystemTime, PathBuf, u64)> = entries
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .filter(|e| active != Some(e.file_name().to_string_lossy().as_ref()))
        .map(|e| {
            let modified = e
                .metadata()
                .and_then(|m| m.modified())
                .unwrap_or(std::time::UNIX_EPOCH);
            let size = dir_size(&e.path());
            (modified, e.path(), size)
        })
        .collect();
    runs.sort_by_key(|run| std::cmp::Reverse(run.0));
    let mut total = 0u64;
    for (index, (_, path, size)) in runs.into_iter().enumerate() {
        total += size;
        if index >= max_runs || total > max_bytes {
            let _ = fs::remove_dir_all(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotation_keeps_at_most_five_bounded_files() {
        let dir = tempfile::tempdir().unwrap();
        let mut log = RunLog::with_limit(dir.path(), 100);
        for i in 0..100 {
            log.write_line(&format!("line {i:03} ........................"));
        }
        assert!(!log.degraded());
        let mut files: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap())
            .collect();
        assert_eq!(files.len(), MAX_FILES);
        files.sort_by_key(|e| e.file_name());
        for file in &files {
            assert!(file.metadata().unwrap().len() <= 100);
        }
        let newest = fs::read_to_string(dir.path().join("output.log")).unwrap();
        assert!(newest.contains("line 099"));
    }

    #[cfg(unix)]
    #[test]
    fn write_failures_degrade_without_blocking() {
        // A directory below a regular file cannot be created, even by root.
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("file");
        fs::write(&file, b"x").unwrap();
        let mut log = RunLog::open(&file.join("logs"));
        assert!(log.degraded(), "directory cannot be created");
        log.write_line("still drained");
        // /dev/full accepts the open and fails every write with ENOSPC.
        if Path::new("/dev/full").exists() {
            let mut log = RunLog {
                dir: PathBuf::from("/dev"),
                file: OpenOptions::new().write(true).open("/dev/full").ok(),
                index: 0,
                written: 0,
                degraded: false,
                max_file_bytes: MAX_FILE_BYTES,
            };
            log.write_line("a line that cannot be stored");
            assert!(log.degraded());
            log.write_line("ignored, not retried");
        }
    }

    #[test]
    fn pruning_keeps_the_newest_runs_within_limits() {
        let dir = tempfile::tempdir().unwrap();
        for i in 0..5 {
            let run = dir.path().join(format!("run-{i}"));
            fs::create_dir(&run).unwrap();
            fs::write(run.join("output.log"), vec![b'x'; 1000]).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(15));
        }
        prune(dir.path(), Some("run-0"), 3, 10_000);
        let mut left: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        left.sort();
        assert_eq!(
            left,
            ["run-0", "run-2", "run-3", "run-4"],
            "active run kept"
        );
        prune(dir.path(), None, 20, 2500);
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 2);
    }
}
