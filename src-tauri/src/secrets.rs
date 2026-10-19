//! Secret-bearing types
//! (docs/spec/03-AUTHENTICATION.md "Secret storage", doc 07 "Diagnostics and privacy").
//!
//! Every token, authorization code, PKCE verifier or callback URL handled by the
//! launcher is wrapped in [`SecretString`]. It cannot be serialized, its `Debug`
//! output is redacted, and its memory is zeroed on drop. Text that leaves the Rust
//! core is cleaned where it leaves: game output by `launch::redact`, exports and
//! reports by `diagnostics::sanitize`.

use std::fmt;

use zeroize::Zeroizing;

/// Replacement text for removed secrets.
pub const REDACTED: &str = "<redacted>";

/// An owned secret string. Deliberately implements neither `Serialize`, `Display`
/// nor `Clone`: copying or exposing it requires an explicit call to [`expose`].
///
/// [`expose`]: SecretString::expose
pub struct SecretString(Zeroizing<String>);

impl SecretString {
    pub fn new(value: impl Into<String>) -> Self {
        Self(Zeroizing::new(value.into()))
    }

    /// Returns the secret value. Call sites must not log or persist the result.
    pub fn expose(&self) -> &str {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Debug for SecretString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SecretString({REDACTED})")
    }
}

impl PartialEq for SecretString {
    fn eq(&self, other: &Self) -> bool {
        self.expose() == other.expose()
    }
}

impl Eq for SecretString {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_debug_never_shows_value() {
        let secret = SecretString::new("super-secret-token-value");
        assert_eq!(format!("{secret:?}"), "SecretString(<redacted>)");
        assert_eq!(secret.expose(), "super-secret-token-value");
    }
}
