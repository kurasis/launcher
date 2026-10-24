//! The backup store (D-047): content-addressed objects plus one manifest per
//! snapshot, under `DATA_ROOT/backups`.
//!
//! - `objects/<first two hex digits>/<sha256>` holds each distinct file once, so a
//!   snapshot costs only the files that changed since any earlier one.
//! - `snapshots/<id>.json` lists every file of a snapshot with its size,
//!   modification time and SHA-256.
//! - A file whose size and modification time match the previous snapshot of the
//!   same scope is not read again (incremental), as long as its object exists.
//! - Restores write into a fresh folder, verify every file against its SHA-256,
//!   and only then swap it in.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::error::{ErrorCode, LauncherError, LauncherResult};
use crate::storage::document::SCHEMA_VERSION;
use crate::storage::paths::InstanceId;

/// What a snapshot holds.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Scope {
    /// One world: `saves/<name>` of the installation's game folder.
    World { name: String },
    /// The whole game folder: worlds, mods, settings, resource packs.
    Installation,
}

/// Why a snapshot was made.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reason {
    Manual,
    /// Made automatically after a game session; pruned to the kept count.
    AfterPlay,
    /// The state a restore replaced.
    BeforeRestore,
    /// The installation just before it was deleted (D-055).
    BeforeDelete,
    /// The installation just before a modpack update changed it (D-066).
    BeforeUpdate,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileEntry {
    /// Relative to the scope's folder, `/`-separated.
    pub path: String,
    pub size: u64,
    pub sha256: String,
    pub modified_ms: i64,
}

/// Everything about a snapshot except its file list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotInfo {
    pub id: String,
    pub instance_id: InstanceId,
    pub instance_name: String,
    pub version_label: String,
    /// The Minecraft version and loader, so a whole-installation backup can
    /// bring back a deleted installation (D-055). Missing in older backups.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loader: Option<crate::loader::LoaderChoice>,
    pub scope: Scope,
    pub reason: Reason,
    pub created_at: DateTime<Utc>,
    pub file_count: u64,
    /// Size of all files in the snapshot.
    pub total_bytes: u64,
    /// Bytes this snapshot added to the store.
    pub new_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub schema_version: u64,
    #[serde(flatten)]
    pub info: SnapshotInfo,
    pub files: Vec<FileEntry>,
}

/// Who and what a new snapshot belongs to.
pub struct Origin {
    pub instance_id: InstanceId,
    pub instance_name: String,
    pub version_label: String,
    pub version_id: String,
    pub loader: Option<crate::loader::LoaderChoice>,
    pub scope: Scope,
    pub reason: Reason,
}

fn io(e: &std::io::Error, what: &'static str) -> LauncherError {
    LauncherError::from_io(e, what)
}

/// A world folder name: one path component, nothing that could leave `saves`.
pub fn valid_world_name(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && !name.contains(['/', '\\', ':', '\0'])
        && name.trim() == name
}

/// A manifest path: relative, `/`-separated, without `.`/`..` or drive letters.
fn safe_relative(path: &str) -> Option<PathBuf> {
    let mut out = PathBuf::new();
    for part in path.split('/') {
        if part.is_empty() || part == "." || part == ".." || part.contains(['\\', ':', '\0']) {
            return None;
        }
        out.push(part);
    }
    (!out.as_os_str().is_empty()).then_some(out)
}

/// A snapshot manifest; large worlds list many files.
fn read_manifest(path: &Path) -> Option<Snapshot> {
    use crate::storage::document::{self, Loaded};
    match document::load_limited::<Snapshot>(path, None, document::MAX_LARGE_DOCUMENT_BYTES) {
        // A hash also names the object's file: anything but 64 hex digits
        // (a damaged or edited manifest) would point elsewhere.
        Ok(Loaded::Valid(s) | Loaded::RecoveredFromBackup { value: s, .. })
            if s.files.iter().all(|f| is_sha256(&f.sha256)) =>
        {
            Some(s)
        }
        _ => None,
    }
}

fn is_sha256(text: &str) -> bool {
    text.len() == 64 && text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

fn modified_ms(meta: &fs::Metadata) -> i64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

/// Regular files under `dir`, sorted, with paths relative to it. Symbolic links
/// are skipped: the launcher never creates them and they may point anywhere.
/// A file found by [`walk`]: relative path, full path, metadata.
pub(crate) type Found = (String, PathBuf, fs::Metadata);

pub(crate) fn walk(dir: &Path) -> LauncherResult<(Vec<Found>, u64)> {
    let mut files = Vec::new();
    let mut skipped = 0;
    let mut stack = vec![(dir.to_path_buf(), String::new())];
    while let Some((folder, prefix)) = stack.pop() {
        let entries = fs::read_dir(&folder).map_err(|e| io(&e, "Reading the folder to back up"))?;
        for entry in entries {
            let entry = entry.map_err(|e| io(&e, "Reading the folder to back up"))?;
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                skipped += 1;
                continue;
            };
            let relative = if prefix.is_empty() {
                name
            } else {
                format!("{prefix}/{name}")
            };
            let kind = entry
                .file_type()
                .map_err(|e| io(&e, "Reading the folder to back up"))?;
            if kind.is_symlink() {
                skipped += 1;
            } else if kind.is_dir() {
                stack.push((entry.path(), relative));
            } else if kind.is_file() {
                let meta = entry
                    .metadata()
                    .map_err(|e| io(&e, "Reading the folder to back up"))?;
                files.push((relative, entry.path(), meta));
            }
        }
    }
    files.sort_by(|a, b| a.0.cmp(&b.0));
    Ok((files, skipped))
}

pub struct Store {
    root: PathBuf,
}

impl Store {
    /// The store under `data_root/backups`.
    pub fn new(data_root: &Path) -> Self {
        Self {
            root: data_root.join("backups"),
        }
    }

    /// A store of its own in `root`, such as the hosted worlds' (W9).
    pub fn at(root: PathBuf) -> Self {
        Self { root }
    }

    fn objects(&self) -> PathBuf {
        self.root.join("objects")
    }

    fn snapshots(&self) -> PathBuf {
        self.root.join("snapshots")
    }

    fn object_path(&self, sha256: &str) -> PathBuf {
        self.objects().join(&sha256[..2]).join(sha256)
    }

    fn manifest_path(&self, id: &str) -> PathBuf {
        self.snapshots().join(format!("{id}.json"))
    }

    /// Copies `source` into the store while hashing it; returns the SHA-256 and
    /// whether the object was new.
    fn put(&self, source: &Path) -> LauncherResult<(String, bool)> {
        let staging = self.root.join("staging");
        fs::create_dir_all(&staging).map_err(|e| io(&e, "Preparing the backup folder"))?;
        let temp = staging.join(Uuid::new_v4().to_string());
        let result = (|| {
            let mut input =
                fs::File::open(source).map_err(|e| io(&e, "Reading a file to back up"))?;
            let mut output = fs::File::create(&temp).map_err(|e| io(&e, "Writing the backup"))?;
            let mut hasher = Sha256::new();
            let mut buffer = vec![0u8; 256 * 1024];
            loop {
                let n = input
                    .read(&mut buffer)
                    .map_err(|e| io(&e, "Reading a file to back up"))?;
                if n == 0 {
                    break;
                }
                hasher.update(&buffer[..n]);
                output
                    .write_all(&buffer[..n])
                    .map_err(|e| io(&e, "Writing the backup"))?;
            }
            output
                .sync_all()
                .map_err(|e| io(&e, "Writing the backup"))?;
            let sha256 = hex::encode(hasher.finalize());
            let target = self.object_path(&sha256);
            if target.is_file() {
                return Ok((sha256, false));
            }
            fs::create_dir_all(target.parent().unwrap_or(&self.root))
                .map_err(|e| io(&e, "Writing the backup"))?;
            crate::storage::atomic::rename(&temp, &target)
                .map_err(|e| io(&e, "Writing the backup"))?;
            Ok((sha256, true))
        })();
        let _ = fs::remove_file(&temp);
        result
    }

    /// Backs up `source` as a new snapshot. Files unchanged since `previous`
    /// (same size and modification time) are taken from it without reading.
    pub fn create(
        &self,
        source: &Path,
        origin: Origin,
        previous: Option<&Snapshot>,
    ) -> LauncherResult<(Snapshot, u64)> {
        if !source.is_dir() {
            return Err(LauncherError::new(
                ErrorCode::InvalidInput,
                "There is nothing to back up yet.",
            ));
        }
        let known: HashMap<&str, &FileEntry> = previous
            .map(|p| p.files.iter().map(|f| (f.path.as_str(), f)).collect())
            .unwrap_or_default();
        let (found, skipped) = walk(source)?;
        let mut files = Vec::with_capacity(found.len());
        let (mut total_bytes, mut new_bytes) = (0u64, 0u64);
        for (path, full, meta) in found {
            let (size, modified) = (meta.len(), modified_ms(&meta));
            let reused = known.get(path.as_str()).filter(|f| {
                f.size == size && f.modified_ms == modified && self.object_path(&f.sha256).is_file()
            });
            let sha256 = match reused {
                Some(entry) => entry.sha256.clone(),
                None => {
                    let (sha256, added) = self.put(&full)?;
                    if added {
                        new_bytes += size;
                    }
                    sha256
                }
            };
            total_bytes += size;
            files.push(FileEntry {
                path,
                size,
                sha256,
                modified_ms: modified,
            });
        }
        let snapshot = Snapshot {
            schema_version: SCHEMA_VERSION,
            info: SnapshotInfo {
                id: Uuid::new_v4().to_string(),
                instance_id: origin.instance_id,
                instance_name: origin.instance_name,
                version_label: origin.version_label,
                version_id: Some(origin.version_id),
                loader: origin.loader,
                scope: origin.scope,
                reason: origin.reason,
                created_at: Utc::now(),
                file_count: files.len() as u64,
                total_bytes,
                new_bytes,
            },
            files,
        };
        fs::create_dir_all(self.snapshots()).map_err(|e| io(&e, "Writing the backup"))?;
        crate::storage::document::save(&self.manifest_path(&snapshot.info.id), &snapshot, None)
            .map_err(|e| io(&e, "Writing the backup"))?;
        Ok((snapshot, skipped))
    }

    /// True when `source` has exactly the files, sizes and times of `previous`.
    pub fn unchanged(source: &Path, previous: &Snapshot) -> bool {
        let Ok((found, _)) = walk(source) else {
            return false;
        };
        found.len() == previous.files.len()
            && found
                .iter()
                .zip(&previous.files)
                .all(|((path, _, meta), entry)| {
                    *path == entry.path
                        && meta.len() == entry.size
                        && modified_ms(meta) == entry.modified_ms
                })
    }

    /// Every readable snapshot, newest first.
    pub fn list(&self) -> Vec<Snapshot> {
        let Ok(entries) = fs::read_dir(self.snapshots()) else {
            return Vec::new();
        };
        let mut snapshots: Vec<Snapshot> = entries
            .filter_map(Result::ok)
            .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
            .filter_map(|e| read_manifest(&e.path()))
            .collect();
        snapshots.sort_by_key(|s| std::cmp::Reverse(s.info.created_at));
        snapshots
    }

    pub fn load(&self, id: &str) -> LauncherResult<Snapshot> {
        // Only the form the store writes: no braces, `urn:` or other spellings in a path.
        if !Uuid::parse_str(id).is_ok_and(|uuid| uuid.hyphenated().to_string() == id) {
            return Err(not_found());
        }
        read_manifest(&self.manifest_path(id)).ok_or_else(not_found)
    }

    /// The newest snapshot of `scope` of an installation.
    pub fn latest(&self, instance: InstanceId, scope: &Scope) -> Option<Snapshot> {
        self.list()
            .into_iter()
            .find(|s| s.info.instance_id == instance && s.info.scope == *scope)
    }

    /// Writes `snapshot` into `target`, which must not exist yet; every file is
    /// checked against its SHA-256. A failure leaves nothing behind.
    pub fn restore_into(&self, snapshot: &Snapshot, target: &Path) -> LauncherResult<()> {
        if target.exists() {
            return Err(LauncherError::internal(
                "The restore target already exists.",
            ));
        }
        let result = (|| {
            fs::create_dir_all(target).map_err(|e| io(&e, "Restoring the backup"))?;
            for entry in &snapshot.files {
                let relative = safe_relative(&entry.path).ok_or_else(damaged)?;
                let destination = target.join(relative);
                if let Some(parent) = destination.parent() {
                    fs::create_dir_all(parent).map_err(|e| io(&e, "Restoring the backup"))?;
                }
                let mut input =
                    fs::File::open(self.object_path(&entry.sha256)).map_err(|_| damaged())?;
                let mut output =
                    fs::File::create(&destination).map_err(|e| io(&e, "Restoring the backup"))?;
                let mut hasher = Sha256::new();
                let mut buffer = vec![0u8; 256 * 1024];
                let mut size = 0u64;
                loop {
                    let n = input.read(&mut buffer).map_err(|_| damaged())?;
                    if n == 0 {
                        break;
                    }
                    hasher.update(&buffer[..n]);
                    size += n as u64;
                    output
                        .write_all(&buffer[..n])
                        .map_err(|e| io(&e, "Restoring the backup"))?;
                }
                if size != entry.size || hex::encode(hasher.finalize()) != entry.sha256 {
                    return Err(damaged());
                }
            }
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_dir_all(target);
        }
        result
    }

    /// Restores `snapshot` in place of `target`: written beside it first, then
    /// swapped in, so a failed restore keeps the current folder.
    pub fn restore_replacing(&self, snapshot: &Snapshot, target: &Path) -> LauncherResult<()> {
        let tag = Uuid::new_v4().simple().to_string();
        let name = target
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("restore");
        let staged = target.with_file_name(format!("{name}.restoring-{tag}"));
        self.restore_into(snapshot, &staged)?;
        let old = target.with_file_name(format!("{name}.replaced-{tag}"));
        if target.exists()
            && let Err(e) = crate::storage::atomic::rename(target, &old)
        {
            let _ = fs::remove_dir_all(&staged);
            return Err(io(&e, "Restoring the backup"));
        }
        if let Err(e) = crate::storage::atomic::rename(&staged, target) {
            // Put the current folder back.
            let _ = crate::storage::atomic::rename(&old, target);
            let _ = fs::remove_dir_all(&staged);
            return Err(io(&e, "Restoring the backup"));
        }
        let _ = fs::remove_dir_all(&old);
        Ok(())
    }

    /// Deletes a snapshot's manifest; its objects go with the next `collect`.
    pub fn delete(&self, id: &str) -> LauncherResult<()> {
        self.load(id)?;
        fs::remove_file(self.manifest_path(id)).map_err(|e| io(&e, "Deleting the backup"))
    }

    /// Removes objects no snapshot refers to; returns the bytes freed.
    pub fn collect(&self) -> u64 {
        let referenced: HashSet<String> = self
            .list()
            .into_iter()
            .flat_map(|s| s.files.into_iter().map(|f| f.sha256))
            .collect();
        let mut freed = 0;
        let Ok(prefixes) = fs::read_dir(self.objects()) else {
            return 0;
        };
        for prefix in prefixes.filter_map(Result::ok) {
            let Ok(objects) = fs::read_dir(prefix.path()) else {
                continue;
            };
            for object in objects.filter_map(Result::ok) {
                let name = object.file_name().to_string_lossy().into_owned();
                if !referenced.contains(&name) {
                    let size = object.metadata().map_or(0, |m| m.len());
                    if fs::remove_file(object.path()).is_ok() {
                        freed += size;
                    }
                }
            }
        }
        let _ = fs::remove_dir_all(self.root.join("staging"));
        freed
    }

    /// Deletes the oldest automatic snapshots of `scope` beyond `keep`.
    pub fn prune(&self, instance: InstanceId, scope: &Scope, keep: usize) -> usize {
        let old: Vec<String> = self
            .list()
            .into_iter()
            .filter(|s| {
                s.info.instance_id == instance
                    && s.info.scope == *scope
                    && s.info.reason == Reason::AfterPlay
            })
            .skip(keep)
            .map(|s| s.info.id)
            .collect();
        old.iter().filter(|id| self.delete(id).is_ok()).count()
    }

    /// Bytes the stored objects take.
    pub fn size(&self) -> u64 {
        let Ok(prefixes) = fs::read_dir(self.objects()) else {
            return 0;
        };
        prefixes
            .filter_map(Result::ok)
            .filter_map(|p| fs::read_dir(p.path()).ok())
            .flatten()
            .filter_map(Result::ok)
            .filter_map(|o| o.metadata().ok())
            .map(|m| m.len())
            .sum()
    }
}

fn not_found() -> LauncherError {
    LauncherError::new(ErrorCode::InvalidInput, "This backup no longer exists.")
}

fn damaged() -> LauncherError {
    LauncherError::new(
        ErrorCode::HashMismatch,
        "This backup is damaged and cannot be restored.",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn origin(scope: Scope, reason: Reason) -> Origin {
        Origin {
            instance_id: InstanceId::generate(),
            instance_name: "A".into(),
            version_label: "Minecraft 1.21.1".into(),
            version_id: "1.21.1".into(),
            loader: None,
            scope,
            reason,
        }
    }

    fn write(dir: &Path, path: &str, text: &str) {
        let file = dir.join(path);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, text).unwrap();
    }

    #[test]
    fn snapshots_are_incremental_and_deduplicated() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(&dir.path().join("data"));
        let world = dir.path().join("World");
        write(&world, "level.dat", "level");
        write(&world, "region/r.0.0.mca", &"x".repeat(1000));
        write(&world, "copy.dat", "level");
        let (first, _) = store
            .create(&world, origin(Scope::Installation, Reason::Manual), None)
            .unwrap();
        assert_eq!(first.info.file_count, 3);
        assert_eq!(first.info.total_bytes, 1010);
        assert_eq!(
            first.info.new_bytes, 1005,
            "identical files are stored once"
        );

        // Only the changed file is new; the unchanged ones are not even read.
        std::thread::sleep(std::time::Duration::from_millis(20));
        write(&world, "level.dat", "level 2");
        let (second, _) = store
            .create(
                &world,
                origin(Scope::Installation, Reason::Manual),
                Some(&first),
            )
            .unwrap();
        assert_eq!(second.info.new_bytes, 7);
        assert!(!Store::unchanged(&world, &first));
        assert!(Store::unchanged(&world, &second));
        assert_eq!(store.list().len(), 2);
        assert_eq!(store.size(), 1012);

        // Deleting the first snapshot frees nothing: "level" is still copy.dat.
        store.delete(&first.info.id).unwrap();
        assert_eq!(store.collect(), 0);
        let restored = dir.path().join("restored");
        store
            .restore_into(&store.load(&second.info.id).unwrap(), &restored)
            .unwrap();
        assert_eq!(
            fs::read_to_string(restored.join("level.dat")).unwrap(),
            "level 2"
        );
        assert_eq!(
            fs::read_to_string(restored.join("copy.dat")).unwrap(),
            "level"
        );
        // Deleting the last one frees everything.
        store.delete(&second.info.id).unwrap();
        assert_eq!(store.collect(), 1012);
        assert_eq!(store.size(), 0);
    }

    #[test]
    fn a_restore_replaces_the_folder_or_keeps_it() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(&dir.path().join("data"));
        let world = dir.path().join("saves").join("World");
        write(&world, "level.dat", "old");
        let (snapshot, _) = store
            .create(
                &world,
                origin(
                    Scope::World {
                        name: "World".into(),
                    },
                    Reason::Manual,
                ),
                None,
            )
            .unwrap();
        write(&world, "level.dat", "new");
        write(&world, "extra.dat", "later");
        store.restore_replacing(&snapshot, &world).unwrap();
        assert_eq!(fs::read_to_string(world.join("level.dat")).unwrap(), "old");
        assert!(!world.join("extra.dat").exists(), "an exact restore");
        let leftovers: Vec<_> = fs::read_dir(world.parent().unwrap()).unwrap().collect();
        assert_eq!(leftovers.len(), 1, "no staging folders left");

        // A damaged object stops the restore and keeps the current folder.
        let object = store.object_path(&snapshot.files[0].sha256);
        fs::write(&object, "tampered").unwrap();
        write(&world, "level.dat", "current");
        assert_eq!(
            store.restore_replacing(&snapshot, &world).unwrap_err().code,
            ErrorCode::HashMismatch
        );
        assert_eq!(
            fs::read_to_string(world.join("level.dat")).unwrap(),
            "current"
        );
    }

    #[test]
    fn a_manifest_with_a_bad_hash_is_damaged_not_followed() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(&dir.path().join("data"));
        let world = dir.path().join("World");
        write(&world, "level.dat", "x");
        let (snapshot, _) = store
            .create(
                &world,
                origin(
                    Scope::World {
                        name: "World".into(),
                    },
                    Reason::Manual,
                ),
                None,
            )
            .unwrap();
        let path = store.manifest_path(&snapshot.info.id);
        let text = fs::read_to_string(&path).unwrap();
        for bad in ["a", "../../../../outside"] {
            fs::write(&path, text.replace(&snapshot.files[0].sha256, bad)).unwrap();
            assert!(store.load(&snapshot.info.id).is_err(), "{bad}");
            assert!(store.list().is_empty(), "{bad}");
        }
    }

    #[test]
    fn automatic_snapshots_are_pruned_manual_ones_kept() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(&dir.path().join("data"));
        let world = dir.path().join("World");
        write(&world, "level.dat", "a");
        let id = InstanceId::generate();
        let scope = Scope::World {
            name: "World".into(),
        };
        for reason in [
            Reason::Manual,
            Reason::AfterPlay,
            Reason::AfterPlay,
            Reason::AfterPlay,
        ] {
            let mut o = origin(scope.clone(), reason);
            o.instance_id = id;
            store.create(&world, o, None).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!(store.prune(id, &scope, 2), 1);
        let left: Vec<Reason> = store.list().iter().map(|s| s.info.reason).collect();
        assert_eq!(left, [Reason::AfterPlay, Reason::AfterPlay, Reason::Manual]);
        assert_eq!(
            store.latest(id, &scope).unwrap().info.reason,
            Reason::AfterPlay
        );
    }

    #[test]
    fn names_and_manifest_paths_cannot_leave_their_folder() {
        for bad in ["", ".", "..", "a/b", "a\\b", "C:", " x"] {
            assert!(!valid_world_name(bad), "{bad:?}");
        }
        assert!(valid_world_name("New World (1)"));
        for bad in ["../x", "a//b", "/abs", "a/../b", "C:/x", "a\\b"] {
            assert!(safe_relative(bad).is_none(), "{bad}");
        }
        assert!(safe_relative("region/r.0.0.mca").is_some());
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path());
        assert!(store.load("../../etc/passwd").is_err());
    }
}
