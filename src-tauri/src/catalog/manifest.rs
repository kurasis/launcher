//! The Mojang version catalog (`version_manifest_v2.json`).

use serde::Serialize;
use serde_json::Value;

use crate::error::{ErrorCode, LauncherError, LauncherResult};
use crate::storage::paths::Sha1;

pub const CATALOG_URL: &str = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";
pub const MAX_CATALOG_BYTES: usize = 8 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogEntry {
    pub id: String,
    pub version_type: String,
    pub url: String,
    pub sha1: Sha1,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Catalog {
    /// Informational only; never used to choose or upgrade a version.
    pub latest_release: Option<String>,
    pub versions: Vec<CatalogEntry>,
}

impl Catalog {
    pub fn find(&self, id: &str) -> Option<&CatalogEntry> {
        self.versions.iter().find(|v| v.id == id)
    }
}

fn malformed() -> LauncherError {
    LauncherError::new(
        ErrorCode::UnsupportedVersionMetadata,
        "The Minecraft version catalog could not be read.",
    )
}

/// Parses catalog bytes. Entries with unusable fields are skipped (they cannot be
/// installed anyway); a missing `versions` array is an error.
pub fn parse(bytes: &[u8]) -> LauncherResult<Catalog> {
    if bytes.len() > MAX_CATALOG_BYTES {
        return Err(malformed());
    }
    let value: Value = serde_json::from_slice(bytes).map_err(|_| malformed())?;
    let versions = value
        .get("versions")
        .and_then(Value::as_array)
        .ok_or_else(malformed)?;
    let entries = versions
        .iter()
        .filter_map(|v| {
            Some(CatalogEntry {
                id: v.get("id")?.as_str()?.to_owned(),
                version_type: v.get("type")?.as_str()?.to_owned(),
                url: v.get("url")?.as_str()?.to_owned(),
                sha1: Sha1::parse(v.get("sha1")?.as_str()?).ok()?,
            })
        })
        .collect();
    Ok(Catalog {
        latest_release: value
            .pointer("/latest/release")
            .and_then(Value::as_str)
            .map(str::to_owned),
        versions: entries,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_entries_and_skips_unusable_ones() {
        let catalog = parse(
            br#"{"latest":{"release":"26.3","snapshot":"26.4-snapshot-4"},"versions":[
            {"id":"26.4-snapshot-4","type":"snapshot","url":"https://piston-meta.mojang.com/a.json","sha1":"0000000000000000000000000000000000000001"},
            {"id":"26.3","type":"release","url":"https://piston-meta.mojang.com/b.json","sha1":"11f1aefe473eefba0af62e3286c33b692807b2eb"},
            {"id":"broken","type":"release","url":"x","sha1":"zz"}]}"#,
        )
        .unwrap();
        assert_eq!(catalog.latest_release.as_deref(), Some("26.3"));
        assert_eq!(catalog.versions.len(), 2);
        assert_eq!(catalog.find("26.3").unwrap().version_type, "release");
        assert!(catalog.find("broken").is_none());
    }

    #[test]
    fn missing_versions_is_an_error() {
        assert!(parse(br#"{"latest":{}}"#).is_err());
        assert!(parse(b"[]").is_err());
    }
}
