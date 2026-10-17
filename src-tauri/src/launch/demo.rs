//! Minecraft demo mode without an account (D-033).
//!
//! Minecraft's demo is the free, time-limited edition (five in-game days, no
//! multiplayer). "Play demo" launches it under a local player name with Minecraft's
//! `--demo` argument, so it works before or without a Microsoft sign-in:
//!
//! - the game always runs with `--demo`; the profile has no token the Minecraft
//!   services would accept, so online play and server joins fail as they should;
//! - full play still goes only through the signed-in account and its fresh access
//!   check; nothing here unlocks it.
//!
//! Debug builds started with `LAUNCHER_NOTOKEN_PROFILE=<name>` use the same profile
//! for every launch, so automated download and launch tests need no account (D-030).
//! Evidence gathered that way is labelled "test profile, demo" and does not satisfy
//! A03–A06 or A13.

use sha1::{Digest, Sha1};

use super::compile::LaunchIdentity;
use super::service::Authorizer;
use crate::error::{LauncherError, LauncherResult};
use crate::install::service::BoxFuture;
use crate::secrets::SecretString;

/// Environment variable naming the debug-build test profile.
#[cfg(debug_assertions)]
pub const ENV: &str = "LAUNCHER_NOTOKEN_PROFILE";

/// Player name of demo launches.
pub const DEFAULT_NAME: &str = "Player";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DemoProfile {
    name: String,
}

impl Default for DemoProfile {
    fn default() -> Self {
        Self {
            name: DEFAULT_NAME.to_owned(),
        }
    }
}

impl DemoProfile {
    /// Accepts Minecraft player names: 3–16 ASCII letters, digits or `_`.
    pub fn new(name: &str) -> LauncherResult<Self> {
        let valid = (3..=16).contains(&name.len())
            && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_');
        if !valid {
            return Err(LauncherError::invalid_input(
                "A demo player name has 3 to 16 letters, digits or underscores.",
            ));
        }
        Ok(Self {
            name: name.to_owned(),
        })
    }

    /// The debug-build test profile named by `LAUNCHER_NOTOKEN_PROFILE`, if set and valid.
    #[cfg(debug_assertions)]
    pub fn from_env() -> Option<Self> {
        let name = std::env::var(ENV).ok()?;
        Self::new(name.trim()).ok()
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// Stable 32-hex UUID derived from the name (version 5 layout over SHA-1),
    /// so worlds keep the same player data between runs.
    pub fn uuid(&self) -> String {
        let digest = Sha1::digest(format!("NoTokenProfile:{}", self.name).as_bytes());
        let mut bytes = [0u8; 16];
        bytes.copy_from_slice(&digest[..16]);
        bytes[6] = (bytes[6] & 0x0f) | 0x50;
        bytes[8] = (bytes[8] & 0x3f) | 0x80;
        hex::encode(bytes)
    }
}

impl Authorizer for DemoProfile {
    fn authorize(&self) -> BoxFuture<'_, LauncherResult<LaunchIdentity>> {
        Box::pin(async move {
            Ok(LaunchIdentity {
                player_name: self.name.clone(),
                uuid: self.uuid(),
                // Never a real credential; the game only needs a non-empty value.
                access_token: SecretString::new("notoken-profile"),
                xuid: None,
                client_id: String::new(),
                demo: true,
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_follow_minecraft_rules() {
        for ok in ["Abc", "TestPlayer_01", "abcdefghijklmnop", DEFAULT_NAME] {
            assert!(DemoProfile::new(ok).is_ok(), "{ok}");
        }
        for bad in ["", "ab", "abcdefghijklmnopq", "has space", "Jürgen", "a-b"] {
            assert!(DemoProfile::new(bad).is_err(), "{bad}");
        }
    }

    #[tokio::test]
    async fn the_identity_is_demo_only_and_stable() {
        let profile = DemoProfile::new("TestPlayer").unwrap();
        let identity = profile.authorize().await.unwrap();
        assert!(identity.demo);
        assert_eq!(identity.player_name, "TestPlayer");
        assert_eq!(identity.uuid, profile.uuid());
        assert_eq!(identity.uuid.len(), 32);
        assert_eq!(&identity.uuid[12..13], "5", "version nibble");
        assert_ne!(
            DemoProfile::new("Other").unwrap().uuid(),
            identity.uuid,
            "names map to different players"
        );
        assert_eq!(DemoProfile::default().name(), DEFAULT_NAME);
    }
}
