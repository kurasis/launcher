//! Can the managed Java receive these arguments intact? (docs/spec/05 "Command
//! length and secret-bearing arguments", A16.)
//!
//! The Java launcher decodes its arguments with the platform's native encoding:
//! the ANSI code page on Windows, the locale's character set elsewhere (under a
//! plain POSIX locale every non-ASCII character becomes `?`). When an argument
//! contains non-ASCII text, preflight passes the same text to the managed Java
//! once as a system property and reads it back. Characters Java could not decode
//! are printed as `?`; more `?` bytes than the original contains means the path
//! would be corrupted, so the launch stops with `UNSUPPORTED_PATH_ENCODING`
//! instead. The output code page does not matter: a character Java decoded
//! correctly is printed in the same code page and never as `?`.

use std::path::Path;
use std::time::Duration;

use crate::error::{ErrorCode, LauncherError, LauncherResult};
use crate::runtime::probe;

const PROPERTY: &str = "launcher.argument.check";

/// The value of `PROPERTY` in raw `-XshowSettings` output bytes.
fn echoed(output: &[u8]) -> Option<&[u8]> {
    let needle = format!("{PROPERTY} = ");
    let start = output
        .windows(needle.len())
        .position(|w| w == needle.as_bytes())?
        + needle.len();
    let rest = &output[start..];
    let end = rest
        .iter()
        .position(|b| *b == b'\n' || *b == b'\r')
        .unwrap_or(rest.len());
    Some(&rest[..end])
}

/// True when every character of `sent` survived the round trip.
pub fn intact(sent: &str, output: &[u8]) -> bool {
    echoed(output).is_some_and(|value| {
        let question = |bytes: &[u8]| bytes.iter().filter(|b| **b == b'?').count();
        question(value) == question(sent.as_bytes())
    })
}

pub fn unsupported() -> LauncherError {
    LauncherError::new(
        ErrorCode::UnsupportedPathEncoding,
        "A game path contains characters that Java cannot receive with this system's \
         language settings. Use a UTF-8 system locale, or move the launcher data to a folder \
         whose path uses only plain ASCII characters.",
    )
}

/// Blocking: checks all non-ASCII arguments against the managed Java.
pub fn check(java: &Path, args: &[&str]) -> LauncherResult<()> {
    let mut seen = Vec::new();
    for arg in args.iter().filter(|a| !a.is_ascii()) {
        if !seen.contains(arg) {
            seen.push(*arg);
        }
    }
    if seen.is_empty() {
        return Ok(());
    }
    let joined = seen.join("|");
    let output = probe::settings_output(
        java,
        &[format!("-D{PROPERTY}={joined}")],
        Duration::from_secs(30),
    )?;
    if intact(&joined, &output) {
        Ok(())
    } else {
        Err(unsupported())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn substituted_characters_are_detected() {
        let ok = b"    launcher.argument.check = /home/J\xc3\xbcrgen?x\n    java.version = 21\n";
        assert!(intact("/home/Jürgen?x", ok));
        // Windows ANSI output of a correctly decoded value: not UTF-8, still intact.
        assert!(intact(
            "C:\\Users\\Jürgen",
            b"    launcher.argument.check = C:\\Users\\J\xfcrgen\r\n"
        ));
        let lossy = b"    launcher.argument.check = /home/J??rgen?x\n";
        assert!(!intact("/home/Jürgen?x", lossy));
        assert!(!intact("x", b"no property here"));
    }
}
