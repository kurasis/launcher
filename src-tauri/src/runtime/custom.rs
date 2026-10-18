//! The user's own Java for an installation and extra JVM options (D-044).
//!
//! A chosen Java is started once with the sanitized environment to read its
//! version and architecture; the result is compared with the Java the game was
//! built for. Mismatches are warnings the user may accept, not errors: only a
//! Java that does not start at all is refused.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::probe;
use crate::error::{ErrorCode, LauncherError, LauncherResult};
use crate::storage::models::JavaIssue;
use crate::storage::paths::TargetArch;

/// How long a chosen Java may take to report its settings.
pub const CHECK_TIMEOUT: Duration = Duration::from_secs(15);
/// Longest JVM options text, in characters.
pub const MAX_JVM_ARGUMENTS_CHARS: usize = 4000;
/// Most JVM options.
pub const MAX_JVM_ARGUMENTS: usize = 64;

/// What a chosen Java reported, and how it suits the game.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JavaCheck {
    pub path: PathBuf,
    pub java_version: String,
    pub major: u16,
    pub vendor: Option<String>,
    pub arch: String,
    /// The Java the game was built for.
    pub required_major: u16,
    pub issues: Vec<JavaIssue>,
}

fn arch_matches(reported: &str, arch: TargetArch) -> bool {
    match arch {
        TargetArch::X64 => matches!(reported, "amd64" | "x86_64"),
        TargetArch::Aarch64 => matches!(reported, "aarch64" | "arm64"),
    }
}

/// Compatibility warnings for a probed Java.
pub fn issues(probe: &probe::JavaProbe, required_major: u16, arch: TargetArch) -> Vec<JavaIssue> {
    let mut issues = Vec::new();
    match probe.specification_version.cmp(&required_major) {
        std::cmp::Ordering::Less => issues.push(JavaIssue::Older),
        std::cmp::Ordering::Greater => issues.push(JavaIssue::Newer),
        std::cmp::Ordering::Equal => {}
    }
    if !arch_matches(&probe.os_arch, arch) {
        issues.push(JavaIssue::Architecture);
    }
    issues
}

/// Checks that `path` names a Java executable file by an absolute path.
pub fn validate_path(path: &Path) -> LauncherResult<()> {
    if !path.is_absolute() {
        return Err(LauncherError::invalid_input(
            "Enter the full path to the Java program.",
        ));
    }
    if !path.is_file() {
        return Err(LauncherError::new(
            ErrorCode::RuntimeUnavailable,
            "No Java program was found at this path.",
        ));
    }
    Ok(())
}

/// Blocking: starts the Java at `path` and compares it with what the game needs.
pub fn check(path: &Path, required_major: u16, arch: TargetArch) -> LauncherResult<JavaCheck> {
    validate_path(path)?;
    let probe = probe::run(path, CHECK_TIMEOUT).map_err(|e| {
        // Blocked by antivirus software or the system says more than "not Java".
        if e.code == ErrorCode::PermissionDenied {
            return e;
        }
        let mut error = LauncherError::new(
            ErrorCode::RuntimeUnavailable,
            "This program could not be started as Java.",
        );
        error.safe_details = e.safe_details;
        error
    })?;
    Ok(JavaCheck {
        path: path.to_path_buf(),
        issues: issues(&probe, required_major, arch),
        java_version: probe.java_version,
        major: probe.specification_version,
        vendor: probe.vendor,
        arch: probe.os_arch,
        required_major,
    })
}

/// Splits a line at whitespace; quotes ("..." or '...') keep spaces inside one
/// argument. `None` when a quote is not closed.
pub fn split_quoted(text: &str) -> Option<Vec<String>> {
    let mut args = Vec::new();
    let mut current: Option<String> = None;
    let mut quote: Option<char> = None;
    for c in text.chars() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), c) => current.get_or_insert_with(String::new).push(c),
            (None, '"' | '\'') => {
                quote = Some(c);
                current.get_or_insert_with(String::new);
            }
            (None, c) if c.is_whitespace() => {
                if let Some(arg) = current.take() {
                    args.push(arg);
                }
            }
            (None, c) => current.get_or_insert_with(String::new).push(c),
        }
    }
    if quote.is_some() {
        return None;
    }
    args.extend(current);
    Some(args)
}

/// Splits JVM options typed as one line. Quotes ("..." or '...') keep spaces
/// inside one option. Every option must start with `-`: a bare word would be
/// taken for the main class, so an option's value goes after `=` or `:`.
pub fn parse_jvm_arguments(text: &str) -> LauncherResult<Vec<String>> {
    if text.chars().count() > MAX_JVM_ARGUMENTS_CHARS {
        return Err(LauncherError::templated(
            ErrorCode::InvalidInput,
            "JVM options can be at most {max} characters long.",
            crate::error::params([("max", (MAX_JVM_ARGUMENTS_CHARS as u64).into())]),
        ));
    }
    if text.chars().any(|c| c.is_control() && c != '\t') {
        return Err(LauncherError::invalid_input(
            "JVM options must be on one line, without control characters.",
        ));
    }
    let Some(args) = split_quoted(text) else {
        return Err(LauncherError::invalid_input(
            "A quote in the JVM options is not closed.",
        ));
    };
    if args.len() > MAX_JVM_ARGUMENTS {
        return Err(LauncherError::templated(
            ErrorCode::InvalidInput,
            "Use at most {max} JVM options.",
            crate::error::params([("max", (MAX_JVM_ARGUMENTS as u64).into())]),
        ));
    }
    if let Some(bad) = args.iter().find(|a| !a.starts_with('-') || a.len() < 2) {
        return Err(LauncherError::templated(
            ErrorCode::InvalidInput,
            "Each JVM option must start with \"-\"; put a value after \"=\": {option}",
            crate::error::params([("option", bad.clone().into())]),
        ));
    }
    Ok(args)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn probe(major: u16, arch: &str) -> probe::JavaProbe {
        probe::JavaProbe {
            java_version: format!("{major}.0.1"),
            specification_version: major,
            os_arch: arch.into(),
            vendor: None,
        }
    }

    #[test]
    fn compatibility_is_reported_not_enforced() {
        assert!(issues(&probe(21, "amd64"), 21, TargetArch::X64).is_empty());
        assert_eq!(
            issues(&probe(17, "amd64"), 21, TargetArch::X64),
            [JavaIssue::Older]
        );
        assert_eq!(
            issues(&probe(25, "x86_64"), 21, TargetArch::X64),
            [JavaIssue::Newer]
        );
        assert_eq!(
            issues(&probe(8, "amd64"), 8, TargetArch::Aarch64),
            [JavaIssue::Architecture]
        );
    }

    #[test]
    fn jvm_options_split_on_spaces_and_keep_quoted_ones() {
        assert_eq!(parse_jvm_arguments("").unwrap(), Vec::<String>::new());
        assert_eq!(
            parse_jvm_arguments("  -XX:+UseG1GC\t-Dfoo=\"a b\" '-Dbar=c d' ").unwrap(),
            ["-XX:+UseG1GC", "-Dfoo=a b", "-Dbar=c d"]
        );
        assert_eq!(parse_jvm_arguments("-Dpath=Мир").unwrap(), ["-Dpath=Мир"]);
    }

    #[test]
    fn jvm_options_refuse_what_would_break_the_launch() {
        for bad in [
            "-cp lib.jar",
            "Main",
            "@args.txt",
            "-",
            "-Da=\"open",
            "-Da\n-Db",
        ] {
            assert_eq!(
                parse_jvm_arguments(bad).unwrap_err().code,
                ErrorCode::InvalidInput,
                "{bad}"
            );
        }
        let many = vec!["-Da"; MAX_JVM_ARGUMENTS + 1].join(" ");
        assert!(parse_jvm_arguments(&many).is_err());
        assert!(parse_jvm_arguments(&"-D".repeat(MAX_JVM_ARGUMENTS_CHARS)).is_err());
    }

    #[test]
    fn a_path_must_be_absolute_and_exist() {
        assert_eq!(
            validate_path(Path::new("java")).unwrap_err().code,
            ErrorCode::InvalidInput
        );
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            validate_path(&dir.path().join("java")).unwrap_err().code,
            ErrorCode::RuntimeUnavailable
        );
        assert!(validate_path(dir.path()).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn a_program_that_is_not_java_is_refused() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let fake = dir.path().join("java");
        std::fs::write(&fake, "#!/bin/sh\necho hello\n").unwrap();
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(
            check(&fake, 21, TargetArch::X64).unwrap_err().code,
            ErrorCode::RuntimeUnavailable
        );
        std::fs::write(
            &fake,
            "#!/bin/sh\ncat >&2 <<'X'\n    java.specification.version = 17\n    java.version = 17.0.9\n    os.arch = amd64\nX\n",
        )
        .unwrap();
        let checked = check(&fake, 21, TargetArch::X64).unwrap();
        assert_eq!(checked.major, 17);
        assert_eq!(checked.issues, [JavaIssue::Older]);
    }
}
