//! Player identity inside a world (ADAPTATION A2): the launcher account's nick
//! and the offline UUID a server in offline mode derives from it.

use uuid::Uuid;

use crate::error::{LauncherError, LauncherResult};

/// A Minecraft player name as the launcher account allows it: 3–16 ASCII
/// letters, digits or `_` (the account service's rule, which the game accepts).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Nick(String);

impl Nick {
    pub fn parse(text: &str) -> LauncherResult<Self> {
        let valid = (3..=16).contains(&text.len())
            && text.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_');
        if valid {
            Ok(Self(text.to_owned()))
        } else {
            Err(LauncherError::invalid_input(
                "A player name has 3 to 16 letters, digits or underscores.",
            ))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The UUID an offline-mode server gives this player:
    /// `UUID.nameUUIDFromBytes("OfflinePlayer:" + name)`, a version 3 UUID.
    pub fn offline_uuid(&self) -> Uuid {
        // Java's nameUUIDFromBytes hashes the bytes alone, with no namespace;
        // Uuid::new_v3 prepends one, so the MD5 is computed here directly.
        let digest = md5_of(format!("OfflinePlayer:{}", self.0).as_bytes());
        uuid::Builder::from_md5_bytes(digest).into_uuid()
    }
}

fn md5_of(bytes: &[u8]) -> [u8; 16] {
    use md5::Digest;
    md5::Md5::digest(bytes).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offline_uuids_match_the_server() {
        // Values a 1.21.1 server in offline mode writes to usercache.json.
        let notch = Nick::parse("Notch").unwrap().offline_uuid();
        assert_eq!(notch.to_string(), "b50ad385-829d-3141-a216-7e7d7539ba7f");
        assert_eq!(notch.get_version_num(), 3, "skins treat v3 as ours (D-065)");
        let other = Nick::parse("notch").unwrap().offline_uuid();
        assert_ne!(notch, other, "the name is hashed as written");
    }

    #[test]
    fn names_are_bounded_ascii() {
        for bad in [
            "ab",
            "seventeen_letters",
            "with space",
            "Ünï",
            "a-b",
            "",
            "x\0y",
        ] {
            assert!(Nick::parse(bad).is_err(), "{bad:?}");
        }
        assert!(Nick::parse("Player_16_chars_").is_ok());
    }
}
