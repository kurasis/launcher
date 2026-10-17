//! Extracted-tree receipts: every directory, regular file (size, SHA-256,
//! executable bit) and link target of an installed runtime. An archive checksum
//! says nothing about files after extraction; this receipt is what Repair and
//! reuse check. Launcher receipt files at the top level are excluded.

use std::fs;
use std::io::Read;
use std::path::Path;

use serde::{Deserialize, Serialize};
use sha2::Digest as _;

use super::extract::is_executable;
use crate::error::{LauncherError, LauncherResult};
use crate::storage::document::SCHEMA_VERSION;

/// Top-level names starting with this prefix belong to the launcher.
pub const RECEIPT_PREFIX: &str = ".kurasis-";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryKind {
    Dir,
    File,
    Symlink,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TreeEntry {
    /// `/`-separated path relative to the runtime root.
    pub path: String,
    pub kind: EntryKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub executable: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TreeReceipt {
    pub schema_version: u64,
    pub entries: Vec<TreeEntry>,
}

/// The first difference between a receipt and the files on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TreeProblem {
    Missing(String),
    Unexpected(String),
    Changed(String),
}

impl TreeProblem {
    pub fn path(&self) -> &str {
        match self {
            Self::Missing(p) | Self::Unexpected(p) | Self::Changed(p) => p,
        }
    }
}

fn io_error(error: &std::io::Error) -> LauncherError {
    LauncherError::from_io(error, "Checking the Java runtime failed")
}

fn sha256_file(path: &Path) -> LauncherResult<String> {
    let mut file = fs::File::open(path).map_err(|e| io_error(&e))?;
    let mut hasher = sha2::Sha256::new();
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buffer).map_err(|e| io_error(&e))?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

fn walk(dir: &Path, prefix: &str, out: &mut Vec<TreeEntry>) -> LauncherResult<()> {
    let mut children: Vec<_> = fs::read_dir(dir)
        .map_err(|e| io_error(&e))?
        .collect::<Result<_, _>>()
        .map_err(|e| io_error(&e))?;
    children.sort_by_key(fs::DirEntry::file_name);
    for child in children {
        let name = child.file_name().to_string_lossy().into_owned();
        if prefix.is_empty() && name.starts_with(RECEIPT_PREFIX) {
            continue;
        }
        let path = if prefix.is_empty() {
            name
        } else {
            format!("{prefix}/{name}")
        };
        let full = child.path();
        // Never follows links: the link itself is recorded.
        let metadata = fs::symlink_metadata(&full).map_err(|e| io_error(&e))?;
        let file_type = metadata.file_type();
        if file_type.is_symlink() {
            let target = fs::read_link(&full).map_err(|e| io_error(&e))?;
            out.push(TreeEntry {
                path,
                kind: EntryKind::Symlink,
                size: None,
                sha256: None,
                executable: false,
                target: Some(target.to_string_lossy().replace('\\', "/")),
            });
        } else if file_type.is_dir() {
            out.push(TreeEntry {
                path: path.clone(),
                kind: EntryKind::Dir,
                size: None,
                sha256: None,
                executable: false,
                target: None,
            });
            walk(&full, &path, out)?;
        } else if file_type.is_file() {
            out.push(TreeEntry {
                path,
                kind: EntryKind::File,
                size: Some(metadata.len()),
                sha256: Some(sha256_file(&full)?),
                executable: is_executable(&full),
                target: None,
            });
        } else {
            out.push(TreeEntry {
                path,
                kind: EntryKind::File,
                size: None,
                sha256: None,
                executable: false,
                target: Some("special file".into()),
            });
        }
    }
    Ok(())
}

/// Describes the tree under `root` as it is now.
pub fn build(root: &Path) -> LauncherResult<TreeReceipt> {
    let mut entries = Vec::new();
    walk(root, "", &mut entries)?;
    // `verify` merges two lists, so both must be in plain path order.
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(TreeReceipt {
        schema_version: SCHEMA_VERSION,
        entries,
    })
}

/// Compares the tree under `root` with `expected`.
pub fn verify(root: &Path, expected: &TreeReceipt) -> LauncherResult<Option<TreeProblem>> {
    let actual = build(root)?;
    let mut have = actual.entries.iter().peekable();
    let mut want = expected.entries.iter().peekable();
    loop {
        match (want.peek(), have.peek()) {
            (None, None) => return Ok(None),
            (Some(w), None) => return Ok(Some(TreeProblem::Missing(w.path.clone()))),
            (None, Some(h)) => return Ok(Some(TreeProblem::Unexpected(h.path.clone()))),
            (Some(w), Some(h)) => match w.path.cmp(&h.path) {
                std::cmp::Ordering::Less => return Ok(Some(TreeProblem::Missing(w.path.clone()))),
                std::cmp::Ordering::Greater => {
                    return Ok(Some(TreeProblem::Unexpected(h.path.clone())));
                }
                std::cmp::Ordering::Equal => {
                    if w != h {
                        return Ok(Some(TreeProblem::Changed(w.path.clone())));
                    }
                    want.next();
                    have.next();
                }
            },
        }
    }
}

/// Canonical bytes of a receipt; their SHA-256 identifies it in locks.
pub fn to_bytes(receipt: &TreeReceipt) -> Vec<u8> {
    let mut bytes = serde_json::to_vec_pretty(receipt).unwrap_or_default();
    bytes.push(b'\n');
    bytes
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(sha2::Sha256::digest(bytes))
}
