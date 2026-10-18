//! A return to the version before when a new one does not start (D-063 stage 2).
//!
//! Before installing an update, the launcher writes `updates/pending.json`:
//! where it came from and where it goes. Every start of the new version counts
//! itself there; the start is good once the interface has shown and the
//! launcher has stayed up for [`HEALTHY_AFTER`] (or closed normally after
//! showing), and then the file goes. A version that started
//! [`FAILED_STARTS`] times without a good start is left at the next start:
//! the launcher installs the version it came from (signed, from
//! `releases/<version>/` on upd.lapi.sh) and writes `updates/rolled-back.json`,
//! so that version tells the player and never takes the failed one again.

use std::path::Path;
use std::time::Duration;

use serde::{Deserialize, Serialize};

const PENDING: &str = "pending.json";
const ROLLED_BACK: &str = "rolled-back.json";
/// Starts without a good one before the launcher goes back.
pub const FAILED_STARTS: u32 = 2;
/// How long a start must last after the interface shows to count as good.
pub const HEALTHY_AFTER: Duration = Duration::from_secs(20);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Pending {
    from: String,
    to: String,
    starts: u32,
}

/// Shown once by the version the launcher went back to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RolledBack {
    /// The version that did not start.
    pub failed: String,
    /// The version running again.
    pub version: String,
}

/// What this start does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Start {
    Normal,
    /// The first start of the new version: back up what it may change.
    First,
    /// Install `to` again: `failed` did not start.
    GoBack {
        to: String,
        failed: String,
    },
}

fn read<T: for<'de> Deserialize<'de>>(path: &Path) -> Option<T> {
    std::fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
}

fn write<T: Serialize>(path: &Path, value: &T) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let json = serde_json::to_vec(value).map_err(std::io::Error::other)?;
    crate::storage::atomic::write(path, &json)
}

/// Before an update installs: from `from` to `to`, no start yet.
pub fn before_install(dir: &Path, from: &str, to: &str) -> std::io::Result<()> {
    write(
        &dir.join(PENDING),
        &Pending {
            from: from.to_owned(),
            to: to.to_owned(),
            starts: 0,
        },
    )
}

/// At start, before anything else: counts this start of a new version and
/// says whether to go back.
pub fn on_start(dir: &Path, current: &str) -> Start {
    let path = dir.join(PENDING);
    let Some(mut pending) = read::<Pending>(&path) else {
        return Start::Normal;
    };
    if pending.to != current {
        // The update did not install, or another version was installed by
        // hand: nothing to watch.
        let _ = std::fs::remove_file(&path);
        return Start::Normal;
    }
    let failed_before = pending.starts;
    pending.starts += 1;
    let _ = write(&path, &pending);
    if failed_before == 0 {
        Start::First
    } else if failed_before >= FAILED_STARTS && semver::Version::parse(&pending.from).is_ok() {
        Start::GoBack {
            to: pending.from,
            failed: pending.to,
        }
    } else {
        Start::Normal
    }
}

/// This start of `current` was good: it is no longer watched.
pub fn healthy(dir: &Path, current: &str) {
    let path = dir.join(PENDING);
    if read::<Pending>(&path).is_some_and(|p| p.to == current) {
        let _ = std::fs::remove_file(&path);
    }
}

/// Just before the version before is installed again.
pub fn record_going_back(dir: &Path, failed: &str, to: &str) -> std::io::Result<()> {
    write(
        &dir.join(ROLLED_BACK),
        &RolledBack {
            failed: failed.to_owned(),
            version: to.to_owned(),
        },
    )
}

/// The return that brought the launcher to `current`, to tell the player.
pub fn went_back(dir: &Path, current: &str) -> Option<RolledBack> {
    read::<RolledBack>(&dir.join(ROLLED_BACK)).filter(|r| r.version == current)
}

/// The player has read about the return.
pub fn forget(dir: &Path) {
    let _ = std::fs::remove_file(dir.join(ROLLED_BACK));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_version_that_never_starts_well_is_left_on_the_third_start() {
        let dir = tempfile::tempdir().unwrap();
        let dir = dir.path();
        assert_eq!(on_start(dir, "0.2.0"), Start::Normal, "nothing pending");
        before_install(dir, "0.2.0", "0.3.0").unwrap();
        assert_eq!(on_start(dir, "0.3.0"), Start::First);
        assert_eq!(on_start(dir, "0.3.0"), Start::Normal, "second start");
        assert_eq!(
            on_start(dir, "0.3.0"),
            Start::GoBack {
                to: "0.2.0".into(),
                failed: "0.3.0".into()
            }
        );
        // Going back failed (offline): the next start tries again.
        assert!(matches!(on_start(dir, "0.3.0"), Start::GoBack { .. }));

        record_going_back(dir, "0.3.0", "0.2.0").unwrap();
        // The old version runs: the pending update is not its business.
        assert_eq!(on_start(dir, "0.2.0"), Start::Normal);
        assert!(!dir.join(PENDING).exists());
        assert_eq!(
            went_back(dir, "0.2.0"),
            Some(RolledBack {
                failed: "0.3.0".into(),
                version: "0.2.0".into()
            })
        );
        assert_eq!(went_back(dir, "0.4.0"), None, "only for that version");
        forget(dir);
        assert_eq!(went_back(dir, "0.2.0"), None);
    }

    #[test]
    fn a_good_start_ends_the_watch() {
        let dir = tempfile::tempdir().unwrap();
        let dir = dir.path();
        before_install(dir, "0.2.0", "0.3.0").unwrap();
        assert_eq!(on_start(dir, "0.3.0"), Start::First);
        healthy(dir, "0.2.0");
        assert!(dir.join(PENDING).exists(), "another version's start");
        healthy(dir, "0.3.0");
        for _ in 0..5 {
            assert_eq!(on_start(dir, "0.3.0"), Start::Normal);
        }
    }

    #[test]
    fn a_broken_record_never_sends_the_launcher_anywhere() {
        let dir = tempfile::tempdir().unwrap();
        let dir = dir.path();
        std::fs::write(dir.join(PENDING), b"{not json").unwrap();
        assert_eq!(on_start(dir, "0.3.0"), Start::Normal);
        write(
            &dir.join(PENDING),
            &Pending {
                from: "../../etc".into(),
                to: "0.3.0".into(),
                starts: 9,
            },
        )
        .unwrap();
        assert_eq!(on_start(dir, "0.3.0"), Start::Normal, "not a version");
    }
}
