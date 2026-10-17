//! Adoptium (Eclipse Temurin) runtime provider (docs/spec/04 "Managed Java").
//!
//! Asks for the latest GA HotSpot JRE of the exact major, OS and architecture and
//! uses its portable `binary.package` (never `binary.installer`). When no JRE
//! exists, a JDK of the same major is an explicit, recorded fallback. A different
//! major or architecture is never substituted.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use url::Url;

use crate::error::{ErrorCode, LauncherError, LauncherResult};
use crate::install::plan::RuntimeRequirement;
use crate::net::{PublicClient, SourcePolicy, status_error};
use crate::storage::paths::{ImageType, RuntimeKey, Sha256, TargetOs};

pub const API_BASE: &str = "https://api.adoptium.net/v3/assets/latest";
pub const MAX_API_BYTES: usize = 1024 * 1024;
/// Upper bound for a runtime archive; Temurin JREs are about 40–65 MB.
pub const MAX_ARCHIVE_BYTES: u64 = 512 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArchiveFormat {
    TarGz,
    Zip,
}

impl ArchiveFormat {
    pub fn extension(self) -> &'static str {
        match self {
            Self::TarGz => "tar.gz",
            Self::Zip => "zip",
        }
    }
}

/// A resolved, pinned runtime package. Persisted in job receipts and install
/// locks; repair reuses it instead of asking for the latest build again.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimePackage {
    pub provider: String,
    pub key: RuntimeKey,
    pub release_name: String,
    /// Full version string reported by the provider, e.g. `21.0.12.1+1-LTS`.
    pub openjdk_version: String,
    pub archive_name: String,
    pub archive_url: String,
    pub archive_size: u64,
    pub archive_format: ArchiveFormat,
    /// True when no JRE existed and a JDK of the same major was chosen.
    pub jdk_fallback: bool,
}

impl RuntimePackage {
    pub fn archive_sha256(&self) -> &Sha256 {
        &self.key.archive_sha256
    }
}

pub fn query_url(req: &RuntimeRequirement, image: ImageType) -> String {
    format!(
        "{API_BASE}/{}/hotspot?architecture={}&image_type={}&os={}&vendor=eclipse",
        req.major,
        req.arch.as_str(),
        image.as_str(),
        req.os.as_str()
    )
}

fn text<'a>(value: &'a Value, pointer: &str) -> Option<&'a str> {
    value.pointer(pointer).and_then(Value::as_str)
}

/// Picks the matching package from an API response. Entries that do not match
/// the request exactly or have unusable package data are ignored.
pub fn select(
    body: &[u8],
    req: &RuntimeRequirement,
    image: ImageType,
) -> LauncherResult<Option<RuntimePackage>> {
    let value: Value = serde_json::from_slice(body).map_err(|_| malformed())?;
    let entries = value.as_array().ok_or_else(malformed)?;
    let mut candidates: Vec<(String, RuntimePackage)> = entries
        .iter()
        .filter_map(|entry| candidate(entry, req, image))
        .collect();
    // Deterministic choice if the provider ever lists several builds.
    candidates.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.archive_name.cmp(&b.1.archive_name)));
    Ok(candidates.into_iter().next().map(|(_, package)| package))
}

fn candidate(
    entry: &Value,
    req: &RuntimeRequirement,
    image: ImageType,
) -> Option<(String, RuntimePackage)> {
    let binary = entry.get("binary")?;
    let matches = text(entry, "/vendor") == Some("eclipse")
        && entry.pointer("/version/major").and_then(Value::as_u64) == Some(u64::from(req.major))
        && text(binary, "/os") == Some(req.os.as_str())
        && text(binary, "/architecture") == Some(req.arch.as_str())
        && text(binary, "/image_type") == Some(image.as_str())
        && text(binary, "/jvm_impl") == Some("hotspot")
        && text(binary, "/heap_size").is_none_or(|h| h == "normal")
        && text(entry, "/release_type").is_none_or(|t| t == "ga");
    if !matches {
        return None;
    }
    let package = binary.get("package")?;
    let name = text(package, "/name")?;
    let format = match req.os {
        TargetOs::Windows if name.ends_with(".zip") => ArchiveFormat::Zip,
        TargetOs::Linux | TargetOs::Mac if name.ends_with(".tar.gz") => ArchiveFormat::TarGz,
        _ => return None,
    };
    let link = text(package, "/link")?;
    let url = SourcePolicy::Adoptium.check(link).ok()?;
    let size = package.get("size")?.as_u64()?;
    if size == 0 || size > MAX_ARCHIVE_BYTES {
        return None;
    }
    let sha256 = Sha256::parse(text(package, "/checksum")?).ok()?;
    let release_name = text(entry, "/release_name")?;
    if release_name.is_empty() || release_name.len() > 64 {
        return None;
    }
    let updated = text(binary, "/updated_at").unwrap_or("").to_owned();
    Some((
        updated,
        RuntimePackage {
            provider: "temurin".into(),
            key: RuntimeKey {
                os: req.os,
                arch: req.arch,
                major: req.major,
                image_type: image,
                archive_sha256: sha256,
            },
            release_name: release_name.to_owned(),
            openjdk_version: text(entry, "/version/openjdk_version")
                .unwrap_or(release_name)
                .to_owned(),
            archive_name: name.to_owned(),
            archive_url: url.to_string(),
            archive_size: size,
            archive_format: format,
            jdk_fallback: image == ImageType::Jdk,
        },
    ))
}

fn malformed() -> LauncherError {
    LauncherError::new(
        ErrorCode::RuntimeUnavailable,
        "The Java runtime catalog returned data the launcher could not read.",
    )
}

pub fn unavailable(req: &RuntimeRequirement) -> LauncherError {
    LauncherError::new(
        ErrorCode::RuntimeUnavailable,
        format!(
            "No Java {} runtime is available for this computer from the runtime provider.",
            req.major
        ),
    )
    .with_detail("javaMajor", i64::from(req.major))
    .with_detail("os", req.os.as_str())
    .with_detail("arch", req.arch.as_str())
}

/// Live provider client.
pub struct Adoptium {
    client: PublicClient,
}

impl Adoptium {
    pub fn new(client: PublicClient) -> LauncherResult<Self> {
        if client.policy() != SourcePolicy::Adoptium {
            return Err(LauncherError::internal("Wrong source policy for Adoptium."));
        }
        Ok(Self { client })
    }

    async fn query(
        &self,
        req: &RuntimeRequirement,
        image: ImageType,
    ) -> LauncherResult<Option<RuntimePackage>> {
        let url = Url::parse(&query_url(req, image))
            .map_err(|_| LauncherError::internal("Invalid runtime query."))?;
        let request = self.client.get(&url)?.header("Accept", "application/json");
        let (status, _, body) = self.client.get_bounded(request, MAX_API_BYTES).await?;
        match status.as_u16() {
            200 => select(&body, req, image),
            // The API answers 404 when nothing matches the filters.
            404 => Ok(None),
            _ => Err(status_error(status, "the Java runtime catalog")),
        }
    }

    /// JRE first; a same-major JDK only when no JRE exists.
    pub async fn resolve(&self, req: &RuntimeRequirement) -> LauncherResult<RuntimePackage> {
        if let Some(package) = self.query(req, ImageType::Jre).await? {
            return Ok(package);
        }
        if let Some(package) = self.query(req, ImageType::Jdk).await? {
            return Ok(package);
        }
        Err(unavailable(req))
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::storage::paths::TargetArch;

    const LINUX_21: &[u8] = include_bytes!("../../../tests/fixtures/adoptium/21-linux-x64.json");
    const WINDOWS_21: &[u8] =
        include_bytes!("../../../tests/fixtures/adoptium/21-windows-x64.json");
    const MAC_ARM_25: &[u8] =
        include_bytes!("../../../tests/fixtures/adoptium/25-mac-aarch64.json");

    fn req(major: u16, os: TargetOs, arch: TargetArch) -> RuntimeRequirement {
        RuntimeRequirement { major, os, arch }
    }

    #[test]
    fn real_responses_select_the_portable_package() {
        let windows = select(
            WINDOWS_21,
            &req(21, TargetOs::Windows, TargetArch::X64),
            ImageType::Jre,
        )
        .unwrap()
        .unwrap();
        // Matches the example recorded in the specification's source register.
        assert_eq!(
            windows.archive_name,
            "OpenJDK21U-jre_x64_windows_hotspot_21.0.12.1_1.zip"
        );
        assert_eq!(windows.archive_size, 48_999_141);
        assert_eq!(
            windows.archive_sha256().as_str(),
            "d35f31e712f0fcf6ac5a093edc90204fbff22f720ba3950bd09d331d5e621636"
        );
        assert_eq!(windows.archive_format, ArchiveFormat::Zip);
        assert!(!windows.jdk_fallback);

        let mac = select(
            MAC_ARM_25,
            &req(25, TargetOs::Mac, TargetArch::Aarch64),
            ImageType::Jre,
        )
        .unwrap()
        .unwrap();
        assert!(
            mac.archive_name
                .ends_with("_aarch64_mac_hotspot_25.0.4.1_1.tar.gz")
        );
        assert!(!mac.archive_url.ends_with(".pkg"), "installer never chosen");
        assert_eq!(mac.release_name, "jdk-25.0.4.1+1");

        let linux = select(
            LINUX_21,
            &req(21, TargetOs::Linux, TargetArch::X64),
            ImageType::Jre,
        )
        .unwrap()
        .unwrap();
        assert_eq!(linux.archive_format, ArchiveFormat::TarGz);
        assert_eq!(linux.key.major, 21);
    }

    #[test]
    fn mismatches_are_never_substituted() {
        // Right file, wrong request: another major, architecture, OS or image type.
        for (r, image) in [
            (req(25, TargetOs::Linux, TargetArch::X64), ImageType::Jre),
            (
                req(21, TargetOs::Linux, TargetArch::Aarch64),
                ImageType::Jre,
            ),
            (req(21, TargetOs::Mac, TargetArch::X64), ImageType::Jre),
            (req(21, TargetOs::Linux, TargetArch::X64), ImageType::Jdk),
        ] {
            assert_eq!(
                select(LINUX_21, &r, image).unwrap(),
                None,
                "{r:?} {image:?}"
            );
        }
    }

    #[test]
    fn unusable_package_data_is_ignored() {
        let base: Value = serde_json::from_slice(LINUX_21).unwrap();
        let r = req(21, TargetOs::Linux, TargetArch::X64);
        let edits: Vec<(&str, Value)> = vec![
            (
                "/0/binary/package/link",
                json!("http://github.com/x.tar.gz"),
            ),
            (
                "/0/binary/package/link",
                json!("https://evil.example/x.tar.gz"),
            ),
            ("/0/binary/package/checksum", json!("abc")),
            ("/0/binary/package/size", json!(0)),
            ("/0/binary/package/size", json!(MAX_ARCHIVE_BYTES + 1)),
            (
                "/0/binary/package/name",
                json!("OpenJDK21U-jre_x64_linux.pkg"),
            ),
            ("/0/binary/jvm_impl", json!("openj9")),
            ("/0/vendor", json!("other")),
            ("/0/release_type", json!("ea")),
        ];
        for (pointer, replacement) in edits {
            let mut value = base.clone();
            match value.pointer_mut(pointer) {
                Some(slot) => *slot = replacement.clone(),
                // Fields the current API omits are added.
                None => value[0]["release_type"] = replacement.clone(),
            }
            let body = serde_json::to_vec(&value).unwrap();
            assert_eq!(
                select(&body, &r, ImageType::Jre).unwrap(),
                None,
                "{pointer} = {replacement}"
            );
        }
        assert_eq!(select(b"[]", &r, ImageType::Jre).unwrap(), None);
        assert!(select(b"{}", &r, ImageType::Jre).is_err());
    }

    #[test]
    fn query_urls_use_adoptium_names() {
        assert_eq!(
            query_url(&req(25, TargetOs::Mac, TargetArch::Aarch64), ImageType::Jre),
            "https://api.adoptium.net/v3/assets/latest/25/hotspot?architecture=aarch64&image_type=jre&os=mac&vendor=eclipse"
        );
        assert!(
            SourcePolicy::Adoptium
                .check(&query_url(
                    &req(21, TargetOs::Windows, TargetArch::X64),
                    ImageType::Jdk
                ))
                .is_ok()
        );
    }
}
