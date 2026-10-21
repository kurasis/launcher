//! Resolving one loader version into a [`LoaderManifest`] (D-039).
//!
//! The manifest is the launcher's own normalized record of a loader version: its
//! version profile with every library pinned to a Maven path, a URL and a SHA-1,
//! and for Forge and NeoForge the installer and its client-side processors. It
//! is stored under its own SHA-1; the install lock names that hash, so planning
//! from a lock never needs the network.
//!
//! Trust anchors: the meta services' published hashes where they exist, otherwise
//! the `.sha1` file next to each artifact on the project's Maven repository, all
//! fetched over HTTPS from the allowlisted hosts. Files fetched to learn their size
//! are verified and kept in the Maven tree, so the installation reuses them.

use std::collections::BTreeMap;
use std::io::{Cursor, Read};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha1::Digest as _;

use super::coordinate::Coordinate;
use super::meta::{
    FABRIC_META, FORGE_MAVEN, NEOFORGE_LEGACY_GAME, NEOFORGE_MAVEN, QUILT_META, forge_supported,
    neoforge_game,
};
use super::{LoaderChoice, LoaderKind, valid_version};
use crate::catalog::source::Fetch;
use crate::download::resolve_managed_path;
use crate::error::{ErrorCode, LauncherError, LauncherResult};
use crate::install::descriptor::{unsupported, valid_main_class, validate_relative_path};
use crate::net::SourcePolicy;
use crate::storage::atomic;
use crate::storage::document::SCHEMA_VERSION;
use crate::storage::paths::{DataRoot, SafeName, Sha1};

/// DATA_ROOT-relative prefix of the Maven tree ([`DataRoot::maven_root`]).
pub const MAVEN_PREFIX: &str = "shared/maven";
/// Bound for version and install profiles.
pub const MAX_PROFILE_BYTES: usize = 4 * 1024 * 1024;
/// Bound for one installer or library fetched while resolving.
pub const MAX_FILE_BYTES: usize = 128 * 1024 * 1024;
/// Bound for one file inside an installer.
const MAX_ENTRY_BYTES: u64 = 128 * 1024 * 1024;

/// One library of a loader profile or of an installer's tools.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoaderLibrary {
    /// Maven coordinate.
    pub name: String,
    /// Path inside the Maven tree.
    pub path: String,
    /// Empty when the installer provides the file: packed inside it, or built by
    /// its processors.
    #[serde(default)]
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha1: Option<Sha1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rules: Option<Value>,
}

/// A file packed inside an installer for a path in the Maven tree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmbeddedFile {
    pub path: String,
    pub sha1: Sha1,
    pub size: u64,
    /// The installer entry, when it is not `maven/` + `path` (the universal jar of
    /// an old Forge installer).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry: Option<String>,
}

impl EmbeddedFile {
    /// Where the file is inside the installer.
    pub fn entry_name(&self) -> String {
        self.entry
            .clone()
            .unwrap_or_else(|| format!("maven/{}", self.path))
    }
}

/// Where libraries named without a repository come from.
const MOJANG_LIBRARIES: &str = "https://libraries.minecraft.net";
/// Keys of an install profile that are not needed later, and large (base64 images).
const INSTALLER_EXTRAS: [&str; 6] = [
    "icon",
    "logo",
    "welcome",
    "mirrorList",
    "_comment",
    "_comment_",
];

/// One client-side installer processor: a Java tool run with arguments.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Processor {
    /// Coordinate of the tool jar.
    pub jar: String,
    /// Coordinates of the tool's dependencies.
    pub classpath: Vec<String>,
    /// Argument templates (`{DATA}`, `[coordinate]`).
    pub args: Vec<String>,
    /// Output file template → expected SHA-1 template.
    #[serde(default)]
    pub outputs: BTreeMap<String, String>,
}

/// A Forge or NeoForge installer and its install profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Installer {
    pub url: String,
    /// Path inside the Maven tree.
    pub path: String,
    pub sha1: Sha1,
    pub size: u64,
    /// `spec` of the install profile (0 or 1).
    pub spec: u64,
    /// Client values of the install profile's `data`.
    pub data: BTreeMap<String, String>,
    pub processors: Vec<Processor>,
    /// The processors' tools and inputs.
    pub libraries: Vec<LoaderLibrary>,
    pub embedded: Vec<EmbeddedFile>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoaderManifest {
    pub schema_version: u64,
    pub kind: LoaderKind,
    pub version: String,
    pub game_version: String,
    /// Where the profile came from; provenance only.
    pub source: String,
    /// The loader profile's own version id, `${version_name}` at launch.
    pub profile_id: String,
    pub main_class: String,
    /// Classpath libraries in profile order.
    pub libraries: Vec<LoaderLibrary>,
    /// Added after the game's JVM arguments.
    #[serde(default)]
    pub jvm_arguments: Vec<Value>,
    /// Added after the game's arguments.
    #[serde(default)]
    pub game_arguments: Vec<Value>,
    /// A legacy `minecraftArguments` string that replaces the game's (Forge 1.12.2).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minecraft_arguments: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub installer: Option<Installer>,
}

impl LoaderManifest {
    pub fn choice(&self) -> LoaderChoice {
        LoaderChoice {
            kind: self.kind,
            version: self.version.clone(),
        }
    }
}

fn sha1_hex(bytes: &[u8]) -> String {
    hex::encode(sha1::Sha1::digest(bytes))
}

fn hash_mismatch(what: &str) -> LauncherError {
    LauncherError::templated(
        ErrorCode::HashMismatch,
        "Downloaded {what} did not match its expected checksum.",
        crate::error::params([("what", what.into())]),
    )
}

fn storage_error(error: &std::io::Error) -> LauncherError {
    LauncherError::from_io(error, "Saving mod loader files")
}

fn optional_sha1(value: Option<&Value>) -> LauncherResult<Option<Sha1>> {
    match value.and_then(Value::as_str) {
        None | Some("") => Ok(None),
        Some(text) => Sha1::parse(text).map(Some),
    }
}

/// The DATA_ROOT-relative path of a Maven-tree file.
pub fn maven_path(path: &str) -> String {
    format!("{MAVEN_PREFIX}/{path}")
}

/// A stored manifest, if present and intact.
pub fn load(root: &DataRoot, sha1: &Sha1) -> Option<LoaderManifest> {
    let bytes = std::fs::read(root.loader_manifest(sha1)).ok()?;
    if sha1_hex(&bytes) != sha1.as_str() {
        return None;
    }
    serde_json::from_slice(&bytes).ok()
}

/// Stores a manifest under its SHA-1 and returns the hash.
pub fn store(root: &DataRoot, manifest: &LoaderManifest) -> LauncherResult<Sha1> {
    let bytes = serde_json::to_vec(manifest)
        .map_err(|_| LauncherError::internal("A mod loader record could not be written."))?;
    let sha1 = Sha1::parse(&sha1_hex(&bytes))?;
    let path = root.loader_manifest(&sha1);
    if std::fs::read(&path).is_ok_and(|current| current == bytes) {
        return Ok(sha1);
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| storage_error(&e))?;
    }
    atomic::write(&path, &bytes).map_err(|e| storage_error(&e))?;
    Ok(sha1)
}

/// The unsupported-version error naming the loader.
fn old_installer(kind: LoaderKind) -> LauncherError {
    LauncherError::templated(
        ErrorCode::UnsupportedVersionMetadata,
        "This {loader} version uses an older installer format the launcher does not \
         support. Choose a newer {loader} version.",
        crate::error::params([("loader", kind.name().to_owned().into())]),
    )
}

fn not_for_game(kind: LoaderKind) -> LauncherError {
    LauncherError::templated(
        ErrorCode::InvalidInput,
        "This {loader} version is not made for the chosen Minecraft version.",
        crate::error::params([("loader", kind.name().to_owned().into())]),
    )
}

/// Where a Forge or NeoForge installer lives: its URL and Maven path.
pub fn installer_location(
    kind: LoaderKind,
    game: &str,
    version: &str,
) -> LauncherResult<(String, String)> {
    let (repository, group, artifact) = match kind {
        LoaderKind::Forge => {
            if !forge_supported(game, version) {
                return Err(if version.starts_with(&format!("{game}-")) {
                    old_installer(kind)
                } else {
                    not_for_game(kind)
                });
            }
            (FORGE_MAVEN, "net.minecraftforge", "forge")
        }
        LoaderKind::NeoForge if game == NEOFORGE_LEGACY_GAME => {
            if !version.starts_with(&format!("{game}-")) {
                return Err(not_for_game(kind));
            }
            (NEOFORGE_MAVEN, "net.neoforged", "forge")
        }
        LoaderKind::NeoForge => {
            if neoforge_game(version).as_deref() != Some(game) {
                return Err(not_for_game(kind));
            }
            (NEOFORGE_MAVEN, "net.neoforged", "neoforge")
        }
        LoaderKind::Fabric | LoaderKind::Quilt => {
            return Err(LauncherError::internal("This loader has no installer."));
        }
    };
    let coordinate = Coordinate::parse(&format!("{group}:{artifact}:{version}:installer"))?;
    let path = coordinate.path();
    Ok((format!("{repository}/{path}"), path))
}

/// Arguments of a version profile: modern lists, or the legacy string.
fn arguments(profile: &Value) -> LauncherResult<(Vec<Value>, Vec<Value>, Option<String>)> {
    let list = |key: &str| -> LauncherResult<Vec<Value>> {
        match profile.pointer(&format!("/arguments/{key}")) {
            None => Ok(Vec::new()),
            Some(Value::Array(items)) => Ok(items.clone()),
            Some(_) => Err(unsupported("loader arguments")),
        }
    };
    let legacy = match profile.get("minecraftArguments") {
        None => None,
        Some(Value::String(text)) if !text.trim().is_empty() => Some(text.clone()),
        Some(_) => return Err(unsupported("loader minecraftArguments")),
    };
    Ok((list("jvm")?, list("game")?, legacy))
}

fn profile_identity(profile: &Value, game: &str) -> LauncherResult<(String, String)> {
    if profile.get("inheritsFrom").and_then(Value::as_str) != Some(game) {
        return Err(unsupported("loader inheritsFrom"));
    }
    let id = profile
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| unsupported("loader id"))?;
    // The id names the client jar at launch (Forge's ignore list matches it).
    SafeName::parse(id).map_err(|_| unsupported("loader id"))?;
    let main_class = profile
        .get("mainClass")
        .and_then(Value::as_str)
        .filter(|m| valid_main_class(m))
        .ok_or_else(|| unsupported("loader mainClass"))?;
    Ok((id.to_owned(), main_class.to_owned()))
}

/// Resolves loader versions with the network; see the module documentation.
pub struct Resolver<'a> {
    /// Client for the mod loader hosts.
    pub loaders: &'a dyn Fetch,
    /// Client for Mojang's artifact hosts, which Forge profiles also name.
    pub mojang: &'a dyn Fetch,
    pub root: &'a DataRoot,
}

impl Resolver<'_> {
    /// Resolves and stores the manifest of a loader version for a game version.
    pub async fn resolve(
        &self,
        choice: &LoaderChoice,
        game: &str,
    ) -> LauncherResult<(LoaderManifest, Sha1)> {
        choice.validate()?;
        if !valid_version(game) {
            return Err(LauncherError::invalid_input("Choose a Minecraft version."));
        }
        let manifest = match choice.kind {
            LoaderKind::Fabric | LoaderKind::Quilt => {
                self.meta_profile(choice.kind, game, &choice.version)
                    .await?
            }
            LoaderKind::Forge | LoaderKind::NeoForge => {
                self.installer(choice.kind, game, &choice.version).await?
            }
        };
        let sha1 = store(self.root, &manifest)?;
        Ok((manifest, sha1))
    }

    /// Resolves and stores the dedicated server's manifest of a loader version
    /// (W12): for Fabric and Quilt the meta service's server profile, whose
    /// main class starts Mojang's server JAR named by `fabric.gameJarPath`
    /// (Quilt: `loader.gameJarPath`).
    pub async fn resolve_server(
        &self,
        choice: &LoaderChoice,
        game: &str,
    ) -> LauncherResult<(LoaderManifest, Sha1)> {
        choice.validate()?;
        if !valid_version(game) {
            return Err(LauncherError::invalid_input("Choose a Minecraft version."));
        }
        let manifest = match choice.kind {
            LoaderKind::Fabric | LoaderKind::Quilt => {
                self.meta_profile_of(choice.kind, game, &choice.version, "server")
                    .await?
            }
            LoaderKind::Forge | LoaderKind::NeoForge => {
                return Err(LauncherError::invalid_input(
                    "Worlds with this mod loader come later; Fabric and Quilt work now.",
                ));
            }
        };
        let sha1 = store(self.root, &manifest)?;
        Ok((manifest, sha1))
    }

    fn client_for(&self, url: &str) -> LauncherResult<&dyn Fetch> {
        Ok(match SourcePolicy::for_artifact(url)?.0 {
            SourcePolicy::ModLoaders => self.loaders,
            _ => self.mojang,
        })
    }

    async fn get(&self, url: &str, max: usize) -> LauncherResult<Vec<u8>> {
        Ok(self
            .client_for(url)?
            .fetch(url, None, None, max)
            .await?
            .body)
    }

    /// The SHA-1 published next to a Maven artifact.
    async fn sidecar_sha1(&self, url: &str) -> LauncherResult<Sha1> {
        let body = self.get(&format!("{url}.sha1"), 1024).await?;
        let text = String::from_utf8_lossy(&body);
        Sha1::parse(text.split_whitespace().next().unwrap_or_default())
    }

    /// Makes sure a Maven-tree file with this hash exists, fetching it when needed,
    /// and returns its size.
    async fn ensure_file(&self, path: &str, url: &str, sha1: &Sha1) -> LauncherResult<u64> {
        let destination = resolve_managed_path(self.root, &maven_path(path))?;
        if let Ok(bytes) = std::fs::read(&destination)
            && sha1_hex(&bytes) == sha1.as_str()
        {
            return Ok(bytes.len() as u64);
        }
        let body = self.get(url, MAX_FILE_BYTES).await?;
        if sha1_hex(&body) != sha1.as_str() {
            return Err(hash_mismatch("mod loader file"));
        }
        if let Some(parent) = destination.parent() {
            std::fs::create_dir_all(parent).map_err(|e| storage_error(&e))?;
        }
        atomic::write(&destination, &body).map_err(|e| storage_error(&e))?;
        Ok(body.len() as u64)
    }

    /// Pins one declared library: its SHA-1 and size, from the installer, the
    /// declaration or the repository.
    async fn pin(
        &self,
        mut library: LoaderLibrary,
        embedded: &[EmbeddedFile],
    ) -> LauncherResult<LoaderLibrary> {
        validate_relative_path(&library.path)?;
        if library.url.is_empty() {
            if let Some(file) = embedded.iter().find(|e| e.path == library.path) {
                if library.sha1.as_ref().is_some_and(|s| *s != file.sha1) {
                    return Err(hash_mismatch("mod loader file"));
                }
                library.sha1 = Some(file.sha1.clone());
                library.size = Some(file.size);
            }
        } else {
            SourcePolicy::for_artifact(&library.url)?;
            if library.sha1.is_none() || library.size.is_none() {
                let pinned = match library.sha1.take() {
                    Some(sha1) => sha1,
                    None => self.sidecar_sha1(&library.url).await?,
                };
                library.size = Some(
                    self.ensure_file(&library.path, &library.url, &pinned)
                        .await?,
                );
                library.sha1 = Some(pinned);
            }
        }
        Ok(library)
    }

    /// Mojang-format `libraries` entries (`downloads.artifact`).
    async fn libraries(
        &self,
        list: &[Value],
        embedded: &[EmbeddedFile],
    ) -> LauncherResult<Vec<LoaderLibrary>> {
        let mut out = Vec::with_capacity(list.len());
        for entry in list {
            let name = entry
                .get("name")
                .and_then(Value::as_str)
                .ok_or_else(|| unsupported("loader library name"))?;
            let rules = entry.get("rules");
            let library = match entry.pointer("/downloads/artifact") {
                Some(artifact) => {
                    let coordinate = Coordinate::parse(name)?;
                    let declared = LoaderLibrary {
                        name: name.to_owned(),
                        path: artifact
                            .get("path")
                            .and_then(Value::as_str)
                            .map_or_else(|| coordinate.path(), str::to_owned),
                        url: artifact
                            .get("url")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_owned(),
                        sha1: optional_sha1(artifact.get("sha1"))?,
                        size: artifact.get("size").and_then(Value::as_u64),
                        rules: rules.cloned(),
                    };
                    self.pin(declared, embedded).await?
                }
                // Fabric and Quilt name the repository; the path follows from the
                // coordinate.
                None => {
                    let base = entry
                        .get("url")
                        .and_then(Value::as_str)
                        .ok_or_else(|| unsupported("loader library url"))?;
                    let path = Coordinate::parse(name)?.path();
                    let url = format!("{}/{path}", base.trim_end_matches('/'));
                    let declared = LoaderLibrary {
                        name: name.to_owned(),
                        path,
                        url,
                        sha1: optional_sha1(entry.get("sha1"))?,
                        size: entry.get("size").and_then(Value::as_u64),
                        rules: rules.cloned(),
                    };
                    self.pin(declared, embedded).await?
                }
            };
            out.push(library);
        }
        Ok(out)
    }

    async fn meta_profile(
        &self,
        kind: LoaderKind,
        game: &str,
        version: &str,
    ) -> LauncherResult<LoaderManifest> {
        self.meta_profile_of(kind, game, version, "profile").await
    }

    /// The meta service's `profile` (the game) or `server` profile.
    async fn meta_profile_of(
        &self,
        kind: LoaderKind,
        game: &str,
        version: &str,
        side: &str,
    ) -> LauncherResult<LoaderManifest> {
        let base = if kind == LoaderKind::Fabric {
            FABRIC_META
        } else {
            QUILT_META
        };
        let url = format!("{base}/versions/loader/{game}/{version}/{side}/json");
        let body = self
            .loaders
            .fetch(&url, None, None, MAX_PROFILE_BYTES)
            .await?
            .body;
        let profile: Value =
            serde_json::from_slice(&body).map_err(|_| unsupported("loader profile JSON"))?;
        let (profile_id, main_class) = profile_identity(&profile, game)?;
        let (jvm_arguments, game_arguments, minecraft_arguments) = arguments(&profile)?;
        let libraries = self
            .libraries(
                profile
                    .get("libraries")
                    .and_then(Value::as_array)
                    .ok_or_else(|| unsupported("loader libraries"))?,
                &[],
            )
            .await?;
        if libraries.iter().any(|l| l.url.is_empty()) {
            return Err(unsupported("loader library url"));
        }
        Ok(LoaderManifest {
            schema_version: SCHEMA_VERSION,
            kind,
            version: version.to_owned(),
            game_version: game.to_owned(),
            source: url,
            profile_id,
            main_class,
            libraries,
            jvm_arguments,
            game_arguments,
            minecraft_arguments,
            installer: None,
        })
    }

    async fn installer(
        &self,
        kind: LoaderKind,
        game: &str,
        version: &str,
    ) -> LauncherResult<LoaderManifest> {
        let (url, path) = installer_location(kind, game, version)?;
        let sha1 = self.sidecar_sha1(&url).await?;
        let size = self.ensure_file(&path, &url, &sha1).await?;
        let bytes = std::fs::read(resolve_managed_path(self.root, &maven_path(&path))?)
            .map_err(|e| storage_error(&e))?;
        if sha1_hex(&bytes) != sha1.as_str() {
            return Err(hash_mismatch("mod loader installer"));
        }
        let contents = InstallerContents::read(&bytes, kind)?;
        let install = &contents.install_profile;
        if install.get("minecraft").and_then(Value::as_str) != Some(game) {
            return Err(not_for_game(kind));
        }
        let profile = &contents.version_profile;
        let (profile_id, main_class) = profile_identity(profile, game)?;
        let (jvm_arguments, game_arguments, minecraft_arguments) = arguments(profile)?;
        let libraries = self
            .libraries(
                profile
                    .get("libraries")
                    .and_then(Value::as_array)
                    .ok_or_else(|| unsupported("loader libraries"))?,
                &contents.embedded,
            )
            .await?;
        let tools = self
            .libraries(
                install
                    .get("libraries")
                    .and_then(Value::as_array)
                    .map_or(&[][..], Vec::as_slice),
                &contents.embedded,
            )
            .await?;
        Ok(LoaderManifest {
            schema_version: SCHEMA_VERSION,
            kind,
            version: version.to_owned(),
            game_version: game.to_owned(),
            source: url.clone(),
            profile_id,
            main_class,
            libraries,
            jvm_arguments,
            game_arguments,
            minecraft_arguments,
            installer: Some(Installer {
                url,
                path,
                sha1,
                size,
                spec: contents.spec,
                data: contents.data,
                processors: contents.processors,
                libraries: tools,
                embedded: contents.embedded,
            }),
        })
    }
}

/// What an installer jar holds, read with bounds.
pub struct InstallerContents {
    pub spec: u64,
    pub install_profile: Value,
    pub version_profile: Value,
    pub data: BTreeMap<String, String>,
    pub processors: Vec<Processor>,
    pub embedded: Vec<EmbeddedFile>,
}

fn read_entry<R: Read + std::io::Seek>(
    zip: &mut zip::ZipArchive<R>,
    name: &str,
    max: u64,
) -> LauncherResult<Option<Vec<u8>>> {
    let mut entry = match zip.by_name(name) {
        Ok(entry) => entry,
        Err(zip::result::ZipError::FileNotFound) => return Ok(None),
        Err(_) => return Err(unsupported("installer archive")),
    };
    if entry.size() > max {
        return Err(unsupported("installer entry too large"));
    }
    let mut bytes = Vec::with_capacity(usize::try_from(entry.size()).unwrap_or(0));
    entry
        .by_ref()
        .take(max + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| unsupported("installer archive"))?;
    if bytes.len() as u64 > max {
        return Err(unsupported("installer entry too large"));
    }
    Ok(Some(bytes))
}

/// Reads one file out of an installer jar.
pub fn installer_entry(installer: &[u8], name: &str) -> LauncherResult<Vec<u8>> {
    let mut zip = zip::ZipArchive::new(Cursor::new(installer))
        .map_err(|_| unsupported("installer archive"))?;
    read_entry(&mut zip, name, MAX_ENTRY_BYTES)?.ok_or_else(|| unsupported("installer entry"))
}

fn strings(value: Option<&Value>) -> LauncherResult<Vec<String>> {
    match value {
        None => Ok(Vec::new()),
        Some(Value::Array(items)) => items
            .iter()
            .map(|v| {
                v.as_str()
                    .map(str::to_owned)
                    .ok_or_else(|| unsupported("installer processor"))
            })
            .collect(),
        Some(_) => Err(unsupported("installer processor")),
    }
}

impl InstallerContents {
    pub fn read(bytes: &[u8], kind: LoaderKind) -> LauncherResult<Self> {
        let mut zip = zip::ZipArchive::new(Cursor::new(bytes))
            .map_err(|_| unsupported("installer archive"))?;
        let install_bytes = read_entry(&mut zip, "install_profile.json", MAX_PROFILE_BYTES as u64)?
            .ok_or_else(|| unsupported("install_profile.json"))?;
        let mut install_profile: Value = serde_json::from_slice(&install_bytes)
            .map_err(|_| unsupported("install_profile.json"))?;
        if let (Some(install), Some(info)) = (
            install_profile.get("install"),
            install_profile.get("versionInfo"),
        ) {
            return Self::legacy(&mut zip, install.clone(), info.clone());
        }
        let spec = install_profile.get("spec").and_then(Value::as_u64);
        let json_name = install_profile.get("json").and_then(Value::as_str);
        let (Some(spec), Some(json_name)) = (spec, json_name) else {
            return Err(old_installer(kind));
        };
        if spec > 1 {
            return Err(unsupported("install profile spec"));
        }
        let version_bytes = read_entry(
            &mut zip,
            json_name.trim_start_matches('/'),
            MAX_PROFILE_BYTES as u64,
        )?
        .ok_or_else(|| unsupported("installer version profile"))?;
        let version_profile: Value = serde_json::from_slice(&version_bytes)
            .map_err(|_| unsupported("installer version profile"))?;

        let mut data = BTreeMap::new();
        if let Some(entries) = install_profile.get("data").and_then(Value::as_object) {
            for (key, value) in entries {
                let valid_key = !key.is_empty()
                    && key
                        .bytes()
                        .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_');
                let client = value.get("client").and_then(Value::as_str);
                match (valid_key, client) {
                    (true, Some(client)) => {
                        data.insert(key.clone(), client.to_owned());
                    }
                    _ => return Err(unsupported("install profile data")),
                }
            }
        }
        let mut processors = Vec::new();
        for processor in install_profile
            .get("processors")
            .and_then(Value::as_array)
            .map_or(&[][..], Vec::as_slice)
        {
            let sides = strings(processor.get("sides"))?;
            if !sides.is_empty() && !sides.iter().any(|s| s == "client") {
                continue;
            }
            let jar = processor
                .get("jar")
                .and_then(Value::as_str)
                .ok_or_else(|| unsupported("installer processor"))?;
            Coordinate::parse(jar)?;
            let classpath = strings(processor.get("classpath"))?;
            for entry in &classpath {
                Coordinate::parse(entry)?;
            }
            let mut outputs = BTreeMap::new();
            if let Some(map) = processor.get("outputs").and_then(Value::as_object) {
                for (file, sha1) in map {
                    let sha1 = sha1
                        .as_str()
                        .ok_or_else(|| unsupported("installer processor"))?;
                    outputs.insert(file.clone(), sha1.to_owned());
                }
            }
            processors.push(Processor {
                jar: jar.to_owned(),
                classpath,
                args: strings(processor.get("args"))?,
                outputs,
            });
        }

        let mut embedded = Vec::new();
        let names: Vec<String> = zip
            .file_names()
            .filter(|n| n.starts_with("maven/") && !n.ends_with('/'))
            .map(str::to_owned)
            .collect();
        for name in names {
            let path = name["maven/".len()..].to_owned();
            validate_relative_path(&path)?;
            let bytes = read_entry(&mut zip, &name, MAX_ENTRY_BYTES)?
                .ok_or_else(|| unsupported("installer entry"))?;
            embedded.push(EmbeddedFile {
                path,
                sha1: Sha1::parse(&sha1_hex(&bytes))?,
                size: bytes.len() as u64,
                entry: None,
            });
        }
        embedded.sort_by(|a, b| a.path.cmp(&b.path));
        if let Some(map) = install_profile.as_object_mut() {
            for key in INSTALLER_EXTRAS {
                map.remove(key);
            }
        }
        Ok(Self {
            spec,
            install_profile,
            version_profile,
            data,
            processors,
            embedded,
        })
    }

    /// Forge's installers before 1.12.2 build 2851: `install` names the universal jar
    /// packed in the installer, `versionInfo` is the version profile in the launcher's
    /// old format, and nothing runs at install time (FML patches the game as it
    /// starts). The profile's libraries are rewritten to the current format: the
    /// universal jar comes from the installer, a library without a repository from
    /// Mojang's, and a declared checksum pins the jar (the first is the jar's own; two
    /// of Forge's old Scala jars have no `.sha1` file). Server-only libraries are left
    /// out.
    fn legacy<R: Read + std::io::Seek>(
        zip: &mut zip::ZipArchive<R>,
        mut install: Value,
        mut version_profile: Value,
    ) -> LauncherResult<Self> {
        let forge = install
            .get("path")
            .and_then(Value::as_str)
            .ok_or_else(|| unsupported("install path"))?
            .to_owned();
        let entry = install
            .get("filePath")
            .and_then(Value::as_str)
            .filter(|name| !name.contains('/') && !name.contains('\\'))
            .ok_or_else(|| unsupported("install filePath"))?
            .to_owned();
        validate_relative_path(&entry)?;
        let bytes = read_entry(zip, &entry, MAX_ENTRY_BYTES)?
            .ok_or_else(|| unsupported("installer entry"))?;
        let universal = EmbeddedFile {
            path: Coordinate::parse(&forge)?.path(),
            sha1: Sha1::parse(&sha1_hex(&bytes))?,
            size: bytes.len() as u64,
            entry: Some(entry),
        };

        let mut libraries = Vec::new();
        for library in version_profile
            .get("libraries")
            .and_then(Value::as_array)
            .ok_or_else(|| unsupported("loader libraries"))?
        {
            let name = library
                .get("name")
                .and_then(Value::as_str)
                .ok_or_else(|| unsupported("loader library name"))?;
            if library.get("clientreq").and_then(Value::as_bool) == Some(false) {
                continue;
            }
            let path = Coordinate::parse(name)?.path();
            let url = if name == forge {
                String::new()
            } else {
                let base = match library.get("url") {
                    None => MOJANG_LIBRARIES,
                    Some(Value::String(base)) => base.as_str(),
                    Some(_) => return Err(unsupported("loader library url")),
                };
                format!("{}/{path}", base.trim_end_matches('/'))
            };
            let mut artifact = serde_json::json!({ "path": path, "url": url });
            if let Some(sha1) = library.pointer("/checksums/0") {
                artifact["sha1"] = sha1.clone();
            }
            libraries.push(serde_json::json!({
                "name": name,
                "downloads": { "artifact": artifact },
            }));
        }
        version_profile["libraries"] = Value::Array(libraries);
        if let Some(map) = install.as_object_mut() {
            for key in INSTALLER_EXTRAS {
                map.remove(key);
            }
        }
        Ok(Self {
            spec: 0,
            install_profile: install,
            version_profile,
            data: BTreeMap::new(),
            processors: Vec::new(),
            embedded: vec![universal],
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installers_live_on_their_maven() {
        let (url, path) = installer_location(LoaderKind::Forge, "1.21.1", "1.21.1-52.1.0").unwrap();
        assert_eq!(
            url,
            "https://maven.minecraftforge.net/net/minecraftforge/forge/1.21.1-52.1.0/forge-1.21.1-52.1.0-installer.jar"
        );
        assert_eq!(
            path,
            "net/minecraftforge/forge/1.21.1-52.1.0/forge-1.21.1-52.1.0-installer.jar"
        );
        let (url, _) = installer_location(LoaderKind::NeoForge, "1.21.1", "21.1.262").unwrap();
        assert_eq!(
            url,
            "https://maven.neoforged.net/releases/net/neoforged/neoforge/21.1.262/neoforge-21.1.262-installer.jar"
        );
        let (url, _) =
            installer_location(LoaderKind::NeoForge, "1.20.1", "1.20.1-47.1.106").unwrap();
        assert!(
            url.ends_with(
                "/net/neoforged/forge/1.20.1-47.1.106/forge-1.20.1-47.1.106-installer.jar"
            )
        );
        let old =
            installer_location(LoaderKind::Forge, "1.12.2", "1.12.2-14.23.5.2847").unwrap_err();
        assert_eq!(old.code, ErrorCode::UnsupportedVersionMetadata);
        let wrong = installer_location(LoaderKind::NeoForge, "1.21.1", "21.4.1").unwrap_err();
        assert_eq!(wrong.code, ErrorCode::InvalidInput);
        assert!(installer_location(LoaderKind::Forge, "1.21.1", "1.20.1-47.4.10").is_err());
    }

    fn legacy_installer(file_path: &str) -> Vec<u8> {
        use std::io::Write as _;
        let profile = serde_json::json!({
            "install": {
                "path": "net.minecraftforge:forge:1.8.9-11.15.1.2318-1.8.9",
                "filePath": file_path,
                "minecraft": "1.8.9",
                "logo": "/big_logo.png",
            },
            "versionInfo": {
                "id": "1.8.9-forge1.8.9-11.15.1.2318-1.8.9",
                "inheritsFrom": "1.8.9",
                "jar": "1.8.9",
                "mainClass": "net.minecraft.launchwrapper.Launch",
                "minecraftArguments": "--username ${auth_player_name} --tweakClass net.minecraftforge.fml.common.launcher.FMLTweaker",
                "libraries": [
                    {"name": "net.minecraftforge:forge:1.8.9-11.15.1.2318-1.8.9", "url": "https://maven.minecraftforge.net/"},
                    {"name": "net.minecraft:launchwrapper:1.12", "serverreq": true},
                    {"name": "jline:jline:2.13", "url": "https://maven.minecraftforge.net/", "serverreq": true, "clientreq": false},
                    {
                        "name": "org.scala-lang:scala-xml_2.11:1.0.2",
                        "url": "https://maven.minecraftforge.net/",
                        "checksums": ["7a80ec00aec122fba7cd4e0d4cdd87ff7e4cb6d0", "62736b01689d56b6d09a0164b7ef9da2b0b9633d"],
                        "serverreq": true,
                        "clientreq": true
                    }
                ]
            }
        });
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file("install_profile.json", options).unwrap();
        zip.write_all(profile.to_string().as_bytes()).unwrap();
        zip.start_file("forge-1.8.9-11.15.1.2318-1.8.9-universal.jar", options)
            .unwrap();
        zip.write_all(b"universal").unwrap();
        zip.finish().unwrap().into_inner()
    }

    #[test]
    fn an_old_forge_installer_brings_its_universal_jar_and_a_profile() {
        let bytes = legacy_installer("forge-1.8.9-11.15.1.2318-1.8.9-universal.jar");
        let contents = InstallerContents::read(&bytes, LoaderKind::Forge).unwrap();
        assert_eq!(contents.install_profile["minecraft"], "1.8.9");
        assert!(contents.install_profile.get("logo").is_none());
        assert!(contents.processors.is_empty() && contents.data.is_empty());
        let forge_path =
            "net/minecraftforge/forge/1.8.9-11.15.1.2318-1.8.9/forge-1.8.9-11.15.1.2318-1.8.9.jar";
        assert_eq!(
            contents.embedded,
            [EmbeddedFile {
                path: forge_path.to_owned(),
                sha1: Sha1::parse(&sha1_hex(b"universal")).unwrap(),
                size: 9,
                entry: Some("forge-1.8.9-11.15.1.2318-1.8.9-universal.jar".to_owned()),
            }]
        );
        assert_eq!(
            installer_entry(&bytes, &contents.embedded[0].entry_name()).unwrap(),
            b"universal"
        );
        let libraries = contents.version_profile["libraries"].as_array().unwrap();
        let artifacts: Vec<&Value> = libraries
            .iter()
            .map(|l| &l["downloads"]["artifact"])
            .collect();
        // The server-only jline is left out.
        assert_eq!(artifacts.len(), 3);
        assert_eq!(artifacts[0]["path"], forge_path);
        assert_eq!(artifacts[0]["url"], "");
        assert_eq!(
            artifacts[1]["url"],
            "https://libraries.minecraft.net/net/minecraft/launchwrapper/1.12/launchwrapper-1.12.jar"
        );
        assert!(artifacts[1].get("sha1").is_none());
        assert_eq!(
            artifacts[2]["url"],
            "https://maven.minecraftforge.net/org/scala-lang/scala-xml_2.11/1.0.2/scala-xml_2.11-1.0.2.jar"
        );
        assert_eq!(
            artifacts[2]["sha1"],
            "7a80ec00aec122fba7cd4e0d4cdd87ff7e4cb6d0"
        );
        assert_eq!(
            arguments(&contents.version_profile).unwrap().2.as_deref(),
            Some(
                "--username ${auth_player_name} --tweakClass net.minecraftforge.fml.common.launcher.FMLTweaker"
            )
        );
        assert_eq!(
            profile_identity(&contents.version_profile, "1.8.9").unwrap(),
            (
                "1.8.9-forge1.8.9-11.15.1.2318-1.8.9".to_owned(),
                "net.minecraft.launchwrapper.Launch".to_owned()
            )
        );

        // The universal jar is named by a plain file name inside the installer.
        for bad in ["../forge.jar", "maven/forge.jar", "missing.jar", ""] {
            assert!(
                InstallerContents::read(&legacy_installer(bad), LoaderKind::Forge).is_err(),
                "{bad:?}"
            );
        }
    }

    /// Serves one meta service answer; anything else is offline.
    struct Meta(String, Value);

    impl Fetch for Meta {
        fn fetch<'a>(
            &'a self,
            url: &'a str,
            _etag: Option<&'a str>,
            _last_modified: Option<&'a str>,
            _max_bytes: usize,
        ) -> crate::catalog::source::FetchFuture<'a> {
            Box::pin(async move {
                if url != self.0 {
                    return Err(LauncherError::new(ErrorCode::NetworkError, "offline"));
                }
                Ok(crate::catalog::source::Fetched {
                    not_modified: false,
                    etag: None,
                    last_modified: None,
                    body: self.1.to_string().into_bytes(),
                })
            })
        }
    }

    #[tokio::test]
    async fn a_loader_server_comes_from_the_meta_services_server_profile() {
        let dir = tempfile::tempdir().unwrap();
        let root = DataRoot::new(dir.path().to_path_buf()).unwrap();
        let url = "https://meta.fabricmc.net/v2/versions/loader/1.21.1/0.17.2/server/json";
        let profile = serde_json::json!({
            "id": "fabric-loader-0.17.2-1.21.1",
            "inheritsFrom": "1.21.1",
            "mainClass": "net.fabricmc.loader.impl.launch.knot.KnotServer",
            "arguments": {"game": []},
            "libraries": [{
                "name": "org.ow2.asm:asm:9.8",
                "url": "https://maven.fabricmc.net/",
                "sha1": "dc19ecb3f7889b7860697215cae99c0f9b6f6b4b",
                "size": 126_113
            }]
        });
        let meta = Meta(url.to_owned(), profile.clone());
        let resolver = Resolver {
            loaders: &meta,
            mojang: &meta,
            root: &root,
        };
        let fabric = LoaderChoice {
            kind: LoaderKind::Fabric,
            version: "0.17.2".into(),
        };
        let (manifest, sha1) = resolver.resolve_server(&fabric, "1.21.1").await.unwrap();
        assert_eq!(
            manifest.main_class,
            "net.fabricmc.loader.impl.launch.knot.KnotServer"
        );
        assert_eq!(manifest.source, url);
        assert_eq!(
            manifest.libraries[0].url,
            "https://maven.fabricmc.net/org/ow2/asm/asm/9.8/asm-9.8.jar"
        );
        assert_eq!(load(&root, &sha1), Some(manifest));

        // Another game version's profile, or a loader with an installer, is refused.
        assert!(resolver.resolve_server(&fabric, "1.20.1").await.is_err());
        let forge = LoaderChoice {
            kind: LoaderKind::Forge,
            version: "1.21.1-52.1.0".into(),
        };
        assert!(resolver.resolve_server(&forge, "1.21.1").await.is_err());
        let mut wrong = profile;
        wrong["inheritsFrom"] = "1.20.1".into();
        let meta = Meta(url.to_owned(), wrong);
        let resolver = Resolver {
            loaders: &meta,
            mojang: &meta,
            root: &root,
        };
        assert!(resolver.resolve_server(&fabric, "1.21.1").await.is_err());
    }
}
