//! Pure, deterministic install planning (docs/spec/04 "Metadata resolution",
//! doc 10 "Identity and storage contract").
//!
//! The planner consumes verified metadata and a target and performs no I/O. It
//! produces a [`GamePlan`] with canonical content-addressed destinations, expected
//! hashes, sizes and source URLs, plus an unresolved [`RuntimeRequirement`]. Java
//! runtime totals stay unknown until P07 resolves the runtime.

use std::collections::{BTreeMap, HashMap, HashSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::descriptor::{self, Arguments, Descriptor, Download, unsupported};
use super::rules::{self, MetaOs, RuleContext};
use crate::error::{ErrorCode, LauncherError, LauncherResult};
use crate::loader::LoaderKind;
use crate::loader::coordinate::Coordinate;
use crate::loader::manifest::{LoaderManifest, maven_path};
use crate::net::SourcePolicy;
use crate::storage::models::ArtifactRole;
use crate::storage::paths::{SafeName, Sha1, TargetArch, TargetOs};

/// Bound for asset index documents.
pub const MAX_ASSET_INDEX_BYTES: usize = 32 * 1024 * 1024;

/// Placeholders the launch compiler (P09) implements. Any other placeholder in an
/// included argument makes the version unsupported.
pub const KNOWN_PLACEHOLDERS: &[&str] = &[
    "auth_player_name",
    "version_name",
    "game_directory",
    "assets_root",
    "assets_index_name",
    "auth_uuid",
    "auth_access_token",
    "clientid",
    "auth_xuid",
    "user_type",
    "version_type",
    "resolution_width",
    "resolution_height",
    "natives_directory",
    "launcher_name",
    "launcher_version",
    "classpath",
    "classpath_separator",
    // Legacy `minecraftArguments` (before 1.13).
    "game_assets",
    "auth_session",
    "user_properties",
    // Mod loaders (D-039): the Maven tree of the installation's libraries.
    "library_directory",
];

/// How the game expects its asset files (from the asset index flags).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssetLayout {
    /// `assets/indexes` + `assets/objects`; the game reads the index itself.
    #[default]
    Modern,
    /// `virtual: true` (1.6–1.7.2): files by name under `assets/virtual/<index>`.
    Virtual,
    /// `map_to_resources: true` (before 1.6): files by name under
    /// `<game directory>/resources`.
    Resources,
}

/// A native jar extracted into the run's natives directory before launch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeJar {
    /// Canonical path of the downloaded jar (also listed in `libraries`).
    pub path: String,
    pub sha1: Sha1,
    /// Entry-name prefixes not to extract.
    pub exclude: Vec<String>,
}

/// Java runtime majors the provider no longer publishes, mapped to the nearest
/// compatible one: Minecraft 1.17 declares Java 16, which Temurin never shipped;
/// it runs on Java 17 (as Mojang's own runtime later did).
pub fn runtime_major(declared: u16) -> u16 {
    match declared {
        16 => 17,
        other => other,
    }
}

/// JVM arguments for legacy descriptors, which declare none (what Mojang's
/// launcher supplied for them).
/// Main class of Forge 1.20.3+ (`net.minecraftforge:bootstrap`).
const FORGE_BOOTSTRAP: &str = "net.minecraftforge.bootstrap.ForgeBootstrap";

fn legacy_jvm_arguments(ctx: &RuleContext) -> Vec<String> {
    let mut args = Vec::new();
    if ctx.os == MetaOs::Windows {
        // Selects the Intel graphics driver profile for Minecraft.
        args.push(
            "-XX:HeapDumpPath=MojangTricksIntelDriversForPerformance_javaw.exe_minecraft.exe.heapdump"
                .to_owned(),
        );
    }
    args.extend(
        [
            "-Djava.library.path=${natives_directory}",
            "-Dminecraft.launcher.brand=${launcher_name}",
            "-Dminecraft.launcher.version=${launcher_version}",
            "-cp",
            "${classpath}",
        ]
        .map(str::to_owned),
    );
    args
}

/// One managed file with its canonical destination relative to DATA_ROOT.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlannedArtifact {
    pub role: ArtifactRole,
    /// `/`-separated path relative to DATA_ROOT.
    pub path: String,
    pub url: String,
    pub sha1: Sha1,
    pub size: u64,
    /// Maven coordinate or asset name; provenance only, never a cache key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provenance: Option<String>,
}

/// The Java runtime a plan needs; P07 turns it into a pinned runtime plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeRequirement {
    /// Major for Temurin (see [`runtime_major`]).
    pub major: u16,
    pub os: TargetOs,
    pub arch: TargetArch,
    /// Mojang runtime component the version names, when it names one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub component: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetIndexPlan {
    pub id: SafeName,
    pub artifact: PlannedArtifact,
    pub declared_total_size: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoggingPlan {
    pub id: SafeName,
    /// Argument template containing `${path}`.
    pub argument: String,
    pub artifact: PlannedArtifact,
}

/// Where an installation's libraries live.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LibraryLayout {
    /// `shared/libraries/<sha1>/artifact.jar`, shared by content (vanilla).
    Shared,
    /// `shared/maven/<Maven path>` under their own file names: mod loaders find
    /// libraries by name and Maven path (D-039).
    Maven,
}

/// A classpath file that a loader's installer provides instead of a download.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProvidedFile {
    pub path: String,
    /// When the loader profile states it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha1: Option<Sha1>,
}

/// The mod loader part of a plan (D-039).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoaderPlan {
    pub kind: LoaderKind,
    pub version: String,
    /// The stored loader manifest this plan derives from.
    pub manifest_sha1: Sha1,
    /// The loader profile's id: `${version_name}` and the client jar's name.
    pub profile_id: String,
    /// The game's own main class (only Minecraft's knows `--demo`).
    pub game_main_class: String,
    /// Classpath files the installer provides.
    pub provided: Vec<ProvidedFile>,
}

/// Everything needed to install one version for one target, minus the runtime.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GamePlan {
    pub version_id: String,
    pub version_type: String,
    pub descriptor_url: String,
    pub descriptor_sha1: Sha1,
    pub main_class: String,
    pub client: PlannedArtifact,
    /// Applicable libraries in descriptor order, deduplicated by content.
    pub libraries: Vec<PlannedArtifact>,
    /// Ordered classpath (libraries in descriptor order, then the client).
    pub classpath: Vec<String>,
    pub asset_index: AssetIndexPlan,
    /// Unique asset objects sorted by hash. Empty until the index is planned.
    pub assets: Vec<PlannedArtifact>,
    #[serde(default)]
    pub asset_layout: AssetLayout,
    /// Native jars to extract before launch (legacy natives), in descriptor order.
    #[serde(default)]
    pub natives: Vec<NativeJar>,
    pub logging: Option<LoggingPlan>,
    /// Applicable JVM argument templates in order (placeholders unresolved).
    pub jvm_arguments: Vec<String>,
    /// Applicable game argument templates in order (placeholders unresolved).
    pub game_arguments: Vec<String>,
    pub runtime: RuntimeRequirement,
    /// Mojang's client mappings, offered to a loader installer (Maven layout only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mappings: Option<PlannedArtifact>,
    /// Files only a loader installer uses: the installer, its tools and inputs.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub loader_files: Vec<PlannedArtifact>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loader: Option<LoaderPlan>,
}

impl GamePlan {
    /// Every game artifact in a stable order (client, libraries, index, logging,
    /// assets). Runtime archives are not included.
    pub fn artifacts(&self) -> impl Iterator<Item = &PlannedArtifact> {
        std::iter::once(&self.client)
            .chain(self.libraries.iter())
            .chain(std::iter::once(&self.asset_index.artifact))
            .chain(self.logging.iter().map(|l| &l.artifact))
            .chain(self.loader_files.iter())
            .chain(self.assets.iter())
    }

    /// `${version_name}`: the loader profile's id, or the game version.
    pub fn version_name(&self) -> &str {
        self.loader
            .as_ref()
            .map_or(&self.version_id, |l| &l.profile_id)
    }

    /// The game's own main class, whatever starts it.
    pub fn game_main_class(&self) -> &str {
        self.loader
            .as_ref()
            .map_or(&self.main_class, |l| &l.game_main_class)
    }

    pub fn total_bytes(&self) -> u64 {
        self.artifacts().map(|a| a.size).sum()
    }
}

/// Maps the rule context to the runtime target.
pub fn runtime_target(ctx: &RuleContext) -> LauncherResult<(TargetOs, TargetArch)> {
    let os = match ctx.os {
        MetaOs::Windows => TargetOs::Windows,
        MetaOs::Osx => TargetOs::Mac,
        MetaOs::Linux => TargetOs::Linux,
    };
    let arch = match (ctx.os, ctx.arch) {
        (_, "x86_64") => TargetArch::X64,
        // Apple silicon and Windows on ARM; Mojang has no Java for Linux on ARM.
        (MetaOs::Osx | MetaOs::Windows, "arm64") => TargetArch::Aarch64,
        _ => {
            return Err(LauncherError::new(
                ErrorCode::UnsupportedPlatform,
                "This processor architecture is not supported on this operating system.",
            ));
        }
    };
    Ok((os, arch))
}

/// Whether a library this machine uses declares natives for its OS (the
/// layout before 1.19, with no ARM natives on Windows or macOS).
fn has_os_natives(descriptor: &Descriptor, ctx: &RuleContext) -> LauncherResult<bool> {
    for library in &descriptor.libraries {
        let Some(declared) = &library.natives else {
            continue;
        };
        if declared.classifiers.contains_key(ctx.os.as_str())
            && rules::evaluate(library.rules.as_ref(), ctx)?
        {
            return Ok(true);
        }
    }
    Ok(false)
}

fn artifact(
    role: ArtifactRole,
    path: String,
    download: &Download,
    policy: SourcePolicy,
    provenance: Option<String>,
) -> LauncherResult<PlannedArtifact> {
    policy.check(&download.url)?;
    Ok(PlannedArtifact {
        role,
        path,
        url: download.url.clone(),
        sha1: download.sha1.clone(),
        size: download.size,
        provenance,
    })
}

/// Rejects two records that claim the same digest with different sizes.
fn check_size_conflict(
    seen: &mut HashMap<(ArtifactRole, Sha1), u64>,
    a: &PlannedArtifact,
) -> LauncherResult<bool> {
    let category = match a.role {
        ArtifactRole::Library | ArtifactRole::Client => ArtifactRole::Library,
        other => other,
    };
    match seen.get(&(category, a.sha1.clone())) {
        Some(size) if *size != a.size => Err(LauncherError::new(
            ErrorCode::MetadataConflict,
            "The version metadata lists the same file with two different sizes.",
        )
        .with_detail("sha1", a.sha1.to_string())),
        Some(_) => Ok(false),
        None => {
            seen.insert((category, a.sha1.clone()), a.size);
            Ok(true)
        }
    }
}

fn evaluate_arguments(list: &[Value], ctx: &RuleContext) -> LauncherResult<Vec<String>> {
    let mut out = Vec::new();
    for entry in list {
        let values: Vec<&str> = match entry {
            Value::String(s) => vec![s.as_str()],
            Value::Object(map) => {
                if !rules::evaluate(map.get("rules"), ctx)? {
                    continue;
                }
                match map.get("value") {
                    Some(Value::String(s)) => vec![s.as_str()],
                    Some(Value::Array(items)) => items
                        .iter()
                        .map(|v| v.as_str().ok_or_else(|| unsupported("argument value")))
                        .collect::<LauncherResult<_>>()?,
                    _ => return Err(unsupported("argument value")),
                }
            }
            _ => return Err(unsupported("argument entry")),
        };
        for value in values {
            check_placeholders(value)?;
            out.push(value.to_owned());
        }
    }
    Ok(out)
}

/// Every `${name}` must be implemented.
pub fn check_placeholders(value: &str) -> LauncherResult<()> {
    let mut rest = value;
    while let Some(start) = rest.find("${") {
        let after = &rest[start + 2..];
        let end = after
            .find('}')
            .ok_or_else(|| unsupported("unterminated placeholder"))?;
        let name = &after[..end];
        if !KNOWN_PLACEHOLDERS.contains(&name) {
            return Err(unsupported(&format!("placeholder {name}")));
        }
        rest = &after[end + 1..];
    }
    Ok(())
}

/// Plans everything except asset objects (which need the asset index bytes).
pub fn plan_game(
    descriptor_bytes: &[u8],
    descriptor_url: &str,
    descriptor_sha1: &Sha1,
    version_id: &str,
    ctx: &RuleContext,
) -> LauncherResult<GamePlan> {
    plan_game_in(
        descriptor_bytes,
        descriptor_url,
        descriptor_sha1,
        version_id,
        ctx,
        LibraryLayout::Shared,
    )
}

/// [`plan_game`] with a choice of library layout.
pub fn plan_game_in(
    descriptor_bytes: &[u8],
    descriptor_url: &str,
    descriptor_sha1: &Sha1,
    version_id: &str,
    ctx: &RuleContext,
    layout: LibraryLayout,
) -> LauncherResult<GamePlan> {
    let descriptor: Descriptor = descriptor::parse(descriptor_bytes, version_id)?;
    let (os, mut arch) = runtime_target(ctx)?;
    // Before 1.19 the native libraries exist only for x64. Windows on ARM
    // runs those versions with x64 Java, which Windows 11 emulates.
    if os == TargetOs::Windows && arch == TargetArch::Aarch64 && has_os_natives(&descriptor, ctx)? {
        arch = TargetArch::X64;
    }
    let (jvm_arguments, game_arguments) = match &descriptor.arguments {
        Arguments::Modern { jvm, game } => (
            evaluate_arguments(jvm, ctx)?,
            evaluate_arguments(game, ctx)?,
        ),
        Arguments::Legacy(text) => {
            let game = text
                .split_whitespace()
                .map(|arg| check_placeholders(arg).map(|()| arg.to_owned()))
                .collect::<LauncherResult<Vec<_>>>()?;
            (legacy_jvm_arguments(ctx), game)
        }
    };
    let mut seen = HashMap::new();

    // A version file from our update server names its game jar there, and only
    // there (D-073); Mojang's name theirs on Mojang's hosts.
    let client_source = if SourcePolicy::ExtraVersions.check(descriptor_url).is_ok() {
        SourcePolicy::ExtraVersions
    } else {
        SourcePolicy::MojangArtifacts
    };
    let client = artifact(
        ArtifactRole::Client,
        format!("shared/clients/{}/client.jar", descriptor.client.sha1),
        &descriptor.client,
        client_source,
        Some(format!("{} client", descriptor.id)),
    )?;
    check_size_conflict(&mut seen, &client)?;

    // Every declared source must be trusted, even for libraries another OS uses.
    for library in &descriptor.libraries {
        if let Some(artifact) = &library.artifact {
            SourcePolicy::MojangArtifacts.check(&artifact.download.url)?;
        }
        for native in library.natives.iter().flat_map(|n| n.downloads.values()) {
            SourcePolicy::MojangArtifacts.check(&native.download.url)?;
        }
    }
    let mut libraries = Vec::new();
    let mut classpath = Vec::new();
    let mut natives: Vec<NativeJar> = Vec::new();
    let library_artifact = |jar: &descriptor::LibraryArtifact, provenance: String| {
        let download = &jar.download;
        let path = match layout {
            LibraryLayout::Shared => format!("shared/libraries/{}/artifact.jar", download.sha1),
            LibraryLayout::Maven => maven_path(&jar.path),
        };
        let planned = artifact(
            ArtifactRole::Library,
            path,
            download,
            SourcePolicy::MojangArtifacts,
            Some(provenance),
        )?;
        if planned.sha1 == client.sha1 {
            return Err(LauncherError::new(
                ErrorCode::MetadataConflict,
                "A library has the same content as the game client.",
            ));
        }
        Ok(planned)
    };
    for library in &descriptor.libraries {
        if !rules::evaluate(library.rules.as_ref(), ctx)? {
            continue;
        }
        if let Some(jar) = &library.artifact {
            let planned = library_artifact(jar, library.name.clone())?;
            check_size_conflict(&mut seen, &planned)?;
            if !classpath.contains(&planned.path) {
                classpath.push(planned.path.clone());
            }
            push_unique(&mut libraries, planned)?;
        }
        let Some(declared) = &library.natives else {
            continue;
        };
        // No classifier for this OS: the library has no natives here.
        let Some(template) = declared.classifiers.get(ctx.os.as_str()) else {
            continue;
        };
        if ctx.os == MetaOs::Osx && ctx.arch == "arm64" {
            return Err(LauncherError::new(
                ErrorCode::UnsupportedPlatform,
                "This Minecraft version's native libraries exist only for Intel Macs. \
                 Versions from 1.19 on run on Apple silicon.",
            ));
        }
        // Only 64-bit JVMs are supported.
        let classifier = template.replace("${arch}", "64");
        let jar = declared
            .downloads
            .get(&classifier)
            .ok_or_else(|| unsupported("missing native classifier download"))?;
        let planned = library_artifact(jar, format!("{} {classifier}", library.name))?;
        check_size_conflict(&mut seen, &planned)?;
        push_unique(&mut libraries, planned.clone())?;
        if !natives.iter().any(|n| n.path == planned.path) {
            natives.push(NativeJar {
                path: planned.path,
                sha1: planned.sha1,
                exclude: declared.exclude.clone(),
            });
        }
    }
    classpath.push(client.path.clone());

    let index = &descriptor.asset_index;
    let asset_index = AssetIndexPlan {
        id: index.id.clone(),
        artifact: artifact(
            ArtifactRole::AssetIndex,
            format!("metadata/asset-indexes/{}.json", index.download.sha1),
            &index.download,
            SourcePolicy::MojangMetadata,
            Some(format!("asset index {}", index.id.as_str())),
        )?,
        declared_total_size: index.total_size,
    };

    let logging = descriptor
        .logging
        .as_ref()
        .map(|logging| -> LauncherResult<LoggingPlan> {
            Ok(LoggingPlan {
                id: logging.id.clone(),
                argument: logging.argument.clone(),
                artifact: artifact(
                    ArtifactRole::LoggingConfig,
                    format!(
                        "shared/log-configs/{}/{}",
                        logging.download.sha1,
                        logging.id.as_str()
                    ),
                    &logging.download,
                    SourcePolicy::MojangArtifacts,
                    None,
                )?,
            })
        })
        .transpose()?;

    let mappings = match (layout, &descriptor.client_mappings) {
        (LibraryLayout::Maven, Some(download)) => Some(artifact(
            ArtifactRole::Library,
            format!("shared/mappings/{}/client.txt", download.sha1),
            download,
            SourcePolicy::MojangArtifacts,
            Some(format!("{} client mappings", descriptor.id)),
        )?),
        _ => None,
    };

    Ok(GamePlan {
        version_id: descriptor.id.clone(),
        version_type: descriptor.version_type.clone(),
        descriptor_url: descriptor_url.to_owned(),
        descriptor_sha1: descriptor_sha1.clone(),
        main_class: descriptor.main_class.clone(),
        client,
        libraries,
        classpath,
        asset_index,
        assets: Vec::new(),
        asset_layout: AssetLayout::Modern,
        natives,
        logging,
        jvm_arguments,
        game_arguments,
        runtime: RuntimeRequirement {
            major: runtime_major(descriptor.java_major),
            os,
            arch,
            component: descriptor.java_component.clone(),
        },
        mappings,
        loader_files: Vec::new(),
        loader: None,
    })
}

/// Adds a file unless one with its path is listed; the same path with other
/// content is a conflict.
fn push_unique(list: &mut Vec<PlannedArtifact>, planned: PlannedArtifact) -> LauncherResult<()> {
    match list.iter().find(|a| a.path == planned.path) {
        Some(existing) if existing.sha1 != planned.sha1 || existing.size != planned.size => {
            Err(LauncherError::new(
                ErrorCode::MetadataConflict,
                "The version metadata lists the same file with two different contents.",
            )
            .with_detail("path", planned.path))
        }
        Some(_) => Ok(()),
        None => {
            list.push(planned);
            Ok(())
        }
    }
}

/// A download of a loader manifest's library.
fn loader_artifact(
    library: &crate::loader::manifest::LoaderLibrary,
    path: String,
) -> LauncherResult<PlannedArtifact> {
    let (Some(sha1), Some(size)) = (&library.sha1, library.size) else {
        return Err(unsupported("loader library without hash"));
    };
    let (policy, _) = SourcePolicy::for_artifact(&library.url)?;
    artifact(
        ArtifactRole::Library,
        path,
        &Download {
            url: library.url.clone(),
            sha1: sha1.clone(),
            size,
        },
        policy,
        Some(library.name.clone()),
    )
}

/// Merges a mod loader onto a vanilla plan in the Maven layout (D-039), the way
/// a profile's `inheritsFrom` works: the loader's libraries come first and
/// replace the game's copies of the same library, its arguments follow the
/// game's (a legacy argument string replaces them), and its main class starts
/// the game. The client jar is named after the loader profile, as Forge's
/// module loader expects.
pub fn apply_loader(
    mut plan: GamePlan,
    manifest: &LoaderManifest,
    manifest_sha1: &Sha1,
    ctx: &RuleContext,
) -> LauncherResult<GamePlan> {
    if manifest.game_version != plan.version_id || plan.loader.is_some() {
        return Err(unsupported("loader game version"));
    }
    let mut keys = HashSet::new();
    let mut loader_classpath: Vec<String> = Vec::new();
    let mut downloads = Vec::new();
    let mut provided = Vec::new();
    for library in &manifest.libraries {
        if !rules::evaluate(library.rules.as_ref(), ctx)? {
            continue;
        }
        keys.insert(Coordinate::parse(&library.name)?.key());
        let path = maven_path(&library.path);
        if library.url.is_empty() {
            if !provided.iter().any(|p: &ProvidedFile| p.path == path) {
                provided.push(ProvidedFile {
                    path: path.clone(),
                    sha1: library.sha1.clone(),
                });
            }
        } else {
            push_unique(&mut downloads, loader_artifact(library, path.clone())?)?;
        }
        if !loader_classpath.contains(&path) {
            loader_classpath.push(path);
        }
    }
    if !provided.is_empty() && manifest.installer.is_none() {
        return Err(unsupported("loader library url"));
    }

    // The game's copies of libraries the loader brings in another version.
    let replaced: HashSet<String> = plan
        .libraries
        .iter()
        .filter(|l| !loader_classpath.contains(&l.path))
        .filter(|l| {
            l.provenance
                .as_deref()
                .and_then(|name| Coordinate::parse(name).ok())
                .is_some_and(|c| keys.contains(&c.key()))
        })
        .map(|l| l.path.clone())
        .collect();
    plan.libraries.retain(|l| !replaced.contains(&l.path));
    for download in downloads {
        push_unique(&mut plan.libraries, download)?;
    }

    let old_client = plan.client.path.clone();
    plan.client.path = format!(
        "shared/clients/{}/{}.jar",
        plan.client.sha1, manifest.profile_id
    );
    let game_classpath: Vec<String> = plan
        .classpath
        .iter()
        .filter(|p| !replaced.contains(*p) && !loader_classpath.contains(p))
        .map(|p| {
            if *p == old_client {
                plan.client.path.clone()
            } else {
                p.clone()
            }
        })
        .collect();
    plan.classpath = loader_classpath;
    plan.classpath.extend(game_classpath);

    match &manifest.minecraft_arguments {
        Some(text) => {
            plan.game_arguments = text
                .split_whitespace()
                .map(|arg| check_placeholders(arg).map(|()| arg.to_owned()))
                .collect::<LauncherResult<Vec<_>>>()?;
        }
        None => {
            plan.jvm_arguments
                .extend(evaluate_arguments(&manifest.jvm_arguments, ctx)?);
            plan.game_arguments
                .extend(evaluate_arguments(&manifest.game_arguments, ctx)?);
        }
    }
    // Forge's bootstrap finds its libraries by `libraryDirectory` and otherwise
    // looks in `./libraries`; some profiles (Forge 49.0.2 for 1.20.3) do not set it.
    if manifest.main_class == FORGE_BOOTSTRAP
        && !plan
            .jvm_arguments
            .iter()
            .any(|a| a.starts_with("-DlibraryDirectory="))
    {
        plan.jvm_arguments
            .push("-DlibraryDirectory=${library_directory}".to_owned());
    }

    if let Some(installer) = &manifest.installer {
        let mut files = vec![artifact(
            ArtifactRole::Library,
            maven_path(&installer.path),
            &Download {
                url: installer.url.clone(),
                sha1: installer.sha1.clone(),
                size: installer.size,
            },
            SourcePolicy::for_artifact(&installer.url)?.0,
            Some(format!(
                "{} {} installer",
                manifest.kind.name(),
                manifest.version
            )),
        )?];
        for tool in &installer.libraries {
            let path = maven_path(&tool.path);
            if tool.url.is_empty() {
                if !installer.embedded.iter().any(|e| e.path == tool.path) {
                    return Err(unsupported("installer library url"));
                }
            } else if !plan.libraries.iter().any(|l| l.path == path) {
                push_unique(&mut files, loader_artifact(tool, path)?)?;
            }
        }
        let needs_mappings = installer
            .processors
            .iter()
            .any(|p| p.args.iter().any(|a| a == "DOWNLOAD_MOJMAPS"));
        if needs_mappings {
            files.push(
                plan.mappings
                    .clone()
                    .ok_or_else(|| unsupported("client mappings"))?,
            );
        }
        plan.loader_files = files;
    }

    plan.loader = Some(LoaderPlan {
        kind: manifest.kind,
        version: manifest.version.clone(),
        manifest_sha1: manifest_sha1.clone(),
        profile_id: manifest.profile_id.clone(),
        game_main_class: std::mem::replace(&mut plan.main_class, manifest.main_class.clone()),
        provided,
    });
    Ok(plan)
}

/// Adds deduplicated asset objects from the verified asset index bytes.
pub fn plan_assets(plan: &mut GamePlan, index_bytes: &[u8]) -> LauncherResult<()> {
    if index_bytes.len() > MAX_ASSET_INDEX_BYTES {
        return Err(unsupported("asset index too large"));
    }
    let value: Value =
        serde_json::from_slice(index_bytes).map_err(|_| unsupported("asset index JSON"))?;
    let flag = |name: &str| value.get(name).and_then(Value::as_bool) == Some(true);
    plan.asset_layout = if flag("map_to_resources") {
        AssetLayout::Resources
    } else if flag("virtual") {
        AssetLayout::Virtual
    } else {
        AssetLayout::Modern
    };
    let objects = value
        .get("objects")
        .and_then(Value::as_object)
        .ok_or_else(|| unsupported("asset index objects"))?;
    let mut unique: BTreeMap<Sha1, (u64, String)> = BTreeMap::new();
    for (name, object) in objects {
        // Legacy layouts turn names into paths; they must be plain relative paths.
        if plan.asset_layout != AssetLayout::Modern {
            descriptor::validate_relative_path(name).map_err(|_| unsupported("asset name"))?;
        }
        let hash = object
            .get("hash")
            .and_then(Value::as_str)
            .ok_or_else(|| unsupported("asset hash"))?;
        let hash = Sha1::parse(hash).map_err(|_| unsupported("asset hash"))?;
        let size = object
            .get("size")
            .and_then(Value::as_u64)
            .ok_or_else(|| unsupported("asset size"))?;
        match unique.get(&hash) {
            Some((existing, _)) if *existing != size => {
                return Err(LauncherError::new(
                    ErrorCode::MetadataConflict,
                    "The asset index lists the same object with two different sizes.",
                ));
            }
            Some(_) => {}
            None => {
                unique.insert(hash, (size, name.clone()));
            }
        }
    }
    plan.assets = unique
        .into_iter()
        .map(|(hash, (size, name))| PlannedArtifact {
            role: ArtifactRole::AssetObject,
            path: format!("shared/assets/objects/{}/{}", &hash.as_str()[..2], hash),
            url: format!(
                "https://resources.download.minecraft.net/{}/{}",
                &hash.as_str()[..2],
                hash
            ),
            sha1: hash,
            size,
            // The logical name is provenance only; it never becomes a path.
            provenance: Some(name),
        })
        .collect();
    Ok(())
}

#[cfg(test)]
mod tests;
