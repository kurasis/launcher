//! OS-backed exclusive lock on DATA_ROOT for the supervisor's lifetime
//! (docs/spec/02 "Cross-version cache and commit authority", doc 10 MV09).
//!
//! The lock is an advisory OS file lock (`flock` on Unix, `LockFileEx` on Windows)
//! held on an open handle. The operating system releases it when the process exits,
//! including after a crash, so a stale lock file never blocks the next start and the
//! file is never deleted to "force" an unlock.

use std::fs::{self, File, OpenOptions, TryLockError};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// Holds the exclusive DATA_ROOT lock until dropped.
#[derive(Debug)]
pub struct DataRootLock {
    file: File,
    path: PathBuf,
}

#[derive(Debug, thiserror::Error)]
pub enum LockError {
    #[error("another launcher process holds the data directory lock")]
    Held,
    #[error("the data directory lock could not be acquired: {0}")]
    Io(#[from] io::Error),
}

impl DataRootLock {
    /// Tries to take the lock without waiting.
    pub fn acquire(lock_path: &Path) -> Result<Self, LockError> {
        if let Some(parent) = lock_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(lock_path)?;
        match file.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => return Err(LockError::Held),
            Err(TryLockError::Error(error)) => return Err(LockError::Io(error)),
        }
        // Informational only; the OS lock, not this content, is authoritative.
        file.set_len(0)?;
        writeln!(file, "pid={}", std::process::id())?;
        file.sync_all()?;
        Ok(Self {
            file,
            path: lock_path.to_path_buf(),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for DataRootLock {
    fn drop(&mut self) {
        let _ = self.file.unlock();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn second_holder_is_rejected_until_release() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("locks").join("supervisor.lock");
        let first = DataRootLock::acquire(&path).unwrap();
        assert!(matches!(DataRootLock::acquire(&path), Err(LockError::Held)));
        drop(first);
        let again = DataRootLock::acquire(&path).unwrap();
        assert_eq!(again.path(), path);
    }

    #[test]
    fn leftover_lock_file_without_holder_does_not_block() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("supervisor.lock");
        fs::write(&path, "pid=999999\n").unwrap();
        assert!(DataRootLock::acquire(&path).is_ok());
    }
}
