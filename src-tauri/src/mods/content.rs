//! What the catalog puts into an installation's game folder (D-066): mods,
//! resource packs, shaders and data packs, with the mods they need.
//!
//! - A plan is made in the core from the catalog's own answers: the version's
//!   game versions and loaders must fit the installation (a mod strictly; a
//!   resource pack, shader or data pack with a warning), required
//!   dependencies are added (newest release that fits, 8 deep, 50 files), and
//!   an incompatible project already installed stops the plan.
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
/// Files added with one install: the chosen one and what it needs.
const MAX_PLAN: usize = 50;
const MAX_DEPTH: usize = 8;
/// Files listed per folder.
const MAX_LISTED: usize = 2000;
/// Files the catalog is asked to recognize at once.
const MAX_IDENTIFY: usize = 1000;
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
    pub installed_at: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContentLock {
    pub schema_version: u64,
    #[serde(default)]
    pub items: Vec<LockedItem>,
}

impl ContentLock {
    fn of_project(&self, project: &str) -> Option<&LockedItem> {
        self.items.iter().find(|i| i.project_id == project)
    }

    fn at(&self, path: &str) -> Option<&LockedItem> {
        self.items.iter().find(|i| i.path == path)
    }
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
        },
    }
}

fn save_lock(root: &DataRoot, id: InstanceId, lock: &ContentLock) -> LauncherResult<()> {
    crate::storage::document::save(&lock_path(root, id), lock, None)
        .map_err(|e| LauncherError::from_io(&e, "Saving the list of installed content"))
}

// ----- the installation's side -----

/// What an installation runs: its game version and loader.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub game: String,
    pub loader: Option<LoaderKind>,
}

impl Target {
    pub fn of(instance: &InstanceFile) -> Self {
        Self {
            game: instance.version_id.clone(),
            loader: instance.loader.as_ref().map(|l| l.kind),
        }
    }

    /// The loaders whose mods run here; Quilt also runs Fabric's.
    pub fn mod_loaders(&self) -> &'static [&'static str] {
        match self.loader {
            Some(LoaderKind::Fabric) => &["fabric"],
            Some(LoaderKind::Quilt) => &["quilt", "fabric"],
            Some(LoaderKind::Forge) => &["forge"],
            Some(LoaderKind::NeoForge) => &["neoforge"],
            None => &[],
        }
    }
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
    /// `name_taken`: a file the player put there has this name.
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

/// The plan for putting a version (and what it needs) into an installation.
pub async fn plan<C: Catalog>(
    catalog: &C,
    game: &Path,
    target: &Target,
    lock: &ContentLock,
    wanted: &Wanted<'_>,
) -> LauncherResult<Plan> {
    let version = catalog.version(wanted.version).await?;
    let project = catalog.project(&version.project_id).await?;
    let folder = folder_of(&project.kind, wanted.world)?;
    if let Some(world) = wanted.world.filter(|_| project.kind == "datapack")
        && !game.join("saves").join(world).is_dir()
    {
        return Err(LauncherError::invalid_input(
            "This world is not in the installation.",
        ));
    }
    let found = misfit(&project.kind, &version, target);
    if project.kind == "mod"
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

    let mut plan = Plan {
        items: Vec::new(),
        missing: Vec::new(),
        conflicts: Vec::new(),
        misfit: found,
        total_bytes: 0,
        approval: None,
    };
    let mut seen: HashSet<String> = HashSet::new();
    // (version, project title and kind, depth, dependency)
    let mut queue = vec![(
        version,
        project.title.clone(),
        project.icon.clone(),
        0,
        false,
    )];
    seen.insert(project.id.clone());
    while let Some((version, title, icon, depth, dependency)) = queue.pop() {
        if plan.items.len() >= MAX_PLAN {
            return Err(LauncherError::invalid_input(
                "This needs too many other projects to install at once.",
            ));
        }
        let kind = if dependency {
            "mod"
        } else {
            project.kind.as_str()
        };
        let file = version
            .main_file()
            .filter(|f| catalog_file_name(&f.name, kind))
            .ok_or_else(|| {
                LauncherError::invalid_input(
                    "This version has no file the launcher can put into an installation.",
                )
            })?;
        let path = format!("{folder}/{}", file.name);
        let replaces = match lock.of_project(&version.project_id) {
            Some(old) => Some(old.path.clone()),
            None if !dependency => wanted.replaces.map(|r| enabled_form(r).to_owned()),
            None => None,
        }
        .filter(|p| present(game, p).is_some());
        let mut same = false;
        if let Some(there) = present(game, &path) {
            match lock.at(&path) {
                Some(o) if o.project_id != version.project_id => plan.conflicts.push(Conflict {
                    path: there,
                    reason: "name_taken".to_owned(),
                    title: Some(o.title.clone()),
                }),
                Some(o) => same = file.sha256.as_deref() == Some(o.sha256.as_str()),
                None if replaces.as_deref() == Some(path.as_str()) => {}
                None => plan.conflicts.push(Conflict {
                    path: there,
                    reason: "name_taken".to_owned(),
                    title: None,
                }),
            }
        }
        let replaces = replaces.filter(|r| !(same && *r == path));
        plan.items.push(PlanItem {
            project_id: version.project_id.clone(),
            title,
            kind: kind.to_owned(),
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
        });
        if !same {
            plan.total_bytes += file.size;
        }
        if kind != "mod" {
            continue;
        }
        for needed in &version.dependencies {
            match needed.kind.as_str() {
                "incompatible" => {
                    if let Some(other) = needed
                        .project_id
                        .as_deref()
                        .and_then(|p| lock.of_project(p))
                        && present(game, &other.path).is_some()
                    {
                        plan.conflicts.push(Conflict {
                            path: other.path.clone(),
                            reason: "incompatible".to_owned(),
                            title: Some(other.title.clone()),
                        });
                    }
                }
                "required" => {
                    let Some(project_id) = needed.project_id.clone() else {
                        plan.missing.push(Missing {
                            project_id: None,
                            title: None,
                        });
                        continue;
                    };
                    if !seen.insert(project_id.clone()) {
                        continue;
                    }
                    // Installed and there: nothing to add.
                    if lock
                        .of_project(&project_id)
                        .is_some_and(|o| present(game, &o.path).is_some())
                    {
                        continue;
                    }
                    if depth + 1 > MAX_DEPTH {
                        continue;
                    }
                    let other = catalog.project(&project_id).await.ok();
                    let pinned = match &needed.version_id {
                        Some(id) => catalog
                            .version(id)
                            .await
                            .ok()
                            .filter(|v| misfit("mod", v, target).is_none()),
                        None => None,
                    };
                    let chosen = match pinned {
                        Some(v) => Some(v),
                        None => best(&fitting(catalog, &project_id, "mod", target).await?).cloned(),
                    };
                    match chosen {
                        Some(v) => queue.push((
                            v,
                            other.as_ref().map(|p| p.title.clone()).unwrap_or_default(),
                            other.as_ref().and_then(|p| p.icon.clone()),
                            depth + 1,
                            true,
                        )),
                        None => plan.missing.push(Missing {
                            project_id: Some(project_id),
                            title: other.map(|p| p.title),
                        }),
                    }
                }
                _ => {}
            }
        }
    }
    Ok(plan)
}

// ----- installing -----

/// What the interface sees while an install runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub instance_id: String,
    /// `fetching` (the catalog brings files from the source), `downloading`,
    /// `done`, `failed` or `cancelled`.
    pub phase: &'static str,
    /// Files the catalog still fetches, or files downloaded.
    pub files: u64,
    pub total_files: u64,
    pub bytes: u64,
    pub total_bytes: u64,
}

struct Sink<'a> {
    instance: String,
    emit: &'a (dyn Fn(Progress) + Send + Sync),
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
    transport: crate::download::transport::HttpTransport,
    running: Mutex<HashMap<InstanceId, CancelToken>>,
    /// SHA-1 of files the player put there, by path.
    hashes: Mutex<HashMap<PathBuf, Hashed>>,
}

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
            lock_file
                .items
                .retain(|i| i.project_id != item.project_id && i.path != item.path);
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
                installed_at: now(),
            });
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
        if !asked.insert(project.clone()) {
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
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContentView {
    pub items: Vec<ContentItem>,
    /// Worlds a data pack can go into.
    pub worlds: Vec<String>,
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
    for item in &mut items {
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
    ContentView { items, worlds }
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
