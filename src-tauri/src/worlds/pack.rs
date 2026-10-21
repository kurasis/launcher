//! The mods of a world with mods (PLAN W12).
//!
//! A world made from an installation with a mod loader keeps one generation
//! of its mods: the loader and its exact version, and every file by SHA-256
//! with the side it runs on. A generation never changes; an update makes the
//! next one. The files are copied from the installation once:
//!
//! ```text
//! worlds/<worldId>/pack/<sha256>                  each file, by its content
//! worlds/<worldId>/pack/generation-<n>.mrpack     the generation as a .mrpack
//! worlds/<worldId>/pack/room-<n>.mrpack           what guests install
//! ```
//!
//! Mods come only from the catalog: each jar in `mods/` is looked up by its
//! SHA-256, and one the catalog does not have, or holds back as harmful,
//! stops the world from being made. The player's settings come along by the
//! catalog's rules for own packs (text, no code or programs, bounded in size);
//! `options.txt`, saves, screenshots and logs never do, nor the launcher's own
//! skins mod, which every game gets at launch.
//!
//! The side of a mod is read from data, never from its name: the jar's own
//! description first (`environment` of `fabric.mod.json`, of
//! `quilt.mod.json`), then the catalog's `client_side` and `server_side`, else
//! both. Before each start the server's `mods/` is made to hold exactly the
//! generation's server mods, and the host's room installation its client
//! mods; settings are written only where the folder has none yet, so what
//! the server or the player changed stays.
//!
//! Guests get the room pack: the generation's client side only, published
//! to the catalog as a room pack (never shown there) when the world is first
//! hosted with it. The world's session names its version and SHA-256, and a
//! guest installs it into their room installation like any catalog pack.
//!
//! Earlier generations stay while a backup was made with them; an update
//! and its way back are in [`super::update`].

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::Digest as _;
use uuid::Uuid;

use super::service::SessionPack;
use super::world::WorldFile;
use crate::error::{ErrorCode, LauncherError, LauncherResult};
use crate::loader::{LoaderChoice, LoaderKind};
use crate::mods::pack::{ExportChoice, gather, hashes_of, loader_dependency, pack_path};
use crate::mods::publish::{MAX_OVERRIDE, MAX_OVERRIDES, Published, overridable};
use crate::mods::{CDN_HOST, Catalog, FILES_HOST, Project};
use crate::storage::paths::{DataRoot, InstanceId, Sha1, Sha256, WorldId};

/// Mods one world takes at most.
pub const MAX_MODS: usize = 1000;
/// Bound for a mod's own description inside its jar.
const MAX_META: u64 = 1024 * 1024;
/// Names a refusal lists before it stops.
const MAX_NAMED: usize = 20;
/// The `.mrpack` index, and the texts the catalog asks of an own pack.
const INDEX: &str = "modrinth.index.json";
const ROOM_META: &str = "catalog.json";

/// The releases whose server with this loader passed the live test in CI
/// (`World server fabric <version>`); others are refused before anything is
/// downloaded. Snapshots never: loaders and mods rarely follow them.
const TESTED: &[(LoaderKind, &[&str])] = &[(LoaderKind::Fabric, &["1.20.1", "1.21.1", "26.3"])];

/// Whether worlds with this loader are offered for this release.
pub fn tested(kind: LoaderKind, minecraft: &str) -> bool {
    TESTED
        .iter()
        .any(|(k, versions)| *k == kind && versions.contains(&minecraft))
}

/// Refuses a loader or release worlds with mods do not support yet.
pub fn check_supported(loader: &LoaderChoice, minecraft: &str) -> LauncherResult<()> {
    loader.validate()?;
    if !TESTED.iter().any(|(k, _)| *k == loader.kind) {
        return Err(LauncherError::invalid_input(
            "Worlds with this mod loader come later; Fabric works now.",
        ));
    }
    if !tested(loader.kind, minecraft) {
        return Err(LauncherError::templated(
            ErrorCode::InvalidInput,
            "Worlds with mods are available for Minecraft {versions} for now.",
            crate::error::params([("versions", supported_versions(loader.kind).into())]),
        ));
    }
    Ok(())
}

fn supported_versions(kind: LoaderKind) -> String {
    TESTED
        .iter()
        .filter(|(k, _)| *k == kind)
        .flat_map(|(_, v)| v.iter().copied())
        .collect::<Vec<_>>()
        .join(", ")
}

/// Where a file runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    Client,
    Server,
    Both,
}

impl Side {
    pub fn on_server(self) -> bool {
        self != Self::Client
    }

    pub fn on_client(self) -> bool {
        self != Self::Server
    }
}

/// What told a file's side.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SideFrom {
    /// The mod's own description in its jar.
    Jar,
    /// The catalog's `client_side` and `server_side`.
    Catalog,
    /// Nothing said: both sides.
    Default,
}

/// One file of a generation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackFile {
    /// Relative to the game folder: `mods/<name>.jar`, or a setting such as
    /// `config/<name>`.
    pub path: String,
    pub sha1: Sha1,
    pub sha256: Sha256,
    pub sha512: String,
    pub size: u64,
    pub side: Side,
    pub side_from: SideFrom,
    /// The catalog's project and version of a mod.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version_id: Option<String>,
}

impl PackFile {
    pub fn is_mod(&self) -> bool {
        mod_name(&self.path).is_some()
    }
}

/// One generation of a world's mods.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorldPack {
    /// 1 for the first; each update adds one.
    pub generation: u32,
    pub loader: LoaderChoice,
    pub files: Vec<PackFile>,
    pub made_at: DateTime<Utc>,
    /// The generation's `.mrpack`, once written.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archive_sha256: Option<Sha256>,
    /// The loader's server manifest, pinned when the server is installed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_manifest: Option<Sha1>,
    /// The room pack in the catalog, once published.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub room: Option<RoomPack>,
    /// The installation it was made from, offered again for an update.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<InstanceId>,
}

/// A generation's room pack as the catalog keeps it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoomPack {
    pub version: String,
    pub sha256: Sha256,
}

impl WorldPack {
    pub fn mods(&self) -> impl Iterator<Item = &PackFile> {
        self.files.iter().filter(|f| f.is_mod())
    }

    pub fn archive_name(&self) -> String {
        format!("generation-{}.mrpack", self.generation)
    }

    pub fn room_archive_name(&self) -> String {
        format!("room-{}.mrpack", self.generation)
    }
}

/// The name of a mod jar at `mods/<name>.jar`, the only place loaders read
/// them from.
fn mod_name(path: &str) -> Option<&str> {
    let name = path.strip_prefix("mods/")?;
    (!name.contains('/') && name.to_ascii_lowercase().ends_with(".jar")).then_some(name)
}

/// A setting a generation takes: in a folder, by the catalog's rules for own
/// packs; never the player's saves or the look of their game.
pub(super) fn setting(path: &str) -> bool {
    let top = path.split('/').next().unwrap_or_default();
    path.contains('/')
        && !matches!(top, "saves" | "resourcepacks" | "shaderpacks")
        && overridable(path)
}

/// Whether a generation may hold this path: a mod jar or a setting.
fn allowed(path: &str) -> bool {
    pack_path(path) && (mod_name(path).is_some() || setting(path))
}

/// The side a mod states in its jar: `client`, `server` (Fabric) or
/// `dedicated_server` (Quilt); `*` or nothing says nothing.
pub fn jar_side(path: &Path) -> Option<Side> {
    let mut zip = zip::ZipArchive::new(fs::File::open(path).ok()?).ok()?;
    let mut text = |name: &str| -> Option<Value> {
        let entry = zip.by_name(name).ok()?;
        if entry.size() > MAX_META {
            return None;
        }
        let mut out = String::new();
        entry.take(MAX_META).read_to_string(&mut out).ok()?;
        serde_json::from_str(&out).ok()
    };
    let environment = match text("quilt.mod.json") {
        Some(quilt) => quilt["minecraft"]["environment"]
            .as_str()
            .map(str::to_owned),
        None => text("fabric.mod.json")?["environment"]
            .as_str()
            .map(str::to_owned),
    }?;
    match environment.as_str() {
        "client" => Some(Side::Client),
        "server" | "dedicated_server" => Some(Side::Server),
        _ => None,
    }
}

/// The side the catalog gives a project, when it knows.
pub fn catalog_side(project: &Project) -> Option<Side> {
    let runs = |s: &str| match s {
        "required" | "optional" => Some(true),
        "unsupported" => Some(false),
        _ => None,
    };
    match (runs(&project.client_side), runs(&project.server_side)) {
        (Some(true), Some(true)) => Some(Side::Both),
        (Some(true) | None, Some(false)) => Some(Side::Client),
        (Some(false), Some(true) | None) => Some(Side::Server),
        _ => None,
    }
}

/// A file a world would take from an installation.
struct Candidate {
    path: String,
    full: PathBuf,
    size: u64,
    sha1: String,
    sha512: String,
    sha256: String,
    jar_side: Option<Side>,
}

fn disk(error: &std::io::Error) -> LauncherError {
    LauncherError::from_io(error, "Copying the world's mods")
}

/// The installation's mods and settings (blocking).
fn candidates(game: &Path) -> LauncherResult<Vec<Candidate>> {
    let mut out = Vec::new();
    let (mut mods, mut settings_bytes) = (0usize, 0u64);
    for g in gather(game, ExportChoice::default())? {
        if let Some(name) = mod_name(&g.path) {
            if crate::skins_mod::is_ours(name) {
                continue;
            }
            mods += 1;
            if mods > MAX_MODS {
                return Err(LauncherError::invalid_input(
                    "A world takes at most 1000 mods.",
                ));
            }
            let (sha1, sha512, sha256) = match g.hashes {
                Some(hashes) => hashes,
                None => hashes_of(&g.full).map_err(|e| disk(&e))?,
            };
            out.push(Candidate {
                jar_side: jar_side(&g.full),
                path: g.path,
                full: g.full,
                size: g.size,
                sha1,
                sha512,
                sha256,
            });
        } else if allowed(&g.path)
            && g.size <= MAX_OVERRIDE
            && settings_bytes + g.size <= MAX_OVERRIDES
        {
            settings_bytes += g.size;
            let (sha1, sha512, sha256) = hashes_of(&g.full).map_err(|e| disk(&e))?;
            out.push(Candidate {
                jar_side: None,
                path: g.path,
                full: g.full,
                size: g.size,
                sha1,
                sha512,
                sha256,
            });
        }
    }
    Ok(out)
}

/// A generation made from an installation, with where its files are now.
pub struct Built {
    pub pack: WorldPack,
    pub(super) sources: Vec<PathBuf>,
}

fn named(names: &[&str]) -> String {
    let mut list: Vec<&str> = names.iter().copied().take(MAX_NAMED).collect();
    if names.len() > MAX_NAMED {
        list.push("…");
    }
    list.join(", ")
}

/// Publishes a room pack's archive to the catalog, as a new version of the
/// world's room pack when it has one.
pub type PublishRoom = Box<
    dyn FnOnce(
            PathBuf,
            Option<String>,
        ) -> crate::install::service::BoxFuture<'static, LauncherResult<Published>>
        + Send,
>;

/// The room pack of a world with mods, published once per generation; what
/// its sessions name. None for a world without mods.
pub async fn publish_room(
    root: &DataRoot,
    world_id: WorldId,
    publish: PublishRoom,
) -> LauncherResult<Option<SessionPack>> {
    let world = super::world::load(root, world_id)?;
    let Some(generation) = world.pack.clone() else {
        return Ok(None);
    };
    if let Some(room) = &generation.room {
        return Ok(Some(SessionPack {
            version: room.version.clone(),
            sha256: room.sha256.to_string(),
        }));
    }
    let (archive, sha256) = {
        let (root, world, generation) = (root.clone(), world.clone(), generation.clone());
        tokio::task::spawn_blocking(move || room_archive(&root, &world, &generation))
            .await
            .map_err(|_| LauncherError::internal("The world's modpack could not be written."))??
    };
    let published = publish(archive.clone(), world.room_pack.clone()).await;
    let _ = fs::remove_file(&archive);
    let (project, version) = match published? {
        Published::Published {
            project_id,
            version_id,
            ..
        } => (project_id, version_id),
        Published::Refused { paths, .. } => return Err(refused_by_catalog(&paths)),
    };
    let mut world = super::world::load(root, world_id)?;
    let Some(now) = world
        .pack
        .as_mut()
        .filter(|p| p.generation == generation.generation)
    else {
        return Err(LauncherError::new(
            ErrorCode::AppBusy,
            "The world's mods changed while they were sent. Open the world again.",
        ));
    };
    now.room = Some(RoomPack {
        version: version.clone(),
        sha256: sha256.clone(),
    });
    world.room_pack = Some(project);
    super::world::save(root, &world)?;
    Ok(Some(SessionPack {
        version,
        sha256: sha256.to_string(),
    }))
}

/// What a guest agrees to before the room's mods are installed: mods are
/// programs that run on their computer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RoomConsent {
    /// The room pack, to send back with the player's yes.
    pub sha256: String,
    pub mods: u32,
    /// Mods this installation does not have yet, their names and bytes.
    pub new_mods: u32,
    pub names: Vec<String>,
    pub bytes: u64,
}

/// Names a consent lists before it stops.
const CONSENT_NAMES: usize = 30;

/// What a guest's room installation needs for the room pack `pack`, which
/// the catalog describes as `info`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RoomStep {
    /// It holds this pack already.
    Installed,
    /// The player's word first: mods the installation does not have yet.
    Ask(RoomConsent),
    /// Installing it: the player agreed to this pack, or it brings no mod
    /// they have not agreed to before.
    Install,
}

/// The agreement holds for one pack: an update with new mods asks again,
/// one that only drops or keeps mods (or changes settings) does not.
pub fn room_step(
    info: &crate::mods::pack::PackInfo,
    installed: Option<&crate::mods::pack::PackLock>,
    pack: &SessionPack,
    accept: Option<&str>,
) -> RoomStep {
    if installed.is_some_and(|p| p.version_id == pack.version) {
        return RoomStep::Installed;
    }
    let had: HashSet<&str> = installed
        .iter()
        .flat_map(|p| p.files.iter())
        .map(|e| e.sha256.as_str())
        .collect();
    let new_mods: Vec<&crate::mods::pack::PackFile> = info
        .files
        .iter()
        .filter(|f| f.path.starts_with("mods/"))
        .filter(|f| f.sha256.as_deref().is_none_or(|s| !had.contains(s)))
        .collect();
    if new_mods.is_empty() || accept == Some(pack.sha256.as_str()) {
        return RoomStep::Install;
    }
    let mut names: Vec<String> = new_mods
        .iter()
        .filter_map(|f| f.path.rsplit('/').next())
        .map(str::to_owned)
        .collect();
    names.sort_unstable_by_key(|n| n.to_lowercase());
    names.truncate(CONSENT_NAMES);
    RoomStep::Ask(RoomConsent {
        sha256: pack.sha256.clone(),
        mods: info.mods,
        new_mods: u32::try_from(new_mods.len()).unwrap_or(u32::MAX),
        names,
        bytes: new_mods.iter().map(|f| f.size).sum(),
    })
}

/// The catalog refused the room pack for these files: they were taken out
/// of the catalog or held back since the world was made.
pub fn refused_by_catalog(paths: &[String]) -> LauncherError {
    let names: Vec<&str> = paths
        .iter()
        .map(|p| p.rsplit('/').next().unwrap_or(p))
        .collect();
    LauncherError::templated(
        ErrorCode::InvalidInput,
        "The catalog no longer takes these files of the world: {files}. Friends cannot \
         install its mods until it is made again without them.",
        crate::error::params([("files", named(&names).into())]),
    )
}

/// The first generation of a world made from the installation in `game`,
/// which runs `loader`. `harmful` holds the SHA-1 of files the catalog
/// holds back.
pub async fn build<C: Catalog>(
    catalog: &C,
    harmful: &HashSet<String>,
    game: &Path,
    loader: &LoaderChoice,
) -> LauncherResult<Built> {
    let found = {
        let game = game.to_path_buf();
        tokio::task::spawn_blocking(move || candidates(&game))
            .await
            .map_err(|_| LauncherError::internal("A file check stopped unexpectedly."))??
    };
    let jars: Vec<&Candidate> = found
        .iter()
        .filter(|c| mod_name(&c.path).is_some())
        .collect();
    let bad: Vec<&str> = jars
        .iter()
        .filter(|c| harmful.contains(&c.sha1))
        .filter_map(|c| mod_name(&c.path))
        .collect();
    if !bad.is_empty() {
        return Err(LauncherError::templated(
            ErrorCode::InvalidInput,
            "The catalog holds back these mods as harmful: {files}. Remove them from the \
             installation.",
            crate::error::params([("files", named(&bad).into())]),
        ));
    }
    let mut known: HashMap<String, (String, String)> = HashMap::new();
    let hashes: Vec<String> = jars.iter().map(|c| c.sha256.clone()).collect();
    for chunk in hashes.chunks(200) {
        for f in catalog.lookup(chunk).await? {
            known.insert(f.hash.to_ascii_lowercase(), (f.project_id, f.version_id));
        }
    }
    let missing: Vec<&str> = jars
        .iter()
        .filter(|c| !known.contains_key(&c.sha256))
        .filter_map(|c| mod_name(&c.path))
        .collect();
    if !missing.is_empty() {
        return Err(LauncherError::templated(
            ErrorCode::InvalidInput,
            "These mods are not in the catalog: {files}. Add them on the Mods tab by a link, \
             or remove them from the installation.",
            crate::error::params([("files", named(&missing).into())]),
        ));
    }

    let mut projects: HashMap<String, Option<Side>> = HashMap::new();
    let mut files = Vec::with_capacity(found.len());
    let mut sources = Vec::with_capacity(found.len());
    for c in found {
        let catalog_ids = known.get(&c.sha256).cloned();
        let (side, side_from) = match (c.jar_side, &catalog_ids) {
            (Some(side), _) => (side, SideFrom::Jar),
            (None, Some((project, _))) => {
                let side = match projects.get(project) {
                    Some(side) => *side,
                    None => {
                        let side = catalog
                            .project(project)
                            .await
                            .ok()
                            .and_then(|p| catalog_side(&p));
                        projects.insert(project.clone(), side);
                        side
                    }
                };
                side.map_or((Side::Both, SideFrom::Default), |s| (s, SideFrom::Catalog))
            }
            (None, None) => (Side::Both, SideFrom::Default),
        };
        files.push(PackFile {
            path: c.path,
            sha1: Sha1::parse(&c.sha1)?,
            sha256: Sha256::parse(&c.sha256)?,
            sha512: c.sha512,
            size: c.size,
            side,
            side_from,
            project_id: catalog_ids.as_ref().map(|(p, _)| p.clone()),
            version_id: catalog_ids.map(|(_, v)| v),
        });
        sources.push(c.full);
    }
    Ok(Built {
        pack: WorldPack {
            generation: 1,
            loader: loader.clone(),
            files,
            made_at: Utc::now(),
            archive_sha256: None,
            server_manifest: None,
            room: None,
            source: None,
        },
        sources,
    })
}

fn changed() -> LauncherError {
    LauncherError::new(
        ErrorCode::HashMismatch,
        "A mod file changed while the world was being made. Try again.",
    )
}

fn damaged() -> LauncherError {
    LauncherError::new(
        ErrorCode::RepairRequired,
        "This world's mods are damaged. Make the world again from its installation.",
    )
}

/// Copies `from` to `to` through a temporary file beside it, and keeps it
/// only when it has this SHA-256.
pub(super) fn copy_verified(from: &Path, to: &Path, sha256: &Sha256) -> LauncherResult<()> {
    let parent = to.parent().ok_or_else(damaged)?;
    let temp = parent.join(format!(".world-file.tmp-{}", Uuid::new_v4().simple()));
    let result = (|| -> LauncherResult<bool> {
        let mut source = fs::File::open(from).map_err(|e| disk(&e))?;
        let mut target = fs::File::create_new(&temp).map_err(|e| disk(&e))?;
        let mut hasher = sha2::Sha256::new();
        let mut buffer = vec![0u8; 256 * 1024];
        loop {
            let read = source.read(&mut buffer).map_err(|e| disk(&e))?;
            if read == 0 {
                break;
            }
            hasher.update(&buffer[..read]);
            target.write_all(&buffer[..read]).map_err(|e| disk(&e))?;
        }
        target.sync_all().map_err(|e| disk(&e))?;
        Ok(hex::encode(hasher.finalize()) == sha256.as_str())
    })();
    match result {
        Ok(true) => crate::storage::atomic::rename(&temp, to).map_err(|e| {
            let _ = fs::remove_file(&temp);
            disk(&e)
        }),
        Ok(false) => {
            let _ = fs::remove_file(&temp);
            Err(changed())
        }
        Err(error) => {
            let _ = fs::remove_file(&temp);
            Err(error)
        }
    }
}

/// Whether a regular file has this size and SHA-256.
pub(super) fn holds(path: &Path, size: u64, sha256: &Sha256) -> bool {
    fs::symlink_metadata(path).is_ok_and(|m| m.is_file() && m.len() == size)
        && hashes_of(path).is_ok_and(|(_, _, sha)| sha == sha256.as_str())
}

/// Copies a built generation's files into the world and writes its
/// `.mrpack` (blocking). Returns the generation with the archive's hash.
pub fn store(root: &DataRoot, world: &WorldFile, built: Built) -> LauncherResult<WorldPack> {
    let dir = root.world_pack_dir(world.id);
    fs::create_dir_all(&dir).map_err(|e| disk(&e))?;
    for (file, source) in built.pack.files.iter().zip(&built.sources) {
        let target = dir.join(file.sha256.as_str());
        if !holds(&target, file.size, &file.sha256) {
            copy_verified(source, &target, &file.sha256)?;
        }
    }
    let mut pack = built.pack;
    pack.archive_sha256 = Some(write_archive(root, world, &pack, Archive::Generation)?);
    Ok(pack)
}

/// The `.mrpack` index of a generation: mods by their hashes from the
/// catalog's files, with the side each runs on.
pub fn index(world: &WorldFile, pack: &WorldPack) -> Value {
    index_of(world, pack, Archive::Generation)
}

/// What an archive of a generation holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Archive {
    /// All of it.
    Generation,
    /// What guests install: the client side, and the texts the catalog asks
    /// of a pack.
    Room,
}

impl Archive {
    fn holds(self, file: &PackFile) -> bool {
        self == Self::Generation || file.side.on_client()
    }
}

/// Names a room pack in the catalog; never shown there.
const ROOM_TITLE: &str = "World pack";

fn index_of(world: &WorldFile, pack: &WorldPack, archive: Archive) -> Value {
    let mut dependencies = serde_json::Map::new();
    dependencies.insert("minecraft".to_owned(), world.minecraft.clone().into());
    let (name, version) = loader_dependency(&world.minecraft, &pack.loader);
    dependencies.insert(name.to_owned(), version.into());
    let env = |on: bool| if on { "required" } else { "unsupported" };
    let files: Vec<Value> = pack
        .mods()
        .filter(|f| archive.holds(f))
        .map(|f| {
            json!({
                "path": f.path,
                "hashes": { "sha1": f.sha1.as_str(), "sha512": f.sha512 },
                "env": { "client": env(f.side.on_client()), "server": env(f.side.on_server()) },
                "downloads": [
                    format!("https://{CDN_HOST}/f/{}", f.sha256.as_str()),
                    format!("https://{FILES_HOST}/f/{}", f.sha256.as_str()),
                ],
                "fileSize": f.size,
            })
        })
        .collect();
    json!({
        "formatVersion": 1,
        "game": "minecraft",
        "versionId": format!("generation-{}", pack.generation),
        "name": match archive {
            Archive::Generation => world.name.as_str(),
            Archive::Room => ROOM_TITLE,
        },
        "files": files,
        "dependencies": dependencies,
    })
}

/// Writes the room pack's `.mrpack` beside the generation's files (blocking);
/// its path and SHA-256.
pub fn room_archive(
    root: &DataRoot,
    world: &WorldFile,
    pack: &WorldPack,
) -> LauncherResult<(PathBuf, Sha256)> {
    let sha256 = write_archive(root, world, pack, Archive::Room)?;
    Ok((
        root.world_pack_dir(world.id).join(pack.room_archive_name()),
        sha256,
    ))
}

/// Writes an archive of the generation beside its files; settings go into
/// `overrides/`. Returns its SHA-256.
fn write_archive(
    root: &DataRoot,
    world: &WorldFile,
    pack: &WorldPack,
    archive: Archive,
) -> LauncherResult<Sha256> {
    let dir = root.world_pack_dir(world.id);
    let to = dir.join(match archive {
        Archive::Generation => pack.archive_name(),
        Archive::Room => pack.room_archive_name(),
    });
    let zipped = |_| LauncherError::internal("The world's modpack could not be written.");
    let temp = dir.join(format!(".archive.tmp-{}", Uuid::new_v4().simple()));
    let result = (|| -> LauncherResult<()> {
        let file = fs::File::create_new(&temp).map_err(|e| disk(&e))?;
        let mut zip = zip::ZipWriter::new(std::io::BufWriter::new(file));
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        zip.start_file(INDEX, options).map_err(zipped)?;
        zip.write_all(index_of(world, pack, archive).to_string().as_bytes())
            .map_err(|e| disk(&e))?;
        if archive == Archive::Room {
            // A number of its own each time: a publish whose answer was
            // lost never blocks the next one.
            let number = format!(
                "{}-{}",
                pack.generation,
                &Uuid::new_v4().simple().to_string()[..8]
            );
            let meta = json!({ "title": ROOM_TITLE, "version": number });
            zip.start_file(ROOM_META, options).map_err(zipped)?;
            zip.write_all(meta.to_string().as_bytes())
                .map_err(|e| disk(&e))?;
        }
        for file in pack
            .files
            .iter()
            .filter(|f| !f.is_mod() && archive.holds(f))
        {
            zip.start_file(format!("overrides/{}", file.path), options)
                .map_err(zipped)?;
            let mut source =
                fs::File::open(dir.join(file.sha256.as_str())).map_err(|e| disk(&e))?;
            std::io::copy(&mut source, &mut zip).map_err(|e| disk(&e))?;
        }
        let mut out = zip.finish().map_err(zipped)?;
        out.flush().map_err(|e| disk(&e))?;
        out.into_inner()
            .map_err(|e| disk(e.error()))?
            .sync_all()
            .map_err(|e| disk(&e))
    })();
    if let Err(error) = result {
        let _ = fs::remove_file(&temp);
        return Err(error);
    }
    let (_, _, sha256) = hashes_of(&temp).map_err(|e| disk(&e))?;
    crate::storage::atomic::rename(&temp, &to).map_err(|e| {
        let _ = fs::remove_file(&temp);
        disk(&e)
    })?;
    Sha256::parse(&sha256)
}

/// Makes `dest` hold the generation's files for one side: `mods/` exactly
/// its mods (besides files `keep` names), settings where there are none yet.
/// Every file is checked by SHA-256 on the way (blocking).
fn place(
    root: &DataRoot,
    owner: WorldId,
    pack: &WorldPack,
    dest: &Path,
    wanted: fn(Side) -> bool,
    keep: fn(&str) -> bool,
) -> LauncherResult<()> {
    if pack.files.iter().any(|f| !allowed(&f.path)) {
        return Err(damaged());
    }
    let store = root.world_pack_dir(owner);
    let mods_dir = dest.join("mods");
    if fs::symlink_metadata(&mods_dir).is_ok_and(|m| !m.is_dir()) {
        return Err(damaged());
    }
    fs::create_dir_all(&mods_dir).map_err(|e| disk(&e))?;
    let ours: HashMap<&str, &PackFile> = pack
        .mods()
        .filter(|f| wanted(f.side))
        .filter_map(|f| Some((mod_name(&f.path)?, f)))
        .collect();
    for entry in fs::read_dir(&mods_dir).map_err(|e| disk(&e))?.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Ok(meta) = fs::symlink_metadata(entry.path()) else {
            continue;
        };
        if !meta.is_dir() && !ours.contains_key(name.as_str()) && !keep(&name) {
            fs::remove_file(entry.path()).map_err(|e| disk(&e))?;
        }
    }
    for (name, file) in &ours {
        let target = mods_dir.join(name);
        if !holds(&target, file.size, &file.sha256) {
            copy_verified(&store.join(file.sha256.as_str()), &target, &file.sha256)
                .map_err(|_| damaged())?;
        }
    }
    for file in pack.files.iter().filter(|f| !f.is_mod() && wanted(f.side)) {
        let Some((name, folders)) = file
            .path
            .split('/')
            .collect::<Vec<_>>()
            .split_last()
            .map(|(name, folders)| (*name, folders.to_vec()))
        else {
            return Err(damaged());
        };
        // Every folder on the way is a real one, never a link out of `dest`.
        let mut at = dest.to_path_buf();
        for folder in folders {
            at = at.join(folder);
            match fs::symlink_metadata(&at) {
                Ok(meta) if meta.is_dir() => {}
                Ok(_) => return Err(damaged()),
                Err(_) => fs::create_dir(&at).map_err(|e| disk(&e))?,
            }
        }
        let target = at.join(name);
        if fs::symlink_metadata(&target).is_ok() {
            continue;
        }
        copy_verified(&store.join(file.sha256.as_str()), &target, &file.sha256)
            .map_err(|_| damaged())?;
    }
    Ok(())
}

/// Before a start: the server's `mods/` holds exactly the generation's server
/// mods. A world without mods has none.
pub fn place_server(root: &DataRoot, world: &WorldFile) -> LauncherResult<()> {
    match &world.pack {
        Some(pack) => place(
            root,
            world.pack_owner(),
            pack,
            &root.world_server_dir(world.id),
            Side::on_server,
            |_| false,
        ),
        None => Ok(()),
    }
}

/// The host's room installation in `game` holds the generation's client mods,
/// beside the launcher's own skins mod.
pub fn place_client(root: &DataRoot, world: &WorldFile, game: &Path) -> LauncherResult<()> {
    match &world.pack {
        Some(pack) => place(
            root,
            world.id,
            pack,
            game,
            Side::on_client,
            crate::skins_mod::is_ours,
        ),
        None => Ok(()),
    }
}

#[cfg(test)]
#[path = "pack_tests.rs"]
mod tests;
