//! What the file system said about a runtime the last time every file was
//! hashed and matched its tree receipt: each entry with its size, link target,
//! executable bit and the time it last changed. A launch compares these instead
//! of hashing every file again; any difference, or no stamp, falls back to the
//! full check, and Repair always hashes. Damage that keeps a file's size and
//! time is Repair's to find, as it already is for game files.
//!
//! The stamp is not a security boundary: whoever can rewrite it can rewrite the
//! receipts and the install lock beside it.

use std::fs;
use std::path::Path;
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};

use super::tree::{self, Seen};
use crate::storage::atomic;
use crate::storage::document::SCHEMA_VERSION;
use crate::storage::paths::Sha256;

pub const STAMP: &str = ".launcher-checked.json";

/// A stamp of a runtime within its limits (20,000 entries) stays far below this.
const MAX_STAMP_BYTES: u64 = 16 * 1024 * 1024;

/// Files changed this close to a check are not stamped: a later change in the
/// same clock tick (two seconds on FAT) would leave the time as it was.
const SETTLE: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Stamp {
    schema_version: u64,
    /// The tree receipt the files were checked against.
    tree_receipt_sha256: Sha256,
    entries: Vec<Seen>,
}

/// True when the runtime in `dir` looks exactly as when it was last stamped
/// against this tree receipt. Any doubt answers false.
pub fn unchanged(dir: &Path, tree_receipt_sha256: &Sha256) -> bool {
    let path = dir.join(STAMP);
    if !fs::metadata(&path).is_ok_and(|m| m.is_file() && m.len() <= MAX_STAMP_BYTES) {
        return false;
    }
    let Ok(bytes) = fs::read(&path) else {
        return false;
    };
    let Ok(stamp) = serde_json::from_slice::<Stamp>(&bytes) else {
        return false;
    };
    stamp.schema_version == SCHEMA_VERSION
        && stamp.tree_receipt_sha256 == *tree_receipt_sha256
        && tree::scan(dir, false).is_ok_and(|seen| seen == stamp.entries)
}

/// Records a hashed scan that matched the tree receipt; `started` is when that
/// scan began. Best effort: without a stamp the next launch hashes again.
pub fn write(dir: &Path, tree_receipt_sha256: &Sha256, seen: &[Seen], started: SystemTime) {
    let settled = started
        .checked_sub(SETTLE)
        .is_some_and(|before| tree::settled(seen, before));
    if !settled {
        remove(dir);
        return;
    }
    let stamp = Stamp {
        schema_version: SCHEMA_VERSION,
        tree_receipt_sha256: tree_receipt_sha256.clone(),
        entries: tree::without_hashes(seen),
    };
    match serde_json::to_vec(&stamp) {
        Ok(bytes) => {
            if atomic::write(&dir.join(STAMP), &bytes).is_err() {
                remove(dir);
            }
        }
        Err(_) => remove(dir),
    }
}

/// Forgets the stamp, so the next launch hashes every file.
pub fn remove(dir: &Path) {
    let _ = fs::remove_file(dir.join(STAMP));
}
