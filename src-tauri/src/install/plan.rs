//! Pure, deterministic install planning (docs/spec/04 "Metadata resolution",
//! doc 10 "Identity and storage contract").
//!
//! The planner consumes verified metadata and a target and performs no I/O. It
//! produces a [`GamePlan`] with canonical content-addressed destinations, expected
//! hashes, sizes and source URLs, plus an unresolved [`RuntimeRequirement`]. Java
//! runtime totals stay unknown until P07 resolves the runtime.

use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::descriptor::{self, Arguments, Descriptor, Download, unsupported};
use super::rules::{self, MetaOs, RuleContext};
use crate::error::{ErrorCode, LauncherError, LauncherResult};
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
}

impl GamePlan {
    /// Every game artifact in a stable order (client, libraries, index, logging,
    /// assets). Runtime archives are not included.
    pub fn artifacts(&self) -> impl Iterator<Item = &PlannedArtifact> {
        std::iter::once(&self.client)
            .chain(self.libraries.iter())
            .chain(std::iter::once(&self.asset_index.artifact))
            .chain(self.logging.iter().map(|l| &l.artifact))
            .chain(self.assets.iter())
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
        // MVP targets: Apple Silicon only; Windows/Linux ARM are out of scope.
        (MetaOs::Osx, "arm64") => TargetArch::Aarch64,
        _ => {
            return Err(LauncherError::new(
                ErrorCode::UnsupportedPlatform,
                "This processor architecture is not supported on this operating system.",
            ));
        }
    };
    Ok((os, arch))
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

/// Every `${name}` must be implemented; `library_directory` is rejected until a
/// Maven-root adapter exists (doc 05).
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
    let descriptor: Descriptor = descriptor::parse(descriptor_bytes, version_id)?;
    let (os, arch) = runtime_target(ctx)?;
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

    let client = artifact(
        ArtifactRole::Client,
        format!("shared/clients/{}/client.jar", descriptor.client.sha1),
        &descriptor.client,
        SourcePolicy::MojangArtifacts,
        Some(format!("{} client", descriptor.id)),
    )?;
    check_size_conflict(&mut seen, &client)?;

    // Every declared source must be trusted, even for libraries another OS uses.
    for library in &descriptor.libraries {
        if let Some(artifact) = &library.artifact {
            SourcePolicy::MojangArtifacts.check(&artifact.download.url)?;
        }
        for download in library.natives.iter().flat_map(|n| n.downloads.values()) {
            SourcePolicy::MojangArtifacts.check(&download.url)?;
        }
    }
    let mut libraries = Vec::new();
    let mut classpath = Vec::new();
    let mut natives: Vec<NativeJar> = Vec::new();
    let library_artifact = |download: &Download, provenance: String| {
        let planned = artifact(
            ArtifactRole::Library,
            format!("shared/libraries/{}/artifact.jar", download.sha1),
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
            let planned = library_artifact(&jar.download, library.name.clone())?;
            let new = check_size_conflict(&mut seen, &planned)?;
            if !classpath.contains(&planned.path) {
                classpath.push(planned.path.clone());
            }
            if new {
                libraries.push(planned);
            }
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
        let download = declared
            .downloads
            .get(&classifier)
            .ok_or_else(|| unsupported("missing native classifier download"))?;
        let planned = library_artifact(download, format!("{} {classifier}", library.name))?;
        if check_size_conflict(&mut seen, &planned)? {
            libraries.push(planned.clone());
        }
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
    })
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
