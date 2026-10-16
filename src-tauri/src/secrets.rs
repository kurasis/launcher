//! Secret-bearing types and the central redaction interface
//! (docs/spec/03-AUTHENTICATION.md "Secret storage", doc 07 "Diagnostics and privacy").
//!
//! Every token, authorization code, PKCE verifier or callback URL handled by the
//! launcher is wrapped in [`SecretString`]. It cannot be serialized, its `Debug`
//! output is redacted, and its memory is zeroed on drop. Text that leaves the Rust
//! core for logs or diagnostics goes through [`Redactor`] first.

use std::borrow::Cow;
use std::fmt;
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;
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

/// Patterns for credentials that may appear in text even when the exact value is
/// not registered with the redactor (for example in upstream error strings).
static PATTERNS: LazyLock<Vec<(Regex, &'static str)>> = LazyLock::new(|| {
    [
        // Authorization headers and similar.
        (r"(?i)\b(bearer)\s+[A-Za-z0-9._~+/=-]+", "$1 <redacted>"),
        // Xbox identity strings: XBL3.0 x=<uhs>;<token>
        (r"(?i)XBL3\.0\s+x=[^\s;]+;[^\s\x22']+", "XBL3.0 x=<redacted>"),
        // JWT-shaped values (header.payload.signature, base64url).
        (
            r"\beyJ[A-Za-z0-9_-]{6,}\.[A-Za-z0-9_-]{6,}\.[A-Za-z0-9_-]+",
            "<redacted-jwt>",
        ),
        // OAuth parameters in URLs, forms and JSON.
        (
            r#"(?i)(["']?\b(?:access_token|refresh_token|id_token|code|code_verifier|client_secret|state|accessToken|refreshToken|xtoken|Token|RpsTicket)\b["']?\s*[=:]\s*["']?)[^&\s"',}]+"#,
            "${1}<redacted>",
        ),
        // Microsoft compact tickets (d=<token>, t=<token>).
        (r"\b([dt])=EwA[A-Za-z0-9!*%._~+/=-]+", "$1=<redacted>"),
    ]
    .into_iter()
    .map(|(pattern, replacement)| {
        (
            Regex::new(pattern).expect("static redaction pattern"),
            replacement,
        )
    })
    .collect()
});

/// Removes registered secret values and known credential patterns from text.
#[derive(Default)]
pub struct Redactor {
    secrets: Vec<Zeroizing<String>>,
    home: Option<String>,
}

impl fmt::Debug for Redactor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Redactor")
            .field("secrets", &self.secrets.len())
            .finish()
    }
}

/// Values shorter than this are not registered: replacing very short strings would
/// mangle ordinary text, and real tokens are much longer.
const MIN_SECRET_LEN: usize = 8;

impl Redactor {
    pub fn new() -> Self {
        Self::default()
    }

    /// Also replace the user's home directory with `~` (doc 07: redact obvious
    /// user-directory identifiers).
    pub fn with_home_dir(mut self, home: &Path) -> Self {
        let text = home
            .to_string_lossy()
            .trim_end_matches(['/', '\\'])
            .to_owned();
        if text.len() > 1 {
            self.home = Some(text);
        }
        self
    }

    /// Registers an exact secret value to remove wherever it appears.
    pub fn register(&mut self, secret: &SecretString) {
        if secret.expose().len() >= MIN_SECRET_LEN
            && !self.secrets.iter().any(|s| s.as_str() == secret.expose())
        {
            self.secrets
                .push(Zeroizing::new(secret.expose().to_owned()));
            // Longest first so a secret that contains another is removed whole.
            self.secrets.sort_by_key(|s| std::cmp::Reverse(s.len()));
        }
    }

    /// The longest registered secret, which bounds how much text a streaming
    /// caller must hold back to catch a secret split across chunks.
    pub fn longest_secret_len(&self) -> usize {
        self.secrets.first().map_or(0, |s| s.len())
    }

    pub fn redact<'a>(&self, text: &'a str) -> Cow<'a, str> {
        let mut out = Cow::Borrowed(text);
        for secret in &self.secrets {
            if out.contains(secret.as_str()) {
                out = Cow::Owned(out.replace(secret.as_str(), REDACTED));
            }
        }
        for (pattern, replacement) in PATTERNS.iter() {
            if pattern.is_match(&out) {
                out = Cow::Owned(pattern.replace_all(&out, *replacement).into_owned());
            }
        }
        if let Some(home) = &self.home
            && out.contains(home.as_str())
        {
            out = Cow::Owned(out.replace(home.as_str(), "~"));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_debug_never_shows_value() {
        let secret = SecretString::new("super-secret-token-value");
        assert_eq!(format!("{secret:?}"), "SecretString(<redacted>)");
        assert_eq!(secret.expose(), "super-secret-token-value");
    }

    #[test]
    fn registered_secrets_are_removed() {
        let mut redactor = Redactor::new();
        let token = SecretString::new("abcdefgh12345678");
        redactor.register(&token);
        assert_eq!(
            redactor.redact("token abcdefgh12345678 used twice abcdefgh12345678"),
            "token <redacted> used twice <redacted>"
        );
        assert_eq!(redactor.longest_secret_len(), 16);
    }

    #[test]
    fn short_values_are_not_registered() {
        let mut redactor = Redactor::new();
        redactor.register(&SecretString::new("abc"));
        assert_eq!(redactor.redact("abc"), "abc");
    }

    #[test]
    fn longer_secret_wins_over_contained_secret() {
        let mut redactor = Redactor::new();
        redactor.register(&SecretString::new("12345678"));
        redactor.register(&SecretString::new("12345678-extended"));
        assert_eq!(redactor.redact("x 12345678-extended y"), "x <redacted> y");
    }

    #[test]
    fn known_credential_patterns_are_removed() {
        let redactor = Redactor::new();
        let cases = [
            (
                "Authorization: Bearer abc.def-ghi",
                "Authorization: Bearer <redacted>",
            ),
            (
                "https://localhost:1234/cb?code=M.C123_BAY.2.abc&state=xyz",
                "https://localhost:1234/cb?code=<redacted>&state=<redacted>",
            ),
            (
                r#"{"refresh_token":"M.R3_BAY.abc"}"#,
                r#"{"refresh_token":"<redacted>"}"#,
            ),
            (
                r#"{"accessToken": "eyJabc"}"#,
                r#"{"accessToken": "<redacted>"}"#,
            ),
            ("XBL3.0 x=123456;eyJtoken", "XBL3.0 x=<redacted>"),
            ("RpsTicket=d=EwAAbcdef", "RpsTicket=<redacted>"),
            ("ticket t=EwB0x/abc", "ticket t=EwB0x/abc"),
            ("t=EwA0x/abc", "t=<redacted>"),
            (
                "jwt eyJhbGciOi.eyJzdWIiOiIx.c2lnbmF0dXJl end",
                "jwt <redacted-jwt> end",
            ),
        ];
        for (input, expected) in cases {
            assert_eq!(redactor.redact(input), expected, "{input}");
        }
    }

    #[test]
    fn ordinary_text_is_untouched() {
        let redactor = Redactor::new();
        let text = "Downloaded 12 files; codec=vorbis state of the art";
        assert!(matches!(redactor.redact(text), Cow::Borrowed(_)));
    }

    #[test]
    fn home_directory_is_shortened() {
        let redactor = Redactor::new().with_home_dir(Path::new("/home/alice/"));
        assert_eq!(
            redactor.redact("log at /home/alice/.local/share/x"),
            "log at ~/.local/share/x"
        );
    }
}
