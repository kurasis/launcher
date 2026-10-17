//! Java launcher argument files (`java @file`) for long, non-secret JVM
//! arguments (docs/spec/05 "Command length and secret-bearing arguments").
//!
//! Every argument is written on its own line inside double quotes; within quotes
//! the Java launcher treats `\` as an escape, so `\` and `"` are escaped. Quoting
//! also neutralizes `#` comment markers and whitespace. Argument files are read in
//! the platform's native encoding; on Windows that is the ANSI code page, so
//! non-ASCII content is refused there with `UNSUPPORTED_PATH_ENCODING` instead of
//! being silently corrupted. Tokens are never written to an argument file.

use std::path::Path;
use std::process::Command;

use super::compile::CompiledLaunch;
use crate::error::{ErrorCode, LauncherError, LauncherResult};

/// Windows `CreateProcess` accepts at most 32,767 UTF-16 units; leave headroom.
pub const MAX_DIRECT_COMMAND_CHARS: usize = 30_000;

pub fn quote(arg: &str) -> String {
    let mut out = String::with_capacity(arg.len() + 2);
    out.push('"');
    for c in arg.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

/// Encodes arguments for an argfile. `ascii_only` is required where the native
/// encoding cannot be relied on (Windows).
pub fn encode(args: &[String], ascii_only: bool) -> LauncherResult<Vec<u8>> {
    let mut text = String::new();
    for arg in args {
        if ascii_only && !arg.is_ascii() {
            return Err(LauncherError::new(
                ErrorCode::UnsupportedPathEncoding,
                "A game path contains characters that Java cannot read from an argument file \
                 on this system. Move the launcher data to a folder with a plain-ASCII path.",
            ));
        }
        if arg.contains('\0') {
            return Err(LauncherError::new(
                ErrorCode::UnsupportedPathEncoding,
                "A launch argument is invalid.",
            ));
        }
        text.push_str(&quote(arg));
        text.push('\n');
    }
    Ok(text.into_bytes())
}

fn direct_length(java: &Path, launch: &CompiledLaunch) -> usize {
    let java_len = java.as_os_str().len() + 3;
    java_len
        + launch
            .jvm_args
            .iter()
            .chain(std::iter::once(&launch.main_class))
            .chain(launch.game_args.iter())
            .map(|a| a.chars().count() + 3)
            .sum::<usize>()
}

/// Builds the process command. JVM arguments go to `argfile` when forced or when
/// the direct command would be too long for Windows; the main class and game
/// arguments (including the token) are always direct.
pub fn build_command(
    java: &Path,
    launch: &CompiledLaunch,
    argfile: &Path,
    force_argfile: bool,
) -> LauncherResult<(Command, bool)> {
    let mut command = crate::runtime::probe::sanitized_command(java);
    command.current_dir(&launch.working_dir);
    let use_argfile =
        force_argfile || (cfg!(windows) && direct_length(java, launch) > MAX_DIRECT_COMMAND_CHARS);
    if use_argfile {
        let bytes = encode(&launch.jvm_args, cfg!(windows))?;
        write_private(argfile, &bytes)?;
        let path = argfile.to_str().ok_or_else(|| {
            LauncherError::new(
                ErrorCode::UnsupportedPathEncoding,
                "The run folder path is invalid.",
            )
        })?;
        command.arg(format!("@{path}"));
    } else {
        command.args(&launch.jvm_args);
    }
    command.arg(&launch.main_class).args(&launch.game_args);
    Ok((command, use_argfile))
}

fn write_private(path: &Path, bytes: &[u8]) -> LauncherResult<()> {
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .map_err(|e| LauncherError::from_io(&e, "Preparing the launch"))?;
    file.write_all(bytes)
        .map_err(|e| LauncherError::from_io(&e, "Preparing the launch"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quoting_escapes_what_the_java_launcher_interprets() {
        assert_eq!(quote(r"C:\Users\A B\x.jar"), r#""C:\\Users\\A B\\x.jar""#);
        assert_eq!(quote(r#"say "hi""#), r#""say \"hi\"""#);
        assert_eq!(quote("#not-a-comment"), "\"#not-a-comment\"");
        assert_eq!(quote("@literal"), "\"@literal\"");
        assert_eq!(quote("a\tb"), "\"a\\tb\"");
    }

    #[test]
    fn non_ascii_is_refused_only_where_required() {
        let args = vec!["-Dp=/home/Jürgen/игры".to_owned()];
        assert_eq!(
            encode(&args, true).unwrap_err().code,
            ErrorCode::UnsupportedPathEncoding
        );
        let bytes = encode(&args, false).unwrap();
        assert_eq!(
            String::from_utf8(bytes).unwrap(),
            "\"-Dp=/home/Jürgen/игры\"\n"
        );
        assert!(encode(&["a\0b".to_owned()], false).is_err());
    }
}
