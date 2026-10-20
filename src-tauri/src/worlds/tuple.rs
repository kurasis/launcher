//! World versions: the Minecraft releases a world can be made for (ADAPTATION A8).
//!
//! Every release from 1.7.6 on, the first whose server keeps players by UUID
//! (`whitelist.json`, `playerdata/<uuid>.dat`), which the whitelist of offline
//! UUIDs needs. Snapshots and pre-releases are not world versions. The rest
//! comes from elsewhere: the server's Java from the version's descriptor, its
//! game protocol from the server itself when it is ready, the Login Start
//! layout from the client's handshake (`tunnel::handshake`), and the few
//! properties that changed between releases from [`Release`].

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

    pub fn named_modes(self) -> bool {
        self >= NAMED_MODES
    }

    pub fn preset_ids(self) -> bool {
        self >= PRESET_IDS
    }
}

/// The world version `minecraft` names, if it is one.
pub fn release(minecraft: &str) -> Option<Release> {
    Release::parse(minecraft).filter(|r| *r >= OLDEST && (r.major == 1 || r.major >= FIRST_YEAR))
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
        let r = |id| release(id).unwrap();
        assert!(!r("1.13.2").named_modes());
        assert!(r("1.14").named_modes());
        assert!(!r("1.18.2").preset_ids());
        assert!(r("1.19").preset_ids());
        assert!(r("26.1").preset_ids() && r("26.1").named_modes());
    }
}
