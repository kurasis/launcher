//! Extra Minecraft (game) arguments of an installation (D-056).
//!
//! They are added after the version's own arguments, such as
//! `--quickPlaySingleplayer "My World"` or `--server example.org`. Arguments the
//! launcher sets itself (the player's identity, folders, the version, window
//! size and fullscreen) are refused: a second copy would either fail the game's
//! option parser or let a typed value override the signed-in player.

use crate::error::{ErrorCode, LauncherError, LauncherResult};
use crate::runtime::custom::split_quoted;

pub const MAX_GAME_ARGUMENTS_CHARS: usize = 4000;
pub const MAX_GAME_ARGUMENTS: usize = 64;

/// Options the launcher passes itself; window size and fullscreen have their
/// own settings.
const RESERVED: &[&str] = &[
    "--username",
    "--uuid",
    "--accessToken",
    "--clientId",
    "--xuid",
    "--userType",
    "--userProperties",
    "--profileProperties",
    "--session",
    "--version",
    "--versionType",
    "--gameDir",
    "--assetsDir",
    "--assetIndex",
    "--demo",
    "--width",
    "--height",
    "--fullscreen",
    "--launchTarget",
];

/// Splits the arguments typed as one line; quotes keep spaces inside one.
pub fn parse_game_arguments(text: &str) -> LauncherResult<Vec<String>> {
    if text.chars().count() > MAX_GAME_ARGUMENTS_CHARS {
        return Err(LauncherError::templated(
            ErrorCode::InvalidInput,
            "Minecraft arguments can be at most {max} characters long.",
            crate::error::params([("max", (MAX_GAME_ARGUMENTS_CHARS as u64).into())]),
        ));
    }
    if text.chars().any(|c| c.is_control() && c != '\t') {
        return Err(LauncherError::invalid_input(
            "Minecraft arguments must be on one line, without control characters.",
        ));
    }
    let Some(args) = split_quoted(text) else {
        return Err(LauncherError::invalid_input(
            "A quote in the Minecraft arguments is not closed.",
        ));
    };
    if args.len() > MAX_GAME_ARGUMENTS {
        return Err(LauncherError::templated(
            ErrorCode::InvalidInput,
            "Use at most {max} Minecraft arguments.",
            crate::error::params([("max", (MAX_GAME_ARGUMENTS as u64).into())]),
        ));
    }
    if let Some(taken) = args.iter().find(|arg| {
        let name = arg.split('=').next().unwrap_or(arg);
        RESERVED.contains(&name) || name.starts_with("--fml.")
    }) {
        return Err(LauncherError::templated(
            ErrorCode::InvalidInput,
            "The launcher sets {option} itself; window size and fullscreen are \
             separate settings.",
            crate::error::params([("option", taken.split('=').next().unwrap_or(taken).into())]),
        ));
    }
    Ok(args)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_quoted_values_and_refuses_what_the_launcher_sets() {
        assert_eq!(parse_game_arguments("  ").unwrap(), Vec::<String>::new());
        assert_eq!(
            parse_game_arguments(r#"--quickPlaySingleplayer "My World" --server example.org"#)
                .unwrap(),
            [
                "--quickPlaySingleplayer",
                "My World",
                "--server",
                "example.org"
            ]
        );
        for taken in [
            "--username Notch",
            "--accessToken=x",
            "--gameDir /tmp",
            "--width 800",
            "--fullscreen",
            "--fml.forgeVersion 1",
            "--demo",
        ] {
            let error = parse_game_arguments(taken).unwrap_err();
            assert_eq!(error.code, ErrorCode::InvalidInput, "{taken}");
        }
        assert!(parse_game_arguments("--server \"open").is_err());
        assert!(parse_game_arguments("a\nb").is_err());
        assert!(parse_game_arguments(&"x ".repeat(65)).is_err());
        assert!(parse_game_arguments(&"x".repeat(4001)).is_err());
    }
}
