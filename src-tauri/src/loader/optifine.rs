//! OptiFine on its own, without Forge (D-039, D-066).
//!
//! OptiFine is on no Maven repository. The mods catalog keeps its builds from
//! optifine.net (D-066), and the launcher takes them from there, pinned by the
//! catalog's SHA-256. A build's jar is also its installer. What the official
//! installer does for a vanilla game, the launcher does itself, as it runs
//! Forge's installer (see [`super::processors`]):
//!
//! - `optifine.Patcher` builds the library `optifine:OptiFine:<build>` from
//!   the vanilla client jar and the build's jar, with the private Java;
//! - `launchwrapper-of-<v>.jar`, packed at the root of the jar, is unpacked as
//!   the library `optifine:launchwrapper-of:<v>`;
//! - the game starts through LaunchWrapper with OptiFine's tweaker.
//!
//! Builds older than `launchwrapper-of` (Minecraft 1.7.10) are refused.

use std::collections::BTreeMap;

use sha2::Digest as _;

use super::coordinate::Coordinate;
use super::manifest::{
    EmbeddedFile, Installer, LoaderLibrary, LoaderManifest, MAX_FILE_BYTES, Processor,
    installer_entry, maven_path, not_for_game, old_installer,
};
use super::meta::LoaderVersion;
use super::{LoaderKind, valid_version};
use crate::catalog::source::Fetch;
use crate::download::resolve_managed_path;
use crate::error::{ErrorCode, LauncherError, LauncherResult};
use crate::install::service::BoxFuture;
use crate::storage::atomic;
use crate::storage::document::SCHEMA_VERSION;
use crate::storage::paths::{DataRoot, Sha1};

/// OptiFine's project in the mods catalog.
pub const PROJECT: &str = "optifine";
const MAIN_CLASS: &str = "net.minecraft.launchwrapper.Launch";
const TWEAKER: &str = "optifine.OptiFineTweaker";
/// The installer's own tool that builds the patched library.
const PATCHER: &str = "optifine.Patcher";
/// Names the version of the packed `launchwrapper-of-<v>.jar`.
const LAUNCHWRAPPER_VERSION: &str = "launchwrapper-of.txt";

/// One OptiFine build the catalog serves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    /// `1.21.1_HD_U_J1`: the game version and the edition.
    pub id: String,
    pub game: String,
    /// `HD U J1`.
    pub label: String,
    /// False for previews.
    pub stable: bool,
    /// Where the catalog serves the jar.
    pub url: String,
    pub sha256: String,
    pub size: u64,
}

/// Where OptiFine builds come from: the mods catalog, or a stand-in in tests.
pub trait Releases: Send + Sync {
    /// The builds served now, newest first; for one game version when given.
    fn releases<'a>(&'a self, game: Option<&'a str>)
    -> BoxFuture<'a, LauncherResult<Vec<Release>>>;
    /// A served file, at most `max` bytes.
    fn file<'a>(&'a self, url: &'a str, max: usize) -> BoxFuture<'a, LauncherResult<Vec<u8>>>;
}

/// The mods catalog's builds: its API for the list, its file hosts for jars.
pub struct CatalogReleases {
    pub catalog: crate::mods::Client,
    /// A client for [`crate::net::SourcePolicy::ModsCatalog`].
    pub files: crate::net::PublicClient,
}

impl Releases for CatalogReleases {
    fn releases<'a>(
        &'a self,
        game: Option<&'a str>,
    ) -> BoxFuture<'a, LauncherResult<Vec<Release>>> {
        Box::pin(async move {
            let versions =
                crate::mods::Catalog::versions(&self.catalog, PROJECT, game, None).await?;
            Ok(versions.iter().filter_map(release_of).collect())
        })
    }

    fn file<'a>(&'a self, url: &'a str, max: usize) -> BoxFuture<'a, LauncherResult<Vec<u8>>> {
        Box::pin(async move {
            match self.files.fetch(url, None, None, max).await {
                Ok(fetched) => Ok(fetched.body),
                // R2 first, our server as its reserve.
                Err(error) => match url::Url::parse(url)
                    .ok()
                    .and_then(|u| crate::mods::reserve_url(&u))
                {
                    Some(reserve) => Ok(self
                        .files
                        .fetch(reserve.as_str(), None, None, max)
                        .await?
                        .body),
                    None => Err(error),
                },
            }
        })
    }
}

/// An edition as OptiFine names it: `HD_U_J1`, `HD_U_J7_pre11`.
fn valid_edition(edition: &str) -> bool {
    (1..=48).contains(&edition.len())
        && edition
            .bytes()
            .next()
            .is_some_and(|b| b.is_ascii_alphabetic())
        && edition
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

/// The edition part of a build id for a game version.
pub fn edition(id: &str, game: &str) -> Option<String> {
    let edition = id.strip_prefix(game)?.strip_prefix('_')?;
    (valid_version(game) && valid_version(id) && valid_edition(edition)).then(|| edition.to_owned())
}

/// The build id a file name gives for a game version:
/// `OptiFine_1.21.1_HD_U_J1.jar` and `preview_OptiFine_1.21.10_HD_U_J7_pre11.jar`
/// give `1.21.1_HD_U_J1` and `1.21.10_HD_U_J7_pre11`.
pub fn build_id(file_name: &str, game: &str) -> Option<String> {
    let stem = file_name.strip_suffix(".jar")?;
    let stem = stem.strip_prefix("preview_").unwrap_or(stem);
    let id = stem.strip_prefix("OptiFine_")?;
    edition(id, game).map(|_| id.to_owned())
}

/// A catalog version as a build: for one game version, its jar served.
pub fn release_of(version: &crate::mods::Version) -> Option<Release> {
    let [game] = version.games.as_slice() else {
        return None;
    };
    let file = version.main_file()?;
    if file.held {
        return None;
    }
    let (sha256, url) = (file.sha256.clone()?, file.url.clone()?);
    let id = build_id(&file.name, game)?;
    let label = edition(&id, game)?.replace('_', " ");
    Some(Release {
        id,
        game: game.clone(),
        label,
        stable: version.channel == "release",
        url,
        sha256,
        size: file.size,
    })
}

/// Game versions with a build, in no particular order.
pub async fn game_versions(releases: &dyn Releases) -> LauncherResult<Vec<String>> {
    let games: std::collections::BTreeSet<String> = releases
        .releases(None)
        .await?
        .into_iter()
        .map(|r| r.game)
        .collect();
    Ok(games.into_iter().collect())
}

/// The builds for a game version, newest first; the newest release is the
/// recommended one.
pub async fn loader_versions(
    releases: &dyn Releases,
    game: &str,
) -> LauncherResult<Vec<LoaderVersion>> {
    if !valid_version(game) {
        return Err(LauncherError::invalid_input("Choose a Minecraft version."));
    }
    let mut recommended = false;
    Ok(releases
        .releases(Some(game))
        .await?
        .into_iter()
        .filter(|r| r.game == game)
        .map(|r| {
            let first = r.stable && !recommended;
            recommended |= first;
            LoaderVersion {
                id: r.id,
                label: r.label,
                stable: r.stable,
                recommended: first,
            }
        })
        .collect())
}

fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(sha2::Sha256::digest(bytes))
}

fn sha1_hex(bytes: &[u8]) -> String {
    hex::encode(sha1::Sha1::digest(bytes))
}

fn storage_error(error: &std::io::Error) -> LauncherError {
    LauncherError::from_io(error, "Saving mod loader files")
}

/// Where a build's jar lives in the Maven tree.
pub fn installer_coordinate(id: &str) -> String {
    format!("optifine:OptiFine:{id}:installer")
}

/// Resolves a build for a game version into a manifest; the jar is kept in
/// the Maven tree, where the installation finds it.
pub async fn resolve(
    releases: &dyn Releases,
    root: &DataRoot,
    game: &str,
    id: &str,
) -> LauncherResult<LoaderManifest> {
    if edition(id, game).is_none() {
        return Err(not_for_game(LoaderKind::OptiFine));
    }
    let release = releases
        .releases(Some(game))
        .await?
        .into_iter()
        .find(|r| r.id == id && r.game == game)
        .ok_or_else(|| {
            LauncherError::invalid_input("Choose a mod loader version from the list.")
        })?;
    let path = Coordinate::parse(&installer_coordinate(id))?.path();
    let destination = resolve_managed_path(root, &maven_path(&path))?;
    let bytes = match std::fs::read(&destination) {
        Ok(bytes) if sha256_hex(&bytes) == release.sha256 => bytes,
        _ => {
            let body = releases.file(&release.url, MAX_FILE_BYTES).await?;
            if sha256_hex(&body) != release.sha256 {
                return Err(LauncherError::templated(
                    ErrorCode::HashMismatch,
                    "Downloaded {what} did not match its expected checksum.",
                    crate::error::params([("what", "mod loader installer".into())]),
                ));
            }
            if let Some(parent) = destination.parent() {
                std::fs::create_dir_all(parent).map_err(|e| storage_error(&e))?;
            }
            atomic::write(&destination, &body).map_err(|e| storage_error(&e))?;
            body
        }
    };
    manifest(&bytes, &release, &path)
}

/// The manifest of a build from its jar, which sits at `path` in the Maven tree.
pub fn manifest(jar: &[u8], release: &Release, path: &str) -> LauncherResult<LoaderManifest> {
    let (id, game) = (release.id.as_str(), release.game.as_str());
    let edition = edition(id, game).ok_or_else(|| not_for_game(LoaderKind::OptiFine))?;
    // Builds without it start another way (1.7.10), not supported.
    let wrapper = installer_entry(jar, LAUNCHWRAPPER_VERSION)
        .map_err(|_| old_installer(LoaderKind::OptiFine))?;
    let wrapper = String::from_utf8_lossy(&wrapper).trim().to_owned();
    if wrapper.is_empty()
        || wrapper.len() > 16
        || !wrapper.bytes().all(|b| b.is_ascii_digit() || b == b'.')
    {
        return Err(old_installer(LoaderKind::OptiFine));
    }
    installer_entry(jar, "optifine/Patcher.class")
        .map_err(|_| old_installer(LoaderKind::OptiFine))?;
    let entry = format!("launchwrapper-of-{wrapper}.jar");
    let wrapper_jar =
        installer_entry(jar, &entry).map_err(|_| old_installer(LoaderKind::OptiFine))?;
    let wrapper_name = format!("optifine:launchwrapper-of:{wrapper}");
    let wrapper_file = EmbeddedFile {
        path: Coordinate::parse(&wrapper_name)?.path(),
        sha1: Sha1::parse(&sha1_hex(&wrapper_jar))?,
        size: wrapper_jar.len() as u64,
        entry: Some(entry),
    };
    let library = format!("optifine:OptiFine:{id}");
    Ok(LoaderManifest {
        schema_version: SCHEMA_VERSION,
        kind: LoaderKind::OptiFine,
        version: id.to_owned(),
        game_version: game.to_owned(),
        source: release.url.clone(),
        profile_id: format!("{game}-OptiFine_{edition}"),
        main_class: MAIN_CLASS.to_owned(),
        libraries: vec![
            // Built by the patcher.
            LoaderLibrary {
                name: library.clone(),
                path: Coordinate::parse(&library)?.path(),
                url: String::new(),
                sha1: None,
                size: None,
                rules: None,
            },
            LoaderLibrary {
                name: wrapper_name,
                path: wrapper_file.path.clone(),
                url: String::new(),
                sha1: Some(wrapper_file.sha1.clone()),
                size: Some(wrapper_file.size),
                rules: None,
            },
        ],
        jvm_arguments: Vec::new(),
        game_arguments: vec!["--tweakClass".into(), TWEAKER.into()],
        minecraft_arguments: None,
        installer: Some(Installer {
            url: release.url.clone(),
            path: path.to_owned(),
            sha1: Sha1::parse(&sha1_hex(jar))?,
            size: jar.len() as u64,
            spec: 0,
            data: BTreeMap::new(),
            processors: vec![Processor {
                jar: installer_coordinate(id),
                classpath: Vec::new(),
                args: vec![
                    "{MINECRAFT_JAR}".into(),
                    "{INSTALLER}".into(),
                    format!("[{library}]"),
                ],
                outputs: BTreeMap::new(),
                main_class: Some(PATCHER.to_owned()),
            }],
            libraries: Vec::new(),
            embedded: vec![wrapper_file],
        }),
        server_arguments: None,
    })
}

#[cfg(test)]
mod tests;
