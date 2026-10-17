//! Loading and saving versioned app-owned JSON documents
//! (docs/spec/02 "Persistent models").
//!
//! Every app-owned document has a top-level `schemaVersion`. A newer, unknown
//! version is never rewritten. A corrupt file is preserved under a new name for
//! diagnosis before any fallback is used, so nothing is silently erased.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use chrono::Utc;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use super::atomic;

/// The schema version written by this build for every app-owned document.
pub const SCHEMA_VERSION: u64 = 1;

/// Upper bound for app-owned documents. They are small; a larger file is treated
/// as corrupt instead of being loaded into memory.
pub const MAX_DOCUMENT_BYTES: u64 = 1024 * 1024;

/// Upper bound for install locks and job receipts, which list every artifact
/// (about 1.6 MB for a current release with ~4,000 asset objects).
pub const MAX_LARGE_DOCUMENT_BYTES: u64 = 64 * 1024 * 1024;

/// Result of loading one document.
#[derive(Debug)]
pub enum Loaded<T> {
    /// The file does not exist.
    Missing,
    /// The primary file was valid.
    Valid(T),
    /// The primary file was corrupt and preserved; the backup was used.
    RecoveredFromBackup { value: T, preserved_as: PathBuf },
    /// The file (and backup, if any) was corrupt. The primary was preserved.
    Corrupt { preserved_as: Option<PathBuf> },
    /// The file was written by a newer launcher. It was left untouched.
    UnsupportedSchema { found: u64 },
}

/// Why a single file could not be used.
#[derive(Debug)]
enum Problem {
    Missing,
    Io(io::Error),
    Corrupt,
    Unsupported(u64),
}

fn parse<T: DeserializeOwned>(path: &Path) -> Result<T, Problem> {
    parse_limited(path, MAX_DOCUMENT_BYTES)
}

fn parse_limited<T: DeserializeOwned>(path: &Path, max_bytes: u64) -> Result<T, Problem> {
    let metadata = match fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Err(Problem::Missing),
        Err(error) => return Err(Problem::Io(error)),
    };
    if !metadata.is_file() || metadata.len() > max_bytes {
        return Err(Problem::Corrupt);
    }
    let bytes = fs::read(path).map_err(Problem::Io)?;
    let value: Value = serde_json::from_slice(&bytes).map_err(|_| Problem::Corrupt)?;
    match value.get("schemaVersion").and_then(Value::as_u64) {
        Some(SCHEMA_VERSION) => serde_json::from_value(value).map_err(|_| Problem::Corrupt),
        Some(found) if found > SCHEMA_VERSION => Err(Problem::Unsupported(found)),
        _ => Err(Problem::Corrupt),
    }
}

/// Renames a corrupt file to `<name>.corrupt-<timestamp>` next to it.
pub fn preserve_corrupt(path: &Path) -> io::Result<PathBuf> {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let stamp = Utc::now().format("%Y%m%dT%H%M%S%.3fZ");
    let target = path.with_file_name(format!("{name}.corrupt-{stamp}"));
    fs::rename(path, &target)?;
    Ok(target)
}

/// Loads `path`, falling back to `backup` when the primary is corrupt.
///
/// I/O errors other than "not found" are returned as errors because they do not
/// prove the file is bad (for example a temporary permission problem).
pub fn load<T: DeserializeOwned>(path: &Path, backup: Option<&Path>) -> io::Result<Loaded<T>> {
    load_limited(path, backup, MAX_DOCUMENT_BYTES)
}

/// [`load`] with a different size limit, for documents that list artifacts.
pub fn load_limited<T: DeserializeOwned>(
    path: &Path,
    backup: Option<&Path>,
    max_bytes: u64,
) -> io::Result<Loaded<T>> {
    let parse = |p: &Path| parse_limited::<T>(p, max_bytes);
    match parse(path) {
        Ok(value) => Ok(Loaded::Valid(value)),
        Err(Problem::Missing) => match backup.map(parse) {
            // A missing primary with a valid backup means the primary was lost
            // after a crash between backup and replacement; use the backup.
            Some(Ok(value)) => Ok(Loaded::Valid(value)),
            _ => Ok(Loaded::Missing),
        },
        Err(Problem::Io(error)) => Err(error),
        Err(Problem::Unsupported(found)) => Ok(Loaded::UnsupportedSchema { found }),
        Err(Problem::Corrupt) => {
            let preserved_as = preserve_corrupt(path)?;
            match backup.map(parse) {
                Some(Ok(value)) => Ok(Loaded::RecoveredFromBackup {
                    value,
                    preserved_as,
                }),
                Some(Err(Problem::Io(error))) => Err(error),
                _ => Ok(Loaded::Corrupt {
                    preserved_as: Some(preserved_as),
                }),
            }
        }
    }
}

/// Serializes and atomically writes a document. When `backup` is given and the
/// current primary file is a valid document, it is first copied to the backup so
/// one last-good version always exists.
pub fn save<T: Serialize + DeserializeOwned>(
    path: &Path,
    value: &T,
    backup: Option<&Path>,
) -> io::Result<()> {
    let mut bytes = serde_json::to_vec_pretty(value).map_err(io::Error::other)?;
    bytes.push(b'\n');
    if let Some(backup) = backup
        && parse::<T>(path).is_ok()
    {
        let current = fs::read(path)?;
        atomic::write(backup, &current)?;
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    atomic::write(path, &bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    #[serde(rename_all = "camelCase")]
    struct Doc {
        schema_version: u64,
        name: String,
    }

    fn doc(name: &str) -> Doc {
        Doc {
            schema_version: SCHEMA_VERSION,
            name: name.into(),
        }
    }

    #[test]
    fn missing_file_is_reported() {
        let dir = tempfile::tempdir().unwrap();
        let loaded = load::<Doc>(&dir.path().join("a.json"), None).unwrap();
        assert!(matches!(loaded, Loaded::Missing));
    }

    #[test]
    fn round_trip_and_backup() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let backup = dir.path().join("settings.json.bak");
        save(&path, &doc("one"), Some(&backup)).unwrap();
        assert!(
            !backup.exists(),
            "no backup before a previous valid version exists"
        );
        save(&path, &doc("two"), Some(&backup)).unwrap();
        assert!(matches!(load::<Doc>(&path, None).unwrap(), Loaded::Valid(d) if d.name == "two"));
        assert!(matches!(load::<Doc>(&backup, None).unwrap(), Loaded::Valid(d) if d.name == "one"));
    }

    #[test]
    fn corrupt_primary_is_preserved_and_backup_used() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let backup = dir.path().join("settings.json.bak");
        save(&path, &doc("one"), Some(&backup)).unwrap();
        save(&path, &doc("two"), Some(&backup)).unwrap();
        fs::write(&path, b"{ not json").unwrap();

        match load::<Doc>(&path, Some(&backup)).unwrap() {
            Loaded::RecoveredFromBackup {
                value,
                preserved_as,
            } => {
                assert_eq!(value.name, "one");
                assert_eq!(fs::read(&preserved_as).unwrap(), b"{ not json");
                assert!(
                    preserved_as
                        .file_name()
                        .unwrap()
                        .to_string_lossy()
                        .starts_with("settings.json.corrupt-")
                );
            }
            other => panic!("unexpected {other:?}"),
        }
        assert!(!path.exists());
    }

    #[test]
    fn corrupt_without_backup_is_preserved() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("instance.json");
        for content in [
            &b"[]"[..],
            b"{\"schemaVersion\":\"1\"}",
            b"{\"schemaVersion\":1}",
            b"{\"schemaVersion\":0,\"name\":\"x\"}",
        ] {
            fs::write(&path, content).unwrap();
            match load::<Doc>(&path, None).unwrap() {
                Loaded::Corrupt {
                    preserved_as: Some(p),
                } => assert_eq!(fs::read(p).unwrap(), content),
                other => panic!("unexpected {other:?}"),
            }
        }
    }

    #[test]
    fn newer_schema_is_left_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let content = br#"{"schemaVersion":2,"name":"future","extra":true}"#;
        fs::write(&path, content).unwrap();
        assert!(matches!(
            load::<Doc>(&path, None).unwrap(),
            Loaded::UnsupportedSchema { found: 2 }
        ));
        assert_eq!(fs::read(&path).unwrap(), content);
    }

    #[test]
    fn oversized_document_is_corrupt() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("big.json");
        fs::write(&path, vec![b' '; (MAX_DOCUMENT_BYTES + 1) as usize]).unwrap();
        assert!(matches!(
            load::<Doc>(&path, None).unwrap(),
            Loaded::Corrupt { .. }
        ));
    }

    #[test]
    fn missing_primary_uses_valid_backup() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let backup = dir.path().join("settings.json.bak");
        save(&backup, &doc("backup"), None).unwrap();
        assert!(
            matches!(load::<Doc>(&path, Some(&backup)).unwrap(), Loaded::Valid(d) if d.name == "backup")
        );
    }
}
