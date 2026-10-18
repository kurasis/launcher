//! Data-root paths, atomic JSON documents, schema handling and locks
//! (docs/spec/02-ARCHITECTURE-AND-STORAGE.md).

pub mod atomic;
pub mod document;
pub mod lock;
pub mod models;
pub mod paths;
pub mod reconcile;

use std::fs;
use std::path::Path;

use serde::Serialize;

use crate::error::{ErrorCode, LauncherError, LauncherResult};
use document::Loaded;
use lock::{DataRootLock, LockError};
use models::{AccountsFile, InstanceFile, Settings};
use paths::{DataRoot, InstanceId};

/// A non-fatal storage finding shown to the user, such as a recovered or
/// preserved corrupt document. Paths are relative to DATA_ROOT.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageIssue {
    pub code: ErrorCode,
    pub message: String,
    pub document: String,
    /// English template of `message` when it carries values (D-038).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub template: Option<String>,
    #[serde(skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub params: std::collections::BTreeMap<String, String>,
}

impl StorageIssue {
    /// A notice whose text carries values named `{name}` in `template`.
    pub fn templated(
        code: ErrorCode,
        template: &str,
        params: &[(&str, String)],
        document: String,
    ) -> Self {
        let params: std::collections::BTreeMap<String, String> = params
            .iter()
            .map(|(name, value)| ((*name).to_owned(), value.clone()))
            .collect();
        let mut message = template.to_owned();
        for (name, value) in &params {
            message = message.replace(&format!("{{{name}}}"), value);
        }
        Self {
            code,
            message,
            document,
            template: Some(template.to_owned()),
            params,
        }
    }
}

/// A loaded document plus whether this build may rewrite it.
#[derive(Debug)]
pub struct LoadedDocument<T> {
    pub value: T,
    pub writable: bool,
    pub issue: Option<StorageIssue>,
}

/// Exclusive owner of DATA_ROOT for the lifetime of the supervisor.
#[derive(Debug)]
pub struct Store {
    root: DataRoot,
    _lock: DataRootLock,
}

impl Store {
    /// Creates the base directories, takes the lifetime lock and removes stale
    /// temporary files. Fails closed when another process holds the lock.
    pub fn open(root: DataRoot) -> LauncherResult<Self> {
        for dir in [
            root.path().to_path_buf(),
            root.locks_dir(),
            root.instances_dir(),
        ] {
            fs::create_dir_all(&dir)
                .map_err(|e| LauncherError::from_io(&e, "Creating the launcher data directory"))?;
        }
        let lock = DataRootLock::acquire(&root.supervisor_lock()).map_err(|error| match error {
            LockError::Held => LauncherError::new(
                ErrorCode::DataRootLocked,
                "Another launcher process is already using the launcher data directory. \
                 Close it, or switch to its window.",
            ),
            LockError::Io(io) => LauncherError::from_io(&io, "Locking the launcher data directory"),
        })?;
        let store = Self { root, _lock: lock };
        store.remove_stale_temps();
        Ok(store)
    }

    pub fn root(&self) -> &DataRoot {
        &self.root
    }

    fn remove_stale_temps(&self) {
        let _ = atomic::remove_stale_temps(self.root.path());
        if let Ok(entries) = fs::read_dir(self.root.instances_dir()) {
            for entry in entries.flatten() {
                let _ = atomic::remove_stale_temps(&entry.path());
            }
        }
    }

    fn relative(&self, path: &Path) -> String {
        path.strip_prefix(self.root.path())
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/")
    }

    /// Loads settings with backup recovery. Never fails: unusable settings fall
    /// back to defaults with an issue, and a newer schema becomes read-only.
    pub fn load_settings(&self) -> LoadedDocument<Settings> {
        let path = self.root.settings();
        let document = self.relative(&path);
        match document::load::<Settings>(&path, Some(&self.root.settings_backup())) {
            Ok(Loaded::Valid(value)) => LoadedDocument {
                value,
                writable: true,
                issue: None,
            },
            Ok(Loaded::Missing) => LoadedDocument {
                value: Settings::default(),
                writable: true,
                issue: None,
            },
            Ok(Loaded::RecoveredFromBackup {
                value,
                preserved_as,
            }) => LoadedDocument {
                value,
                writable: true,
                issue: Some(StorageIssue::templated(
                    ErrorCode::StorageError,
                    "Settings were damaged and restored from the last good copy. The damaged \
                     file was kept as {file}.",
                    &[("file", self.relative(&preserved_as))],
                    document,
                )),
            },
            Ok(Loaded::Corrupt { preserved_as }) => LoadedDocument {
                value: Settings::default(),
                writable: true,
                issue: Some(StorageIssue::templated(
                    ErrorCode::StorageError,
                    "Settings were damaged and have been reset to defaults. The damaged file \
                     was kept as {file}.",
                    &[(
                        "file",
                        preserved_as
                            .as_deref()
                            .map(|p| self.relative(p))
                            .unwrap_or_else(|| "-".into()),
                    )],
                    document,
                )),
            },
            Ok(Loaded::UnsupportedSchema { found }) => LoadedDocument {
                value: Settings::default(),
                writable: false,
                issue: Some(unsupported_issue(document, found)),
            },
            Err(error) => LoadedDocument {
                value: Settings::default(),
                writable: false,
                issue: Some(StorageIssue {
                    template: None,
                    params: std::collections::BTreeMap::new(),
                    code: LauncherError::from_io(&error, "Reading settings").code,
                    message: "Settings could not be read; defaults are used and changes will not \
                              be saved until the launcher is restarted."
                        .into(),
                    document,
                }),
            },
        }
    }

    pub fn save_settings(&self, settings: &Settings) -> LauncherResult<()> {
        document::save(
            &self.root.settings(),
            settings,
            Some(&self.root.settings_backup()),
        )
        .map_err(|e| LauncherError::from_io(&e, "Saving settings"))
    }

    /// Loads account display metadata. Unknown or damaged files are never
    /// rewritten here; account changes arrive in P04.
    pub fn load_accounts(&self) -> LoadedDocument<AccountsFile> {
        let path = self.root.accounts();
        let document = self.relative(&path);
        match document::load::<AccountsFile>(&path, None) {
            Ok(Loaded::Valid(value)) => LoadedDocument {
                value,
                writable: true,
                issue: None,
            },
            Ok(Loaded::Missing) => LoadedDocument {
                value: AccountsFile::default(),
                writable: true,
                issue: None,
            },
            Ok(Loaded::RecoveredFromBackup { value, .. }) => LoadedDocument {
                value,
                writable: true,
                issue: None,
            },
            Ok(Loaded::Corrupt { .. }) => LoadedDocument {
                value: AccountsFile::default(),
                writable: true,
                issue: Some(StorageIssue {
                    template: None,
                    params: std::collections::BTreeMap::new(),
                    code: ErrorCode::StorageError,
                    message: "The saved account record was damaged and was set aside. Sign in \
                              again."
                        .into(),
                    document,
                }),
            },
            Ok(Loaded::UnsupportedSchema { found }) => LoadedDocument {
                value: AccountsFile::default(),
                writable: false,
                issue: Some(unsupported_issue(document, found)),
            },
            Err(error) => LoadedDocument {
                value: AccountsFile::default(),
                writable: false,
                issue: Some(StorageIssue {
                    template: None,
                    params: std::collections::BTreeMap::new(),
                    code: LauncherError::from_io(&error, "Reading accounts").code,
                    message: "The saved account record could not be read.".into(),
                    document,
                }),
            },
        }
    }

    pub fn save_accounts(&self, accounts: &AccountsFile) -> LauncherResult<()> {
        document::save(&self.root.accounts(), accounts, None)
            .map_err(|e| LauncherError::from_io(&e, "Saving account metadata"))
    }

    /// Loads every instance directory whose name is a canonical UUID, reconciling
    /// each summary with its install lock. Corrections are written back.
    pub fn load_instances(&self) -> (Vec<InstanceFile>, Vec<StorageIssue>) {
        let mut instances = Vec::new();
        let mut issues = Vec::new();
        let Ok(entries) = fs::read_dir(self.root.instances_dir()) else {
            return (instances, issues);
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            if name.to_str().is_some_and(|n| n.starts_with(DELETED_PREFIX)) {
                let _ = fs::remove_dir_all(entry.path());
                continue;
            }
            let Some(id) = name.to_str().and_then(|n| n.parse::<InstanceId>().ok()) else {
                continue;
            };
            if !entry.file_type().is_ok_and(|t| t.is_dir()) {
                continue;
            }
            let path = self.root.instance_file(id);
            let document = self.relative(&path);
            let summary = match document::load::<InstanceFile>(&path, None) {
                Ok(Loaded::Valid(instance)) if instance.id == id => instance,
                Ok(Loaded::Valid(_)) => {
                    issues.push(StorageIssue {
                        template: None,
                        params: std::collections::BTreeMap::new(),
                        code: ErrorCode::StorageError,
                        message: "An installation record names a different identifier than its \
                                  folder and was ignored."
                            .into(),
                        document,
                    });
                    continue;
                }
                Ok(Loaded::Missing) => continue,
                Ok(Loaded::UnsupportedSchema { found }) => {
                    issues.push(unsupported_issue(document, found));
                    continue;
                }
                Ok(Loaded::Corrupt { .. } | Loaded::RecoveredFromBackup { .. }) => {
                    issues.push(StorageIssue {
                        template: None,
                        params: std::collections::BTreeMap::new(),
                        code: ErrorCode::StorageError,
                        message: "An installation record was damaged and was set aside. Its \
                                  game folder and worlds were not touched."
                            .into(),
                        document,
                    });
                    continue;
                }
                Err(error) => {
                    issues.push(StorageIssue {
                        template: None,
                        params: std::collections::BTreeMap::new(),
                        code: LauncherError::from_io(&error, "Reading an installation").code,
                        message: "An installation record could not be read.".into(),
                        document,
                    });
                    continue;
                }
            };
            let lock = match reconcile::read_lock(&self.root.install_lock(id)) {
                Ok(lock) => lock,
                Err(_) => reconcile::LockState::Invalid,
            };
            let reconciled = reconcile::reconcile(&summary, &lock);
            if reconciled.changed
                && let Err(error) = self.save_instance(&reconciled.instance)
            {
                issues.push(StorageIssue {
                    template: None,
                    params: std::collections::BTreeMap::new(),
                    code: error.code,
                    message: "An installation summary was corrected in memory but could not \
                              be saved."
                        .into(),
                    document,
                });
            }
            instances.push(reconciled.instance);
        }
        instances.sort_by(|a, b| a.created_at.cmp(&b.created_at).then(a.id.cmp(&b.id)));
        (instances, issues)
    }

    /// Creates the instance directories and writes its definition.
    pub fn create_instance(&self, instance: &InstanceFile) -> LauncherResult<()> {
        fs::create_dir_all(self.root.game_dir(instance.id))
            .map_err(|e| LauncherError::from_io(&e, "Creating the installation folder"))?;
        self.save_instance(instance)
    }

    /// Deletes an installation's folder: first moved aside in one rename, so
    /// an interrupted delete never leaves half an installation behind, then
    /// removed. What a failed removal leaves is cleared at the next start.
    pub fn remove_instance(&self, id: InstanceId) -> LauncherResult<()> {
        let folder = self.root.instance_dir(id);
        let trash = self
            .root
            .instances_dir()
            .join(format!("{DELETED_PREFIX}{id}-{}", uuid::Uuid::new_v4()));
        match fs::rename(&folder, &trash) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(LauncherError::from_io(&e, "Deleting the installation")),
        }
        let _ = fs::remove_dir_all(&trash);
        Ok(())
    }

    pub fn save_instance(&self, instance: &InstanceFile) -> LauncherResult<()> {
        document::save(&self.root.instance_file(instance.id), instance, None)
            .map_err(|e| LauncherError::from_io(&e, "Saving the installation"))
    }

    /// Writes an app-owned JSON cache document such as `compatibility.json`.
    pub fn write_cache<T: Serialize>(&self, path: &Path, value: &T) -> LauncherResult<()> {
        let mut bytes = serde_json::to_vec_pretty(value)
            .map_err(|_| LauncherError::internal("Serializing a cache document failed."))?;
        bytes.push(b'\n');
        atomic::write(path, &bytes).map_err(|e| LauncherError::from_io(&e, "Writing a cache file"))
    }
}

/// Folders of deleted installations waiting to be removed.
const DELETED_PREFIX: &str = ".deleted-";

fn unsupported_issue(document: String, found: u64) -> StorageIssue {
    StorageIssue::templated(
        ErrorCode::UnsupportedSchema,
        "This file was written by a newer launcher version (schema {schema}). It is kept \
         unchanged and is read-only here; update the launcher to use it.",
        &[("schema", found.to_string())],
        document,
    )
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::*;
    use crate::storage::document::SCHEMA_VERSION;
    use crate::storage::models::{InstallationHealth, Resolution};

    fn open_temp() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let root = DataRoot::new(dir.path().join("data root ü")).unwrap();
        let store = Store::open(root).unwrap();
        (dir, store)
    }

    fn instance(name: &str) -> InstanceFile {
        InstanceFile {
            schema_version: SCHEMA_VERSION,
            id: InstanceId::generate(),
            display_name: name.into(),
            version_id: "1.21.1".into(),
            manifest_sha1: None,
            created_at: Utc::now(),
            last_played_at: None,
            max_heap_mib: 2048,
            resolution: Resolution::default(),
            health: InstallationHealth::NotInstalled,
            last_result: None,
            java: None,
            loader: None,
            notes: String::new(),
            java_provider: None,
            custom_java: None,
            jvm_arguments: String::new(),
            profile: None,
        }
    }

    #[test]
    fn second_store_on_same_root_fails_closed() {
        let (_dir, store) = open_temp();
        let error = Store::open(store.root().clone()).unwrap_err();
        assert_eq!(error.code, ErrorCode::DataRootLocked);
    }

    #[test]
    fn instances_survive_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let root = DataRoot::new(dir.path().to_path_buf()).unwrap();
        let created = instance("Survival");
        {
            let store = Store::open(root.clone()).unwrap();
            store.create_instance(&created).unwrap();
            let settings = Settings {
                selected_instance_id: Some(created.id),
                ..Settings::default()
            };
            store.save_settings(&settings).unwrap();
        }
        let store = Store::open(root).unwrap();
        let (instances, issues) = store.load_instances();
        assert!(issues.is_empty());
        assert_eq!(instances, vec![created.clone()]);
        assert_eq!(
            store.load_settings().value.selected_instance_id,
            Some(created.id)
        );
        assert!(store.root().game_dir(created.id).is_dir());
    }

    #[test]
    fn foreign_directories_and_damaged_records_are_not_deleted() {
        let (_dir, store) = open_temp();
        let instances = store.root().instances_dir();
        fs::create_dir_all(instances.join("not-a-uuid")).unwrap();
        let damaged = instance("Damaged");
        store.create_instance(&damaged).unwrap();
        fs::write(store.root().instance_file(damaged.id), b"{oops").unwrap();
        fs::write(
            store.root().game_dir(damaged.id).join("options.txt"),
            b"keep",
        )
        .unwrap();

        let (loaded, issues) = store.load_instances();
        assert!(loaded.is_empty());
        assert_eq!(issues.len(), 1);
        assert!(instances.join("not-a-uuid").is_dir());
        assert_eq!(
            fs::read(store.root().game_dir(damaged.id).join("options.txt")).unwrap(),
            b"keep"
        );
    }

    #[test]
    fn newer_settings_schema_is_read_only() {
        let (_dir, store) = open_temp();
        fs::write(store.root().settings(), br#"{"schemaVersion":3}"#).unwrap();
        let loaded = store.load_settings();
        assert!(!loaded.writable);
        assert_eq!(loaded.issue.unwrap().code, ErrorCode::UnsupportedSchema);
        assert_eq!(
            fs::read(store.root().settings()).unwrap(),
            br#"{"schemaVersion":3}"#
        );
    }

    #[test]
    fn corrupt_settings_recover_from_backup_with_issue() {
        let (_dir, store) = open_temp();
        let mut settings = Settings {
            default_max_heap_mib: Some(3000),
            ..Settings::default()
        };
        store.save_settings(&settings).unwrap();
        settings.default_max_heap_mib = Some(5000);
        store.save_settings(&settings).unwrap();
        fs::write(store.root().settings(), b"\0\0\0").unwrap();

        let loaded = store.load_settings();
        assert_eq!(loaded.value.default_max_heap_mib, Some(3000));
        let issue = loaded.issue.unwrap();
        assert_eq!(issue.document, "settings.json");
        assert!(issue.message.contains("settings.json.corrupt-"));
    }

    #[test]
    fn stale_summary_is_reconciled_and_rewritten() {
        let (_dir, store) = open_temp();
        let mut stale = instance("Stale");
        stale.health = InstallationHealth::Ready;
        store.create_instance(&stale).unwrap();
        let (loaded, _) = store.load_instances();
        assert_eq!(loaded[0].health, InstallationHealth::NotInstalled);
        let on_disk: InstanceFile =
            serde_json::from_slice(&fs::read(store.root().instance_file(stale.id)).unwrap())
                .unwrap();
        assert_eq!(on_disk.health, InstallationHealth::NotInstalled);
    }

    #[test]
    fn interrupted_instance_write_leaves_readable_previous_version() {
        let (_dir, store) = open_temp();
        let mut record = instance("Before");
        store.create_instance(&record).unwrap();
        record.display_name = "After".into();
        let bytes = serde_json::to_vec(&record).unwrap();
        let prepared = atomic::prepare(&store.root().instance_file(record.id), &bytes).unwrap();
        std::mem::forget(prepared); // crash before commit

        let (loaded, issues) = store.load_instances();
        assert!(issues.is_empty());
        assert_eq!(loaded[0].display_name, "Before");
    }
}
