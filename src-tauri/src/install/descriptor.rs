//! Strict parsing of a verified modern vanilla version descriptor
//! (docs/spec/04 "Metadata resolution").
//!
//! Only the pinned modern shape is supported. Inheritance, legacy
//! `minecraftArguments`, classifier/natives extraction, snapshots and missing
//! download metadata are rejected with `UNSUPPORTED_VERSION_METADATA` instead of
//! being guessed. Unknown optional fields are tolerated.

use serde_json::{Map, Value};

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Library {
    /// Maven coordinate, provenance only.
    pub name: String,
    /// Declared relative Maven path, validated but never used as a destination.
    pub path: String,
    pub download: Download,
    pub rules: Option<Value>,
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
    pub client: Download,
    pub asset_index: AssetIndexRef,
    pub logging: Option<Logging>,
    pub libraries: Vec<Library>,
    pub jvm_arguments: Vec<Value>,
    pub game_arguments: Vec<Value>,
}

pub fn unsupported(reason: &str) -> LauncherError {
    LauncherError::new(
        ErrorCode::UnsupportedVersionMetadata,
        "This Minecraft version's metadata uses a format the launcher does not support.",
    )
    .with_detail("reason", reason.to_owned())
}

fn obj<'a>(value: &'a Value, key: &str) -> LauncherResult<&'a Map<String, Value>> {
    value
        .get(key)
        .and_then(Value::as_object)
        .ok_or_else(|| unsupported(&format!("missing object {key}")))
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

fn valid_main_class(name: &str) -> bool {
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
    for legacy in ["inheritsFrom", "minecraftArguments"] {
        if value.get(legacy).is_some() {
            return Err(unsupported(legacy));
        }
    }
    let id = string(&value, "id")?;
    if id != expected_id {
        return Err(unsupported("unexpected version id"));
    }
    let version_type = string(&value, "type")?;
    if version_type != "release" {
        return Err(unsupported("only release versions are supported"));
    }
    let main_class = string(&value, "mainClass")?;
    if !valid_main_class(main_class) {
        return Err(unsupported("mainClass"));
    }
    let java_major = obj(&value, "javaVersion")?
        .get("majorVersion")
        .and_then(Value::as_u64)
        .and_then(|m| u16::try_from(m).ok())
        .filter(|m| (8..=99).contains(m))
        .ok_or_else(|| unsupported("javaVersion.majorVersion"))?;
    let downloads = value
        .get("downloads")
        .ok_or_else(|| unsupported("downloads"))?;
    let client = download(
        downloads
            .get("client")
            .ok_or_else(|| unsupported("downloads.client"))?,
        "downloads.client",
    )?;

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
        for legacy in ["natives", "extract"] {
            if library.get(legacy).is_some() {
                return Err(unsupported("legacy native extraction"));
            }
        }
        let name = string(library, "name")?;
        let library_downloads = library
            .get("downloads")
            .ok_or_else(|| unsupported("library without downloads"))?;
        if library_downloads.get("classifiers").is_some() {
            return Err(unsupported("library classifiers"));
        }
        let artifact = library_downloads
            .get("artifact")
            .ok_or_else(|| unsupported("library without artifact"))?;
        let path = string(artifact, "path")?;
        validate_relative_path(path)?;
        libraries.push(Library {
            name: name.to_owned(),
            path: path.to_owned(),
            download: download(artifact, "library")?,
            rules: library.get("rules").cloned(),
        });
    }

    let arguments = obj(&value, "arguments")?;
    let list = |key: &str| {
        arguments
            .get(key)
            .and_then(Value::as_array)
            .cloned()
            .ok_or_else(|| unsupported(&format!("arguments.{key}")))
    };
    Ok(Descriptor {
        id: id.to_owned(),
        version_type: version_type.to_owned(),
        main_class: main_class.to_owned(),
        java_major,
        client,
        asset_index,
        logging,
        libraries,
        jvm_arguments: list("jvm")?,
        game_arguments: list("game")?,
    })
}
