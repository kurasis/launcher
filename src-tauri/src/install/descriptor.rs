//! Strict parsing of a verified vanilla version descriptor (docs/spec/04
//! "Metadata resolution", D-035).
//!
//! Every vanilla version type Mojang lists is accepted: releases, snapshots and
//! the old beta and alpha versions. Both argument formats are parsed: the modern
//! `arguments` object and the legacy `minecraftArguments` string (before 1.13),
//! as are native libraries shipped as classifier jars that must be extracted.
//! Missing download metadata is rejected with `UNSUPPORTED_VERSION_METADATA`
//! instead of being guessed, and so is inheritance (`inheritsFrom`): a mod
//! loader's profile is never parsed here but merged onto the vanilla plan by
//! [`super::plan::apply_loader`] (D-039). Unknown optional fields are tolerated.

use serde_json::Value;

use crate::error::{ErrorCode, LauncherError, LauncherResult};
use crate::storage::paths::{SafeName, Sha1};

/// Bound for descriptor documents (doc 04 "Cancellation and resource bounds").
pub const MAX_DESCRIPTOR_BYTES: usize = 8 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Download {
    pub url: String,
    pub sha1: Sha1,
    pub size: u64,
}

/// Version types Mojang's catalog uses for vanilla versions.
pub const VERSION_TYPES: &[&str] = &["release", "snapshot", "old_beta", "old_alpha"];

/// Java major for descriptors that predate the `javaVersion` field.
pub const LEGACY_JAVA_MAJOR: u16 = 8;
/// Mojang runtime component for those descriptors (what Mojang's launcher uses).
pub const LEGACY_JAVA_COMPONENT: &str = "jre-legacy";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Library {
    /// Maven coordinate, provenance only.
    pub name: String,
    /// The classpath jar; native-only libraries have none.
    pub artifact: Option<LibraryArtifact>,
    /// Native jars to extract before launch, by classifier.
    pub natives: Option<Natives>,
    pub rules: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryArtifact {
    /// Declared relative Maven path, validated but never used as a destination.
    pub path: String,
    pub download: Download,
}

/// A library's native jars (`natives` + `downloads.classifiers` + `extract`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Natives {
    /// Metadata OS name (`windows`, `osx`, `linux`) to classifier template; the
    /// template may contain `${arch}`.
    pub classifiers: std::collections::BTreeMap<String, String>,
    pub downloads: std::collections::BTreeMap<String, LibraryArtifact>,
    /// Entry-name prefixes not to extract, e.g. `META-INF/`.
    pub exclude: Vec<String>,
}

/// The two argument formats.
#[derive(Debug, Clone, PartialEq)]
pub enum Arguments {
    /// `arguments.jvm` and `arguments.game` with rules.
    Modern { jvm: Vec<Value>, game: Vec<Value> },
    /// `minecraftArguments`: whitespace-separated game arguments; the launcher
    /// supplies the JVM arguments.
    Legacy(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetIndexRef {
    pub id: SafeName,
    pub download: Download,
    pub total_size: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Logging {
    pub id: SafeName,
    pub argument: String,
    pub download: Download,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Descriptor {
    pub id: String,
    pub version_type: String,
    pub main_class: String,
    pub java_major: u16,
    /// Mojang runtime component (`javaVersion.component`), e.g. `java-runtime-delta`.
    pub java_component: Option<String>,
    pub client: Download,
    /// The dedicated server JAR (`downloads.server`), absent for versions Mojang
    /// published without one; only world hosting uses it (D-068).
    pub server: Option<Download>,
    /// Mojang's client mappings (1.14.4 and later); Forge's installer needs them.
    pub client_mappings: Option<Download>,
    pub asset_index: AssetIndexRef,
    pub logging: Option<Logging>,
    pub libraries: Vec<Library>,
    pub arguments: Arguments,
}

pub fn unsupported(reason: &str) -> LauncherError {
    LauncherError::new(
        ErrorCode::UnsupportedVersionMetadata,
        "This Minecraft version's metadata uses a format the launcher does not support.",
    )
    .with_detail("reason", reason.to_owned())
}

fn string<'a>(value: &'a Value, key: &str) -> LauncherResult<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| unsupported(&format!("missing string {key}")))
}

fn download(value: &Value, what: &str) -> LauncherResult<Download> {
    let url = string(value, "url").map_err(|_| unsupported(&format!("{what}.url")))?;
    let sha1 =
        Sha1::parse(string(value, "sha1")?).map_err(|_| unsupported(&format!("{what}.sha1")))?;
    let size = value
        .get("size")
        .and_then(Value::as_u64)
        .ok_or_else(|| unsupported(&format!("{what}.size")))?;
    Ok(Download {
        url: url.to_owned(),
        sha1,
        size,
    })
}

/// A Maven-style relative path: `/`-separated, no empty, `.` or `..` segments, no
/// drive or backslash, no leading slash.
pub fn validate_relative_path(path: &str) -> LauncherResult<()> {
    let bad = path.is_empty()
        || path.len() > 512
        || path.starts_with('/')
        || path.contains('\\')
        || path.contains(':')
        || path.contains('\0')
        || path
            .split('/')
            .any(|segment| segment.is_empty() || segment == "." || segment == "..");
    if bad {
        Err(unsupported("unsafe library path"))
    } else {
        Ok(())
    }
}

/// Runtime component names are lowercase words joined by `-`.
pub fn valid_component(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && !name.starts_with('-')
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

pub fn valid_main_class(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 256
        && name.split('.').all(|part| {
            !part.is_empty()
                && part
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
        })
}

/// Parses descriptor bytes whose SHA-1 the caller has already verified.
pub fn parse(bytes: &[u8], expected_id: &str) -> LauncherResult<Descriptor> {
    if bytes.len() > MAX_DESCRIPTOR_BYTES {
        return Err(unsupported("descriptor too large"));
    }
    let value: Value = serde_json::from_slice(bytes).map_err(|_| unsupported("invalid JSON"))?;
    if value.get("inheritsFrom").is_some() {
        return Err(unsupported("inheritsFrom"));
    }
    let id = string(&value, "id")?;
    if id != expected_id {
        return Err(unsupported("unexpected version id"));
    }
    let version_type = string(&value, "type")?;
    if !VERSION_TYPES.contains(&version_type) {
        return Err(unsupported("version type"));
    }
    let main_class = string(&value, "mainClass")?;
    if !valid_main_class(main_class) {
        return Err(unsupported("mainClass"));
    }
    // The oldest descriptors have no javaVersion; they all run on Java 8.
    let (java_major, java_component) = match value.get("javaVersion") {
        None => (LEGACY_JAVA_MAJOR, Some(LEGACY_JAVA_COMPONENT.to_owned())),
        Some(java) => (
            java.get("majorVersion")
                .and_then(Value::as_u64)
                .and_then(|m| u16::try_from(m).ok())
                .filter(|m| (8..=99).contains(m))
                .ok_or_else(|| unsupported("javaVersion.majorVersion"))?,
            match java.get("component") {
                None => None,
                Some(component) => Some(
                    component
                        .as_str()
                        .filter(|c| valid_component(c))
                        .ok_or_else(|| unsupported("javaVersion.component"))?
                        .to_owned(),
                ),
            },
        ),
    };
    let downloads = value
        .get("downloads")
        .ok_or_else(|| unsupported("downloads"))?;
    let client = download(
        downloads
            .get("client")
            .ok_or_else(|| unsupported("downloads.client"))?,
        "downloads.client",
    )?;
    let server = match downloads.get("server") {
        None => None,
        Some(server) => Some(download(server, "downloads.server")?),
    };
    let client_mappings = match downloads.get("client_mappings") {
        None => None,
        Some(mappings) => Some(download(mappings, "downloads.client_mappings")?),
    };

    let asset_value = value
        .get("assetIndex")
        .ok_or_else(|| unsupported("assetIndex"))?;
    let asset_index = AssetIndexRef {
        id: SafeName::parse(string(asset_value, "id")?)
            .map_err(|_| unsupported("assetIndex.id"))?,
        download: download(asset_value, "assetIndex")?,
        total_size: asset_value
            .get("totalSize")
            .and_then(Value::as_u64)
            .ok_or_else(|| unsupported("assetIndex.totalSize"))?,
    };

    let logging = match value.pointer("/logging/client") {
        None => None,
        Some(client_logging) => {
            if client_logging.get("type").and_then(Value::as_str) != Some("log4j2-xml") {
                return Err(unsupported("logging.client.type"));
            }
            let argument = string(client_logging, "argument")?;
            if !argument.contains("${path}") {
                return Err(unsupported("logging.client.argument"));
            }
            let file = client_logging
                .get("file")
                .ok_or_else(|| unsupported("logging.client.file"))?;
            Some(Logging {
                id: SafeName::parse(string(file, "id")?)
                    .map_err(|_| unsupported("logging.client.file.id"))?,
                argument: argument.to_owned(),
                download: download(file, "logging.client.file")?,
            })
        }
    };

    let mut libraries = Vec::new();
    for library in value
        .get("libraries")
        .and_then(Value::as_array)
        .ok_or_else(|| unsupported("libraries"))?
    {
        let name = string(library, "name")?;
        let library_downloads = library
            .get("downloads")
            .ok_or_else(|| unsupported("library without downloads"))?;
        let artifact = match library_downloads.get("artifact") {
            None => None,
            Some(artifact) => {
                let path = string(artifact, "path")?;
                validate_relative_path(path)?;
                Some(LibraryArtifact {
                    path: path.to_owned(),
                    download: download(artifact, "library")?,
                })
            }
        };
        let natives = parse_natives(library, library_downloads)?;
        if artifact.is_none() && natives.is_none() {
            return Err(unsupported("library without artifact"));
        }
        libraries.push(Library {
            name: name.to_owned(),
            artifact,
            natives,
            rules: library.get("rules").cloned(),
        });
    }

    let arguments = match (value.get("arguments"), value.get("minecraftArguments")) {
        (Some(arguments), _) => {
            let list = |key: &str| {
                arguments
                    .get(key)
                    .and_then(Value::as_array)
                    .cloned()
                    .ok_or_else(|| unsupported(&format!("arguments.{key}")))
            };
            Arguments::Modern {
                jvm: list("jvm")?,
                game: list("game")?,
            }
        }
        (None, Some(Value::String(legacy))) if !legacy.trim().is_empty() => {
            Arguments::Legacy(legacy.clone())
        }
        _ => return Err(unsupported("arguments")),
    };
    Ok(Descriptor {
        id: id.to_owned(),
        version_type: version_type.to_owned(),
        main_class: main_class.to_owned(),
        java_major,
        java_component,
        client,
        server,
        client_mappings,
        asset_index,
        logging,
        libraries,
        arguments,
    })
}

/// `natives`, `downloads.classifiers` and `extract` of one library, if present.
fn parse_natives(library: &Value, downloads: &Value) -> LauncherResult<Option<Natives>> {
    let Some(map) = library.get("natives") else {
        if downloads.get("classifiers").is_some() {
            return Err(unsupported("classifiers without natives"));
        }
        return Ok(None);
    };
    let map = map
        .as_object()
        .ok_or_else(|| unsupported("library natives"))?;
    let mut classifiers = std::collections::BTreeMap::new();
    for (os, classifier) in map {
        let classifier = classifier
            .as_str()
            .filter(|c| !c.is_empty())
            .ok_or_else(|| unsupported("native classifier"))?;
        classifiers.insert(os.clone(), classifier.to_owned());
    }
    let mut native_downloads = std::collections::BTreeMap::new();
    if let Some(declared) = downloads.get("classifiers") {
        let declared = declared
            .as_object()
            .ok_or_else(|| unsupported("library classifiers"))?;
        for (classifier, entry) in declared {
            let path = string(entry, "path")?;
            validate_relative_path(path)?;
            native_downloads.insert(
                classifier.clone(),
                LibraryArtifact {
                    path: path.to_owned(),
                    download: download(entry, "native")?,
                },
            );
        }
    }
    let exclude = match library.pointer("/extract/exclude") {
        None => Vec::new(),
        Some(list) => list
            .as_array()
            .ok_or_else(|| unsupported("extract.exclude"))?
            .iter()
            .map(|e| {
                e.as_str()
                    .filter(|e| !e.is_empty())
                    .map(str::to_owned)
                    .ok_or_else(|| unsupported("extract.exclude"))
            })
            .collect::<LauncherResult<_>>()?,
    };
    Ok(Some(Natives {
        classifiers,
        downloads: native_downloads,
        exclude,
    }))
}
