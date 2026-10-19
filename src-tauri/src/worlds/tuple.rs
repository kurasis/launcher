//! Qualified world tuples: the versions a world can be created for (ADAPTATION A8).
//!
//! A version the catalog shows is not automatically a world version: each tuple
//! names the protocol the tunnel's login check parses and the Java the server
//! needs, and is added only after its server has been run in CI.

/// One supported combination of game version, protocol and Java.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorldTuple {
    pub minecraft: &'static str,
    /// Protocol number in the Handshake packet of this version's clients.
    pub protocol: i32,
    pub java_major: u16,
}

/// Every tuple, newest first.
pub const TUPLES: &[WorldTuple] = &[WorldTuple {
    minecraft: "1.21.1",
    protocol: 767,
    java_major: 21,
}];

pub fn find(minecraft: &str) -> Option<&'static WorldTuple> {
    TUPLES.iter().find(|t| t.minecraft == minecraft)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_listed_versions_are_world_versions() {
        assert_eq!(find("1.21.1").map(|t| t.protocol), Some(767));
        assert!(find("1.21").is_none());
        assert!(find("26.3").is_none());
        assert!(find("").is_none());
    }
}
