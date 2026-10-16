//! Crash-safe file replacement.
//!
//! A write goes to a uniquely named temporary sibling, is flushed to disk and then
//! renamed over the destination. Rust's `std::fs::rename` replaces an existing file
//! on every supported OS (on Windows through `MoveFileExW` with
//! `MOVEFILE_REPLACE_EXISTING` or its POSIX-semantics successor), so the previous
//! file is never deleted first: an interruption leaves either the old or the new
//! complete content, plus at most a stray temporary file that startup removes.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use uuid::Uuid;

const TEMP_MARKER: &str = ".tmp-";

/// A fully written and flushed temporary file waiting to replace its destination.
#[derive(Debug)]
pub struct PreparedWrite {
    temp: PathBuf,
    destination: PathBuf,
}

impl PreparedWrite {
    /// Atomically replaces the destination with the prepared content.
    pub fn commit(self) -> io::Result<()> {
        if let Err(error) = fs::rename(&self.temp, &self.destination) {
            let _ = fs::remove_file(&self.temp);
            return Err(error);
        }
        sync_parent(&self.destination)
    }

    /// Path of the temporary file (tests use it to simulate a crash before commit).
    pub fn temp_path(&self) -> &Path {
        &self.temp
    }
}

/// Writes `bytes` to a temporary sibling of `destination` and flushes it.
pub fn prepare(destination: &Path, bytes: &[u8]) -> io::Result<PreparedWrite> {
    let parent = destination
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "destination has no parent"))?;
    let name = destination
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "destination has no name"))?
        .to_string_lossy();
    let temp = parent.join(format!(".{name}{TEMP_MARKER}{}", Uuid::new_v4().simple()));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()
    })();
    if let Err(error) = result {
        let _ = fs::remove_file(&temp);
        return Err(error);
    }
    Ok(PreparedWrite {
        temp,
        destination: destination.to_path_buf(),
    })
}

/// Atomically replaces `destination` with `bytes`.
pub fn write(destination: &Path, bytes: &[u8]) -> io::Result<()> {
    prepare(destination, bytes)?.commit()
}

/// Removes temporary files left behind by interrupted writes in `dir`. Only files
/// that match this module's naming pattern are touched.
pub fn remove_stale_temps(dir: &Path) -> io::Result<usize> {
    let mut removed = 0;
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(0),
        Err(error) => return Err(error),
    };
    for entry in entries {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.') && name.contains(TEMP_MARKER) && entry.file_type()?.is_file() {
            fs::remove_file(entry.path())?;
            removed += 1;
        }
    }
    Ok(removed)
}

#[cfg(unix)]
fn sync_parent(path: &Path) -> io::Result<()> {
    match path.parent() {
        Some(parent) => fs::File::open(parent)?.sync_all(),
        None => Ok(()),
    }
}

#[cfg(not(unix))]
fn sync_parent(_path: &Path) -> io::Result<()> {
    // Windows offers no portable directory handle flush; the rename itself is
    // durable once MoveFileEx returns with the file data already flushed.
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaces_existing_content() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        write(&path, b"first").unwrap();
        write(&path, b"second").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"second");
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn interrupted_write_keeps_previous_version_and_is_cleaned_up() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        write(&path, b"last good").unwrap();

        // Simulate a crash after the temporary file is flushed but before rename.
        let prepared = prepare(&path, b"never committed").unwrap();
        let temp = prepared.temp_path().to_path_buf();
        std::mem::forget(prepared);

        assert_eq!(fs::read(&path).unwrap(), b"last good");
        assert!(temp.exists());
        assert_eq!(remove_stale_temps(dir.path()).unwrap(), 1);
        assert!(!temp.exists());
        assert_eq!(fs::read(&path).unwrap(), b"last good");
    }

    #[test]
    fn stale_temp_cleanup_ignores_other_files() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("instance.json"), b"{}").unwrap();
        fs::write(dir.path().join(".hidden"), b"x").unwrap();
        fs::write(dir.path().join("notes.tmp-1"), b"x").unwrap();
        assert_eq!(remove_stale_temps(dir.path()).unwrap(), 0);
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 3);
        assert_eq!(remove_stale_temps(&dir.path().join("missing")).unwrap(), 0);
    }

    #[test]
    fn failed_commit_removes_temp() {
        let dir = tempfile::tempdir().unwrap();
        let destination = dir.path().join("target");
        fs::create_dir(&destination).unwrap();
        fs::write(destination.join("child"), b"x").unwrap();
        // Renaming a file over a non-empty directory fails on every platform.
        let prepared = prepare(&destination, b"data").unwrap();
        let temp = prepared.temp_path().to_path_buf();
        assert!(prepared.commit().is_err());
        assert!(!temp.exists());
    }
}
