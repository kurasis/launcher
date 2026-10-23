//! World versions: the Minecraft versions a world can be made for (ADAPTATION A8).
//!
//! Every release from 1.7.6 on, the first whose server keeps players by UUID
//! (`whitelist.json`, `playerdata/<uuid>.dat`), which the whitelist of offline
//! UUIDs needs; and, when the player turns snapshots on, the snapshots from
//! 14w17a (the first after 1.7.6) and the pre-releases and release candidates
//! of those releases. The rest comes from elsewhere: the server's Java from the
//! version's descriptor, its game protocol from the server itself when it is
//! ready, the Login Start layout from the client's handshake
//! (`tunnel::handshake`), and the few properties that changed between releases
//! from [`WorldVersion`].
//!
//! The versions of our own list (D-073) have ids of every shape, so the catalog
//! says which they are: those with a server from 1.7.6's day on are world
//! versions too, behind a switch of their own ([`of_entry`]), and a world made
//! for one says so in `world.json` ([`world_version`]).

use crate::catalog::manifest::CatalogEntry;
use crate::storage::paths::SafeName;

/// A release number: `1.7.10`, `1.21` or `26.1`; ordered by its numbers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Release {
    major: u16,
    minor: u16,
    patch: u16,
}

/// The oldest world version.
pub const OLDEST: Release = Release::new(1, 7, 6);
/// Difficulty and game mode are read by name from here on, as numbers before.
const NAMED_MODES: Release = Release::new(1, 14, 0);
/// `level-type` is a world preset ID (`minecraft:flat`) from here on.
const PRESET_IDS: Release = Release::new(1, 19, 0);
/// The year-numbered releases (26.1 and later).
const FIRST_YEAR: u16 = 26;

impl Release {
    const fn new(major: u16, minor: u16, patch: u16) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }

    /// Two or three numbers of at most three digits, without leading zeros:
    /// anything with a suffix (`-pre1`, `-rc1`, `-snapshot-2`) or another
    /// shape (`24w14a`) is not a release.
    fn parse(id: &str) -> Option<Self> {
        let number = |part: &str| -> Option<u16> {
            let plain = (1..=3).contains(&part.len())
                && part.bytes().all(|b| b.is_ascii_digit())
                && (part == "0" || !part.starts_with('0'));
            plain.then(|| part.parse().ok()).flatten()
        };
        let mut parts = id.split('.');
        let major = number(parts.next()?)?;
        let minor = number(parts.next()?)?;
        let patch = match parts.next() {
            Some(part) => number(part)?,
            None => 0,
        };
        parts
            .next()
            .is_none()
            .then_some(Self::new(major, minor, patch))
    }
}

/// The release `minecraft` names, if it is a world version.
fn release(minecraft: &str) -> Option<Release> {
    Release::parse(minecraft).filter(|r| *r >= OLDEST && (r.major == 1 || r.major >= FIRST_YEAR))
}

/// The first weekly snapshot after 1.7.6, as (year, week).
const OLDEST_WEEKLY: (u8, u8) = (14, 17);

/// One or two digits, without a leading zero, from 1.
fn small_number(text: &str) -> Option<u8> {
    let plain = (1..=2).contains(&text.len())
        && text.bytes().all(|b| b.is_ascii_digit())
        && !text.starts_with('0');
    plain.then(|| text.parse().ok()).flatten()
}

/// A snapshot or pre-release id: `24w14a`, `1.21-pre1`, `1.21-rc1`,
/// `26.1-snapshot-2`, `26.1-pre-1` or `26.1-rc-1`. Anything else (April fools
/// versions, combat tests, ids with spaces) is not a world version.
fn snapshot(id: &str) -> bool {
    if let Some((year, rest)) = id.split_once('w') {
        let bytes = rest.as_bytes();
        let weekly = year.len() == 2
            && year.bytes().all(|b| b.is_ascii_digit())
            && bytes.len() == 3
            && bytes[..2].iter().all(u8::is_ascii_digit)
            && bytes[2].is_ascii_lowercase();
        return weekly
            && match (year.parse::<u8>(), rest[..2].parse::<u8>()) {
                (Ok(year), Ok(week)) => (1..=53).contains(&week) && (year, week) >= OLDEST_WEEKLY,
                _ => false,
            };
    }
    let Some((base, tail)) = id.split_once('-') else {
        return false;
    };
    let numbered = ["snapshot-", "pre-", "rc-", "pre", "rc"]
        .iter()
        .find_map(|prefix| tail.strip_prefix(prefix))
        .and_then(small_number)
        .is_some();
    numbered && release(base).is_some()
}

/// A world version, with what its server reads in `server.properties`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorldVersion {
    Release(Release),
    /// A snapshot or pre-release. Its properties are written in the oldest
    /// forms (numbers, the old world type names), which every server from 1.7.6
    /// to 26.3 still reads, since a snapshot's id does not say which forms it
    /// reads.
    Snapshot,
}

impl WorldVersion {
    /// Difficulty and game mode by name rather than by number.
    pub fn named_modes(self) -> bool {
        matches!(self, Self::Release(r) if r >= NAMED_MODES)
    }

    /// `level-type` as a world preset ID rather than a world type name.
    pub fn preset_ids(self) -> bool {
        matches!(self, Self::Release(r) if r >= PRESET_IDS)
    }

    pub fn is_snapshot(self) -> bool {
        self == Self::Snapshot
    }
}

/// The world version `minecraft` names, if it is one.
pub fn version(minecraft: &str) -> Option<WorldVersion> {
    match release(minecraft) {
        Some(release) => Some(WorldVersion::Release(release)),
        None => snapshot(minecraft).then_some(WorldVersion::Snapshot),
    }
}

/// 1.7.6's day: a version of our own list released before it is not a world
/// version (`2.0`, 1.5's early releases, Beta).
const OLDEST_LISTED: (i32, u32, u32) = (2014, 4, 9);

/// The world version of a catalog entry: Mojang's by its id, and a version of
/// our own list (D-073) when it has a server and came from 1.7.6's day on. Its
/// properties are written as a snapshot's, since its id does not say which
/// forms it reads.
pub fn of_entry(entry: &CatalogEntry) -> Option<WorldVersion> {
    let Some(special) = &entry.special else {
        return version(&entry.id);
    };
    let (year, month, day) = OLDEST_LISTED;
    let oldest = chrono::NaiveDate::from_ymd_opt(year, month, day)?.and_hms_opt(0, 0, 0)?;
    let released = entry
        .release_time
        .as_deref()
        .and_then(|t| chrono::DateTime::parse_from_rfc3339(t).ok())
        .is_some_and(|t| t.naive_utc() >= oldest);
    (special.server && released && SafeName::parse(&entry.id).is_ok())
        .then(|| version(&entry.id).unwrap_or(WorldVersion::Snapshot))
}

/// The world version of a world made for `minecraft`; `special` when it was
/// made for a version of our own list that [`of_entry`] took.
pub fn world_version(minecraft: &str, special: bool) -> Option<WorldVersion> {
    version(minecraft).or_else(|| {
        (special && SafeName::parse(minecraft).is_ok()).then_some(WorldVersion::Snapshot)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn releases_from_1_7_6_are_world_versions() {
        for id in [
            "1.7.6", "1.7.10", "1.8", "1.8.9", "1.12.2", "1.16.5", "1.19.2", "1.20.1", "1.21.1",
            "1.21.11", "26.1", "26.3", "26.3.1",
        ] {
            assert!(release(id).is_some(), "{id}");
        }
        assert!(release("1.21.1") > release("1.21"));
        assert!(release("26.1") > release("1.21.11"));
        assert!(release("1.10") > release("1.9.4"), "numbers, not text");
    }

    #[test]
    fn everything_else_is_not() {
        for id in [
            "",
            "1",
            "1.7.5",
            "1.7.2",
            "1.6.4",
            "b1.7.3",
            "a1.0.4",
            "rd-132211",
            "24w14a",
            "1.21-pre1",
            "1.21-rc1",
            "26.1-snapshot-2",
            "1.20.1 ",
            " 1.20.1",
            "1.20.1.1",
            "1..1",
            "1.21.",
            ".1.21",
            "1.021",
            "1.21.0001",
            "+1.21",
            "-1.21",
            "1.2a",
            "2.0",
            "25.4",
            "1.21\n",
            "1.21/../x",
            "١.٢١",
        ] {
            assert!(release(id).is_none(), "{id:?}");
        }
    }

    #[test]
    fn properties_change_where_the_server_changed() {
        let r = |id| version(id).unwrap();
        assert!(!r("1.13.2").named_modes());
        assert!(r("1.14").named_modes());
        assert!(!r("1.18.2").preset_ids());
        assert!(r("1.19").preset_ids());
        assert!(r("26.1").preset_ids() && r("26.1").named_modes());
        for snapshot in ["24w14a", "1.21-pre1", "26.1-snapshot-2"] {
            assert!(!r(snapshot).named_modes() && !r(snapshot).preset_ids());
            assert!(r(snapshot).is_snapshot());
        }
        assert!(!r("1.21.1").is_snapshot());
    }

    #[test]
    fn snapshots_from_14w17a_and_pre_releases_are_world_versions() {
        for id in [
            "14w17a",
            "14w34d",
            "18w43a",
            "24w14a",
            "25w46a",
            "1.7.6-pre1",
            "1.8-pre3",
            "1.16-rc1",
            "1.21-pre1",
            "1.21.1-rc1",
            "26.1-snapshot-1",
            "26.1-snapshot-12",
            "26.1-pre-1",
            "26.1-rc-2",
        ] {
            assert_eq!(version(id), Some(WorldVersion::Snapshot), "{id}");
        }
    }

    #[test]
    fn other_snapshot_shapes_are_not() {
        for id in [
            "14w02a",
            "14w11b",
            "13w47e",
            "24w00a",
            "24w54a",
            "24w14",
            "24w14A",
            "24w14aa",
            "4w14a",
            "24w1a",
            "20w14infinite",
            "24w14potato",
            "23w13a_or_b",
            "1.RV-Pre1",
            "3D Shareware v1.34",
            "1.14 Pre-Release 1",
            "1.14.3 - Combat Test",
            "1.7.2-pre1",
            "1.6.4-pre1",
            "1.21-pre0",
            "1.21-pre01",
            "1.21-pre100",
            "1.21-pre",
            "1.21-pre-",
            "1.21-snapshot1x",
            "1.21-beta-1",
            "-pre1",
            "1.21-pre1\n",
            "24w14a/../x",
        ] {
            assert_eq!(version(id), None, "{id:?}");
        }
    }

    fn listed(id: &str, release_time: &str, server: bool) -> CatalogEntry {
        CatalogEntry {
            id: id.to_owned(),
            version_type: "snapshot".to_owned(),
            url: format!("https://upd.lapi.sh/versions/meta/{}.json", "a".repeat(40)),
            sha1: crate::storage::paths::Sha1::parse(&"a".repeat(40)).unwrap(),
            release_time: Some(release_time.to_owned()),
            special: Some(crate::catalog::extra::Special {
                name: id.to_owned(),
                title: None,
                group: "joke".to_owned(),
                variant: None,
                server,
            }),
        }
    }

    #[test]
    fn versions_of_our_list_with_a_server_from_1_7_6_on_are_world_versions() {
        // The worlds service takes the same ids for rooms.
        let data: serde_json::Value =
            serde_json::from_slice(include_bytes!("../../../data/extra-versions.json")).unwrap();
        let taken: Vec<_> = data["versions"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|v| {
                let entry = listed(
                    v["id"].as_str().unwrap(),
                    v["releaseTime"].as_str().unwrap(),
                    v["server"].as_bool().unwrap(),
                );
                of_entry(&entry) == Some(WorldVersion::Snapshot)
            })
            .map(|v| v["id"].as_str().unwrap())
            .collect();
        let service: Vec<_> = include_str!("../../../data/extra-world-versions.txt")
            .lines()
            .collect();
        assert_eq!(taken, service);
        for id in ["24w14potato", "3D Shareware v1.34", "combat-8c", "15w14a"] {
            assert!(taken.contains(&id), "{id}");
        }
        for id in ["2.0-blue", "13w11a", "b1.7_01", "1.0.0-rc2-3"] {
            assert!(!taken.contains(&id), "{id}");
        }
    }

    #[test]
    fn a_version_of_our_list_needs_a_server_a_time_and_a_safe_id() {
        let at = "2020-04-01T12:00:00+00:00";
        assert_eq!(
            of_entry(&listed("20w14infinite", at, true)),
            Some(WorldVersion::Snapshot)
        );
        assert_eq!(of_entry(&listed("20w14infinite", at, false)), None);
        for time in [
            "2014-04-08T23:59:59+00:00",
            "2014-04-09T01:00:00+02:00",
            "yesterday",
            "",
        ] {
            assert_eq!(of_entry(&listed("x", time, true)), None, "{time}");
        }
        assert!(of_entry(&listed("x", "2014-04-09T00:00:00+00:00", true)).is_some());
        let mut timeless = listed("x", at, true);
        timeless.release_time = None;
        assert_eq!(of_entry(&timeless), None);
        for id in ["../x", "a/b", "CON", " x", "x.", ""] {
            assert_eq!(of_entry(&listed(id, at, true)), None, "{id:?}");
        }
        // Mojang's by their ids, as before.
        let mut mojang = listed("1.21.1", at, true);
        mojang.special = None;
        assert!(matches!(of_entry(&mojang), Some(WorldVersion::Release(_))));
        mojang.id = "20w14infinite".to_owned();
        assert_eq!(of_entry(&mojang), None);
    }

    #[test]
    fn a_world_says_when_it_is_for_a_version_of_our_list() {
        assert_eq!(world_version("24w14potato", false), None);
        assert_eq!(
            world_version("24w14potato", true),
            Some(WorldVersion::Snapshot)
        );
        assert!(matches!(
            world_version("1.21.1", true),
            Some(WorldVersion::Release(_))
        ));
        for id in ["../x", "a\\b", "NUL", "", "x\n"] {
            assert_eq!(world_version(id, true), None, "{id:?}");
        }
    }
}
