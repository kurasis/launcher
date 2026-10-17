//! Stable error envelope shared by every command and operation outcome
//! (docs/spec/06-UI-AND-IPC.md, "Stable error vocabulary").
//!
//! Messages are written for users and must never contain secrets, raw upstream
//! bodies or full filesystem paths of unrelated files.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Machine-readable error codes. Serialized as `SCREAMING_SNAKE_CASE` strings that
/// the frontend mirrors in `src/lib/contracts.ts`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorCode {
    AuthCancelled,
    AuthTimeout,
    AuthStateMismatch,
    AuthReauthRequired,
    AuthAppNotApproved,
    AuthAccountRestricted,
    AuthForbidden,
    AuthProtocolError,
    AuthNotConfigured,
    NoGameAccess,
    /// The no-token profile cannot start this version (Minecraft has no demo
    /// mode there). Earlier launchers wrote `DEMO_UNAVAILABLE`.
    #[serde(rename = "NOTOKEN_UNAVAILABLE", alias = "DEMO_UNAVAILABLE")]
    NoTokenUnavailable,
    ProfileSetupRequired,
    ServiceUnavailable,
    SecretStoreUnavailable,
    SecretStoreDeleteFailed,
    UnsupportedPlatform,
    UnsupportedVersionMetadata,
    UnsupportedSchema,
    UnsupportedPathEncoding,
    UnsupportedDataPath,
    NetworkError,
    RateLimited,
    DiskFull,
    PermissionDenied,
    HashMismatch,
    MetadataConflict,
    UnsafeArchive,
    RuntimeUnavailable,
    RuntimeMismatch,
    InstanceBusy,
    InstanceNotFound,
    GameAlreadyRunning,
    GameSpawnFailed,
    GameExitedWithError,
    OperationCancelled,
    OperationInterrupted,
    OperationNotFound,
    AppBusy,
    RepairRequired,
    LoaderInstallFailed,
    RequestIdConflict,
    InvalidInput,
    InsufficientMemory,
    DataRootLocked,
    StorageUnavailable,
    StorageError,
    Internal,
}

impl ErrorCode {
    /// Whether retrying the same request later can reasonably succeed.
    pub const fn default_retryable(self) -> bool {
        matches!(
            self,
            Self::AuthTimeout
                | Self::ServiceUnavailable
                | Self::NetworkError
                | Self::RateLimited
                | Self::AppBusy
                | Self::InstanceBusy
                | Self::OperationInterrupted
                | Self::StorageError
                | Self::SecretStoreDeleteFailed
        )
    }
}

/// A non-secret diagnostic value attached to an error.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SafeValue {
    Bool(bool),
    Number(i64),
    Text(String),
}

impl std::fmt::Display for SafeValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Bool(value) => write!(f, "{value}"),
            Self::Number(value) => write!(f, "{value}"),
            Self::Text(value) => f.write_str(value),
        }
    }
}

impl From<u32> for SafeValue {
    fn from(value: u32) -> Self {
        Self::Number(i64::from(value))
    }
}

impl From<u16> for SafeValue {
    fn from(value: u16) -> Self {
        Self::Number(i64::from(value))
    }
}

impl From<u64> for SafeValue {
    fn from(value: u64) -> Self {
        Self::Number(i64::try_from(value).unwrap_or(i64::MAX))
    }
}

impl From<char> for SafeValue {
    fn from(value: char) -> Self {
        Self::Text(value.to_string())
    }
}

/// A user-facing text built from an English template with `{name}` placeholders
/// (D-038). The interface translates the template and fills in the values; the
/// rendered English stays in `message` for logs and as the fallback.
pub fn render(template: &str, params: &BTreeMap<String, SafeValue>) -> String {
    let mut out = template.to_owned();
    for (name, value) in params {
        out = out.replace(&format!("{{{name}}}"), &value.to_string());
    }
    out
}

/// Parameters from `(name, value)` pairs.
pub fn params<const N: usize>(pairs: [(&str, SafeValue); N]) -> BTreeMap<String, SafeValue> {
    pairs
        .into_iter()
        .map(|(name, value)| (name.to_owned(), value))
        .collect()
}

impl From<&str> for SafeValue {
    fn from(value: &str) -> Self {
        Self::Text(value.to_owned())
    }
}

impl From<String> for SafeValue {
    fn from(value: String) -> Self {
        Self::Text(value)
    }
}

impl From<bool> for SafeValue {
    fn from(value: bool) -> Self {
        Self::Bool(value)
    }
}

impl From<i64> for SafeValue {
    fn from(value: i64) -> Self {
        Self::Number(value)
    }
}

/// The error envelope returned by commands and recorded in operation outcomes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, thiserror::Error)]
#[serde(rename_all = "camelCase")]
#[error("{code:?}: {message}")]
pub struct LauncherError {
    pub code: ErrorCode,
    pub message: String,
    pub retryable: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation_id: Option<String>,
    pub diagnostic_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub safe_details: Option<BTreeMap<String, SafeValue>>,
    /// The translatable form of `message` when it carries values (D-038);
    /// boxed so a plain error stays small.
    #[serde(flatten, default, skip_serializing_if = "Option::is_none")]
    pub text: Option<Box<TemplatedText>>,
}

/// An English template with `{name}` placeholders and the values to put in them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TemplatedText {
    pub template: String,
    pub params: BTreeMap<String, SafeValue>,
}

impl LauncherError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            retryable: code.default_retryable(),
            operation_id: None,
            diagnostic_id: Uuid::new_v4().to_string(),
            safe_details: None,
            text: None,
        }
    }

    /// An error whose text carries values: `template` names them `{name}`.
    pub fn templated(code: ErrorCode, template: &str, params: BTreeMap<String, SafeValue>) -> Self {
        Self {
            text: Some(Box::new(TemplatedText {
                template: template.to_owned(),
                params: params.clone(),
            })),
            ..Self::new(code, render(template, &params))
        }
    }

    pub fn invalid_input(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::InvalidInput, message)
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::Internal, message)
    }

    pub fn with_detail(mut self, key: &str, value: impl Into<SafeValue>) -> Self {
        self.safe_details
            .get_or_insert_with(BTreeMap::new)
            .insert(key.to_owned(), value.into());
        self
    }

    pub fn with_operation(mut self, operation_id: impl Into<String>) -> Self {
        self.operation_id = Some(operation_id.into());
        self
    }

    pub fn with_retryable(mut self, retryable: bool) -> Self {
        self.retryable = retryable;
        self
    }

    /// Maps an I/O failure on launcher-managed storage to a stable code. The
    /// operating-system message is kept out of the user text because it may name
    /// unrelated paths; `context` describes what was being done.
    pub fn from_io(error: &std::io::Error, context: &str) -> Self {
        use std::io::ErrorKind;
        // `context` is itself a translatable text (a template without values).
        let (code, template) = match error.kind() {
            ErrorKind::StorageFull | ErrorKind::QuotaExceeded => (
                ErrorCode::DiskFull,
                "{context}: the disk is full. Free some space and try again.",
            ),
            ErrorKind::PermissionDenied | ErrorKind::ReadOnlyFilesystem => (
                ErrorCode::PermissionDenied,
                "{context}: permission denied by the operating system.",
            ),
            _ => (
                ErrorCode::StorageError,
                "{context}: a file operation failed.",
            ),
        };
        Self::templated(code, template, params([("context", context.into())]))
            .with_detail("ioKind", format!("{:?}", error.kind()))
    }
}

pub type LauncherResult<T> = Result<T, LauncherError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_notoken_code_reads_its_old_name() {
        let code: ErrorCode = serde_json::from_str("\"DEMO_UNAVAILABLE\"").unwrap();
        assert_eq!(code, ErrorCode::NoTokenUnavailable);
        assert_eq!(
            serde_json::to_string(&code).unwrap(),
            "\"NOTOKEN_UNAVAILABLE\""
        );
    }

    #[test]
    fn serializes_to_the_frontend_envelope() {
        let error = LauncherError::new(ErrorCode::AppBusy, "Another operation is running.")
            .with_detail("activeKind", "install")
            .with_operation("op-1");
        let json = serde_json::to_value(&error).unwrap();
        assert_eq!(json["code"], "APP_BUSY");
        assert_eq!(json["retryable"], true);
        assert_eq!(json["operationId"], "op-1");
        assert_eq!(json["safeDetails"]["activeKind"], "install");
        assert!(Uuid::parse_str(json["diagnosticId"].as_str().unwrap()).is_ok());
    }

    #[test]
    fn templated_errors_carry_the_template_and_values() {
        let error = LauncherError::templated(
            ErrorCode::InvalidInput,
            "Maximum memory must be between {min} and {max} MiB on this computer.",
            params([("min", 1024u32.into()), ("max", 13108u32.into())]),
        );
        assert_eq!(
            error.message,
            "Maximum memory must be between 1024 and 13108 MiB on this computer."
        );
        let json = serde_json::to_value(&error).unwrap();
        assert_eq!(
            json["template"],
            "Maximum memory must be between {min} and {max} MiB on this computer."
        );
        assert_eq!(json["params"]["min"], 1024);
        // A plain message has neither.
        let plain = serde_json::to_value(LauncherError::internal("x")).unwrap();
        assert!(plain.get("template").is_none() && plain.get("params").is_none());
        let io = LauncherError::from_io(
            &std::io::Error::from(std::io::ErrorKind::PermissionDenied),
            "Saving settings",
        );
        assert_eq!(
            io.text.as_ref().map(|t| t.template.as_str()),
            Some("{context}: permission denied by the operating system.")
        );
        // Recorded results are read back with or without a template.
        let back: LauncherError = serde_json::from_value(json).unwrap();
        assert_eq!(back.text, error.text);
        let back: LauncherError = serde_json::from_value(plain).unwrap();
        assert!(back.text.is_none());
        assert_eq!(
            io.message,
            "Saving settings: permission denied by the operating system."
        );
    }

    #[test]
    fn optional_fields_are_omitted() {
        let json = serde_json::to_value(LauncherError::invalid_input("bad")).unwrap();
        assert!(json.get("operationId").is_none());
        assert!(json.get("safeDetails").is_none());
        assert_eq!(json["retryable"], false);
    }

    #[test]
    fn io_errors_map_to_stable_codes_without_os_text() {
        let error = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "/home/x secret");
        let mapped = LauncherError::from_io(&error, "Saving settings");
        assert_eq!(mapped.code, ErrorCode::PermissionDenied);
        assert!(!mapped.message.contains("/home/x"));
        let full = std::io::Error::from(std::io::ErrorKind::StorageFull);
        assert_eq!(LauncherError::from_io(&full, "x").code, ErrorCode::DiskFull);
    }
}
