//! Who played a hosted world under which nick (plan W7). The server knows a
//! player only by the offline UUID of their nick, so a member who changes
//! their nick would arrive as a stranger with an empty inventory. The host
//! remembers each member account's last nick in `worlds/<id>/players.json`
//! and, when an account is admitted under a new nick, moves its player files
//! to the new UUID first. The member cannot be online then: an account holds
//! one place in a room at a time.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::identity::Nick;
use crate::error::{LauncherError, LauncherResult};
use crate::storage::atomic;
use crate::storage::paths::{DataRoot, WorldId};

/// Accounts remembered per world; the oldest are forgotten past this.
pub const MAX_PLAYERS: usize = 1000;

/// The files a server keeps per player, by directory and suffix.
const PLAYER_FILES: [(&str, &str); 4] = [
    ("playerdata", ".dat"),
    ("playerdata", ".dat_old"),
    ("advancements", ".json"),
    ("stats", ".json"),
];

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Players {
    /// Account ID to its nick and the Unix time it was last admitted.
    accounts: BTreeMap<String, (String, i64)>,
}

fn file(root: &DataRoot, world: WorldId) -> std::path::PathBuf {
    root.world_dir(world).join("players.json")
}

fn io(error: &std::io::Error) -> LauncherError {
    LauncherError::from_io(error, "Keeping the world's players")
}

fn load(root: &DataRoot, world: WorldId) -> Players {
    std::fs::read(file(root, world))
        .ok()
        .filter(|bytes| bytes.len() <= 1024 * 1024)
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

/// Records that `account` is admitted as `nick`; when it played under another
/// nick before, its player files move to the new nick first. Returns that
/// earlier nick.
pub fn admitted(
    root: &DataRoot,
    world: WorldId,
    account: &str,
    nick: &Nick,
    now: i64,
) -> LauncherResult<Option<Nick>> {
    let valid_account = !account.is_empty()
        && account.len() <= 64
        && account
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-');
    if !valid_account {
        return Err(LauncherError::invalid_input("This is not an account."));
    }
    let mut players = load(root, world);
    let before = players
        .accounts
        .get(account)
        .and_then(|(name, _)| Nick::parse(name).ok())
        .filter(|before| before != nick);
    if let Some(before) = &before {
        migrate(
            &root.world_server_dir(world).join("world"),
            before,
            nick,
            now,
        )?;
    }
    players
        .accounts
        .insert(account.to_owned(), (nick.as_str().to_owned(), now));
    while players.accounts.len() > MAX_PLAYERS {
        let oldest = players
            .accounts
            .iter()
            .min_by_key(|(_, (_, at))| *at)
            .map(|(account, _)| account.clone());
        match oldest {
            Some(oldest) => players.accounts.remove(&oldest),
            None => break,
        };
    }
    let json = serde_json::to_vec(&players)
        .map_err(|_| LauncherError::internal("The world's players could not be written."))?;
    atomic::write(&file(root, world), &json).map_err(|e| io(&e))?;
    Ok(before)
}

/// Moves the player files of `from` to `to` in the save directory `save`. A
/// file already there for `to` (someone who had that nick before) is kept
/// aside, never overwritten.
pub fn migrate(save: &Path, from: &Nick, to: &Nick, now: i64) -> LauncherResult<()> {
    let (from, to) = (
        from.offline_uuid().hyphenated().to_string(),
        to.offline_uuid().hyphenated().to_string(),
    );
    for (dir, suffix) in PLAYER_FILES {
        let dir = save.join(dir);
        let source = dir.join(format!("{from}{suffix}"));
        if !source.is_file() {
            continue;
        }
        let target = dir.join(format!("{to}{suffix}"));
        if target.exists() {
            let aside = dir.join(format!("{to}{suffix}.replaced-{now}"));
            std::fs::rename(&target, &aside).map_err(|e| io(&e))?;
        }
        std::fs::rename(&source, &target).map_err(|e| io(&e))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nick(name: &str) -> Nick {
        Nick::parse(name).unwrap()
    }

    fn put(save: &Path, dir: &str, name: &str, body: &str) {
        std::fs::create_dir_all(save.join(dir)).unwrap();
        std::fs::write(save.join(dir).join(name), body).unwrap();
    }

    fn read(save: &Path, dir: &str, name: &str) -> Option<String> {
        std::fs::read_to_string(save.join(dir).join(name)).ok()
    }

    #[test]
    fn a_new_nick_takes_the_player_files_along() {
        let dir = tempfile::tempdir().unwrap();
        let root = DataRoot::new(dir.path().to_path_buf()).unwrap();
        let world = WorldId::generate();
        let save = root.world_server_dir(world).join("world");
        let (old, new) = (nick("Old_Name"), nick("New_Name"));
        let old_id = old.offline_uuid().hyphenated().to_string();
        let new_id = new.offline_uuid().hyphenated().to_string();
        put(&save, "playerdata", &format!("{old_id}.dat"), "inventory");
        put(&save, "stats", &format!("{old_id}.json"), "stats");
        // Someone held the new nick before: their file is kept aside.
        put(
            &save,
            "playerdata",
            &format!("{new_id}.dat"),
            "someone else",
        );

        let account = "0b9d0c1e-6a5c-4c8f-9a39-1f3c1a1b2c3d";
        assert_eq!(admitted(&root, world, account, &old, 100).unwrap(), None);
        assert_eq!(
            admitted(&root, world, account, &new, 200).unwrap(),
            Some(old.clone())
        );
        assert_eq!(
            read(&save, "playerdata", &format!("{new_id}.dat")).as_deref(),
            Some("inventory")
        );
        assert_eq!(
            read(&save, "stats", &format!("{new_id}.json")).as_deref(),
            Some("stats")
        );
        assert_eq!(
            read(&save, "playerdata", &format!("{new_id}.dat.replaced-200")).as_deref(),
            Some("someone else")
        );
        assert!(read(&save, "playerdata", &format!("{old_id}.dat")).is_none());
        // The same nick again moves nothing.
        assert_eq!(admitted(&root, world, account, &new, 300).unwrap(), None);
        assert_eq!(
            read(&save, "playerdata", &format!("{new_id}.dat")).as_deref(),
            Some("inventory")
        );
    }

    #[test]
    fn junk_accounts_and_records_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let root = DataRoot::new(dir.path().to_path_buf()).unwrap();
        let world = WorldId::generate();
        for account in ["", "../x", "a/b", &"a".repeat(65)] {
            assert!(admitted(&root, world, account, &nick("Notch"), 1).is_err());
        }
        std::fs::create_dir_all(root.world_dir(world)).unwrap();
        std::fs::write(file(&root, world), b"{\"accounts\": 5}").unwrap();
        // A damaged record is started again, not trusted.
        assert_eq!(
            admitted(&root, world, "abc", &nick("Notch"), 1).unwrap(),
            None
        );
    }
}
