//! Shipped compatibility policy (docs/spec/01 "Supported release policy").
//!
//! This list is compiled into the application and is the only authority for which
//! Minecraft versions may be installed. `DATA_ROOT/compatibility.json` is a written
//! copy for diagnostics, never read back as an authorization source. The live
//! Mojang catalog (P05) is intersected with this list; it is not a source here.

use serde::Serialize;

use crate::storage::document::SCHEMA_VERSION;

/// Version of the policy content, bumped whenever entries change.
pub const POLICY_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QualificationStatus {
    /// Candidate for the qualification build; real launch matrix not yet passed.
    PendingVerification,
    /// Passed the real-machine matrix (P13).
    Qualified,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SupportedVersion {
    pub id: &'static str,
    pub status: QualificationStatus,
    /// Expected Java major from the researched descriptor; the locked descriptor
    /// remains authoritative at install time.
    pub expected_java_major: u16,
    /// Descriptor SHA-1 values accepted for this entry. A different upstream hash is
    /// a new candidate and is not trusted automatically.
    pub accepted_descriptor_sha1: &'static [&'static str],
}

pub const SUPPORTED_VERSIONS: &[SupportedVersion] = &[
    SupportedVersion {
        id: "1.21.1",
        status: QualificationStatus::PendingVerification,
        expected_java_major: 21,
        accepted_descriptor_sha1: &["499680d659b7a0ccb2fa9a809c345c5baf2ace7d"],
    },
    SupportedVersion {
        id: "26.3",
        status: QualificationStatus::PendingVerification,
        expected_java_major: 25,
        accepted_descriptor_sha1: &["11f1aefe473eefba0af62e3286c33b692807b2eb"],
    },
];

/// Looks up an exact version ID. IDs are compared as strings, never numerically.
pub fn find(version_id: &str) -> Option<&'static SupportedVersion> {
    SUPPORTED_VERSIONS.iter().find(|v| v.id == version_id)
}

/// The document written to `compatibility.json`.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PolicyDocument {
    pub schema_version: u64,
    pub policy_version: u32,
    pub source: &'static str,
    pub versions: &'static [SupportedVersion],
}

pub fn policy_document() -> PolicyDocument {
    PolicyDocument {
        schema_version: SCHEMA_VERSION,
        policy_version: POLICY_VERSION,
        source: "shipped_with_application",
        versions: SUPPORTED_VERSIONS,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::paths::{SafeName, Sha1};

    #[test]
    fn lookup_is_exact() {
        assert!(find("1.21.1").is_some());
        assert!(find("26.3").is_some());
        for unsupported in [
            "1.21",
            "1.21.10",
            "26.3.0",
            "26.4-snapshot-4",
            "",
            "1.21.1 ",
        ] {
            assert!(find(unsupported).is_none(), "{unsupported}");
        }
    }

    #[test]
    fn entries_are_well_formed() {
        for version in SUPPORTED_VERSIONS {
            assert!(SafeName::parse(version.id).is_ok());
            assert_eq!(version.status, QualificationStatus::PendingVerification);
            for sha1 in version.accepted_descriptor_sha1 {
                assert!(Sha1::parse(sha1).is_ok());
            }
        }
    }
}
