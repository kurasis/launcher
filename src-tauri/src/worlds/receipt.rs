//! The run receipt of a world server, `locks/world-server.json` (PLAN W3).
//!
//! Written right after spawn and removed when the server exits. A receipt
//! whose process is still the recorded one after a core restart names a
//! surviving server: it keeps the hosting permit until it exits, and the
//! recorded RCON port and per-run password stop it cleanly. A PID alone is
//! never used to kill anything (ADAPTATION A1).

use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::launch::orphan;
use crate::secrets::SecretString;
use crate::storage::atomic;
use crate::storage::document::SCHEMA_VERSION;
use crate::storage::paths::{DataRoot, WorldId};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerReceipt {
    pub schema_version: u64,
    pub world_id: WorldId,
    pub operation_id: String,
    pub pid: u32,
    pub process_start: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub boot_id: Option<String>,
    pub executable: PathBuf,
    pub port: u16,
    pub rcon_port: u16,
    /// Per-run, local only; the server's own `server.properties` holds it too.
    pub rcon_password: String,
    pub started_at: DateTime<Utc>,
}

impl ServerReceipt {
    pub fn rcon_password(&self) -> SecretString {
        SecretString::new(self.rcon_password.clone())
    }

    pub fn is_alive(&self) -> bool {
        orphan::is_same_process(
            self.pid,
            self.process_start,
            self.boot_id.as_deref(),
            &self.executable,
        )
    }
}

fn path(root: &DataRoot) -> PathBuf {
    root.locks_dir().join("world-server.json")
}

/// What the receipt of a just-spawned server records.
pub struct Spawned<'a> {
    pub world_id: WorldId,
    pub operation_id: &'a str,
    pub pid: u32,
    pub executable: &'a Path,
    pub port: u16,
    pub rcon_port: u16,
    pub rcon_password: &'a SecretString,
}

/// Records the running server. Best effort: a failure only weakens recovery.
pub fn record(root: &DataRoot, spawned: &Spawned<'_>) -> bool {
    let Some((process_start, _)) = orphan::process_identity(spawned.pid) else {
        return false;
    };
    let receipt = ServerReceipt {
        schema_version: SCHEMA_VERSION,
        world_id: spawned.world_id,
        operation_id: spawned.operation_id.to_owned(),
        pid: spawned.pid,
        process_start,
        boot_id: orphan::boot_id(),
        executable: spawned.executable.to_path_buf(),
        port: spawned.port,
        rcon_port: spawned.rcon_port,
        rcon_password: spawned.rcon_password.expose().to_owned(),
        started_at: Utc::now(),
    };
    let Ok(mut bytes) = serde_json::to_vec_pretty(&receipt) else {
        return false;
    };
    bytes.push(b'\n');
    std::fs::create_dir_all(root.locks_dir()).is_ok() && atomic::write(&path(root), &bytes).is_ok()
}

pub fn read(root: &DataRoot) -> Option<ServerReceipt> {
    let bytes = std::fs::read(path(root)).ok()?;
    if bytes.len() > 64 * 1024 {
        return None;
    }
    serde_json::from_slice(&bytes).ok()
}

/// Removes the receipt if it belongs to `operation_id`.
pub fn clear(root: &DataRoot, operation_id: &str) {
    if read(root).is_some_and(|r| r.operation_id == operation_id) {
        let _ = std::fs::remove_file(path(root));
    }
}

/// At startup: the surviving server, or `None` after removing a stale receipt.
pub fn survivor(root: &DataRoot) -> Option<ServerReceipt> {
    let receipt = read(root)?;
    if receipt.is_alive() {
        Some(receipt)
    } else {
        let _ = std::fs::remove_file(path(root));
        None
    }
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    use super::*;

    #[test]
    fn a_receipt_names_a_survivor_until_the_process_exits() {
        let dir = tempfile::tempdir().unwrap();
        let root = DataRoot::new(dir.path().to_path_buf()).unwrap();
        let (program, args): (&str, &[&str]) = if cfg!(windows) {
            (
                "powershell",
                &["-NoProfile", "-Command", "Start-Sleep -Seconds 30"],
            )
        } else {
            ("sleep", &["30"])
        };
        let mut child = Command::new(program).args(args).spawn().unwrap();
        let exe = orphan::exe_after_exec(child.id());
        let password = SecretString::new("pw");
        let world_id = WorldId::generate();
        assert!(record(
            &root,
            &Spawned {
                world_id,
                operation_id: "op-1",
                pid: child.id(),
                executable: &exe,
                port: 40000,
                rcon_port: 40001,
                rcon_password: &password,
            }
        ));
        let found = survivor(&root).expect("survivor");
        assert_eq!((found.world_id, found.rcon_port), (world_id, 40001));
        assert_eq!(found.rcon_password(), password);
        clear(&root, "op-other");
        assert!(read(&root).is_some(), "another operation's clear keeps it");
        child.kill().unwrap();
        let _ = child.wait();
        assert!(survivor(&root).is_none());
        assert!(read(&root).is_none(), "stale receipt removed");
    }
}
