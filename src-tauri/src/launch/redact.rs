//! Redaction of launcher-captured game output (docs/spec/05 "Process
//! supervision"). Lines are complete when they arrive, except when the capture
//! truncated an overlong line; a secret cut by that truncation would leave a
//! prefix, so a trailing prefix of a secret is redacted too.

use super::compile::REDACTED;
use crate::lifecycle::child::TRUNCATION_MARKER;

/// Shortest trailing secret prefix that is still redacted.
const MIN_PREFIX: usize = 4;

#[derive(Clone, Default)]
pub struct Redactor {
    secrets: Vec<String>,
}

impl Redactor {
    pub fn new(secrets: &[String]) -> Self {
        Self {
            secrets: secrets.iter().filter(|s| !s.is_empty()).cloned().collect(),
        }
    }

    pub fn redact(&self, line: &str) -> String {
        let (body, marker) = match line.strip_suffix(TRUNCATION_MARKER) {
            Some(body) => (body, TRUNCATION_MARKER),
            None => (line, ""),
        };
        let mut text = body.to_owned();
        for secret in &self.secrets {
            if text.contains(secret.as_str()) {
                text = text.replace(secret.as_str(), REDACTED);
            }
            if !marker.is_empty() {
                // The truncated line may end with the start of a secret.
                for len in (MIN_PREFIX..secret.len()).rev() {
                    let prefix = &secret[..len];
                    if text.ends_with(prefix) {
                        text.truncate(text.len() - len);
                        text.push_str(REDACTED);
                        break;
                    }
                }
            }
        }
        text.push_str(marker);
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secrets_are_replaced_everywhere() {
        let redactor = Redactor::new(&["eyJ.secret.token".to_owned(), String::new()]);
        assert_eq!(
            redactor.redact("--accessToken eyJ.secret.token and eyJ.secret.token"),
            "--accessToken <redacted> and <redacted>"
        );
        assert_eq!(redactor.redact("nothing here"), "nothing here");
    }

    #[test]
    fn a_secret_cut_by_truncation_does_not_leak_its_prefix() {
        let redactor = Redactor::new(&["eyJ.secret.token".to_owned()]);
        let line = format!("x eyJ.sec{TRUNCATION_MARKER}");
        assert_eq!(
            redactor.redact(&line),
            format!("x <redacted>{TRUNCATION_MARKER}")
        );
        // Without truncation an ordinary word that happens to match is kept.
        assert_eq!(redactor.redact("x eyJ.sec"), "x eyJ.sec");
    }
}
