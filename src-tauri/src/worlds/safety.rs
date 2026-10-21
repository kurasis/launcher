//! World safety (W9): backups of a stopped hosted world, a restore with a
//! backup first, a single-player world imported as a copy, and an export as
//! zip.
//!
//! Backups use the D-047 store in a store of its own under `worlds/backups`,
//! so the installations' backups never list them. There a snapshot's
//! `instance_id` is the world's id and its scope is the save folder
//! `server/world` (`World { name: "world" }`). Every change records its kind
//! in `world.json` first; [`recover`] finishes what an interrupted one left.
//! Callers make sure the world's server is not running.

use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use chrono::Utc;
use serde::Serialize;
use uuid::Uuid;

use super::properties::WorldSettings;
use super::tuple;
use super::world::{self, Mutation, MutationKind, WorldFile};
use crate::backup::store::{self, Origin, Reason, Scope, SnapshotInfo, Store};
use crate::error::{ErrorCode, LauncherError, LauncherResult};
use crate::storage::models::InstanceFile;
use crate::storage::paths::{DataRoot, InstanceId, WorldId};

/// The save folder in the server's folder (`level-name=world`).
pub const SAVE: &str = "world";
/// Automatic backups kept after hosting sessions, per world.
pub const KEEP_AFTER_PLAY: usize = 10;
/// Room left on the disk beyond what a change needs.
const SPACE_MARGIN: u64 = 64 * 1024 * 1024;
/// Most files and bytes a world brought in or written out may have.
const MAX_FILES: usize = 200_000;
const MAX_BYTES: u64 = 64 * 1024 * 1024 * 1024;
/// Names in an export.
const MANIFEST: &str = "manifest.json";
const FORMAT: &str = "launcher-world/1";

fn io(e: &std::io::Error, what: &'static str) -> LauncherError {
    LauncherError::from_io(e, what)
}

pub fn store(root: &DataRoot) -> Store {
    Store::at(root.world_backups_dir())
}

pub fn save_dir(root: &DataRoot, id: WorldId) -> PathBuf {
    root.world_server_dir(id).join(SAVE)
}

fn owner(id: WorldId) -> InstanceId {
    id.to_string()
        .parse()
        .expect("a world id is a canonical UUID")
}

fn scope() -> Scope {
    Scope::World { name: SAVE.into() }
}

/// Whether `available` bytes are enough for `need` and the margin. An
/// unknown amount does not stop the change; the write fails cleanly then.
pub fn enough_space(available: Option<u64>, need: u64) -> bool {
    available.is_none_or(|free| free >= need.saturating_add(SPACE_MARGIN))
}

/// The nearest existing folder of `path`, for the free space there.
fn existing(path: &Path) -> &Path {
    path.ancestors().find(|p| p.is_dir()).unwrap_or(path)
}

pub(super) fn ensure_space(path: &Path, need: u64) -> LauncherResult<()> {
    if enough_space(crate::download::available_space(existing(path)), need) {
        return Ok(());
    }
    Err(LauncherError::templated(
        ErrorCode::DiskFull,
        "There is not enough free space: {need} MiB more are needed.",
        crate::error::params([("need", (need.saturating_add(SPACE_MARGIN) >> 20).into())]),
    ))
}

/// The files of a folder, bounded; links are left out.
fn files_of(dir: &Path) -> LauncherResult<(Vec<store::Found>, u64)> {
    let (files, _) = store::walk(dir)?;
    if files.len() > MAX_FILES {
        return Err(LauncherError::invalid_input(
            "This world has more files than a world can have.",
        ));
    }
    let bytes = files.iter().map(|(_, _, m)| m.len()).sum::<u64>();
    if bytes > MAX_BYTES {
        return Err(LauncherError::invalid_input(
            "This world is larger than a world can be.",
        ));
    }
    Ok((files, bytes))
}

/// Records `kind` in `world.json` while `work` runs, then clears it. A
/// failure of `work` clears it too, after [`recover`] tidied up.
fn with_mutation<T>(
    root: &DataRoot,
    id: WorldId,
    kind: MutationKind,
    work: impl FnOnce(&WorldFile) -> LauncherResult<T>,
) -> LauncherResult<T> {
    let mut world = world::load(root, id)?;
    if world.mutation.is_some() {
        recover(root, &mut world)?;
    }
    world.mutation = Some(Mutation {
        kind,
        operation_id: Uuid::new_v4().to_string(),
        started_at: Utc::now(),
    });
    world::save(root, &world)?;
    let result = work(&world);
    let mut after = world::load(root, id).unwrap_or(world);
    if result.is_err() {
        recover(root, &mut after)?;
    } else {
        after.mutation = None;
        world::save(root, &after)?;
    }
    result
}

/// Finishes a backup, restore or import that an exit or a crash cut short:
/// staged folders go, a world moved aside for a restore comes back when its
/// replacement never arrived, and an import that never finished is removed
/// with its world. Other changes (installing the server, hosting) are left
/// to their owners. Returns false when the world itself is gone.
pub fn recover(root: &DataRoot, world: &mut WorldFile) -> LauncherResult<bool> {
    let Some(kind) = world.mutation.as_ref().map(|m| m.kind) else {
        return Ok(true);
    };
    if kind == MutationKind::Update {
        super::update::recover(root, world)?;
        return Ok(true);
    }
    if !matches!(
        kind,
        MutationKind::Backup | MutationKind::Restore | MutationKind::Import
    ) {
        return Ok(true);
    }
    let server = root.world_server_dir(world.id);
    let save = server.join(SAVE);
    let mut moved_aside = Vec::new();
    if let Ok(entries) = fs::read_dir(&server) {
        for entry in entries.filter_map(Result::ok) {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with(&format!("{SAVE}.restoring-"))
                || name.starts_with(&format!("{SAVE}.importing-"))
            {
                let _ = fs::remove_dir_all(entry.path());
            } else if name.starts_with(&format!("{SAVE}.replaced-")) {
                moved_aside.push(entry.path());
            }
        }
    }
    if !save.exists()
        && let Some(old) = moved_aside.pop()
    {
        crate::storage::atomic::rename(&old, &save)
            .map_err(|e| io(&e, "Putting the world back"))?;
    }
    for old in moved_aside {
        let _ = fs::remove_dir_all(old);
    }
    if kind == MutationKind::Import && !save.exists() {
        fs::remove_dir_all(root.world_dir(world.id)).map_err(|e| io(&e, "Removing the import"))?;
        return Ok(false);
    }
    world.mutation = None;
    world::save(root, world)?;
    Ok(true)
}

/// The world's backups, newest first.
pub fn list(root: &DataRoot, id: WorldId) -> Vec<SnapshotInfo> {
    let owner = owner(id);
    store(root)
        .list()
        .into_iter()
        .filter(|s| s.info.instance_id == owner && s.info.scope == scope())
        .map(|s| s.info)
        .collect()
}

/// Backs up the stopped world. After a hosting session an unchanged world
/// makes no backup (None) and the oldest automatic ones beyond
/// [`KEEP_AFTER_PLAY`] go.
pub fn backup(
    root: &DataRoot,
    id: WorldId,
    reason: Reason,
) -> LauncherResult<Option<SnapshotInfo>> {
    let save = save_dir(root, id);
    if !save.is_dir() {
        return Err(LauncherError::invalid_input(
            "This world has not been played yet: there is nothing to back up.",
        ));
    }
    with_mutation(root, id, MutationKind::Backup, |world| {
        backup_locked(root, world, reason)
    })
}

/// [`backup`] inside a change already recorded (a restore's backup first).
fn backup_locked(
    root: &DataRoot,
    world: &WorldFile,
    reason: Reason,
) -> LauncherResult<Option<SnapshotInfo>> {
    let save = save_dir(root, world.id);
    let store = store(root);
    let previous = store.latest(owner(world.id), &scope());
    if reason == Reason::AfterPlay
        && let Some(previous) = &previous
        && Store::unchanged(&save, previous)
    {
        return Ok(None);
    }
    let (_, bytes) = files_of(&save)?;
    ensure_space(&root.world_backups_dir(), bytes)?;
    let origin = Origin {
        instance_id: owner(world.id),
        instance_name: world.name.clone(),
        version_label: format!("Minecraft {}", world.minecraft),
        version_id: world.minecraft.clone(),
        loader: None,
        scope: scope(),
        reason,
    };
    let (snapshot, _) = store.create(&save, origin, previous.as_ref())?;
    if reason == Reason::AfterPlay {
        store.prune(owner(world.id), &scope(), KEEP_AFTER_PLAY);
        store.collect();
    }
    // The generation of its mods comes back with it. Generations of pruned
    // backups go after a session; an update may be staging one otherwise.
    if world.pack.is_some() {
        let mut now = world::load(root, world.id)?;
        super::update::record_backup(&mut now, &snapshot.info.id);
        if reason == Reason::AfterPlay {
            super::update::tidy(root, &mut now);
        }
        world::save(root, &now)?;
    }
    Ok(Some(snapshot.info))
}

/// Puts `backup` in place of the world's save. The current save is backed
/// up first; the backup is written beside it and checked before the swap,
/// so a damaged backup or a full disk keeps the world as it was. A world
/// with mods gets back the generation the backup was made with.
pub fn restore(root: &DataRoot, id: WorldId, backup: &str) -> LauncherResult<()> {
    let store = store(root);
    let snapshot = store.load(backup)?;
    if snapshot.info.instance_id != owner(id) || snapshot.info.scope != scope() {
        return Err(LauncherError::invalid_input(
            "This backup belongs to another world.",
        ));
    }
    let save = save_dir(root, id);
    ensure_space(&save, snapshot.info.total_bytes)?;
    with_mutation(root, id, MutationKind::Restore, |world| {
        if save.is_dir() {
            backup_locked(root, world, Reason::BeforeRestore)?;
            store.restore_replacing(&snapshot, &save)?;
        } else {
            store.restore_into(&snapshot, &save)?;
        }
        let mut now = world::load(root, id)?;
        super::update::bring_back(root, &mut now, backup);
        world::save(root, &now)
    })
}

/// A single-player world an installation can bring into worlds with friends.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Importable {
    pub instance_id: InstanceId,
    pub instance_name: String,
    pub minecraft: String,
    /// The mod loader, whose mods the world takes (W12).
    pub loader: Option<crate::loader::LoaderChoice>,
    pub saves: Vec<String>,
}

/// Installations whose worlds can be imported: a Minecraft version worlds
/// support, a mod loader only where worlds with mods run it (W12), and at
/// least one world.
pub fn importable(root: &DataRoot, instances: &[InstanceFile]) -> Vec<Importable> {
    instances
        .iter()
        .filter(|i| i.world_room.is_none())
        .filter(|i| tuple::version(&i.version_id).is_some())
        .filter(|i| {
            i.loader
                .as_ref()
                .is_none_or(|l| super::pack::tested(l.kind, &i.version_id))
        })
        .filter_map(|i| {
            let saves = crate::backup::worlds(&root.game_dir(i.id));
            (!saves.is_empty()).then(|| Importable {
                instance_id: i.id,
                instance_name: i.display_name.clone(),
                minecraft: i.version_id.clone(),
                loader: i.loader.clone(),
                saves,
            })
        })
        .collect()
}

/// Makes a new world from a copy of `saves/<save>` of `instance`, which
/// must not be running (the caller holds its permit). The original stays as
/// it is. The copy is written beside the new save folder and moved in whole.
/// The mods of an installation with a mod loader are the caller's to add.
pub fn import(
    root: &DataRoot,
    instance: &InstanceFile,
    save: &str,
    name: &str,
) -> LauncherResult<WorldFile> {
    if !store::valid_world_name(save) {
        return Err(LauncherError::invalid_input("This world name is invalid."));
    }
    let source = root.game_dir(instance.id).join("saves").join(save);
    if !source.join("level.dat").is_file() {
        return Err(LauncherError::invalid_input(
            "This folder is not a Minecraft world.",
        ));
    }
    let (files, bytes) = files_of(&source)?;
    ensure_space(&root.worlds_dir(), bytes)?;
    let mut created = world::create(root, name, &instance.version_id, WorldSettings::default())?;
    created.mutation = Some(Mutation {
        kind: MutationKind::Import,
        operation_id: Uuid::new_v4().to_string(),
        started_at: Utc::now(),
    });
    world::save(root, &created)?;
    let target = save_dir(root, created.id);
    let staged = target.with_file_name(format!("{SAVE}.importing-{}", Uuid::new_v4().simple()));
    let copied = copy_files(&files, &staged).and_then(|()| {
        crate::storage::atomic::rename(&staged, &target).map_err(|e| io(&e, "Importing the world"))
    });
    if let Err(error) = copied {
        let _ = recover(root, &mut created);
        let _ = fs::remove_dir_all(root.world_dir(created.id));
        return Err(error);
    }
    created.mutation = None;
    world::save(root, &created)?;
    Ok(created)
}

/// Copies found files under `target`, flushed; the game's lock file stays
/// behind, it belongs to the running game only.
fn copy_files(files: &[store::Found], target: &Path) -> LauncherResult<()> {
    for (relative, full, meta) in files {
        if relative == "session.lock" {
            continue;
        }
        let destination = target.join(relative);
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent).map_err(|e| io(&e, "Importing the world"))?;
        }
        let mut input = fs::File::open(full).map_err(|e| io(&e, "Reading the world"))?;
        let mut output =
            fs::File::create(&destination).map_err(|e| io(&e, "Importing the world"))?;
        let copied =
            std::io::copy(&mut input, &mut output).map_err(|e| io(&e, "Importing the world"))?;
        if copied != meta.len() {
            return Err(LauncherError::new(
                ErrorCode::InvalidInput,
                "The world changed while it was copied. Close the game and try again.",
            ));
        }
        output
            .sync_all()
            .map_err(|e| io(&e, "Importing the world"))?;
    }
    if !target.exists() {
        fs::create_dir_all(target).map_err(|e| io(&e, "Importing the world"))?;
    }
    Ok(())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ExportManifest<'a> {
    format: &'static str,
    name: &'a str,
    minecraft: &'a str,
    exported_at: chrono::DateTime<Utc>,
    files: usize,
    bytes: u64,
}

/// Writes the stopped world as a zip at `destination`: `manifest.json` and
/// the save under `world/`. Nothing else of the server goes in (its
/// properties carry a console password, the whitelist names players). The
/// zip is written beside `destination` and moved there once complete.
pub fn export(root: &DataRoot, id: WorldId, destination: &Path) -> LauncherResult<u64> {
    let world = world::load(root, id)?;
    let save = save_dir(root, id);
    if !save.is_dir() {
        return Err(LauncherError::invalid_input(
            "This world has not been played yet: there is nothing to export.",
        ));
    }
    let (files, bytes) = files_of(&save)?;
    let folder = destination
        .parent()
        .filter(|p| p.is_dir())
        .ok_or_else(|| LauncherError::invalid_input("Choose a folder that exists."))?;
    ensure_space(folder, bytes)?;
    let file_name = destination
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("world.zip");
    let temp = folder.join(format!(".{file_name}.tmp-{}", Uuid::new_v4().simple()));
    let result = write_zip(&world, &files, bytes, &temp).and_then(|()| {
        crate::storage::atomic::rename(&temp, destination).map_err(|e| io(&e, "Saving the export"))
    });
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result.map(|()| fs::metadata(destination).map_or(0, |m| m.len()))
}

fn write_zip(
    world: &WorldFile,
    files: &[store::Found],
    bytes: u64,
    path: &Path,
) -> LauncherResult<()> {
    use zip::write::SimpleFileOptions;
    let file = fs::File::create(path).map_err(|e| io(&e, "Saving the export"))?;
    let mut zip = zip::ZipWriter::new(std::io::BufWriter::new(file));
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let manifest = ExportManifest {
        format: FORMAT,
        name: &world.name,
        minecraft: &world.minecraft,
        exported_at: Utc::now(),
        files: files.len(),
        bytes,
    };
    let zipped =
        |_: zip::result::ZipError| LauncherError::internal("The export could not be written.");
    zip.start_file(MANIFEST, options).map_err(zipped)?;
    zip.write_all(&serde_json::to_vec_pretty(&manifest).unwrap_or_default())
        .map_err(|e| io(&e, "Saving the export"))?;
    let mut buffer = vec![0u8; 256 * 1024];
    for (relative, full, meta) in files {
        if relative == "session.lock" {
            continue;
        }
        let entry = options.large_file(meta.len() > u64::from(u32::MAX));
        zip.start_file(format!("{SAVE}/{relative}"), entry)
            .map_err(zipped)?;
        let mut input = fs::File::open(full).map_err(|e| io(&e, "Reading the world"))?;
        loop {
            let n = input
                .read(&mut buffer)
                .map_err(|e| io(&e, "Reading the world"))?;
            if n == 0 {
                break;
            }
            zip.write_all(&buffer[..n])
                .map_err(|e| io(&e, "Saving the export"))?;
        }
    }
    let writer = zip.finish().map_err(zipped)?;
    let file = writer
        .into_inner()
        .map_err(|e| io(e.error(), "Saving the export"))?;
    file.sync_all().map_err(|e| io(&e, "Saving the export"))
}

#[cfg(test)]
#[path = "safety_tests.rs"]
mod tests;
