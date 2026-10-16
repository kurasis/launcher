//! Build-time configuration of the project-owned Microsoft public-client ID.
//!
//! The ID is non-secret configuration (a public client has no secret). It is read from
//! the `KURASIS_MSA_CLIENT_ID` environment variable when the Rust core is compiled.
//! There is deliberately no fallback value: the launcher must never borrow another
//! application's client ID. See docs/AUTH-SETUP.md.

use serde::Serialize;

/// Environment variable read at compile time.
pub const CLIENT_ID_ENV: &str = "KURASIS_MSA_CLIENT_ID";

const BUILD_TIME_VALUE: Option<&str> = option_env!("KURASIS_MSA_CLIENT_ID");

/// A syntactically valid Microsoft application (client) ID, normalized to lowercase.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientId(String);

impl ClientId {
    /// Parses a GUID in the canonical `8-4-4-4-12` hexadecimal form. Surrounding
    /// whitespace is ignored; the nil GUID is rejected.
    pub fn parse(raw: &str) -> Result<Self, InvalidClientId> {
        let value = raw.trim().to_ascii_lowercase();
        let groups: Vec<&str> = value.split('-').collect();
        let expected_lengths = [8, 4, 4, 4, 12];
        let well_formed = groups.len() == expected_lengths.len()
            && groups.iter().zip(expected_lengths).all(|(group, len)| {
                group.len() == len && group.bytes().all(|b| b.is_ascii_hexdigit())
            });
        if !well_formed || value.bytes().all(|b| b == b'0' || b == b'-') {
            return Err(InvalidClientId);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The supplied value is not a usable Microsoft application ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidClientId;

/// Configuration state reported to the UI. A configured ID is never reported as
/// verified: verification requires a real sign-in (P04).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ClientIdStatus {
    NotConfigured,
    Invalid,
    ConfiguredUnverified,
}

/// Classifies a raw configuration value. Absent and blank values are not configured.
pub fn classify(raw: Option<&str>) -> Result<Option<ClientId>, InvalidClientId> {
    match raw.map(str::trim) {
        None | Some("") => Ok(None),
        Some(value) => ClientId::parse(value).map(Some),
    }
}

/// Status derived from a raw configuration value.
pub fn status_of(raw: Option<&str>) -> ClientIdStatus {
    match classify(raw) {
        Ok(None) => ClientIdStatus::NotConfigured,
        Ok(Some(_)) => ClientIdStatus::ConfiguredUnverified,
        Err(InvalidClientId) => ClientIdStatus::Invalid,
    }
}

/// Status of the ID compiled into this build.
pub fn build_time_status() -> ClientIdStatus {
    status_of(BUILD_TIME_VALUE)
}

/// The ID compiled into this build, if present and valid.
pub fn build_time_client_id() -> Result<Option<ClientId>, InvalidClientId> {
    classify(BUILD_TIME_VALUE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_canonical_guid_and_normalizes_case() {
        let id = ClientId::parse(" 0F1E2D3C-4B5A-6978-8796-A5B4C3D2E1F0\n").unwrap();
        assert_eq!(id.as_str(), "0f1e2d3c-4b5a-6978-8796-a5b4c3d2e1f0");
    }

    #[test]
    fn rejects_malformed_values() {
        for raw in [
            "not-a-guid",
            "0f1e2d3c4b5a69788796a5b4c3d2e1f0",
            "{0f1e2d3c-4b5a-6978-8796-a5b4c3d2e1f0}",
            "0f1e2d3c-4b5a-6978-8796-a5b4c3d2e1f",
            "0f1e2d3c-4b5a-6978-8796-a5b4c3d2e1f00",
            "0f1e2d3c-4b5a-6978-8796-a5b4c3d2e1fg",
            "0f1e2d3c-4b5a-6978-8796a-5b4c3d2e1f0",
            "00000000-0000-0000-0000-000000000000",
            "000000004C12AE6F",
        ] {
            assert_eq!(ClientId::parse(raw), Err(InvalidClientId), "{raw}");
        }
    }

    #[test]
    fn missing_or_blank_value_is_not_configured() {
        assert_eq!(status_of(None), ClientIdStatus::NotConfigured);
        assert_eq!(status_of(Some("")), ClientIdStatus::NotConfigured);
        assert_eq!(status_of(Some("   ")), ClientIdStatus::NotConfigured);
    }

    #[test]
    fn valid_value_is_configured_but_never_verified() {
        assert_eq!(
            status_of(Some("0f1e2d3c-4b5a-6978-8796-a5b4c3d2e1f0")),
            ClientIdStatus::ConfiguredUnverified
        );
        assert_eq!(status_of(Some("abc")), ClientIdStatus::Invalid);
    }

    #[test]
    fn status_serializes_to_frontend_contract() {
        let json = serde_json::to_string(&[
            ClientIdStatus::NotConfigured,
            ClientIdStatus::Invalid,
            ClientIdStatus::ConfiguredUnverified,
        ])
        .unwrap();
        assert_eq!(
            json,
            r#"["not_configured","invalid","configured_unverified"]"#
        );
    }

    #[test]
    fn build_time_status_matches_build_time_value() {
        assert_eq!(build_time_status(), status_of(BUILD_TIME_VALUE));
        assert_eq!(
            build_time_client_id().is_ok(),
            build_time_status() != ClientIdStatus::Invalid
        );
    }
}
