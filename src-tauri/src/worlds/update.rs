//! Updating the mods of a world (PLAN W12).
//!
//! The mods and the world change only together. An update makes the next
//! generation from an installation, and the way back is always the pair of
//! the generation before it and the backup made just before it: a world the
//! new mods already changed does not start with the old ones.
//!
//! 1. [`preview`]: what the generation adds, removes and updates, and the
//!    server's settings the host changed that it brings another version of.
//!    The host keeps theirs or takes the new one ([`Keep`]); settings the host
//!    never changed take the new version without asking.
//! 2. [`stage`] keeps the generation's files beside the world's. The stopped
//!    world is backed up (W9), and the generation is tried on a copy of that
//!    backup ([`try_on_copy`]), a world of its own that is removed after. A
//!    copy that does not start, or does not stop cleanly, leaves the world as
//!    it was.
//! 3. [`commit`]: the generation becomes the world's, and [`Pending`] keeps
//!    the pair to go back to and the settings it replaced.
//! 4. The next start confirms it ([`confirm`]); one that fails puts the
//!    backup, the earlier generation and the settings back ([`roll_back`]).
//!
//! Every backup records the generation it was made with, and restoring it
//! brings that generation back ([`bring_back`]). Generations no backup needs
//! any more go with their files ([`tidy`]).

use std::collections::{BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use sha2::Digest as _;
use uuid::Uuid;

use super::pack::{self, Built, PackFile, Side, WorldPack};
use super::world::{self, Mutation, MutationKind, WorldFile};
use super::{host, install, memory, safety};
use crate::catalog::source::{Fetch, MetadataService};
use crate::download::{CancelToken, Executor, ProgressSink};
use crate::error::{ErrorCode, LauncherError, LauncherResult};
use crate::install::service::PlanSource;
use crate::loader::LoaderChoice;
use crate::mods::pack::hashes_of;
use crate::runtime::RuntimeInstaller;
use crate::state::activity::ActivityRegistry;
use crate::storage::paths::{DataRoot, JavaProvider, Sha1, Sha256, WorldId};

/// Most of a setting's text a conflict shows, per side.
const SHOWN: usize = 16 * 1024;
/// Earlier generations a world keeps at most, for its backups.
const MAX_GENERATIONS: usize = 32;

/// An update no start has confirmed yet, and the way back from it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Pending {
    /// The generation before the update.
    pub from: u32,
    /// The backup made just before it; none for a world never played.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backup: Option<String>,
    /// The server's settings it replaced, kept in `pack/` by content.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub replaced: Vec<Replaced>,
    /// False until the generation is the world's: an exit before that puts
    /// the settings back.
    pub committed: bool,
}

/// A setting of the server an update replaced.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Replaced {
    pub path: String,
    pub sha256: Sha256,
}

/// A mod an update adds, removes or changes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModChange {
    /// Its file name.
    pub name: String,
    pub side: Side,
    /// The file it replaces, for a changed mod.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
}

/// One side of a setting in conflict, as far as it is shown.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Shown {
    /// None for a file that is not text.
    pub text: Option<String>,
    pub bytes: u64,
    pub truncated: bool,
}

/// A setting of the server the host changed and the new generation brings
/// another version of.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Conflict {
    pub path: String,
    /// What the current generation brought; none when it had no such file.
    pub base: Option<Shown>,
    /// What the server has now.
    pub mine: Shown,
    /// What the new generation brings.
    pub theirs: Shown,
}

/// What an update from an installation would change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    /// Names exactly this change; the update is refused for another.
    pub fingerprint: String,
    /// The generation now, and the one the update would make.
    pub generation: u32,
    pub next: u32,
    pub loader: LoaderChoice,
    pub loader_changed: bool,
    pub added: Vec<ModChange>,
    pub removed: Vec<ModChange>,
    pub updated: Vec<ModChange>,
    /// Settings that change without asking.
    pub settings: u32,
    pub conflicts: Vec<Conflict>,
    /// Nothing would change.
    pub same: bool,
}

/// The host's answer to a conflict.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Keep {
    Mine,
    Theirs,
}

/// A generation kept beside the world's, not yet its own.
#[derive(Debug, Clone)]
pub struct Staged {
    pub pack: WorldPack,
    from: u32,
    /// Server settings it replaces.
    take: Vec<String>,
}

fn no_mods() -> LauncherError {
    LauncherError::invalid_input(
        "This world has no mods. Make a new world from the installation instead.",
    )
}

fn moved_on() -> LauncherError {
    LauncherError::new(
        ErrorCode::AppBusy,
        "The installation or the world changed since its changes were shown. Look at them again.",
    )
}

fn disk(error: &std::io::Error) -> LauncherError {
    LauncherError::from_io(error, "Updating the world's mods")
}

fn file_name(path: &str) -> String {
    path.rsplit('/').next().unwrap_or(path).to_owned()
}

/// The number the next generation gets.
fn next_generation(world: &WorldFile) -> u32 {
    world
        .pack
        .iter()
        .chain(&world.generations)
        .map(|g| g.generation)
        .max()
        .unwrap_or(0)
        .saturating_add(1)
}

/// The file at `relative` inside `dir` when every folder on the way is a real
/// one and it is a regular file; never a link out of `dir`.
fn inside(dir: &Path, relative: &str) -> Option<PathBuf> {
    let mut at = dir.to_path_buf();
    let parts: Vec<&str> = relative.split('/').collect();
    let (name, folders) = parts.split_last()?;
    for folder in folders {
        if folder.is_empty() || *folder == "." || *folder == ".." {
            return None;
        }
        at = at.join(folder);
        if !fs::symlink_metadata(&at).is_ok_and(|m| m.is_dir()) {
            return None;
        }
    }
    let file = at.join(name);
    fs::symlink_metadata(&file)
        .is_ok_and(|m| m.is_file())
        .then_some(file)
}

/// Makes the folders of `relative` inside `dir`, each a real one; the path of
/// the file.
fn make_inside(dir: &Path, relative: &str) -> LauncherResult<PathBuf> {
    let damaged = || LauncherError::invalid_input("A setting of the world has an unusable name.");
    let parts: Vec<&str> = relative.split('/').collect();
    let (name, folders) = parts.split_last().ok_or_else(damaged)?;
    let mut at = dir.to_path_buf();
    for folder in folders {
        if folder.is_empty() || *folder == "." || *folder == ".." {
            return Err(damaged());
        }
        at = at.join(folder);
        match fs::symlink_metadata(&at) {
            Ok(meta) if meta.is_dir() => {}
            Ok(_) => return Err(damaged()),
            Err(_) => fs::create_dir(&at).map_err(|e| disk(&e))?,
        }
    }
    let file = at.join(name);
    if fs::symlink_metadata(&file).is_ok_and(|m| !m.is_file()) {
        return Err(damaged());
    }
    Ok(file)
}

fn sha256_of(path: &Path) -> LauncherResult<(Sha256, u64)> {
    let (_, _, sha256) = hashes_of(path).map_err(|e| disk(&e))?;
    let size = fs::metadata(path).map_err(|e| disk(&e))?.len();
    Ok((Sha256::parse(&sha256)?, size))
}

fn shown(path: &Path, bytes: u64) -> Shown {
    let read = fs::read(path).unwrap_or_default();
    let cut = &read[..read.len().min(SHOWN)];
    let text = match std::str::from_utf8(cut) {
        Ok(text) => Some(text),
        // A cut in the middle of a character is still text.
        Err(e) if read.len() > SHOWN && cut.len() - e.valid_up_to() < 4 => {
            std::str::from_utf8(&cut[..e.valid_up_to()]).ok()
        }
        Err(_) => None,
    }
    .filter(|t| !t.contains('\0'))
    .map(str::to_owned);
    Shown {
        text,
        bytes,
        truncated: bytes > SHOWN as u64,
    }
}

/// The server's settings against the new generation: those it takes without
/// asking, and the conflicts with what the server has (its SHA-256 too).
struct Settings {
    take: Vec<String>,
    conflicts: Vec<(Conflict, Sha256)>,
}

fn settings(root: &DataRoot, world: &WorldFile, now: &WorldPack, new: &WorldPack) -> Settings {
    let server = root.world_server_dir(world.id);
    let store = root.world_pack_dir(world.id);
    let base: HashMap<&str, &PackFile> = now
        .files
        .iter()
        .filter(|f| !f.is_mod() && f.side.on_server())
        .map(|f| (f.path.as_str(), f))
        .collect();
    let mut out = Settings {
        take: Vec::new(),
        conflicts: Vec::new(),
    };
    for theirs in new
        .files
        .iter()
        .filter(|f| !f.is_mod() && f.side.on_server())
    {
        // A setting the server does not have is written at the next start.
        let Some(mine_path) = inside(&server, &theirs.path) else {
            continue;
        };
        let Ok((mine, mine_bytes)) = sha256_of(&mine_path) else {
            continue;
        };
        let base = base.get(theirs.path.as_str());
        if mine == theirs.sha256 || base.is_some_and(|b| b.sha256 == theirs.sha256) {
            continue;
        }
        if base.is_some_and(|b| b.sha256 == mine) {
            out.take.push(theirs.path.clone());
            continue;
        }
        let conflict = Conflict {
            path: theirs.path.clone(),
            base: base.map(|b| shown(&store.join(b.sha256.as_str()), b.size)),
            mine: shown(&mine_path, mine_bytes),
            theirs: shown(&store.join(theirs.sha256.as_str()), theirs.size),
        };
        out.conflicts.push((conflict, mine));
    }
    out.conflicts.sort_by(|a, b| a.0.path.cmp(&b.0.path));
    out
}

/// The files of `new` and what the server's settings say, as one name.
fn fingerprint(new: &WorldPack, settings: &Settings) -> String {
    let mut hasher = sha2::Sha256::new();
    hasher.update(format!("{:?}|{}|", new.loader.kind, new.loader.version));
    let mut files: Vec<&PackFile> = new.files.iter().collect();
    files.sort_by(|a, b| a.path.cmp(&b.path));
    for f in files {
        hasher.update(format!("{}|{}|{:?}\n", f.path, f.sha256.as_str(), f.side));
    }
    for path in &settings.take {
        hasher.update(format!("take|{path}\n"));
    }
    for (conflict, mine) in &settings.conflicts {
        hasher.update(format!("conflict|{}|{}\n", conflict.path, mine.as_str()));
    }
    hex::encode(hasher.finalize())
}

/// The mods by catalog project (or path, for a file without one).
fn by_project(pack: &WorldPack) -> HashMap<String, &PackFile> {
    pack.mods()
        .map(|f| (f.project_id.clone().unwrap_or_else(|| f.path.clone()), f))
        .collect()
}

fn sorted(mut list: Vec<ModChange>) -> Vec<ModChange> {
    list.sort_by_key(|c| c.name.to_lowercase());
    list
}

fn preview_of(world: &WorldFile, now: &WorldPack, new: &WorldPack, settings: &Settings) -> Preview {
    let (before, after) = (by_project(now), by_project(new));
    let change = |f: &PackFile, from: Option<&PackFile>| ModChange {
        name: file_name(&f.path),
        side: f.side,
        from: from.map(|o| file_name(&o.path)),
    };
    let added = after
        .iter()
        .filter(|(k, _)| !before.contains_key(*k))
        .map(|(_, f)| change(f, None))
        .collect();
    let removed = before
        .iter()
        .filter(|(k, _)| !after.contains_key(*k))
        .map(|(_, f)| change(f, None))
        .collect();
    let updated = after
        .iter()
        .filter_map(|(k, f)| {
            let old = before.get(k)?;
            (old.sha256 != f.sha256 || old.side != f.side).then(|| change(f, Some(old)))
        })
        .collect();
    let set = |p: &WorldPack| -> BTreeSet<(String, String, String)> {
        p.files
            .iter()
            .map(|f| {
                (
                    f.path.clone(),
                    f.sha256.to_string(),
                    format!("{:?}", f.side),
                )
            })
            .collect()
    };
    let loader_changed = now.loader != new.loader;
    Preview {
        fingerprint: fingerprint(new, settings),
        generation: now.generation,
        next: next_generation(world),
        loader: new.loader.clone(),
        loader_changed,
        added: sorted(added),
        removed: sorted(removed),
        updated: sorted(updated),
        settings: u32::try_from(settings.take.len()).unwrap_or(u32::MAX),
        conflicts: settings.conflicts.iter().map(|(c, _)| c.clone()).collect(),
        same: !loader_changed && set(now) == set(new),
    }
}

/// Refuses a generation the world cannot take: another Minecraft version,
/// another loader.
fn check_fits(world: &WorldFile, now: &WorldPack, new: &WorldPack) -> LauncherResult<()> {
    if new.loader.kind != now.loader.kind {
        return Err(LauncherError::invalid_input(
            "The world's mods can only be updated from an installation with the same mod loader.",
        ));
    }
    pack::check_supported(&new.loader, &world.minecraft)
}

/// What updating the world to the generation `built` would change (blocking:
/// it reads the server's settings). New settings are shown from the
/// installation.
pub fn preview(root: &DataRoot, world: &WorldFile, built: &Built) -> LauncherResult<Preview> {
    let now = world.pack.as_ref().ok_or_else(no_mods)?;
    check_fits(world, now, &built.pack)?;
    let mut found = settings(root, world, now, &built.pack);
    // Not staged yet: the new versions are read where they are now.
    for (conflict, _) in &mut found.conflicts {
        if let Some((file, source)) = built
            .pack
            .files
            .iter()
            .zip(&built.sources)
            .find(|(f, _)| f.path == conflict.path)
        {
            conflict.theirs = shown(source, file.size);
        }
    }
    Ok(preview_of(world, now, &built.pack, &found))
}

/// Keeps the generation `built` beside the world's files, when it is still
/// the change `expected` names and every conflict has an answer (blocking).
pub fn stage(
    root: &DataRoot,
    world: &WorldFile,
    mut built: Built,
    expected: &str,
    choices: &HashMap<String, Keep>,
) -> LauncherResult<Staged> {
    let now = world.pack.as_ref().ok_or_else(no_mods)?;
    check_fits(world, now, &built.pack)?;
    let found = settings(root, world, now, &built.pack);
    let shown = preview_of(world, now, &built.pack, &found);
    if shown.same {
        return Err(LauncherError::invalid_input(
            "The world already has these mods.",
        ));
    }
    if shown.fingerprint != expected {
        return Err(moved_on());
    }
    if choices.len() > found.conflicts.len()
        || found
            .conflicts
            .iter()
            .any(|(c, _)| !choices.contains_key(&c.path))
    {
        return Err(LauncherError::invalid_input(
            "Choose for every changed setting whether to keep yours or take the new one.",
        ));
    }
    let mut take = found.take;
    take.extend(
        found
            .conflicts
            .iter()
            .filter(|(c, _)| choices.get(&c.path) == Some(&Keep::Theirs))
            .map(|(c, _)| c.path.clone()),
    );
    built.pack.generation = shown.next;
    let pack = pack::store(root, world, built)?;
    Ok(Staged {
        pack,
        from: now.generation,
        take,
    })
}

/// Records that the world's mods are being updated; an exit while it is
/// finishes in [`recover`].
pub fn begin(root: &DataRoot, id: WorldId) -> LauncherResult<()> {
    let mut world = world::load(root, id)?;
    world.mutation = Some(Mutation {
        kind: MutationKind::Update,
        operation_id: Uuid::new_v4().to_string(),
        started_at: chrono::Utc::now(),
    });
    world::save(root, &world)
}

/// An update that stopped before [`commit`]: the world as it was, without
/// the staged files.
pub fn abandon(root: &DataRoot, id: WorldId) -> LauncherResult<()> {
    let mut world = world::load(root, id)?;
    remove_trials(root, id);
    if let Some(pending) = world.update.clone().filter(|u| !u.committed) {
        put_back(root, &world, &pending)?;
        world.update = None;
    }
    if world
        .mutation
        .as_ref()
        .is_some_and(|m| m.kind == MutationKind::Update)
    {
        world.mutation = None;
    }
    tidy(root, &mut world);
    world::save(root, &world)
}

/// Finishes an update an exit cut short: the copies it was tried on go, and
/// an update not yet the world's is undone.
pub fn recover(root: &DataRoot, world: &mut WorldFile) -> LauncherResult<()> {
    remove_trials(root, world.id);
    if let Some(pending) = world.update.clone().filter(|u| !u.committed) {
        put_back(root, world, &pending)?;
        world.update = None;
    }
    world.mutation = None;
    tidy(root, world);
    world::save(root, world)
}

fn remove_trials(root: &DataRoot, id: WorldId) {
    for trial in world::trials(root) {
        if trial.trial_of == Some(id) {
            let _ = fs::remove_dir_all(root.world_dir(trial.id));
        }
    }
}

/// What a copy is tried with, and where it reports.
pub struct Trial<'a, F: Fetch> {
    pub root: &'a DataRoot,
    pub metadata: &'a MetadataService<F>,
    pub source: &'a dyn PlanSource,
    pub runtimes: &'a RuntimeInstaller,
    pub executor: &'a Executor,
    pub activity: &'a ActivityRegistry,
    pub java: JavaProvider,
    /// The host's own game heap when they play on this computer, else zero.
    pub game_heap_mib: u32,
    pub physical_memory_bytes: u64,
    pub cancel: &'a CancelToken,
    pub sink: &'a dyn ProgressSink,
    /// Told when the copy's server starts.
    pub starting: &'a (dyn Fn() + Send + Sync),
}

/// The copy an update is tried on: the world's settings and server, the new
/// generation, the backup as its save and the server's settings with the
/// host's answers (blocking).
fn make_trial(
    root: &DataRoot,
    id: WorldId,
    staged: &Staged,
    backup: Option<&str>,
) -> LauncherResult<WorldId> {
    let world = world::load(root, id)?;
    let trial = WorldFile {
        id: WorldId::generate(),
        pack: Some(staged.pack.clone()),
        room_pack: None,
        generations: Vec::new(),
        backup_generations: std::collections::BTreeMap::new(),
        update: None,
        mutation: None,
        last_run: None,
        trial_of: Some(world.id),
        ..world.clone()
    };
    let server = root.world_server_dir(trial.id);
    fs::create_dir_all(&server).map_err(|e| disk(&e))?;
    world::save(root, &trial)?;
    let made = (|| {
        if let Some(backup) = backup {
            let store = safety::store(root);
            let snapshot = store.load(backup)?;
            safety::ensure_space(&server, snapshot.info.total_bytes)?;
            store.restore_into(&snapshot, &server.join(safety::SAVE))?;
        }
        copy_settings(&root.world_server_dir(world.id), &server)?;
        for path in &staged.take {
            if let Some(file) = inside(&server, path) {
                fs::remove_file(file).map_err(|e| disk(&e))?;
            }
        }
        Ok(())
    })();
    if let Err(error) = made {
        let _ = fs::remove_dir_all(root.world_dir(trial.id));
        return Err(error);
    }
    Ok(trial.id)
}

/// The server's settings a copy starts with: what its mods read from
/// `config/`, bounded as a generation's settings are.
fn copy_settings(from: &Path, to: &Path) -> LauncherResult<()> {
    let config = from.join("config");
    if !fs::symlink_metadata(&config).is_ok_and(|m| m.is_dir()) {
        return Ok(());
    }
    let (files, _) = crate::backup::store::walk(&config)?;
    let mut bytes = 0u64;
    for (relative, full, meta) in files {
        bytes = bytes.saturating_add(meta.len());
        if bytes > crate::mods::publish::MAX_OVERRIDES * 4 {
            return Err(LauncherError::invalid_input(
                "The world server's settings are larger than a world's settings can be.",
            ));
        }
        let target = make_inside(to, &format!("config/{relative}"))?;
        fs::copy(&full, &target).map_err(|e| disk(&e))?;
    }
    Ok(())
}

/// Tries the staged generation on a copy of `backup` (or of a new world, for
/// a world never played): its loader's server is installed, the copy starts
/// and stops cleanly. Returns the loader server's pinned manifest. The copy
/// is removed whatever happens.
pub async fn try_on_copy<F: Fetch>(
    run: &Trial<'_, F>,
    world: WorldId,
    staged: &Staged,
    backup: Option<&str>,
) -> LauncherResult<Option<Sha1>> {
    let trial = {
        let (root, staged, backup) = (run.root.clone(), staged.clone(), backup.map(str::to_owned));
        tokio::task::spawn_blocking(move || make_trial(&root, world, &staged, backup.as_deref()))
            .await
            .map_err(|_| LauncherError::internal("The world update stopped."))??
    };
    let result = try_trial(run, trial).await;
    let dir = run.root.world_dir(trial);
    let _ = tokio::task::spawn_blocking(move || fs::remove_dir_all(dir)).await;
    result
}

async fn try_trial<F: Fetch>(run: &Trial<'_, F>, trial: WorldId) -> LauncherResult<Option<Sha1>> {
    let (prepared, java) = install::prepare(
        run.root,
        run.metadata,
        run.source,
        run.runtimes,
        run.executor,
        trial,
        run.java,
        run.cancel,
        run.sink,
    )
    .await?;
    (run.starting)();
    let operation_id = Uuid::new_v4().to_string();
    let permit = run
        .activity
        .try_acquire_hosting(trial.uuid(), operation_id.clone())?;
    let launch = host::Launch {
        root: run.root,
        world_id: trial,
        java: &java,
        server_heap_mib: memory::DEFAULT_MODDED_SERVER_HEAP_MIB,
        game_heap_mib: run.game_heap_mib,
        physical_memory_bytes: run.physical_memory_bytes,
        members: &[],
        operation_id,
    };
    let server = Arc::new(host::start(&launch, permit, Arc::new(|| {}))?);
    let outcome = {
        let server = Arc::clone(&server);
        tokio::task::spawn_blocking(move || {
            let ready = server.wait_ready(host::STARTUP_SILENCE);
            let clean = server.stop(host::STOP_GRACE);
            ready.and_then(|()| {
                if clean {
                    Ok(())
                } else {
                    Err(LauncherError::new(
                        ErrorCode::GameExitedWithError,
                        "The world server did not stop cleanly with the new mods.",
                    ))
                }
            })
        })
        .await
        .map_err(|_| LauncherError::internal("Waiting for the world stopped."))?
    };
    outcome?;
    Ok(prepared.pack.and_then(|p| p.server_manifest))
}

/// Makes the staged generation the world's, with the way back: the earlier
/// generation, `backup` and the server settings it replaces (blocking).
pub fn commit(
    root: &DataRoot,
    id: WorldId,
    staged: Staged,
    server_manifest: Option<Sha1>,
    backup: Option<String>,
) -> LauncherResult<WorldFile> {
    let mut world = world::load(root, id)?;
    let Some(now) = world.pack.clone().filter(|p| p.generation == staged.from) else {
        return Err(moved_on());
    };
    let server = root.world_server_dir(id);
    let store = root.world_pack_dir(id);
    let mut replaced = Vec::new();
    for path in &staged.take {
        let Some(file) = inside(&server, path) else {
            continue;
        };
        let (sha256, size) = sha256_of(&file)?;
        let kept = store.join(sha256.as_str());
        if !pack::holds(&kept, size, &sha256) {
            pack::copy_verified(&file, &kept, &sha256)?;
        }
        replaced.push(Replaced {
            path: path.clone(),
            sha256,
        });
    }
    // Written down before the server changes, so an exit puts it back.
    world.update = Some(Pending {
        from: now.generation,
        backup: backup.clone(),
        replaced,
        committed: false,
    });
    world::save(root, &world)?;
    // The new versions are written at the next start, where none is.
    for path in &staged.take {
        if let Some(file) = inside(&server, path) {
            fs::remove_file(file).map_err(|e| disk(&e))?;
        }
    }
    let mut pack = staged.pack;
    pack.server_manifest = server_manifest;
    world.generations.push(now);
    if let Some(backup) = backup {
        world.backup_generations.insert(backup, staged.from);
    }
    world.pack = Some(pack);
    if let Some(update) = world.update.as_mut() {
        update.committed = true;
    }
    world.mutation = None;
    tidy(root, &mut world);
    world::save(root, &world)?;
    Ok(world)
}

/// The world started with its new generation: the update is done.
pub fn confirm(root: &DataRoot, id: WorldId) -> LauncherResult<()> {
    let mut world = world::load(root, id)?;
    if world.update.take().is_some() {
        tidy(root, &mut world);
        world::save(root, &world)?;
    }
    Ok(())
}

/// Whether the world waits for its first start after an update.
pub fn pending(world: &WorldFile) -> bool {
    world.update.as_ref().is_some_and(|u| u.committed)
}

/// The settings an update replaced, back on the server.
fn put_back(root: &DataRoot, world: &WorldFile, pending: &Pending) -> LauncherResult<()> {
    let server = root.world_server_dir(world.id);
    let store = root.world_pack_dir(world.id);
    for kept in &pending.replaced {
        let target = make_inside(&server, &kept.path)?;
        if fs::symlink_metadata(&target).is_ok() {
            fs::remove_file(&target).map_err(|e| disk(&e))?;
        }
        pack::copy_verified(&store.join(kept.sha256.as_str()), &target, &kept.sha256)?;
    }
    Ok(())
}

/// The world did not start with its new generation: the backup made before
/// the update, the generation before it and the settings it replaced come
/// back together (blocking). False when no update waits.
pub fn roll_back(root: &DataRoot, id: WorldId) -> LauncherResult<bool> {
    let mut world = world::load(root, id)?;
    let Some(pending) = world.update.clone().filter(|u| u.committed) else {
        return Ok(false);
    };
    let Some(at) = world
        .generations
        .iter()
        .position(|g| g.generation == pending.from)
    else {
        return Err(LauncherError::new(
            ErrorCode::RepairRequired,
            "The world's earlier mods are gone. Restore one of its backups.",
        ));
    };
    if let Some(backup) = &pending.backup {
        let store = safety::store(root);
        let snapshot = store.load(backup)?;
        world.mutation = Some(Mutation {
            kind: MutationKind::Restore,
            operation_id: Uuid::new_v4().to_string(),
            started_at: chrono::Utc::now(),
        });
        world::save(root, &world)?;
        let save = safety::save_dir(root, id);
        if save.exists() {
            store.restore_replacing(&snapshot, &save)?;
        } else {
            store.restore_into(&snapshot, &save)?;
        }
    }
    put_back(root, &world, &pending)?;
    let earlier = world.generations.remove(at);
    world.pack = Some(earlier);
    world.update = None;
    world.mutation = None;
    tidy(root, &mut world);
    world::save(root, &world)?;
    Ok(true)
}

/// Notes the generation a new backup of the world was made with.
pub fn record_backup(world: &mut WorldFile, backup: &str) {
    if let Some(pack) = &world.pack {
        world
            .backup_generations
            .insert(backup.to_owned(), pack.generation);
    }
}

/// After `backup` was restored: the generation it was made with is the
/// world's again. An update waiting for its first start is over.
pub fn bring_back(root: &DataRoot, world: &mut WorldFile, backup: &str) {
    world.update = None;
    let Some(wanted) = world.backup_generations.get(backup).copied() else {
        return;
    };
    let Some(now) = world.pack.clone() else {
        return;
    };
    if now.generation == wanted {
        return;
    }
    if let Some(at) = world
        .generations
        .iter()
        .position(|g| g.generation == wanted)
    {
        let earlier = world.generations.remove(at);
        world.generations.push(now);
        world.generations.sort_by_key(|g| g.generation);
        world.pack = Some(earlier);
    }
    tidy(root, world);
}

/// Keeps the generations a backup or a pending update needs, and only
/// their files and archives in `pack/`.
pub fn tidy(root: &DataRoot, world: &mut WorldFile) {
    let backups: HashSet<String> = safety::list(root, world.id)
        .into_iter()
        .map(|b| b.id)
        .collect();
    world
        .backup_generations
        .retain(|id, _| backups.contains(id));
    let current = world.pack.as_ref().map(|p| p.generation);
    let needed: HashSet<u32> = world
        .backup_generations
        .values()
        .copied()
        .chain(world.update.as_ref().map(|u| u.from))
        .collect();
    world
        .generations
        .retain(|g| needed.contains(&g.generation) && Some(g.generation) != current);
    world.generations.sort_by_key(|g| g.generation);
    world.generations.dedup_by_key(|g| g.generation);
    let extra = world.generations.len().saturating_sub(MAX_GENERATIONS);
    world.generations.drain(..extra);

    let kept: Vec<&WorldPack> = world.pack.iter().chain(&world.generations).collect();
    let files: HashSet<&str> = kept
        .iter()
        .flat_map(|g| g.files.iter().map(|f| f.sha256.as_str()))
        .chain(
            world
                .update
                .iter()
                .flat_map(|u| u.replaced.iter().map(|r| r.sha256.as_str())),
        )
        .collect();
    let archives: HashSet<String> = kept
        .iter()
        .flat_map(|g| [g.archive_name(), g.room_archive_name()])
        .collect();
    let Ok(entries) = fs::read_dir(root.world_pack_dir(world.id)) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let by_content = name.len() == 64 && name.bytes().all(|b| b.is_ascii_hexdigit());
        let archive = (name.starts_with("generation-") || name.starts_with("room-"))
            && name.ends_with(".mrpack");
        let unused = (by_content && !files.contains(name.as_str()))
            || (archive && !archives.contains(&name));
        if unused && entry.file_type().is_ok_and(|t| t.is_file()) {
            let _ = fs::remove_file(entry.path());
        }
    }
}

#[cfg(test)]
#[path = "update_tests.rs"]
mod tests;
