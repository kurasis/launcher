//! Extracted-tree receipts: every directory, regular file (size, SHA-256,
//! executable bit) and link target of an installed runtime. An archive checksum
//! says nothing about files after extraction; this receipt is what Repair and
//! reuse check. Launcher receipt files at the top level are excluded.

use std::fs;
use std::io::Read;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sha2::Digest as _;

use super::extract::is_executable;
use crate::error::{LauncherError, LauncherResult};
use crate::storage::document::SCHEMA_VERSION;

/// Top-level names starting with this prefix belong to the launcher.
pub const RECEIPT_PREFIX: &str = ".launcher-";

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

/// An entry as the file system sees it: a file's hash only when it was read,
/// and when the file last changed (seconds and nanoseconds since 1970).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Seen {
    #[serde(flatten)]
    pub entry: TreeEntry,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modified: Option<(u64, u32)>,
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

fn modified(metadata: &fs::Metadata) -> Option<(u64, u32)> {
    let since = metadata.modified().ok()?.duration_since(UNIX_EPOCH).ok()?;
    Some((since.as_secs(), since.subsec_nanos()))
}

fn walk(dir: &Path, prefix: &str, hash: bool, out: &mut Vec<Seen>) -> LauncherResult<()> {
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
            out.push(Seen {
                entry: TreeEntry {
                    path,
                    kind: EntryKind::Symlink,
                    size: None,
                    sha256: None,
                    executable: false,
                    target: Some(target.to_string_lossy().replace('\\', "/")),
                },
                modified: None,
            });
        } else if file_type.is_dir() {
            out.push(Seen {
                entry: TreeEntry {
                    path: path.clone(),
                    kind: EntryKind::Dir,
                    size: None,
                    sha256: None,
                    executable: false,
                    target: None,
                },
                modified: None,
            });
            walk(&full, &path, hash, out)?;
        } else if file_type.is_file() {
            out.push(Seen {
                entry: TreeEntry {
                    path,
                    kind: EntryKind::File,
                    size: Some(metadata.len()),
                    sha256: if hash {
                        Some(sha256_file(&full)?)
                    } else {
                        None
                    },
                    executable: is_executable(&full),
                    target: None,
                },
                modified: modified(&metadata),
            });
        } else {
            out.push(Seen {
                entry: TreeEntry {
                    path,
                    kind: EntryKind::File,
                    size: None,
                    sha256: None,
                    executable: false,
                    target: Some("special file".into()),
                },
                modified: None,
            });
        }
    }
    Ok(())
}

/// Every entry under `root` in plain path order, the order `verify` merges in.
/// Files are read and hashed only when `hash` is set.
pub fn scan(root: &Path, hash: bool) -> LauncherResult<Vec<Seen>> {
    let mut seen = Vec::new();
    walk(root, "", hash, &mut seen)?;
    seen.sort_by(|a, b| a.entry.path.cmp(&b.entry.path));
    Ok(seen)
}

/// Describes the tree under `root` as it is now.
pub fn build(root: &Path) -> LauncherResult<TreeReceipt> {
    Ok(TreeReceipt {
        schema_version: SCHEMA_VERSION,
        entries: scan(root, true)?.into_iter().map(|s| s.entry).collect(),
    })
}

/// Compares the tree under `root` with `expected`.
pub fn verify(root: &Path, expected: &TreeReceipt) -> LauncherResult<Option<TreeProblem>> {
    Ok(difference(expected, &scan(root, true)?))
}

/// The first difference between `expected` and a hashed [`scan`].
pub fn difference(expected: &TreeReceipt, actual: &[Seen]) -> Option<TreeProblem> {
    let mut have = actual.iter().map(|s| &s.entry).peekable();
    let mut want = expected.entries.iter().peekable();
    loop {
        match (want.peek(), have.peek()) {
            (None, None) => return None,
            (Some(w), None) => return Some(TreeProblem::Missing(w.path.clone())),
            (None, Some(h)) => return Some(TreeProblem::Unexpected(h.path.clone())),
            (Some(w), Some(h)) => match w.path.cmp(&h.path) {
                std::cmp::Ordering::Less => return Some(TreeProblem::Missing(w.path.clone())),
                std::cmp::Ordering::Greater => {
                    return Some(TreeProblem::Unexpected(h.path.clone()));
                }
                std::cmp::Ordering::Equal => {
                    if w != h {
                        return Some(TreeProblem::Changed(w.path.clone()));
                    }
                    want.next();
                    have.next();
                }
            },
        }
    }
}

/// The scan without hashes, as a later unhashed [`scan`] of the same files sees it.
pub fn without_hashes(seen: &[Seen]) -> Vec<Seen> {
    seen.iter()
        .map(|s| Seen {
            entry: TreeEntry {
                sha256: None,
                ..s.entry.clone()
            },
            modified: s.modified,
        })
        .collect()
}

/// True when every file of the scan last changed at a known time before
/// `since`. A file changed later could change again within the same clock tick
/// and keep its time.
pub fn settled(seen: &[Seen], since: SystemTime) -> bool {
    let Ok(since) = since.duration_since(UNIX_EPOCH) else {
        return false;
    };
    seen.iter()
        .filter(|s| s.entry.kind == EntryKind::File)
        .all(|s| {
            s.modified
                .is_some_and(|(secs, nanos)| std::time::Duration::new(secs, nanos) < since)
        })
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
