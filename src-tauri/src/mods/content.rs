//! What the catalog puts into an installation's game folder (D-066): mods,
//! resource packs, shaders and data packs, with the mods they need.
//!
//! - A plan is made in the core from the catalog's own answers: the version's
//!   game versions and loaders must fit the installation (a mod strictly; a
//!   resource pack, shader or data pack with a warning), required
//!   dependencies are added to the end of the chain (the version named, else
//!   the newest release that fits; 200 files at most, or no plan), and
//!   an incompatible project already installed stops the plan.
//! - Then the set is checked as the mods' own jars say (`mod-meta`, read by
//!   the catalog for its files and here for the folder): a dependency is a
//!   version whose jar runs on the installation's loader version, a mod only
//!   a jar names is looked up by its ID, and a mod there already that is too
//!   old for a new one is updated. What is left is a warning, not a block.
//! - Files the catalog does not keep yet are asked for and waited on; every
//!   file comes from files-mods.lapi.sh through the download executor and is
//!   checked against its SHA-256 before it is renamed into place.
//! - `content.lock.json` next to the installation records what the launcher
//!   put there; an update replaces only its own file. A file the player put
//!   there is never overwritten; the catalog can recognize it by its SHA-1.
//! - Paths from the interface are relative to the game folder and limited to
//!   `mods/`, `resourcepacks/`, `shaderpacks/` and `saves/<world>/datapacks/`.
//!   A file is turned off by adding `.disabled` to its name.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};

use super::{Catalog, Found, Version, our_file};
use crate::download::{
    CancelToken, Digest, DownloadProgress, DownloadRequest, Executor, ExecutorConfig, ProgressSink,
};
use crate::error::{ErrorCode, LauncherError, LauncherResult};
use crate::loader::LoaderKind;
use crate::storage::models::InstanceFile;
use crate::storage::paths::{DataRoot, InstanceId, Sha256};
use crate::sync::lock;

const LOCK_FILE: &str = "content.lock.json";
/// Files added with one install: the chosen one and all it needs. Each
/// project is taken once, so a loop of dependencies ends.
const MAX_PLAN: usize = 200;
/// Files listed per folder.
const MAX_LISTED: usize = 2000;
/// Files the catalog is asked to recognize at once.
const MAX_IDENTIFY: usize = 1000;
/// Files of one installation checked against the harmful list.
const MAX_HARMFUL_CHECKED: usize = 5000;
/// Larger files are not hashed to be recognized.
const MAX_HASHED: u64 = 512 * 1024 * 1024;
/// How long an install waits for the catalog to fetch a version's files.
const FETCH_WAIT: Duration = Duration::from_secs(20 * 60);
const FETCH_POLL: Duration = Duration::from_secs(3);
const DISABLED: &str = ".disabled";

fn busy() -> LauncherError {
    LauncherError::new(
        ErrorCode::InstanceBusy,
        "This installation is busy. Wait for the current operation to finish.",
    )
}

fn bad_path() -> LauncherError {
    LauncherError::invalid_input("This file is not in the installation's content folders.")
}

fn no_such_file() -> LauncherError {
    LauncherError::invalid_input("This file is no longer in the installation.")
}

fn instance_gone() -> LauncherError {
    LauncherError::new(
        ErrorCode::InstanceNotFound,
        "The installation no longer exists.",
    )
}

fn storage_gone() -> LauncherError {
    LauncherError::new(
        ErrorCode::StorageUnavailable,
        "Launcher data is unavailable.",
    )
}

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

/// A name the player could have given a file or world: one path component
/// that cannot leave its folder.
pub fn plain_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 255
        && name != "."
        && name != ".."
        && !name.starts_with('.')
        && !name.contains(['/', '\\', ':', '\0'])
        && !name.chars().any(char::is_control)
}

/// A file name from the catalog that is safe on every system: no
/// separators or characters Windows refuses, no reserved device name, not
/// ending in a dot or space, with an extension its folder takes.
pub fn catalog_file_name(name: &str, kind: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    let extension = match kind {
        "mod" => lower.ends_with(".jar"),
        "resourcepack" | "shader" | "datapack" => lower.ends_with(".zip"),
        _ => false,
    };
    let stem = name
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ((stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.len() == 4
            && stem.as_bytes()[3].is_ascii_digit());
    extension
        && plain_name(name)
        && name.chars().count() <= 200
        && !name.ends_with([' ', '.'])
        && !name.starts_with(' ')
        && !name.contains(['<', '>', '"', '|', '?', '*'])
        && !reserved
}

/// Where a kind of content goes, relative to the game folder.
fn folder_of(kind: &str, world: Option<&str>) -> LauncherResult<String> {
    match kind {
        "mod" => Ok("mods".to_owned()),
        "resourcepack" => Ok("resourcepacks".to_owned()),
        "shader" => Ok("shaderpacks".to_owned()),
        "datapack" => match world {
            Some(world) if plain_name(world) => Ok(format!("saves/{world}/datapacks")),
            _ => Err(LauncherError::invalid_input(
                "Choose the world the data pack goes into.",
            )),
        },
        "modpack" => Err(LauncherError::invalid_input(
            "Modpacks are installed as a new installation; this comes in a later version of the launcher.",
        )),
        _ => Err(LauncherError::invalid_input(
            "The launcher cannot add this kind of content to an installation yet.",
        )),
    }
}

/// A content path from the interface: relative to the game folder, in one of
/// the content folders, nothing that leaves it.
pub fn content_path(game: &Path, relative: &str) -> LauncherResult<PathBuf> {
    let parts: Vec<&str> = relative.split('/').collect();
    if !parts.iter().all(|p| plain_name(p) || matches!(*p, "saves")) {
        return Err(bad_path());
    }
    let fits = match parts.as_slice() {
        ["mods" | "resourcepacks" | "shaderpacks", name] => plain_name(name),
        ["saves", world, "datapacks", name] => plain_name(world) && plain_name(name),
        _ => false,
    };
    if !fits {
        return Err(bad_path());
    }
    let path = parts
        .iter()
        .fold(game.to_path_buf(), |p, part| p.join(part));
    if !crate::storage::paths::is_within(game, &path) {
        return Err(bad_path());
    }
    Ok(path)
}

/// The name a file has when it is on (without `.disabled`).
fn enabled_form(relative: &str) -> &str {
    relative.strip_suffix(DISABLED).unwrap_or(relative)
}

// ----- the lock -----

/// A file the launcher put into the game folder from the catalog.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LockedItem {
    /// Relative to the game folder, as when it is on.
    pub path: String,
    pub sha256: String,
    pub size: u64,
    pub project_id: String,
    pub version_id: String,
    pub kind: String,
    pub title: String,
    pub version_number: String,
    #[serde(default)]
    pub published: i64,
    #[serde(default)]
    pub icon: Option<String>,
    /// Added because another file needs it.
    #[serde(default)]
    pub dependency: bool,
    /// The projects its version requires (unknown in older locks).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requires: Option<Vec<String>>,
    pub installed_at: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContentLock {
    pub schema_version: u64,
    #[serde(default)]
    pub items: Vec<LockedItem>,
    /// The modpack this installation was made from (stage 3).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pack: Option<super::pack::PackLock>,
    /// The catalog's project this installation was published as, by the
    /// player (stage 6): the next publish adds a version to it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub published: Option<String>,
}

impl ContentLock {
    fn of_project(&self, project: &str) -> Option<&LockedItem> {
        self.items.iter().find(|i| i.project_id == project)
    }

    /// The project's file in `folder` (relative to the game folder).
    fn of_project_in(&self, project: &str, folder: &str) -> Option<&LockedItem> {
        self.items
            .iter()
            .find(|i| i.project_id == project && in_folder(&i.path, folder))
    }

    fn at(&self, path: &str) -> Option<&LockedItem> {
        self.items.iter().find(|i| i.path == path)
    }
}

/// Whether a path (relative to the game folder) lies right in `folder`.
fn in_folder(path: &str, folder: &str) -> bool {
    path.strip_prefix(folder)
        .and_then(|rest| rest.strip_prefix('/'))
        .is_some_and(|name| !name.contains('/'))
}

fn lock_path(root: &DataRoot, id: InstanceId) -> PathBuf {
    root.instance_dir(id).join(LOCK_FILE)
}

pub fn load_lock(root: &DataRoot, id: InstanceId) -> ContentLock {
    use crate::storage::document::{Loaded, load};
    match load::<ContentLock>(&lock_path(root, id), None) {
        Ok(Loaded::Valid(lock) | Loaded::RecoveredFromBackup { value: lock, .. }) => lock,
        _ => ContentLock {
            schema_version: crate::storage::document::SCHEMA_VERSION,
            items: Vec::new(),
            pack: None,
            published: None,
        },
    }
}

/// Saves the lock after a publish (the installation remembers its pack).
pub fn save_published(root: &DataRoot, id: InstanceId, lock: &ContentLock) -> LauncherResult<()> {
    save_lock(root, id, lock)
}

pub(super) fn save_lock(root: &DataRoot, id: InstanceId, lock: &ContentLock) -> LauncherResult<()> {
    crate::storage::document::save(&lock_path(root, id), lock, None)
        .map_err(|e| LauncherError::from_io(&e, "Saving the list of installed content"))
}

// ----- the installation's side -----

/// What an installation runs: its game version and loader.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub game: String,
    pub loader: Option<LoaderKind>,
    /// The loader's version, as the installation names it; fixed like the
    /// game version.
    pub loader_version: Option<String>,
}

impl Target {
    pub fn of(instance: &InstanceFile) -> Self {
        Self {
            game: instance.version_id.clone(),
            loader: instance.loader.as_ref().map(|l| l.kind),
            loader_version: instance.loader.as_ref().map(|l| l.version.clone()),
        }
    }

    /// What the mods' jars are checked against (D-066); `None` without a mod
    /// loader.
    pub fn setup(&self) -> Option<mod_meta::Setup<'_>> {
        let loader = match self.loader? {
            LoaderKind::Fabric => mod_meta::Loader::Fabric,
            LoaderKind::Quilt => mod_meta::Loader::Quilt,
            LoaderKind::Forge => mod_meta::Loader::Forge,
            LoaderKind::NeoForge => mod_meta::Loader::NeoForge,
            LoaderKind::OptiFine => return None,
        };
        Some(mod_meta::Setup {
            game: &self.game,
            loader,
            loader_version: self
                .loader_version
                .as_deref()
                .map(|v| loader_number(v, &self.game)),
            java: None,
            side: mod_meta::Side::Client,
        })
    }

    /// The loaders whose mods run here; Quilt also runs Fabric's. OptiFine on
    /// its own runs none.
    pub fn mod_loaders(&self) -> &'static [&'static str] {
        match self.loader {
            Some(LoaderKind::Fabric) => &["fabric"],
            Some(LoaderKind::Quilt) => &["quilt", "fabric"],
            Some(LoaderKind::Forge) => &["forge"],
            Some(LoaderKind::NeoForge) => &["neoforge"],
            Some(LoaderKind::OptiFine) | None => &[],
        }
    }
}

/// The loader's own number, as mods ask for it: Forge's `1.21.1-52.1.0` is
/// `52.1.0`, and `1.7.10-10.13.4.1614-1.7.10` is `10.13.4.1614`.
fn loader_number<'a>(version: &'a str, game: &str) -> &'a str {
    let version = version
        .strip_prefix(game)
        .and_then(|v| v.strip_prefix('-'))
        .unwrap_or(version);
    version
        .strip_suffix(game)
        .and_then(|v| v.strip_suffix('-'))
        .unwrap_or(version)
}

/// Why a version does not fit an installation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Misfit {
    /// A mod for an installation without a loader.
    NoLoader,
    WrongLoader,
    WrongGame,
}

pub fn misfit(kind: &str, version: &Version, target: &Target) -> Option<Misfit> {
    let game = version.games.contains(&target.game);
    if kind == "mod" {
        if target.loader.is_none() {
            return Some(Misfit::NoLoader);
        }
        if !version
            .loaders
            .iter()
            .any(|l| target.mod_loaders().contains(&l.as_str()))
        {
            return Some(Misfit::WrongLoader);
        }
    }
    (!game).then_some(Misfit::WrongGame)
}

/// How a version goes into an installation: as its project's kind, except a
/// mod's data pack version (Terralith and the like publish both), which goes
/// into a world.
pub fn install_kind<'a>(kind: &'a str, version: &Version) -> &'a str {
    if kind == "mod" && datapack_version(version) {
        "datapack"
    } else {
        kind
    }
}

/// Whether a version is a data pack and nothing else.
fn datapack_version(version: &Version) -> bool {
    !version.loaders.is_empty() && version.loaders.iter().all(|l| l == "datapack")
}

/// The version to take: the newest release, else the newest.
pub fn best(versions: &[Version]) -> Option<&Version> {
    versions
        .iter()
        .find(|v| v.channel == "release")
        .or_else(|| versions.first())
}

/// The versions of a project that run here, newest first.
async fn fitting<C: Catalog>(
    catalog: &C,
    project: &str,
    kind: &str,
    target: &Target,
) -> LauncherResult<Vec<Version>> {
    if kind == "mod" {
        for loader in target.mod_loaders() {
            let found = catalog
                .versions(project, Some(&target.game), Some(loader))
                .await?;
            if !found.is_empty() {
                return Ok(found);
            }
        }
        Ok(Vec::new())
    } else if kind == "datapack" {
        // A mod's versions for loaders are not data packs.
        let found = catalog.versions(project, Some(&target.game), None).await?;
        Ok(found
            .into_iter()
            .filter(|v| v.loaders.is_empty() || datapack_version(v))
            .collect())
    } else {
        catalog.versions(project, Some(&target.game), None).await
    }
}

// ----- plans -----

/// One file an install adds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanItem {
    pub project_id: String,
    pub title: String,
    pub kind: String,
    pub icon: Option<String>,
    pub version_id: String,
    pub number: String,
    pub published: i64,
    pub file_name: String,
    pub size: u64,
    /// The catalog keeps the file already; otherwise it fetches it first.
    pub kept: bool,
    /// Added because another file needs it.
    pub dependency: bool,
    /// Where it goes, relative to the game folder.
    pub path: String,
    /// The file it takes the place of (an older version of the same project).
    pub replaces: Option<String>,
    /// The same file is there already.
    pub present: bool,
    /// The projects this version requires (kept in the lock).
    #[serde(skip)]
    pub requires: Vec<String>,
    /// What its jar says, as the catalog read it.
    #[serde(skip)]
    pub manifests: Option<Vec<mod_meta::Manifest>>,
}

/// A required project the catalog has no fitting version of.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Missing {
    pub project_id: Option<String>,
    pub title: Option<String>,
}

/// Something in the way of an install.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Conflict {
    /// The file already there, relative to the game folder.
    pub path: String,
    /// `incompatible`: a project that does not work with this one;
    /// `name_taken`: a file the player put there has this name; `held`: the
    /// catalog's malware check holds the file back.
    pub reason: String,
    pub title: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    pub items: Vec<PlanItem>,
    pub missing: Vec<Missing>,
    pub conflicts: Vec<Conflict>,
    /// The chosen version does not list this game version (allowed for
    /// resource packs, shaders and data packs).
    pub misfit: Option<Misfit>,
    /// Bytes to download.
    pub total_bytes: u64,
    /// On a metered connection, why this download is confirmed first (D-051);
    /// pressing Install with it shown is the confirmation.
    pub approval: Option<crate::state::dto::ApprovalRequest>,
    /// Files there already but turned off that this needs: turned on with
    /// the install.
    pub turn_on: Vec<TurnOn>,
    /// What the mods' jars say is still in the way of the set starting; shown,
    /// not in the way of the install.
    pub problems: Vec<SetProblem>,
    /// The version asked for does not run here, and this one does.
    pub instead: Option<Instead>,
}

/// A file turned off that an install turns on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnOn {
    /// As on disk now (ending in `.disabled`), relative to the game folder.
    pub path: String,
    pub title: String,
}

impl Plan {
    pub fn ready(&self) -> bool {
        self.missing.is_empty() && self.conflicts.is_empty()
    }
}

/// What to install and where.
pub struct Wanted<'a> {
    pub version: &'a str,
    pub world: Option<&'a str>,
    /// A file the player put there that this replaces (an update of a file the
    /// catalog recognized).
    pub replaces: Option<&'a str>,
}

/// Is the entry at `path` (relative to the game folder) there, on or off?
fn present(game: &Path, path: &str) -> Option<String> {
    let on = game.join(path);
    let off = game.join(format!("{path}{DISABLED}"));
    if fs::symlink_metadata(&on).is_ok() {
        Some(path.to_owned())
    } else if fs::symlink_metadata(&off).is_ok() {
        Some(format!("{path}{DISABLED}"))
    } else {
        None
    }
}

/// The plan for putting a version (and what it needs) into an installation,
/// with what the plan adds checked as the mods' jars say.
pub async fn plan<C: Catalog>(
    catalog: &C,
    game: &Path,
    target: &Target,
    lock: &ContentLock,
    wanted: &Wanted<'_>,
    identified: &[Identified],
) -> LauncherResult<Plan> {
    checked_plan(catalog, game, target, lock, wanted, identified, &[]).await
}

/// [`plan`], with the whole set checked as the mods' jars say (D-066): the
/// mods already in the folder (`local`) and those the plan adds. A mod a jar
/// needs that no page names is looked up by its ID; one there already that
/// is too old for a mod the plan adds is updated. What is left is shown in
/// `problems`, and does not stop the install.
pub async fn checked_plan<C: Catalog>(
    catalog: &C,
    game: &Path,
    target: &Target,
    lock: &ContentLock,
    wanted: &Wanted<'_>,
    identified: &[Identified],
    local: &[LocalJar],
) -> LauncherResult<Plan> {
    let version = catalog.version(wanted.version).await?;
    let project = catalog.project(&version.project_id).await?;
    let kind = install_kind(&project.kind, &version).to_owned();
    let folder = folder_of(&kind, wanted.world)?;
    if let Some(world) = wanted.world.filter(|_| kind == "datapack")
        && !game.join("saves").join(world).is_dir()
    {
        return Err(LauncherError::invalid_input(
            "This world is not in the installation.",
        ));
    }
    let found = misfit(&kind, &version, target);
    if kind == "mod"
        && let Some(why) = found
    {
        return Err(match why {
            Misfit::NoLoader => LauncherError::invalid_input(
                "Mods need an installation with a mod loader (Fabric, Quilt, Forge or NeoForge).",
            ),
            Misfit::WrongLoader => LauncherError::invalid_input(
                "This version of the mod is not made for this installation's mod loader.",
            ),
            Misfit::WrongGame => LauncherError::invalid_input(
                "This version of the mod is not made for this installation's game version.",
            ),
        });
    }
    if let Some(replaces) = wanted.replaces {
        content_path(game, replaces)?;
    }

    let mut planner = Planner {
        catalog,
        game,
        target,
        lock,
        identified,
        replaces: wanted.replaces,
        kind,
        folder,
        plan: Plan {
            items: Vec::new(),
            missing: Vec::new(),
            conflicts: Vec::new(),
            misfit: found,
            total_bytes: 0,
            approval: None,
            turn_on: Vec::new(),
            problems: Vec::new(),
            instead: None,
        },
        seen: HashSet::from([project.id.clone()]),
        queue: Vec::new(),
        clashes: Vec::new(),
    };
    let setup = target.setup().filter(|_| planner.kind == "mod");
    // The version asked for does not run here: one that does, if any.
    if let Some(setup) = &setup
        && runs(&version, setup, None) == Some(false)
        && let Some(other) = planner.choose(&project.id, None).await?
        && other.id != version.id
        && runs(&other, setup, None) == Some(true)
    {
        planner.plan.instead = Some(Instead {
            version_id: other.id.clone(),
            number: other.number.clone(),
        });
    }
    planner.queue.push(Queued {
        version,
        title: project.title.clone(),
        icon: project.icon.clone(),
        main: true,
        dependency: false,
    });
    planner.run().await?;
    if let Some(setup) = &setup {
        planner.settle(setup, local).await?;
    }
    Ok(planner.plan)
}

/// A jar in an installation's mods folder and what it says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalJar {
    /// As on disk now (`.disabled` when off), relative to the game folder.
    pub path: String,
    pub manifests: Vec<mod_meta::Manifest>,
}

/// Something in the way of a set of mods starting, as their jars say.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SetProblem {
    /// `missing`, `wrong`, `breaks`, `twice` or `other_loader`.
    pub kind: String,
    /// The mod that says so (its title, else its file's name); for `twice`,
    /// the files that are the same mod.
    pub titles: Vec<String>,
    /// The mod it needs or does not start with; `minecraft`, the loader's ID
    /// (`fabricloader`, `quilt_loader`, `forge`, `neoforge`) or `java` too.
    pub id: String,
    /// The versions it takes, in the loader's own words; none: any.
    pub versions: Vec<String>,
    /// The version there.
    pub found: Option<String>,
    /// The mod that gives it (its title); `None`: the game, the loader or
    /// Java.
    pub from: Option<String>,
    /// For `other_loader`: the loader the jar is made for.
    pub loader: Option<String>,
}

/// A version of the project asked for that runs here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Instead {
    pub version_id: String,
    pub number: String,
}

/// A mod a jar needs: its ID and the versions it takes, in the words of the
/// loader the jar is made for.
struct Want<'a> {
    id: &'a str,
    versions: &'a [String],
    loader: mod_meta::Loader,
}

/// Versions asked about on their own when choosing one, at most.
const MAX_TRIED: usize = 6;
/// Times the set is checked and mended.
const MAX_ROUNDS: usize = 3;

/// Whether a version's jar runs on `setup` (and gives `want`), as the
/// catalog read it; `None` when that is not known.
fn runs(version: &Version, setup: &mod_meta::Setup<'_>, want: Option<&Want<'_>>) -> Option<bool> {
    let manifests = version.main_file()?.manifests.as_deref()?;
    if manifests.is_empty() {
        return None;
    }
    let jar = [mod_meta::Jar {
        file: "",
        manifests,
    }];
    let fine = !mod_meta::check(setup, &jar).iter().any(|p| {
        matches!(
            p,
            mod_meta::Problem::Wrong { from: None, .. }
                | mod_meta::Problem::Breaks { from: None, .. }
                | mod_meta::Problem::OtherLoader { .. }
        )
    });
    let gives = want.is_none_or(|w| {
        mod_meta::for_loader(manifests, setup.loader, setup.game).is_some_and(|m| {
            m.mods.iter().any(|p| {
                p.id == w.id
                    && p.version
                        .as_deref()
                        .is_none_or(|v| mod_meta::allows(w.loader, w.versions, v) != Some(false))
            })
        })
    });
    Some(fine && gives)
}

/// A mod's version to install here: the newest release whose jar runs (and
/// gives `want`), else the newest of another channel, else the first tried.
async fn choose<C: Catalog>(
    catalog: &C,
    target: &Target,
    project: &str,
    want: Option<&Want<'_>>,
) -> LauncherResult<Option<Version>> {
    let list = fitting(catalog, project, "mod", target).await?;
    let mut order: Vec<&Version> = list.iter().filter(|v| v.channel == "release").collect();
    order.extend(list.iter().filter(|v| v.channel != "release"));
    let setup = target.setup();
    let mut first = None;
    for candidate in order.into_iter().take(MAX_TRIED) {
        // Lists leave out what the jars say.
        let Ok(version) = catalog.version(&candidate.id).await else {
            continue;
        };
        match setup.as_ref().and_then(|s| runs(&version, s, want)) {
            Some(false) => {
                first.get_or_insert(version);
            }
            _ => return Ok(Some(version)),
        }
    }
    Ok(first.or_else(|| best(&list).cloned()))
}

/// A version waiting to go into the plan.
struct Queued {
    version: Version,
    title: String,
    icon: Option<String>,
    /// The version asked for.
    main: bool,
    dependency: bool,
}

struct Planner<'a, C> {
    catalog: &'a C,
    game: &'a Path,
    target: &'a Target,
    lock: &'a ContentLock,
    identified: &'a [Identified],
    /// The player's file the version asked for replaces.
    replaces: Option<&'a str>,
    kind: String,
    folder: String,
    plan: Plan,
    /// Projects required so far.
    seen: HashSet<String>,
    queue: Vec<Queued>,
    /// Mods known not to start together, from the catalog.
    clashes: Vec<mod_meta::Clash>,
}

impl<C: Catalog> Planner<'_, C> {
    /// A project's file in the folder: the launcher's, else one of the
    /// player's the catalog recognized; as on disk now.
    fn there(&self, project: &str) -> Option<(String, String)> {
        if let Some(ours) = self.lock.of_project_in(project, &self.folder)
            && let Some(path) = present(self.game, &ours.path)
        {
            return Some((path, ours.title.clone()));
        }
        self.identified
            .iter()
            .find(|i| {
                i.project_id == project
                    && in_folder(enabled_form(&i.path), &self.folder)
                    && self.game.join(&i.path).is_file()
            })
            .map(|i| {
                let name = i.path.rsplit('/').next().unwrap_or_default();
                (i.path.clone(), enabled_form(name).to_owned())
            })
    }

    /// Puts what is queued into the plan, with what it requires.
    async fn run(&mut self) -> LauncherResult<()> {
        let (game, lock) = (self.game, self.lock);
        while let Some(Queued {
            version,
            title,
            icon,
            main,
            dependency,
        }) = self.queue.pop()
        {
            if self.plan.items.len() >= MAX_PLAN {
                return Err(LauncherError::invalid_input(
                    "This needs too many other projects to install at once.",
                ));
            }
            let kind = if main { self.kind.as_str() } else { "mod" }.to_owned();
            let folder = self.folder.clone();
            let file = version
                .main_file()
                .filter(|f| catalog_file_name(&f.name, &kind))
                .ok_or_else(|| {
                    LauncherError::invalid_input(
                        "This version has no file the launcher can put into an installation.",
                    )
                })?;
            let path = format!("{folder}/{}", file.name);
            if file.held {
                self.plan.conflicts.push(Conflict {
                    path: path.clone(),
                    reason: "held".to_owned(),
                    title: Some(title.clone()),
                });
            }
            // An older file of the same project goes, the player's own too: two
            // copies of a mod stop the game.
            // Only in the same folder: a mod's data pack in a world leaves the
            // mod be, and a data pack in one world leaves another world's.
            let replaces = match lock.of_project_in(&version.project_id, &folder) {
                Some(old) => Some(old.path.clone()),
                None => self
                    .replaces
                    .filter(|_| main)
                    .map(str::to_owned)
                    .or_else(|| self.there(&version.project_id).map(|(p, _)| p))
                    .map(|r| enabled_form(&r).to_owned()),
            }
            .filter(|p| in_folder(p, &folder))
            .filter(|p| present(game, p).is_some());
            let mut same = false;
            if let Some(there) = present(game, &path) {
                match lock.at(&path) {
                    Some(o) if o.project_id != version.project_id => {
                        self.plan.conflicts.push(Conflict {
                            path: there,
                            reason: "name_taken".to_owned(),
                            title: Some(o.title.clone()),
                        })
                    }
                    Some(o) => same = file.sha256.as_deref() == Some(o.sha256.as_str()),
                    None if replaces.as_deref() == Some(path.as_str()) => {}
                    None => self.plan.conflicts.push(Conflict {
                        path: there,
                        reason: "name_taken".to_owned(),
                        title: None,
                    }),
                }
            }
            let replaces = replaces.filter(|r| !(same && *r == path));
            self.plan.items.push(PlanItem {
                project_id: version.project_id.clone(),
                title,
                kind: kind.clone(),
                icon,
                version_id: version.id.clone(),
                number: version.number.clone(),
                published: version.published,
                file_name: file.name.clone(),
                size: file.size,
                kept: file.sha256.is_some() && file.url.is_some(),
                dependency,
                path,
                replaces,
                present: same,
                requires: version
                    .dependencies
                    .iter()
                    .filter(|d| d.kind == "required")
                    .filter_map(|d| d.project_id.clone())
                    .collect(),
                manifests: file.manifests.clone(),
            });
            if !same {
                self.plan.total_bytes += file.size;
            }
            if kind != "mod" {
                continue;
            }
            for needed in &version.dependencies {
                match needed.kind.as_str() {
                    "incompatible" => {
                        if let Some((path, title)) =
                            needed.project_id.as_deref().and_then(|p| self.there(p))
                        {
                            self.plan.conflicts.push(Conflict {
                                path: enabled_form(&path).to_owned(),
                                reason: "incompatible".to_owned(),
                                title: Some(title),
                            });
                        }
                    }
                    "required" => match needed.project_id.clone() {
                        Some(project_id) => {
                            self.require(project_id, needed.version_id.as_deref(), None)
                                .await?;
                        }
                        None => self.plan.missing.push(Missing {
                            project_id: None,
                            title: None,
                        }),
                    },
                    _ => {}
                }
            }
        }
        Ok(())
    }

    /// Brings a project something needs: queued, unless it is there already
    /// (turned on if off). Whether it is queued or turned on.
    async fn require(
        &mut self,
        project_id: String,
        pinned: Option<&str>,
        want: Option<&Want<'_>>,
    ) -> LauncherResult<bool> {
        if !self.seen.insert(project_id.clone()) {
            return Ok(false);
        }
        // There already: nothing to add; turned off, it is turned on.
        if let Some((path, title)) = self.there(&project_id) {
            if path.ends_with(DISABLED) && !self.plan.turn_on.iter().any(|t| t.path == path) {
                self.plan.turn_on.push(TurnOn { path, title });
                return Ok(true);
            }
            return Ok(false);
        }
        let other = self.catalog.project(&project_id).await.ok();
        let pinned = match pinned {
            Some(id) => self
                .catalog
                .version(id)
                .await
                .ok()
                .filter(|v| misfit("mod", v, self.target).is_none()),
            None => None,
        };
        let chosen = match pinned {
            Some(v) => Some(v),
            None => self.choose(&project_id, want).await?,
        };
        // Looked up by what a jar needs: only a version known to give it.
        let chosen = chosen.filter(|v| {
            want.is_none_or(|w| {
                self.target
                    .setup()
                    .is_some_and(|s| runs(v, &s, Some(w)) == Some(true))
            })
        });
        match chosen {
            Some(version) => {
                self.queue.push(Queued {
                    version,
                    title: other.as_ref().map(|p| p.title.clone()).unwrap_or_default(),
                    icon: other.as_ref().and_then(|p| p.icon.clone()),
                    main: false,
                    dependency: true,
                });
                Ok(true)
            }
            None if want.is_some() => {
                // Another project may give it.
                self.seen.remove(&project_id);
                Ok(false)
            }
            None => {
                self.plan.missing.push(Missing {
                    project_id: Some(project_id),
                    title: other.map(|p| p.title),
                });
                Ok(false)
            }
        }
    }

    /// The version of a project to bring: the newest release whose jar runs
    /// here (and gives `want`), else the newest such version; where no jar
    /// says, the newest release that fits by the catalog's lists ([`best`]).
    async fn choose(
        &self,
        project: &str,
        want: Option<&Want<'_>>,
    ) -> LauncherResult<Option<Version>> {
        choose(self.catalog, self.target, project, want).await
    }

    /// The set as it would be after the install: the folder's mods that are
    /// on (or turned on by it) and not replaced, and what it adds.
    fn check(&self, setup: &mod_meta::Setup<'_>, local: &[LocalJar]) -> Vec<mod_meta::Problem> {
        let replaced: HashSet<&str> = self
            .plan
            .items
            .iter()
            .filter_map(|i| i.replaces.as_deref())
            .collect();
        let mut jars = Vec::new();
        for jar in local {
            let on = !jar.path.ends_with(DISABLED)
                || self.plan.turn_on.iter().any(|t| t.path == jar.path);
            let file = enabled_form(&jar.path);
            if on && !replaced.contains(file) {
                jars.push(mod_meta::Jar {
                    file,
                    manifests: &jar.manifests,
                });
            }
        }
        for item in self
            .plan
            .items
            .iter()
            .filter(|i| i.kind == "mod" && !i.present)
        {
            if let Some(manifests) = &item.manifests {
                jars.push(mod_meta::Jar {
                    file: &item.path,
                    manifests,
                });
            }
        }
        mod_meta::check_with(setup, &jars, &self.clashes)
    }

    /// Mends what the jars say is in the way, a few rounds at most: a mod
    /// needed that nothing here gives is looked up by its ID, and a mod there
    /// already that is too old for one the plan adds is updated. The rest is
    /// noted in the plan.
    async fn settle(
        &mut self,
        setup: &mod_meta::Setup<'_>,
        local: &[LocalJar],
    ) -> LauncherResult<()> {
        self.clashes = self
            .catalog
            .clashes()
            .await
            .unwrap_or_else(|_| mod_meta::clashes());
        let mut asked: HashSet<String> = HashSet::new();
        let mut updated: HashSet<String> = HashSet::new();
        for _ in 0..MAX_ROUNDS {
            let mut more = false;
            for problem in self.check(setup, local) {
                match problem {
                    mod_meta::Problem::Missing {
                        file, id, versions, ..
                    } if asked.insert(id.clone()) => {
                        more |= self.provide(&file, &id, &versions).await?;
                    }
                    mod_meta::Problem::Wrong {
                        file,
                        id,
                        versions,
                        from: Some(from),
                        ..
                    } if self.adds(&file) && !self.adds(&from) => {
                        let Some(project) = self.project_at(&from) else {
                            continue;
                        };
                        if updated.insert(project.clone()) {
                            more |= self.update(&project, &id, &versions).await?;
                        }
                    }
                    _ => {}
                }
            }
            if !more {
                break;
            }
            self.run().await?;
        }
        let problems = self.check(setup, local);
        self.plan.problems = problems
            .iter()
            .filter(|p| self.touches(p))
            .map(|p| describe(p, |path| self.title_of(path)))
            .collect();
        Ok(())
    }

    /// Whether the plan adds the file at `path`.
    fn adds(&self, path: &str) -> bool {
        self.plan.items.iter().any(|i| i.path == path && !i.present)
    }

    /// Whether a problem has to do with what the plan adds; the others were
    /// there before it.
    fn touches(&self, problem: &mod_meta::Problem) -> bool {
        use mod_meta::Problem as P;
        match problem {
            P::Missing { file, .. } | P::OtherLoader { file, .. } => self.adds(file),
            P::Wrong { file, from, .. } | P::Breaks { file, from, .. } => {
                self.adds(file) || from.as_deref().is_some_and(|f| self.adds(f))
            }
            P::Twice { files, .. } => files.iter().any(|f| self.adds(f)),
        }
    }

    /// The catalog's project of a file in the folder, by the lock or the
    /// player's files it recognized.
    fn project_at(&self, path: &str) -> Option<String> {
        self.lock
            .at(path)
            .map(|o| o.project_id.clone())
            .or_else(|| {
                self.identified
                    .iter()
                    .find(|i| enabled_form(&i.path) == path)
                    .map(|i| i.project_id.clone())
            })
    }

    fn title_of(&self, path: &str) -> String {
        if let Some(item) = self.plan.items.iter().find(|i| i.path == path) {
            return item.title.clone();
        }
        if let Some(ours) = self.lock.at(path) {
            return ours.title.clone();
        }
        enabled_form(path.rsplit('/').next().unwrap_or(path)).to_owned()
    }

    /// A mod a jar needs that nothing here gives: a project whose jars give
    /// it, the most downloaded first, noted as required by what needs it.
    async fn provide(&mut self, file: &str, id: &str, versions: &[String]) -> LauncherResult<bool> {
        let Some(setup) = self.target.setup() else {
            return Ok(false);
        };
        let loader = self
            .plan
            .items
            .iter()
            .find(|i| i.path == file)
            .and_then(|i| i.manifests.as_deref())
            .and_then(|m| mod_meta::for_loader(m, setup.loader, setup.game))
            .map_or(setup.loader, |m| m.loader);
        let want = Want {
            id,
            versions,
            loader,
        };
        let mut found = Vec::new();
        for name in self.target.mod_loaders() {
            // An older catalog does not know; nothing is found then.
            found = self
                .catalog
                .provides(id, &self.target.game, name)
                .await
                .unwrap_or_default();
            if !found.is_empty() {
                break;
            }
        }
        for project in found {
            if self.require(project.clone(), None, Some(&want)).await? {
                if let Some(item) = self.plan.items.iter_mut().find(|i| i.path == file) {
                    item.requires.push(project);
                }
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// A mod there already that is too old for one the plan adds: its newest
    /// version known to give what is asked, in place of its file.
    async fn update(
        &mut self,
        project: &str,
        id: &str,
        versions: &[String],
    ) -> LauncherResult<bool> {
        let Some(setup) = self.target.setup() else {
            return Ok(false);
        };
        let want = Want {
            id,
            versions,
            loader: setup.loader,
        };
        let Some(version) = self
            .choose(project, Some(&want))
            .await?
            .filter(|v| runs(v, &setup, Some(&want)) == Some(true))
        else {
            return Ok(false);
        };
        if self.queue.iter().any(|q| q.version.project_id == project)
            || self.plan.items.iter().any(|i| i.project_id == project)
        {
            return Ok(false);
        }
        let other = self.catalog.project(project).await.ok();
        self.queue.push(Queued {
            version,
            title: other.as_ref().map(|p| p.title.clone()).unwrap_or_default(),
            icon: other.as_ref().and_then(|p| p.icon.clone()),
            main: false,
            // A mod the player put there stays theirs.
            dependency: self.lock.of_project(project).is_some_and(|o| o.dependency),
        });
        Ok(true)
    }
}

/// What the mods turned on in an installation's folder say is in the way of
/// the game starting, with the titles the lock knows.
pub fn check_folder(
    target: &Target,
    lock: &ContentLock,
    local: &[LocalJar],
    clashes: &[mod_meta::Clash],
) -> Vec<SetProblem> {
    let Some(setup) = target.setup() else {
        return Vec::new();
    };
    let jars: Vec<mod_meta::Jar<'_>> = local
        .iter()
        .filter(|j| !j.path.ends_with(DISABLED))
        .map(|j| mod_meta::Jar {
            file: &j.path,
            manifests: &j.manifests,
        })
        .collect();
    mod_meta::check_with(&setup, &jars, clashes)
        .iter()
        .map(|p| {
            describe(p, |path| match lock.at(path) {
                Some(ours) => ours.title.clone(),
                None => path.rsplit('/').next().unwrap_or(path).to_owned(),
            })
        })
        .collect()
}

/// A mod the game said is missing or too old, as the crash window offers to
/// put it in (D-072): the catalog's project that gives it and the version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModFix {
    /// As the loader named it.
    pub mod_id: String,
    pub project_id: String,
    pub title: String,
    pub icon: Option<String>,
    pub version_id: String,
    pub number: String,
    /// The file there now that gives the mod: the fix is an update.
    pub installed: Option<String>,
    /// A file of the player's the install replaces (the launcher's own are
    /// replaced anyway).
    pub replaces: Option<String>,
}

/// A mod ID as the loaders write it.
fn mod_id(id: &str) -> bool {
    (1..=64).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"_-.".contains(&b))
}

/// What puts in, or updates, mod `id` so the jars there that need it get a
/// version they take; `None` when the catalog has nothing better.
pub async fn mod_fix<C: Catalog>(
    catalog: &C,
    target: &Target,
    lock: &ContentLock,
    local: &[LocalJar],
    identified: &[Identified],
    id: &str,
) -> LauncherResult<Option<ModFix>> {
    let Some(setup) = target.setup() else {
        return Ok(None);
    };
    if !mod_id(id) {
        return Ok(None);
    }
    let on: Vec<&LocalJar> = local
        .iter()
        .filter(|j| !j.path.ends_with(DISABLED))
        .collect();
    let jars: Vec<mod_meta::Jar<'_>> = on
        .iter()
        .map(|j| mod_meta::Jar {
            file: &j.path,
            manifests: &j.manifests,
        })
        .collect();
    // The versions the first mod that needs it takes; none: any.
    let versions = mod_meta::check(&setup, &jars)
        .into_iter()
        .find_map(|p| match p {
            mod_meta::Problem::Missing {
                id: i, versions, ..
            }
            | mod_meta::Problem::Wrong {
                id: i, versions, ..
            } if i == id => Some(versions),
            _ => None,
        })
        .unwrap_or_default();
    let want = Want {
        id,
        versions: &versions,
        loader: setup.loader,
    };
    // The jar there that is the mod itself, not one that carries it inside.
    let installed = on
        .iter()
        .find(|j| {
            mod_meta::for_loader(&j.manifests, setup.loader, setup.game)
                .is_some_and(|m| m.mods.iter().any(|p| p.id == id && !p.nested))
        })
        .map(|j| j.path.clone());
    let (known, now) = match installed.as_deref() {
        Some(path) => match lock.at(path) {
            Some(ours) => (Some(ours.project_id.clone()), Some(ours.version_id.clone())),
            None => identified
                .iter()
                .find(|i| enabled_form(&i.path) == path)
                .map_or((None, None), |i| {
                    (Some(i.project_id.clone()), Some(i.version_id.clone()))
                }),
        },
        None => (None, None),
    };
    let replaces = installed
        .as_deref()
        .filter(|path| lock.at(path).is_none())
        .map(str::to_owned);
    let mut projects: Vec<String> = known.into_iter().collect();
    if projects.is_empty() {
        for name in target.mod_loaders() {
            projects = catalog
                .provides(id, &target.game, name)
                .await
                .unwrap_or_default();
            if !projects.is_empty() {
                break;
            }
        }
    }
    for project in projects.iter().take(5) {
        let Some(version) = choose(catalog, target, project, Some(&want)).await? else {
            continue;
        };
        if runs(&version, &setup, Some(&want)) == Some(false)
            || now.as_deref() == Some(version.id.as_str())
        {
            continue;
        }
        let card = catalog.project(project).await.ok();
        return Ok(Some(ModFix {
            mod_id: id.to_owned(),
            project_id: project.clone(),
            title: card
                .as_ref()
                .map_or_else(|| id.to_owned(), |p| p.title.clone()),
            icon: card.and_then(|p| p.icon),
            number: version.number.clone(),
            version_id: version.id,
            installed,
            replaces,
        }));
    }
    Ok(None)
}

/// A problem in words the interface shows, with the mods' titles.
pub fn describe(problem: &mod_meta::Problem, title_of: impl Fn(&str) -> String) -> SetProblem {
    use mod_meta::Problem as P;
    let mut out = SetProblem {
        kind: String::new(),
        titles: Vec::new(),
        id: String::new(),
        versions: Vec::new(),
        found: None,
        from: None,
        loader: None,
    };
    match problem {
        P::Missing {
            file, id, versions, ..
        } => {
            out.kind = "missing".to_owned();
            out.titles = vec![title_of(file)];
            out.id.clone_from(id);
            out.versions.clone_from(versions);
        }
        P::Wrong {
            file,
            id,
            versions,
            found,
            from,
            ..
        } => {
            out.kind = "wrong".to_owned();
            out.titles = vec![title_of(file)];
            out.id.clone_from(id);
            out.versions.clone_from(versions);
            out.found = Some(found.clone());
            out.from = from.as_deref().map(&title_of);
        }
        P::Breaks {
            file,
            id,
            found,
            from,
            ..
        } => {
            out.kind = "breaks".to_owned();
            out.titles = vec![title_of(file)];
            out.id.clone_from(id);
            out.found.clone_from(found);
            out.from = from.as_deref().map(&title_of);
        }
        P::Twice { id, files } => {
            out.kind = "twice".to_owned();
            out.titles = files.iter().map(|f| title_of(f)).collect();
            out.id.clone_from(id);
        }
        P::OtherLoader { file, loader } => {
            out.kind = "other_loader".to_owned();
            out.titles = vec![title_of(file)];
            out.loader = Some(
                match loader {
                    mod_meta::Loader::Fabric => "fabric",
                    mod_meta::Loader::Quilt => "quilt",
                    mod_meta::Loader::Forge => "forge",
                    mod_meta::Loader::NeoForge => "neoforge",
                }
                .to_owned(),
            );
        }
    }
    out
}

// ----- installing -----

/// What the interface sees while an install runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub instance_id: String,
    /// `fetching` (the catalog brings files from the source), `downloading`,
    /// `unpacking` (a modpack's own files), `done`, `failed` or `cancelled`.
    pub phase: &'static str,
    /// Files the catalog still fetches, or files downloaded.
    pub files: u64,
    pub total_files: u64,
    pub bytes: u64,
    pub total_bytes: u64,
}

pub(super) struct Sink<'a> {
    pub(super) instance: String,
    pub(super) emit: &'a (dyn Fn(Progress) + Send + Sync),
}

impl ProgressSink for Sink<'_> {
    fn progress(&self, p: &DownloadProgress) {
        (self.emit)(Progress {
            instance_id: self.instance.clone(),
            phase: "downloading",
            files: p.verified_files,
            total_files: p.total_files,
            bytes: p.downloaded_bytes,
            total_bytes: p.total_download_bytes,
        });
    }
}

/// Installs into game folders; one install or change per installation at a
/// time.
pub struct Installer {
    pub(super) transport: crate::download::transport::HttpTransport,
    running: Mutex<HashMap<InstanceId, CancelToken>>,
    /// SHA-1 of files the player put there, by path.
    hashes: Mutex<HashMap<PathBuf, Hashed>>,
    /// The projects a catalog version requires, as the catalog said.
    requires: Mutex<HashMap<String, Vec<String>>>,
    /// What the jars in mods folders say, by path.
    read: Mutex<HashMap<PathBuf, ReadJar>>,
}

/// A jar's size, time and manifests when it was last read.
type ReadJar = (u64, Option<SystemTime>, Vec<mod_meta::Manifest>);

/// A file's size, time and SHA-1 when it was last read.
type Hashed = (u64, Option<SystemTime>, String);

/// Holds an installation's place until dropped.
pub struct Claim<'a> {
    installer: &'a Installer,
    id: InstanceId,
    pub cancel: CancelToken,
}

impl Drop for Claim<'_> {
    fn drop(&mut self) {
        lock(&self.installer.running).remove(&self.id);
    }
}

impl Installer {
    pub fn new(app_version: &str) -> LauncherResult<Self> {
        Ok(Self {
            transport: crate::download::transport::HttpTransport::public(app_version)?,
            running: Mutex::new(HashMap::new()),
            hashes: Mutex::new(HashMap::new()),
            requires: Mutex::new(HashMap::new()),
            read: Mutex::new(HashMap::new()),
        })
    }

    /// Takes the installation for one change, or says it is busy.
    pub fn claim(&self, id: InstanceId) -> LauncherResult<Claim<'_>> {
        let mut running = lock(&self.running);
        if running.contains_key(&id) {
            return Err(busy());
        }
        let cancel = CancelToken::new();
        running.insert(id, cancel.clone());
        Ok(Claim {
            installer: self,
            id,
            cancel,
        })
    }

    /// Fails while content is being installed or changed in the installation.
    pub fn ensure_idle(&self, id: InstanceId) -> LauncherResult<()> {
        if lock(&self.running).contains_key(&id) {
            Err(busy())
        } else {
            Ok(())
        }
    }

    /// Stops an install; false when none runs.
    pub fn cancel(&self, id: InstanceId) -> bool {
        match lock(&self.running).get(&id) {
            Some(token) => {
                token.cancel();
                true
            }
            None => false,
        }
    }

    /// Carries out a plan that is [`Plan::ready`].
    pub async fn install<C: Catalog>(
        &self,
        catalog: &C,
        root: &DataRoot,
        id: InstanceId,
        plan: Plan,
        cancel: &CancelToken,
        emit: &(dyn Fn(Progress) + Send + Sync),
    ) -> LauncherResult<usize> {
        if !plan.ready() {
            return Err(LauncherError::invalid_input(
                "Something this needs is missing or in the way; see the list before installing.",
            ));
        }
        let instance = id.to_string();
        let say = |phase: &'static str, files: u64, total_files: u64| {
            emit(Progress {
                instance_id: instance.clone(),
                phase,
                files,
                total_files,
                bytes: 0,
                total_bytes: plan.total_bytes,
            });
        };
        // Files the catalog does not keep yet: it fetches them from the source.
        let mut versions: BTreeMap<String, Version> = BTreeMap::new();
        let waiting: Vec<&PlanItem> = plan
            .items
            .iter()
            .filter(|i| !i.kept && !i.present)
            .collect();
        for item in &waiting {
            catalog.fetch(&item.version_id).await?;
        }
        let started = tokio::time::Instant::now();
        let mut left: Vec<&PlanItem> = waiting;
        while !left.is_empty() {
            say("fetching", left.len() as u64, plan.items.len() as u64);
            let mut still = Vec::new();
            for item in left {
                let version = catalog.version(&item.version_id).await?;
                let kept = version
                    .files
                    .iter()
                    .any(|f| f.name == item.file_name && f.sha256.is_some() && f.url.is_some());
                if kept {
                    versions.insert(item.version_id.clone(), version);
                } else {
                    still.push(item);
                }
            }
            left = still;
            if left.is_empty() {
                break;
            }
            if started.elapsed() > FETCH_WAIT {
                return Err(LauncherError::new(
                    ErrorCode::ServiceUnavailable,
                    "The catalog could not bring the files from their source in time. Try again later.",
                ));
            }
            tokio::select! {
                () = tokio::time::sleep(FETCH_POLL) => {}
                () = cancel.cancelled() => {
                    return Err(LauncherError::new(ErrorCode::OperationCancelled, "The download was cancelled."));
                }
            }
        }

        let mut requests = Vec::new();
        let mut placed: Vec<(PlanItem, String)> = Vec::new();
        for item in &plan.items {
            let version = match versions.get(&item.version_id) {
                Some(v) => v.clone(),
                None => catalog.version(&item.version_id).await?,
            };
            let file = version
                .files
                .iter()
                .find(|f| f.name == item.file_name)
                .ok_or_else(|| LauncherError::invalid_input("This version changed; try again."))?;
            let (Some(sha), Some(url)) = (file.sha256.clone(), file.url.as_deref()) else {
                return Err(LauncherError::invalid_input(
                    "This version changed; try again.",
                ));
            };
            let url = our_file(url, &sha).ok_or_else(|| {
                LauncherError::new(
                    ErrorCode::UnsupportedVersionMetadata,
                    "Metadata points to a download location the launcher does not trust.",
                )
            })?;
            if !item.present {
                requests.push(DownloadRequest {
                    source: crate::net::SourcePolicy::ModsCatalog,
                    url,
                    path: format!("instances/{id}/game/{}", item.path),
                    size: file.size,
                    digest: Digest::Sha256(Sha256::parse(&sha)?),
                    label: Some(item.file_name.clone()),
                });
            }
            placed.push((item.clone(), sha));
        }
        let executor = Executor::new(
            self.transport.clone(),
            root.clone(),
            ExecutorConfig::default(),
        );
        let sink = Sink {
            instance: instance.clone(),
            emit,
        };
        executor
            .run(uuid::Uuid::new_v4(), requests, cancel, &sink)
            .await?;

        let game = root.game_dir(id);
        let mut lock_file = load_lock(root, id);
        for (item, sha) in &placed {
            let mut off = false;
            if let Some(old) = &item.replaces {
                let old_on = enabled_form(old).to_owned();
                if let Some(there) = present(&game, &old_on) {
                    off = there.ends_with(DISABLED);
                    if old_on != item.path {
                        let _ = fs::remove_file(game.join(&there));
                    } else if off {
                        // The same name, turned off: the new file takes it.
                        let _ = fs::remove_file(game.join(&there));
                    }
                }
                lock_file.items.retain(|i| i.path != old_on);
            }
            if off && !item.present {
                let _ = fs::rename(
                    game.join(&item.path),
                    game.join(format!("{}{DISABLED}", item.path)),
                );
            }
            // The project's file in another folder stays: a mod and its data
            // pack in a world, or a data pack in two worlds.
            let folder = item.path.rsplit_once('/').map_or("", |(f, _)| f);
            lock_file.items.retain(|i| {
                !(i.project_id == item.project_id && in_folder(&i.path, folder))
                    && i.path != item.path
            });
            lock_file.items.push(LockedItem {
                path: item.path.clone(),
                sha256: sha.clone(),
                size: item.size,
                project_id: item.project_id.clone(),
                version_id: item.version_id.clone(),
                kind: item.kind.clone(),
                title: item.title.clone(),
                version_number: item.number.clone(),
                published: item.published,
                icon: item.icon.clone(),
                dependency: item.dependency,
                requires: Some(item.requires.clone()),
                installed_at: now(),
            });
        }
        // What it needs that was turned off.
        for off in &plan.turn_on {
            let on = enabled_form(&off.path);
            if let (Ok(from), Ok(to)) = (content_path(&game, &off.path), content_path(&game, on))
                && off.path.ends_with(DISABLED)
                && fs::symlink_metadata(&to).is_err()
            {
                let _ = fs::rename(from, to);
            }
        }
        lock_file.schema_version = crate::storage::document::SCHEMA_VERSION;
        save_lock(root, id, &lock_file)?;
        say("done", placed.len() as u64, placed.len() as u64);
        Ok(placed.len())
    }

    /// SHA-1 of a file, remembered while it does not change.
    fn sha1_of(&self, path: &Path) -> Option<String> {
        let meta = fs::metadata(path).ok()?;
        if meta.len() > MAX_HASHED {
            return None;
        }
        let modified = meta.modified().ok();
        if let Some((size, time, hash)) = lock(&self.hashes).get(path)
            && *size == meta.len()
            && *time == modified
        {
            return Some(hash.clone());
        }
        use sha1::Digest as _;
        let mut file = fs::File::open(path).ok()?;
        let mut hasher = sha1::Sha1::new();
        let mut buffer = vec![0u8; 64 * 1024];
        loop {
            let read = file.read(&mut buffer).ok()?;
            if read == 0 {
                break;
            }
            hasher.update(&buffer[..read]);
        }
        let hash = hex::encode(hasher.finalize());
        lock(&self.hashes).insert(path.to_path_buf(), (meta.len(), modified, hash.clone()));
        Some(hash)
    }

    /// Files the player put there, with their SHA-1 (by path relative to the
    /// game folder).
    /// The files of an installation the catalog knows as harmful (by their
    /// SHA-1, from `harmful`): their paths now, turned off or not.
    pub fn harmful_files(
        &self,
        root: &DataRoot,
        id: InstanceId,
        harmful: &HashSet<String>,
    ) -> Vec<String> {
        if harmful.is_empty() {
            return Vec::new();
        }
        let game = root.game_dir(id);
        list(root, id)
            .items
            .iter()
            .filter(|i| !i.directory)
            .take(MAX_HARMFUL_CHECKED)
            .filter(|i| {
                content_path(&game, &i.path)
                    .ok()
                    .and_then(|path| self.sha1_of(&path))
                    .is_some_and(|sha1| harmful.contains(&sha1))
            })
            .map(|i| i.path.clone())
            .collect()
    }

    pub fn unknown_files(&self, root: &DataRoot, id: InstanceId) -> Vec<(String, String)> {
        let view = list(root, id);
        view.items
            .iter()
            .filter(|i| i.project_id.is_none() && !i.directory)
            .take(MAX_IDENTIFY)
            .filter_map(|i| {
                let path = content_path(&root.game_dir(id), &i.path).ok()?;
                Some((i.path.clone(), self.sha1_of(&path)?))
            })
            .collect()
    }
}

impl Installer {
    /// What the jars in an installation's mods folder say, turned off ones
    /// too; a jar is read again only once it changed.
    pub fn local_jars(&self, root: &DataRoot, id: InstanceId) -> Vec<LocalJar> {
        let game = root.game_dir(id);
        let mut found = Vec::new();
        scan(&game, "mods", "mods", None, &mut found);
        found
            .into_iter()
            .filter_map(|item| {
                let path = content_path(&game, &item.path).ok()?;
                Some(LocalJar {
                    manifests: self.manifests_of(&path)?,
                    path: item.path,
                })
            })
            .collect()
    }

    fn manifests_of(&self, path: &Path) -> Option<Vec<mod_meta::Manifest>> {
        let meta = fs::metadata(path).ok()?;
        if meta.len() > MAX_HASHED {
            return None;
        }
        let modified = meta.modified().ok();
        if let Some((size, time, read)) = lock(&self.read).get(path)
            && *size == meta.len()
            && *time == modified
        {
            return Some(read.clone());
        }
        let file = fs::File::open(path).ok()?;
        let read = mod_meta::read(std::io::BufReader::new(file));
        lock(&self.read).insert(path.to_path_buf(), (meta.len(), modified, read.clone()));
        Some(read)
    }
}

/// Recognizes the player's own files by their SHA-1.
pub async fn identify<C: Catalog>(
    catalog: &C,
    files: &[(String, String)],
) -> LauncherResult<Vec<Identified>> {
    let mut out = Vec::new();
    for chunk in files.chunks(200) {
        let hashes: Vec<String> = chunk.iter().map(|(_, h)| h.clone()).collect();
        let found: Vec<Found> = catalog.lookup(&hashes).await?;
        for (path, hash) in chunk {
            if let Some(f) = found.iter().find(|f| f.hash == *hash) {
                out.push(Identified {
                    path: path.clone(),
                    project_id: f.project_id.clone(),
                    version_id: f.version_id.clone(),
                });
            }
        }
    }
    Ok(out)
}

/// Follows projects the catalog joined (the same project on Modrinth and
/// CurseForge becomes one, D-066): the lock's files are looked up by SHA-256,
/// and a file whose version now belongs to another project takes that
/// project's ID, in what it requires too. Only in memory; without the
/// catalog the lock stays as it is.
pub async fn follow_joins<C: Catalog>(catalog: &C, lock: &mut ContentLock) {
    let mut renamed: HashMap<String, String> = HashMap::new();
    let hashes: Vec<String> = lock
        .items
        .iter()
        .take(1000)
        .map(|i| i.sha256.clone())
        .collect();
    for chunk in hashes.chunks(200) {
        let Ok(found) = catalog.lookup(chunk).await else {
            return;
        };
        for item in &lock.items {
            if let Some(f) = found.iter().find(|f| {
                f.hash == item.sha256
                    && f.version_id == item.version_id
                    && f.project_id != item.project_id
                    && super::our_id(&f.project_id)
            }) {
                renamed.insert(item.project_id.clone(), f.project_id.clone());
            }
        }
    }
    if renamed.is_empty() {
        return;
    }
    for item in &mut lock.items {
        if let Some(new) = renamed.get(&item.project_id) {
            item.project_id.clone_from(new);
        }
        for required in item.requires.iter_mut().flatten() {
            if let Some(new) = renamed.get(required) {
                required.clone_from(new);
            }
        }
    }
}

/// What turning a mod off or removing it leaves without something it needs.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NeededBy {
    /// The mods turned on that require it (their names).
    pub needed_by: Vec<String>,
    /// What the launcher added for it that nothing else needs: removed with
    /// it when the player agrees.
    pub unused: Vec<Unused>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Unused {
    /// As on disk now, relative to the game folder.
    pub path: String,
    pub title: String,
}

/// A mod in the folder the catalog knows.
struct KnownMod {
    path: String,
    title: String,
    project: String,
    on: bool,
    dependency: bool,
    requires: Vec<String>,
}

/// Versions asked about at once.
const ASK_AT_ONCE: usize = 16;

impl Installer {
    /// Which mods need the one at `path` (as on disk), and what only it
    /// needed. Without the catalog, what the launcher noted when it
    /// installed them.
    pub async fn needed_by<C: Catalog>(
        &self,
        catalog: Option<&C>,
        root: &DataRoot,
        id: InstanceId,
        identified: &[Identified],
        path: &str,
    ) -> NeededBy {
        let view = list(root, id);
        let mut content_lock = load_lock(root, id);
        if let Some(catalog) = catalog {
            follow_joins(catalog, &mut content_lock).await;
        }
        let mut known: Vec<(KnownMod, Option<String>)> = Vec::new();
        for item in view
            .items
            .iter()
            .filter(|i| i.folder == "mods" && !i.directory)
        {
            let title = item.title.clone().unwrap_or_else(|| item.name.clone());
            let (project, version, ours) = match content_lock.at(enabled_form(&item.path)) {
                Some(o) => (o.project_id.clone(), o.version_id.clone(), Some(o)),
                None => match identified.iter().find(|i| i.path == item.path) {
                    Some(i) => (i.project_id.clone(), i.version_id.clone(), None),
                    None => continue,
                },
            };
            let noted = ours.and_then(|o| o.requires.clone());
            let cached = lock(&self.requires).get(&version).cloned();
            known.push((
                KnownMod {
                    path: item.path.clone(),
                    title,
                    project,
                    on: item.enabled,
                    dependency: ours.is_some_and(|o| o.dependency),
                    requires: noted.clone().or(cached.clone()).unwrap_or_default(),
                },
                (noted.is_none() && cached.is_none()).then_some(version),
            ));
        }
        let Some(target) = known.iter().position(|(k, _)| k.path == path) else {
            return NeededBy::default();
        };
        // What the lock does not say, the catalog does.
        let asks: Vec<(usize, String)> = known
            .iter()
            .enumerate()
            .filter_map(|(i, (_, v))| v.clone().map(|v| (i, v)))
            .collect();
        let chunks = if catalog.is_some() {
            asks.chunks(ASK_AT_ONCE)
        } else {
            [].chunks(ASK_AT_ONCE)
        };
        for chunk in chunks {
            let Some(catalog) = catalog else { break };
            let answers =
                futures_util::future::join_all(chunk.iter().map(|(_, v)| catalog.version(v))).await;
            for ((i, v), answer) in chunk.iter().zip(answers) {
                let Ok(version) = answer else { continue };
                let requires: Vec<String> = version
                    .dependencies
                    .iter()
                    .filter(|d| d.kind == "required")
                    .filter_map(|d| d.project_id.clone())
                    .collect();
                lock(&self.requires).insert(v.clone(), requires.clone());
                known[*i].0.requires = requires;
            }
        }
        let known: Vec<KnownMod> = known.into_iter().map(|(k, _)| k).collect();
        needs(&known, target)
    }
}

/// Who needs `known[target]`, and what goes unused without it.
fn needs(known: &[KnownMod], target: usize) -> NeededBy {
    let gone_project = &known[target].project;
    let mut needed_by: Vec<String> = known
        .iter()
        .enumerate()
        .filter(|(i, k)| *i != target && k.on && k.project != *gone_project)
        .filter(|(_, k)| k.requires.contains(gone_project))
        .map(|(_, k)| k.title.clone())
        .collect();
    needed_by.sort();
    needed_by.dedup();
    // Going with it: the launcher's dependencies only it (or what goes with
    // it) needs; one a mod turned off needs stays for when it is on again.
    let mut going: HashSet<&str> = HashSet::from([gone_project.as_str()]);
    let mut unused = Vec::new();
    loop {
        let mut more = false;
        for k in known {
            if !k.dependency || going.contains(k.project.as_str()) {
                continue;
            }
            let mut users = known
                .iter()
                .filter(|u| u.project != k.project && u.requires.contains(&k.project))
                .peekable();
            if users.peek().is_some() && users.all(|u| going.contains(u.project.as_str())) {
                going.insert(&k.project);
                unused.push(Unused {
                    path: k.path.clone(),
                    title: k.title.clone(),
                });
                more = true;
            }
        }
        if !more {
            break;
        }
    }
    NeededBy { needed_by, unused }
}

/// A file the player put there that the catalog knows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Identified {
    pub path: String,
    pub project_id: String,
    pub version_id: String,
}

/// A newer version for a file in the installation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateOffer {
    /// The file now, relative to the game folder.
    pub path: String,
    pub project_id: String,
    pub from_version_id: String,
    pub to_version_id: String,
    pub to_number: String,
}

/// Newer versions of what the launcher installed and of the player's files
/// the catalog recognized.
pub async fn updates<C: Catalog>(
    catalog: &C,
    target: &Target,
    lock: &ContentLock,
    identified: &[Identified],
    game: &Path,
) -> LauncherResult<Vec<UpdateOffer>> {
    // (path now, project, version, kind)
    let mut items: Vec<(String, String, String, String)> = lock
        .items
        .iter()
        .filter_map(|i| {
            Some((
                present(game, &i.path)?,
                i.project_id.clone(),
                i.version_id.clone(),
                i.kind.clone(),
            ))
        })
        .collect();
    for found in identified {
        let kind = match found.path.split('/').next() {
            Some("mods") => "mod",
            Some("resourcepacks") => "resourcepack",
            Some("shaderpacks") => "shader",
            Some("saves") => "datapack",
            _ => continue,
        };
        items.push((
            found.path.clone(),
            found.project_id.clone(),
            found.version_id.clone(),
            kind.to_owned(),
        ));
    }
    let mut offers = Vec::new();
    let mut asked: HashSet<String> = HashSet::new();
    for (path, project, version, kind) in items.into_iter().take(300) {
        // A mod and its data pack in a world are offered apart.
        if !asked.insert(format!("{kind} {project}")) {
            continue;
        }
        let list = fitting(catalog, &project, &kind, target).await?;
        let Some(newest) = best(&list) else {
            continue;
        };
        let newest_at = list.iter().position(|v| v.id == newest.id);
        let installed_at = list.iter().position(|v| v.id == version);
        let newer = match (newest_at, installed_at) {
            (Some(n), Some(i)) => n < i,
            // What is installed does not fit (anymore): the fitting one is
            // newer only by date.
            (Some(_), None) => lock
                .of_project(&project)
                .is_some_and(|o| newest.published > o.published),
            _ => false,
        };
        if newer {
            offers.push(UpdateOffer {
                path,
                project_id: project,
                from_version_id: version,
                to_version_id: newest.id.clone(),
                to_number: newest.number.clone(),
            });
        }
    }
    Ok(offers)
}

// ----- the game folder -----

/// One entry of a content folder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContentItem {
    /// As on disk now, relative to the game folder.
    pub path: String,
    /// `mods`, `resourcepacks`, `shaderpacks` or `datapacks`.
    pub folder: String,
    /// Its name when on.
    pub name: String,
    /// The world of a data pack.
    pub world: Option<String>,
    pub enabled: bool,
    pub size: u64,
    /// A folder (an unpacked resource pack or shader).
    pub directory: bool,
    // From the catalog, when the launcher installed it.
    pub project_id: Option<String>,
    pub version_id: Option<String>,
    pub title: Option<String>,
    pub version_number: Option<String>,
    pub icon: Option<String>,
    pub dependency: bool,
    /// The modpack the installation was made from brought it.
    pub from_pack: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContentView {
    pub items: Vec<ContentItem>,
    /// Worlds a data pack can go into.
    pub worlds: Vec<String>,
    /// The modpack the installation was made from.
    pub pack: Option<super::pack::PackSummary>,
}

fn takes(folder: &str, name: &str, directory: bool) -> bool {
    let lower = enabled_form(&name.to_ascii_lowercase()).to_owned();
    match folder {
        "mods" => !directory && (lower.ends_with(".jar") || lower.ends_with(".zip")),
        "resourcepacks" | "shaderpacks" => directory || lower.ends_with(".zip"),
        "datapacks" => directory || lower.ends_with(".zip"),
        _ => false,
    }
}

fn scan(
    game: &Path,
    relative: &str,
    folder: &str,
    world: Option<&str>,
    out: &mut Vec<ContentItem>,
) {
    let Ok(entries) = fs::read_dir(game.join(relative)) else {
        return;
    };
    let mut found = 0;
    for entry in entries.filter_map(Result::ok) {
        if found >= MAX_LISTED {
            break;
        }
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        // Not following links.
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        if !plain_name(&name) || meta.file_type().is_symlink() {
            continue;
        }
        let directory = meta.is_dir();
        if !takes(folder, &name, directory) {
            continue;
        }
        found += 1;
        let enabled = directory || !name.ends_with(DISABLED);
        out.push(ContentItem {
            path: format!("{relative}/{name}"),
            folder: folder.to_owned(),
            name: enabled_form(&name).to_owned(),
            world: world.map(str::to_owned),
            enabled,
            size: if directory { 0 } else { meta.len() },
            directory,
            project_id: None,
            version_id: None,
            title: None,
            version_number: None,
            icon: None,
            dependency: false,
            from_pack: false,
        });
    }
}

/// The worlds of an installation.
fn worlds(game: &Path) -> Vec<String> {
    let Ok(entries) = fs::read_dir(game.join("saves")) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .filter_map(Result::ok)
        .filter(|e| e.metadata().is_ok_and(|m| m.is_dir()))
        .filter_map(|e| e.file_name().to_str().map(str::to_owned))
        .filter(|n| plain_name(n))
        .take(500)
        .collect();
    names.sort_by_key(|n| n.to_lowercase());
    names
}

/// The content of an installation's game folder.
pub fn list(root: &DataRoot, id: InstanceId) -> ContentView {
    let game = root.game_dir(id);
    let lock = load_lock(root, id);
    let mut items = Vec::new();
    for folder in ["mods", "resourcepacks", "shaderpacks"] {
        scan(&game, folder, folder, None, &mut items);
    }
    let worlds = worlds(&game);
    // Data packs in worlds the launcher put there.
    let mut seen_worlds = HashSet::new();
    for item in lock.items.iter().filter(|i| i.path.starts_with("saves/")) {
        let parts: Vec<&str> = item.path.split('/').collect();
        if let [_, world, _, _] = parts.as_slice()
            && seen_worlds.insert((*world).to_owned())
        {
            scan(
                &game,
                &format!("saves/{world}/datapacks"),
                "datapacks",
                Some(world),
                &mut items,
            );
        }
    }
    let packed: HashSet<&str> = lock
        .pack
        .iter()
        .flat_map(|p| p.files.iter().chain(&p.overrides).map(|e| e.path.as_str()))
        .collect();
    for item in &mut items {
        item.from_pack = packed.contains(enabled_form(&item.path));
        if let Some(ours) = lock.at(enabled_form(&item.path)) {
            item.project_id = Some(ours.project_id.clone());
            item.version_id = Some(ours.version_id.clone());
            item.title = Some(ours.title.clone());
            item.version_number = Some(ours.version_number.clone());
            item.icon = ours.icon.clone();
            item.dependency = ours.dependency;
        }
    }
    items.sort_by(|a, b| {
        (a.folder.as_str(), a.name.to_lowercase()).cmp(&(b.folder.as_str(), b.name.to_lowercase()))
    });
    let pack = lock.pack.as_ref().map(super::pack::PackSummary::of);
    ContentView {
        items,
        worlds,
        pack,
    }
}

/// Content of an installation a search over all of them found (D-072).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledMatch {
    pub instance_id: InstanceId,
    pub instance_name: String,
    pub item: ContentItem,
}

/// Matches one search over the installations gives at most.
pub const MAX_INSTALLED_MATCHES: usize = 100;

/// What to look for in every installation: words (all of them in its title
/// from the catalog or its file name), or a catalog project by its ID.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InstalledQuery {
    words: Vec<String>,
    project: Option<String>,
}

impl InstalledQuery {
    /// From the player's text (up to 8 words of 64 characters are used) and
    /// a project ID; nothing to look for when both are empty.
    pub fn new(text: &str, project: Option<&str>) -> LauncherResult<Option<Self>> {
        let project = match project.map(str::trim).filter(|p| !p.is_empty()) {
            Some(p) if super::project_ref(p) => Some(p.to_owned()),
            Some(_) => return Err(LauncherError::invalid_input("This search cannot be made.")),
            None => None,
        };
        let words: Vec<String> = text
            .chars()
            .take(200)
            .collect::<String>()
            .to_lowercase()
            .split_whitespace()
            .filter(|w| w.chars().all(|c| !c.is_control()))
            .take(8)
            .map(|w| w.chars().take(64).collect())
            .collect();
        Ok((!words.is_empty() || project.is_some()).then_some(Self { words, project }))
    }

    fn takes(&self, item: &ContentItem) -> bool {
        if let Some(project) = &self.project
            && item.project_id.as_deref() != Some(project.as_str())
        {
            return false;
        }
        let haystack = format!(
            "{} {}",
            item.title.as_deref().unwrap_or_default(),
            item.name
        )
        .to_lowercase();
        self.words.iter().all(|w| haystack.contains(w.as_str()))
    }
}

/// The content of these installations, in their order, that a query finds.
pub fn find_installed(
    root: &DataRoot,
    instances: &[(InstanceId, String)],
    query: &InstalledQuery,
) -> Vec<InstalledMatch> {
    let mut found = Vec::new();
    for (id, name) in instances {
        for item in list(root, *id).items {
            if found.len() >= MAX_INSTALLED_MATCHES {
                return found;
            }
            if query.takes(&item) {
                found.push(InstalledMatch {
                    instance_id: *id,
                    instance_name: name.clone(),
                    item,
                });
            }
        }
    }
    found
}

/// The same over every installation, the last played first.
pub fn find_in_all(
    state: &crate::state::AppState,
    query: &InstalledQuery,
) -> LauncherResult<Vec<InstalledMatch>> {
    let root = state.data_root().ok_or_else(storage_gone)?;
    let mut instances: Vec<InstanceFile> = state
        .instance_ids()
        .into_iter()
        .filter_map(|id| state.instance(id))
        .collect();
    instances.sort_by(|a, b| {
        b.last_played_at.cmp(&a.last_played_at).then_with(|| {
            a.display_name
                .to_lowercase()
                .cmp(&b.display_name.to_lowercase())
        })
    });
    let instances: Vec<(InstanceId, String)> = instances
        .into_iter()
        .map(|i| (i.id, i.display_name))
        .collect();
    Ok(find_installed(&root, &instances, query))
}

/// Turns a file on or off (adds or removes `.disabled`).
pub fn set_enabled(
    root: &DataRoot,
    id: InstanceId,
    path: &str,
    enabled: bool,
) -> LauncherResult<()> {
    let game = root.game_dir(id);
    let from = content_path(&game, path)?;
    let meta = fs::symlink_metadata(&from).map_err(|_| no_such_file())?;
    if !meta.is_file() {
        return Err(bad_path());
    }
    let is_off = path.ends_with(DISABLED);
    if is_off != enabled {
        return Ok(());
    }
    let to_relative = if enabled {
        enabled_form(path).to_owned()
    } else {
        format!("{path}{DISABLED}")
    };
    let to = content_path(&game, &to_relative)?;
    if fs::symlink_metadata(&to).is_ok() {
        return Err(LauncherError::invalid_input(
            "A file with this name is already in the folder.",
        ));
    }
    fs::rename(&from, &to).map_err(|e| LauncherError::from_io(&e, "Renaming the file"))
}

/// Deletes a file from a content folder and from the lock.
pub fn remove(root: &DataRoot, id: InstanceId, path: &str) -> LauncherResult<()> {
    let game = root.game_dir(id);
    let target = content_path(&game, path)?;
    let meta = fs::symlink_metadata(&target).map_err(|_| no_such_file())?;
    if !meta.is_file() {
        return Err(LauncherError::invalid_input(
            "Folders are removed in the file manager.",
        ));
    }
    fs::remove_file(&target).map_err(|e| LauncherError::from_io(&e, "Deleting the file"))?;
    let mut lock_file = load_lock(root, id);
    let canonical = enabled_form(path);
    let before = lock_file.items.len();
    lock_file.items.retain(|i| i.path != canonical);
    if lock_file.items.len() != before {
        save_lock(root, id, &lock_file)?;
    }
    Ok(())
}

/// The installation and the data folder, or why there are none.
pub fn instance_of(
    state: &crate::state::AppState,
    id: InstanceId,
) -> LauncherResult<(DataRoot, InstanceFile)> {
    let instance = state.instance(id).ok_or_else(instance_gone)?;
    let root = state.data_root().ok_or_else(storage_gone)?;
    Ok((root, instance))
}
