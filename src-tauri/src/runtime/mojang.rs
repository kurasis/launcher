//! Mojang Java runtime provider (D-036): the runtimes Mojang's own launcher uses.
//!
//! `all.json` lists, per platform, one build of each runtime component
//! (`jre-legacy`, `java-runtime-delta`, ...). A version descriptor names its
//! component in `javaVersion.component`. The chosen component's manifest lists
//! every file with its SHA-1, size and URL, plus directories and links. The
//! package is pinned by the SHA-256 of that manifest, so a repair rebuilds exactly
//! the same files. When the component is not published for this platform, another
//! component of the same Java major is used; when none exists the caller falls
//! back to Temurin. A different major is never substituted.

use serde_json::{Map, Value};
use sha1::Digest as _;
use url::Url;

use super::adoptium::{ArchiveFormat, RuntimePackage};
use super::extract::{Limits, normalize, unsafe_archive};
use crate::error::{ErrorCode, LauncherError, LauncherResult};
use crate::install::plan::RuntimeRequirement;
use crate::net::{PublicClient, SourcePolicy, status_error};
use crate::storage::paths::{
    ImageType, JavaProvider, RuntimeKey, Sha1, Sha256, TargetArch, TargetOs,
};

/// Mojang's runtime index (the URL Mojang's launcher uses).
pub const INDEX_URL: &str = "https://piston-meta.mojang.com/v1/products/java-runtime/2ec0cc96c44e5a76b9c8b7c39df7210883d12871/all.json";
pub const MAX_INDEX_BYTES: usize = 1024 * 1024;
/// Component manifests are 80–160 KB.
pub const MAX_MANIFEST_BYTES: u64 = 8 * 1024 * 1024;
/// Not a Java runtime: the Windows launcher's own executable.
const NOT_A_RUNTIME: &str = "minecraft-java-exe";

/// Mojang's platform name for a target.
pub fn platform(os: TargetOs, arch: TargetArch) -> Option<&'static str> {
    match (os, arch) {
        (TargetOs::Linux, TargetArch::X64) => Some("linux"),
        (TargetOs::Windows, TargetArch::X64) => Some("windows-x64"),
        (TargetOs::Windows, TargetArch::Aarch64) => Some("windows-arm64"),
        (TargetOs::Mac, TargetArch::X64) => Some("mac-os"),
        (TargetOs::Mac, TargetArch::Aarch64) => Some("mac-os-arm64"),
        // Mojang publishes no runtimes for Linux on ARM.
        (TargetOs::Linux, TargetArch::Aarch64) => None,
    }
}

/// Java executable inside a Mojang runtime.
pub fn java_relative_path(os: TargetOs) -> &'static str {
    match os {
        TargetOs::Windows => "bin/java.exe",
        TargetOs::Mac => "jre.bundle/Contents/Home/bin/java",
        TargetOs::Linux => "bin/java",
    }
}

/// Java major of a Mojang version name: `8u202`, `1.8.0_51`, `16.0.1.9.1`, `21.0.7`.
pub fn parse_major(name: &str) -> Option<u16> {
    let leading = |s: &str| -> Option<u16> {
        let digits: String = s.chars().take_while(char::is_ascii_digit).collect();
        digits.parse().ok()
    };
    let major = match name.strip_prefix("1.") {
        Some(rest) => leading(rest)?,
        None => leading(name)?,
    };
    (8..=99).contains(&major).then_some(major)
}

/// One component build from the index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listing {
    pub component: String,
    pub version_name: String,
    pub major: u16,
    pub manifest_url: Url,
    pub manifest_sha1: Sha1,
    pub manifest_size: u64,
}

fn listing(component: &str, builds: &Value) -> Option<Listing> {
    if component == NOT_A_RUNTIME || !crate::install::descriptor::valid_component(component) {
        return None;
    }
    let build = builds.as_array()?.first()?;
    let manifest = build.get("manifest")?;
    let url = SourcePolicy::MojangArtifacts
        .check(manifest.get("url")?.as_str()?)
        .ok()?;
    let sha1 = Sha1::parse(manifest.get("sha1")?.as_str()?).ok()?;
    let size = manifest.get("size")?.as_u64()?;
    if size == 0 || size > MAX_MANIFEST_BYTES {
        return None;
    }
    let version_name = build.pointer("/version/name")?.as_str()?;
    if version_name.is_empty() || version_name.len() > 64 {
        return None;
    }
    Some(Listing {
        component: component.to_owned(),
        version_name: version_name.to_owned(),
        major: parse_major(version_name)?,
        manifest_url: url,
        manifest_sha1: sha1,
        manifest_size: size,
    })
}

fn malformed() -> LauncherError {
    LauncherError::new(
        ErrorCode::RuntimeUnavailable,
        "Mojang's Java runtime list returned data the launcher could not read.",
    )
}

/// The build to install: the named component, else a component of the same
/// major. `None` when this platform has neither.
pub fn select(index: &[u8], req: &RuntimeRequirement) -> LauncherResult<Option<Listing>> {
    let value: Value = serde_json::from_slice(index).map_err(|_| malformed())?;
    let platforms = value.as_object().ok_or_else(malformed)?;
    let Some(name) = platform(req.os, req.arch) else {
        return Ok(None);
    };
    let Some(components) = platforms.get(name).and_then(Value::as_object) else {
        return Ok(None);
    };
    if let Some(wanted) = &req.component
        && let Some(found) = components.get(wanted).and_then(|b| listing(wanted, b))
    {
        return Ok(Some(found));
    }
    // Map iteration is sorted by name, so the choice is deterministic.
    Ok(components
        .iter()
        .filter_map(|(component, builds)| listing(component, builds))
        .find(|l| l.major == req.major))
}

/// One path of a component manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestEntry {
    pub path: String,
    pub kind: ManifestKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManifestKind {
    Dir,
    File {
        url: Url,
        sha1: Sha1,
        size: u64,
        executable: bool,
    },
    Link {
        target: String,
    },
}

fn manifest_file(entry: &Map<String, Value>) -> Option<ManifestKind> {
    let raw = entry.get("downloads")?.get("raw")?;
    let url = Url::parse(raw.get("url")?.as_str()?).ok()?;
    Some(ManifestKind::File {
        url,
        sha1: Sha1::parse(raw.get("sha1")?.as_str()?).ok()?,
        size: raw.get("size")?.as_u64()?,
        executable: entry
            .get("executable")
            .map_or(Some(false), Value::as_bool)?,
    })
}

/// Parses and bounds a component manifest. Paths are checked with the same rules
/// as archive entries; links are checked again when they are created. File hosts
/// are checked by [`package`] and, on every request and redirect, by the
/// download transport.
pub fn parse_manifest(bytes: &[u8], limits: Limits) -> LauncherResult<Vec<ManifestEntry>> {
    let value: Value = serde_json::from_slice(bytes).map_err(|_| malformed())?;
    let files = value
        .get("files")
        .and_then(Value::as_object)
        .ok_or_else(malformed)?;
    if files.len() > limits.max_entries {
        return Err(unsafe_archive("too many entries"));
    }
    let mut total: u64 = 0;
    let mut entries = Vec::with_capacity(files.len());
    for (path, entry) in files {
        normalize(path)?;
        let entry = entry.as_object().ok_or_else(malformed)?;
        let kind = match entry.get("type").and_then(Value::as_str) {
            Some("directory") => ManifestKind::Dir,
            Some("file") => manifest_file(entry).ok_or_else(malformed)?,
            Some("link") => ManifestKind::Link {
                target: entry
                    .get("target")
                    .and_then(Value::as_str)
                    .filter(|t| t.len() <= 4096)
                    .ok_or_else(malformed)?
                    .to_owned(),
            },
            _ => return Err(unsafe_archive("special file entry")),
        };
        if let ManifestKind::File { size, .. } = &kind {
            if *size > limits.max_file_bytes {
                return Err(unsafe_archive("file too large"));
            }
            total += size;
            if total > limits.max_total_bytes {
                return Err(unsafe_archive("expanded size too large"));
            }
        }
        entries.push(ManifestEntry {
            path: path.clone(),
            kind,
        });
    }
    Ok(entries)
}

/// Bytes of every file a manifest lists.
#[cfg(test)]
pub fn manifest_bytes(entries: &[ManifestEntry]) -> u64 {
    entries
        .iter()
        .map(|e| match e.kind {
            ManifestKind::File { size, .. } => size,
            _ => 0,
        })
        .sum()
}

/// The pinned package for a downloaded, verified manifest.
pub fn package(
    req: &RuntimeRequirement,
    listing: &Listing,
    manifest: &[u8],
) -> LauncherResult<RuntimePackage> {
    if manifest.len() as u64 != listing.manifest_size
        || hex::encode(sha1::Sha1::digest(manifest)) != listing.manifest_sha1.as_str()
    {
        return Err(LauncherError::new(
            ErrorCode::HashMismatch,
            "Mojang's Java runtime manifest did not match its checksum.",
        ));
    }
    let entries = parse_manifest(manifest, Limits::default())?;
    let foreign = entries.iter().any(|e| match &e.kind {
        ManifestKind::File { url, .. } => !SourcePolicy::MojangArtifacts.allows(url),
        _ => false,
    });
    if foreign {
        return Err(unsafe_archive("file outside Mojang's download hosts"));
    }
    let java = java_relative_path(req.os);
    if !entries
        .iter()
        .any(|e| e.path == java && matches!(e.kind, ManifestKind::File { .. }))
    {
        return Err(unsafe_archive("java executable not found"));
    }
    Ok(RuntimePackage {
        provider: JavaProvider::Mojang.as_str().to_owned(),
        key: RuntimeKey {
            provider: JavaProvider::Mojang,
            os: req.os,
            arch: req.arch,
            major: listing.major,
            image_type: ImageType::Jre,
            archive_sha256: Sha256::parse(&super::tree::sha256_hex(manifest))?,
        },
        release_name: listing.version_name.clone(),
        openjdk_version: format!("{} ({})", listing.version_name, listing.component),
        archive_name: listing.component.clone(),
        archive_url: listing.manifest_url.to_string(),
        archive_size: listing.manifest_size,
        archive_format: ArchiveFormat::MojangManifest,
        jdk_fallback: false,
    })
}

/// Live provider client.
pub struct Mojang {
    client: PublicClient,
}

impl Mojang {
    pub fn new(client: PublicClient) -> LauncherResult<Self> {
        if client.policy() != SourcePolicy::MojangMetadata {
            return Err(LauncherError::internal("Wrong source policy for Mojang."));
        }
        Ok(Self { client })
    }

    async fn fetch(&self, url: &Url, max: usize, what: &str) -> LauncherResult<Vec<u8>> {
        let json = |r: reqwest::RequestBuilder| r.header("Accept", "application/json");
        let (status, _, body) = self.client.fetch_bounded(url, json, max).await?;
        if status.as_u16() != 200 {
            return Err(status_error(status, what));
        }
        Ok(body.to_vec())
    }

    /// The pinned Mojang runtime for `req`, or `None` when Mojang publishes no
    /// runtime of that component or major for this platform.
    pub async fn resolve(
        &self,
        req: &RuntimeRequirement,
    ) -> LauncherResult<Option<RuntimePackage>> {
        if platform(req.os, req.arch).is_none() {
            return Ok(None);
        }
        let index_url =
            Url::parse(INDEX_URL).map_err(|_| LauncherError::internal("Invalid runtime index."))?;
        let index = self
            .fetch(&index_url, MAX_INDEX_BYTES, "Mojang's Java runtime list")
            .await?;
        let Some(listing) = select(&index, req)? else {
            return Ok(None);
        };
        let size = usize::try_from(listing.manifest_size).unwrap_or(usize::MAX);
        let manifest = self
            .fetch(
                &listing.manifest_url,
                size,
                "Mojang's Java runtime manifest",
            )
            .await?;
        package(req, &listing, &manifest).map(Some)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    const INDEX: &[u8] = include_bytes!("../../../tests/fixtures/mojang-runtime/all.json");
    const LEGACY_LINUX: &[u8] =
        include_bytes!("../../../tests/fixtures/mojang-runtime/jre-legacy-linux.json");

    fn req(
        major: u16,
        component: Option<&str>,
        os: TargetOs,
        arch: TargetArch,
    ) -> RuntimeRequirement {
        RuntimeRequirement {
            major,
            os,
            arch,
            component: component.map(str::to_owned),
        }
    }

    #[test]
    fn version_names_give_the_major() {
        assert_eq!(parse_major("8u202"), Some(8));
        assert_eq!(parse_major("8u51-cacert462b08"), Some(8));
        assert_eq!(parse_major("1.8.0_51"), Some(8));
        assert_eq!(parse_major("16.0.1.9.1_3"), Some(16));
        assert_eq!(parse_major("21.0.7"), Some(21));
        assert_eq!(parse_major("25.0.1"), Some(25));
        assert_eq!(parse_major("14"), Some(14));
        assert_eq!(parse_major("7u80"), None);
        assert_eq!(parse_major("latest"), None);
    }

    #[test]
    fn the_named_component_is_chosen_for_each_platform() {
        let cases = [
            (TargetOs::Linux, TargetArch::X64, "jre-legacy", 8, "8u202"),
            (
                TargetOs::Windows,
                TargetArch::X64,
                "java-runtime-alpha",
                16,
                "16.0.1.9.1",
            ),
            (
                TargetOs::Mac,
                TargetArch::X64,
                "java-runtime-delta",
                21,
                "21.0.7",
            ),
            (
                TargetOs::Mac,
                TargetArch::Aarch64,
                "java-runtime-epsilon",
                25,
                "25.0.1",
            ),
            (
                TargetOs::Windows,
                TargetArch::Aarch64,
                "java-runtime-gamma",
                17,
                "17.0.15",
            ),
        ];
        for (os, arch, component, major, version) in cases {
            let found = select(INDEX, &req(major, Some(component), os, arch))
                .unwrap()
                .unwrap();
            assert_eq!(found.component, component, "{os:?} {arch:?}");
            assert_eq!(found.major, major);
            assert_eq!(found.version_name, version);
            assert_eq!(
                found.manifest_url.host_str(),
                Some("piston-meta.mojang.com")
            );
        }
    }

    #[test]
    fn a_missing_component_uses_the_same_major_or_nothing() {
        // Apple silicon has no java-runtime-beta (17) and no jre-legacy (8).
        let beta = select(
            INDEX,
            &req(
                17,
                Some("java-runtime-beta"),
                TargetOs::Mac,
                TargetArch::Aarch64,
            ),
        )
        .unwrap()
        .unwrap();
        assert_eq!(beta.major, 17);
        assert_eq!(beta.component, "java-runtime-gamma");
        assert_eq!(
            select(
                INDEX,
                &req(8, Some("jre-legacy"), TargetOs::Mac, TargetArch::Aarch64)
            )
            .unwrap(),
            None
        );
        // Linux on ARM: Mojang publishes nothing.
        assert_eq!(
            select(
                INDEX,
                &req(
                    21,
                    Some("java-runtime-delta"),
                    TargetOs::Linux,
                    TargetArch::Aarch64
                )
            )
            .unwrap(),
            None
        );
        // No component named: by major.
        let unnamed = select(INDEX, &req(21, None, TargetOs::Linux, TargetArch::X64))
            .unwrap()
            .unwrap();
        assert_eq!(unnamed.component, "java-runtime-delta");
    }

    #[test]
    fn unusable_index_entries_are_ignored() {
        let base: Value = serde_json::from_slice(INDEX).unwrap();
        let r = req(8, Some("jre-legacy"), TargetOs::Linux, TargetArch::X64);
        for (pointer, replacement) in [
            (
                "/linux/jre-legacy/0/manifest/url",
                json!("https://evil.example/m.json"),
            ),
            (
                "/linux/jre-legacy/0/manifest/url",
                json!("http://piston-meta.mojang.com/m.json"),
            ),
            ("/linux/jre-legacy/0/manifest/sha1", json!("abc")),
            ("/linux/jre-legacy/0/manifest/size", json!(0)),
            (
                "/linux/jre-legacy/0/manifest/size",
                json!(MAX_MANIFEST_BYTES + 1),
            ),
            ("/linux/jre-legacy/0/version/name", json!("7u80")),
        ] {
            let mut value = base.clone();
            *value.pointer_mut(pointer).unwrap() = replacement.clone();
            let body = serde_json::to_vec(&value).unwrap();
            assert_eq!(
                select(&body, &r).unwrap(),
                None,
                "{pointer} = {replacement}"
            );
        }
        assert!(select(b"[]", &r).is_err());
    }

    #[test]
    fn a_real_manifest_parses_and_pins_the_package() {
        let entries = parse_manifest(LEGACY_LINUX, Limits::default()).unwrap();
        let files = entries
            .iter()
            .filter(|e| matches!(e.kind, ManifestKind::File { .. }))
            .count();
        let links = entries
            .iter()
            .filter(|e| matches!(e.kind, ManifestKind::Link { .. }))
            .count();
        assert_eq!((files, links), (300, 3));
        assert!(manifest_bytes(&entries) > 200 * 1024 * 1024);
        let java = entries.iter().find(|e| e.path == "bin/java").unwrap();
        assert!(matches!(
            java.kind,
            ManifestKind::File {
                executable: true,
                ..
            }
        ));

        let r = req(8, Some("jre-legacy"), TargetOs::Linux, TargetArch::X64);
        let listing = select(INDEX, &r).unwrap().unwrap();
        let package = package(&r, &listing, LEGACY_LINUX).unwrap();
        assert_eq!(package.key.provider, JavaProvider::Mojang);
        assert_eq!(package.key.major, 8);
        assert_eq!(package.archive_format, ArchiveFormat::MojangManifest);
        assert_eq!(package.archive_size, LEGACY_LINUX.len() as u64);
        let mut tampered = LEGACY_LINUX.to_vec();
        tampered[10] ^= 1;
        assert_eq!(
            super::package(&r, &listing, &tampered).unwrap_err().code,
            ErrorCode::HashMismatch
        );
    }

    #[test]
    fn unsafe_manifests_are_refused() {
        let file = |path: &str| {
            json!({ "files": { path: {
                "type": "file", "executable": false,
                "downloads": { "raw": {
                    "sha1": "c725183c757011e7ba96c83c1e86ee7e8b516a2b",
                    "size": 1,
                    "url": "https://piston-data.mojang.com/v1/objects/c725183c757011e7ba96c83c1e86ee7e8b516a2b/x"
                } }
            } } })
        };
        for bad in [
            file("../escape"),
            file("/abs"),
            file("bin/con"),
            json!({ "files": { "dev": { "type": "fifo" } } }),
            json!({ "files": { "x": { "type": "file", "downloads": { "raw": {
                "sha1": "c725183c757011e7ba96c83c1e86ee7e8b516a2b", "size": 1,
                "url": "not a url" } } } } }),
        ] {
            let bytes = serde_json::to_vec(&bad).unwrap();
            assert!(parse_manifest(&bytes, Limits::default()).is_err(), "{bad}");
        }
        let small = Limits {
            max_entries: 10,
            max_file_bytes: 10,
            max_total_bytes: 10,
        };
        let big = serde_json::to_vec(&file("ok")).unwrap();
        assert!(parse_manifest(&big, small).is_ok());
        assert!(parse_manifest(LEGACY_LINUX, small).is_err());

        // A file served from another host is refused when the package is pinned.
        let mut value: Value = serde_json::from_slice(LEGACY_LINUX).unwrap();
        value["files"]["bin/java"]["downloads"]["raw"]["url"] = json!("https://evil.example/java");
        let foreign = serde_json::to_vec(&value).unwrap();
        let r = req(8, Some("jre-legacy"), TargetOs::Linux, TargetArch::X64);
        let listing = Listing {
            manifest_sha1: Sha1::parse(&hex::encode(sha1::Sha1::digest(&foreign))).unwrap(),
            manifest_size: foreign.len() as u64,
            ..select(INDEX, &r).unwrap().unwrap()
        };
        assert_eq!(
            package(&r, &listing, &foreign).unwrap_err().code,
            ErrorCode::UnsafeArchive
        );
    }
}
