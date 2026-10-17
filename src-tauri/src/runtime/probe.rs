//! Runs the managed Java once to confirm its version and architecture
//! (`-XshowSettings:properties -version`), with the sanitized child environment
//! from docs/spec/05 and bounded output capture.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::error::{ErrorCode, LauncherError, LauncherResult};
use crate::storage::paths::{TargetArch, TargetOs};

/// Variables that could inject JVM flags, agents or classpath entries.
pub const FILTERED_ENV: &[&str] = &[
    "JDK_JAVA_OPTIONS",
    "JAVA_TOOL_OPTIONS",
    "_JAVA_OPTIONS",
    "CLASSPATH",
];
pub const MAX_CAPTURE: usize = 64 * 1024;
pub const PROBE_TIMEOUT: Duration = Duration::from_secs(30);

/// The Java executable inside an extracted Temurin package.
pub fn java_relative_path(os: TargetOs) -> &'static str {
    match os {
        TargetOs::Windows => "bin/java.exe",
        TargetOs::Mac => "Contents/Home/bin/java",
        TargetOs::Linux => "bin/java",
    }
}

pub fn java_path(runtime_root: &Path, os: TargetOs) -> PathBuf {
    let mut path = runtime_root.to_path_buf();
    path.extend(java_relative_path(os).split('/'));
    path
}

/// A command with an absolute program, no shell, filtered Java variables and no
/// console window on Windows. The rest of the environment is kept.
pub fn sanitized_command(program: &Path) -> Command {
    let mut command = Command::new(program);
    for name in FILTERED_ENV {
        command.env_remove(name);
    }
    command.stdin(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JavaProbe {
    pub java_version: String,
    pub specification_version: u16,
    pub os_arch: String,
    pub vendor: Option<String>,
}

fn property<'a>(output: &'a str, name: &str) -> Option<&'a str> {
    output.lines().find_map(|line| {
        let (key, value) = line.trim().split_once(" = ")?;
        (key == name).then_some(value.trim())
    })
}

/// Parses `-XshowSettings:properties` output.
pub fn parse(output: &str) -> Option<JavaProbe> {
    let specification_version = property(output, "java.specification.version")?
        .parse()
        .ok()?;
    Some(JavaProbe {
        java_version: property(output, "java.version")?.to_owned(),
        specification_version,
        os_arch: property(output, "os.arch")?.to_owned(),
        vendor: property(output, "java.vendor").map(str::to_owned),
    })
}

fn arch_matches(reported: &str, arch: TargetArch) -> bool {
    match arch {
        TargetArch::X64 => matches!(reported, "amd64" | "x86_64"),
        TargetArch::Aarch64 => matches!(reported, "aarch64" | "arm64"),
    }
}

/// Rejects a runtime that is not the required major or architecture.
pub fn check(probe: &JavaProbe, major: u16, arch: TargetArch) -> LauncherResult<()> {
    if probe.specification_version != major || !arch_matches(&probe.os_arch, arch) {
        return Err(LauncherError::new(
            ErrorCode::RuntimeMismatch,
            "The installed Java runtime is not the version or architecture this game needs.",
        )
        .with_detail("expectedMajor", i64::from(major))
        .with_detail("reportedMajor", i64::from(probe.specification_version))
        .with_detail("expectedArch", arch.as_str())
        .with_detail("reportedArch", probe.os_arch.clone()));
    }
    Ok(())
}

fn capture(mut stream: impl Read + Send + 'static) -> std::thread::JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut kept = Vec::new();
        let mut buffer = [0u8; 8192];
        // Keeps draining after the cap so the child never blocks on a full pipe.
        while let Ok(n) = stream.read(&mut buffer) {
            if n == 0 {
                break;
            }
            let room = MAX_CAPTURE.saturating_sub(kept.len());
            kept.extend_from_slice(&buffer[..n.min(room)]);
        }
        kept
    })
}

fn probe_failed(reason: &str) -> LauncherError {
    LauncherError::new(
        ErrorCode::RuntimeMismatch,
        "The Java runtime could not be started for verification.",
    )
    .with_detail("reason", reason.to_owned())
}

/// Blocking: runs `java <extra> -XshowSettings:properties -version` with the
/// sanitized environment and returns its raw, bounded output (stderr first).
pub fn settings_output(
    java: &Path,
    extra: &[String],
    timeout: Duration,
) -> LauncherResult<Vec<u8>> {
    let mut child = sanitized_command(java)
        .args(extra)
        .args(["-XshowSettings:properties", "-version"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| probe_failed("spawn"))?;
    let stdout = capture(child.stdout.take().ok_or_else(|| probe_failed("pipe"))?);
    let stderr = capture(child.stderr.take().ok_or_else(|| probe_failed("pipe"))?);
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() > timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(probe_failed("timeout"));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(25)),
            Err(_) => return Err(probe_failed("wait")),
        }
    };
    let mut output = stderr.join().unwrap_or_default();
    output.extend(stdout.join().unwrap_or_default());
    if !status.success() {
        return Err(probe_failed("exit status"));
    }
    Ok(output)
}

/// Blocking: runs Java and parses its settings. Kills it after `timeout`.
pub fn run(java: &Path, timeout: Duration) -> LauncherResult<JavaProbe> {
    let output = settings_output(java, &[], timeout)?;
    parse(&String::from_utf8_lossy(&output)).ok_or_else(|| probe_failed("unrecognized output"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "Property settings:\n    file.encoding = UTF-8\n    java.specification.version = 21\n    java.vendor = Eclipse Adoptium\n    java.version = 21.0.12\n    os.arch = amd64\n\nopenjdk version \"21.0.12\" 2026-07-21 LTS\n";

    #[test]
    fn parses_and_checks_properties() {
        let probe = parse(SAMPLE).unwrap();
        assert_eq!(probe.java_version, "21.0.12");
        assert_eq!(probe.vendor.as_deref(), Some("Eclipse Adoptium"));
        assert!(check(&probe, 21, TargetArch::X64).is_ok());
        assert_eq!(
            check(&probe, 25, TargetArch::X64).unwrap_err().code,
            ErrorCode::RuntimeMismatch
        );
        assert_eq!(
            check(&probe, 21, TargetArch::Aarch64).unwrap_err().code,
            ErrorCode::RuntimeMismatch
        );
        assert!(parse("garbage").is_none());
        let arm = SAMPLE.replace("amd64", "aarch64");
        assert!(check(&parse(&arm).unwrap(), 21, TargetArch::Aarch64).is_ok());
    }

    #[test]
    fn java_locations_per_os() {
        assert_eq!(java_relative_path(TargetOs::Mac), "Contents/Home/bin/java");
        assert!(java_path(Path::new("r"), TargetOs::Windows).ends_with("bin/java.exe"));
    }

    #[test]
    fn java_variables_are_removed_from_the_child_environment() {
        let command = sanitized_command(Path::new("java"));
        let removed: Vec<String> = command
            .get_envs()
            .filter(|(_, value)| value.is_none())
            .map(|(key, _)| key.to_string_lossy().into_owned())
            .collect();
        for name in FILTERED_ENV {
            assert!(removed.iter().any(|r| r == name), "{name}");
        }
        assert_eq!(
            command.get_envs().count(),
            FILTERED_ENV.len(),
            "nothing else changed"
        );
    }

    #[cfg(unix)]
    #[test]
    fn output_capture_is_bounded() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("java");
        std::fs::write(
            &script,
            "#!/bin/sh\nhead -c 200000 /dev/zero | tr '\\0' 'x' >&2\n\
             printf '\\n    java.specification.version = 25\\n    java.version = 25.0.4\\n    os.arch = x86_64\\n' >&2\n",
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        // The process finishes (its pipe is drained) but output past the cap is
        // dropped, so the properties are not seen.
        let error = run(&script, Duration::from_secs(10)).unwrap_err();
        assert_eq!(
            error.safe_details.unwrap()["reason"],
            crate::error::SafeValue::Text("unrecognized output".into())
        );
    }

    #[cfg(unix)]
    #[test]
    fn hung_probe_is_killed() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("java");
        std::fs::write(&script, "#!/bin/sh\nexec sleep 30\n").unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        let started = Instant::now();
        let error = run(&script, Duration::from_millis(300)).unwrap_err();
        assert!(started.elapsed() < Duration::from_secs(5));
        assert_eq!(error.code, ErrorCode::RuntimeMismatch);
    }
}
