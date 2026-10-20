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
}
