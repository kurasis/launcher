//! Mod loaders: Fabric, Quilt, Forge, NeoForge and OptiFine (D-039).
//!
//! An installation can run a mod loader on top of a vanilla version. The loader's
//! version profile inherits from the vanilla descriptor: the launcher merges the
//! two into one plan instead of guessing. Every file is still verified by SHA-1
//! before use:
//!
//! - Fabric and Quilt publish a version profile through their meta services;
//!   libraries without a published hash are pinned by their Maven `.sha1` file.
//! - Forge and NeoForge publish an installer jar, pinned by its Maven `.sha1`
//!   file. It holds the version profile, the install profile and the binary
//!   patches; its processors build the patched game with the private Java runtime
//!   (see [`processors`]).
//! - OptiFine on its own comes from the mods catalog, pinned by its SHA-256;
//!   its jar is also its installer (see [`optifine`]).
//!
//! What was resolved is stored once as a launcher-owned [`manifest::LoaderManifest`]
//! under its SHA-1, which the install lock names, so Repair and launch re-derive
//! the exact same plan without the network.

pub mod coordinate;
pub mod manifest;
pub mod meta;
pub mod optifine;
pub mod processors;

use std::cmp::Ordering;

use serde::{Deserialize, Serialize};

use crate::error::{LauncherError, LauncherResult};

/// The supported mod loaders.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LoaderKind {
    Fabric,
    Quilt,
    Forge,
    #[serde(rename = "neoforge")]
    NeoForge,
    /// OptiFine on its own, without Forge.
    #[serde(rename = "optifine")]
    OptiFine,
}

impl LoaderKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Fabric => "fabric",
            Self::Quilt => "quilt",
            Self::Forge => "forge",
            Self::NeoForge => "neoforge",
            Self::OptiFine => "optifine",
        }
    }

    /// Product name, for logs.
    pub fn name(self) -> &'static str {
        match self {
            Self::Fabric => "Fabric",
            Self::Quilt => "Quilt",
            Self::Forge => "Forge",
            Self::NeoForge => "NeoForge",
            Self::OptiFine => "OptiFine",
        }
    }

    /// Whether the loader ships an installer whose processors must run.
    pub fn uses_installer(self) -> bool {
        matches!(self, Self::Forge | Self::NeoForge | Self::OptiFine)
    }

    /// Whether mods can be added: OptiFine on its own takes none.
    pub fn takes_mods(self) -> bool {
        self != Self::OptiFine
    }
}

/// A loader and the exact loader version an installation runs.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoaderChoice {
    pub kind: LoaderKind,
    /// The loader's own version id: `0.16.14` for Fabric, `1.21.1-52.1.0` (the
    /// Maven version) for Forge, `21.1.262` for NeoForge, `1.21.1_HD_U_J1` (the
    /// game version and the edition) for OptiFine.
    pub version: String,
}

impl LoaderChoice {
    pub fn validate(&self) -> LauncherResult<()> {
        if valid_version(&self.version) {
            Ok(())
        } else {
            Err(LauncherError::invalid_input(
                "Choose a mod loader version from the list.",
            ))
        }
    }
}

/// Loader and game version ids: 1–64 characters of letters, digits and `.+_-`,
/// starting with a letter or digit. They become URL path segments and Maven
/// versions, so nothing else is accepted.
pub fn valid_version(version: &str) -> bool {
    (1..=64).contains(&version.len())
        && version
            .bytes()
            .next()
            .is_some_and(|b| b.is_ascii_alphanumeric())
        && version
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'+' | b'_' | b'-'))
}

/// Orders version strings the way people read them: numeric runs compare as
/// numbers, and a pre-release (`-beta.2`, `-rc-1`) comes before its release.
pub fn compare_versions(a: &str, b: &str) -> Ordering {
    let (a_base, a_pre) = a.split_once('-').map_or((a, None), |(x, y)| (x, Some(y)));
    let (b_base, b_pre) = b.split_once('-').map_or((b, None), |(x, y)| (x, Some(y)));
    compare_tokens(a_base, b_base).then_with(|| match (a_pre, b_pre) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Greater,
        (Some(_), None) => Ordering::Less,
        (Some(x), Some(y)) => compare_tokens(x, y),
    })
}

fn compare_tokens(a: &str, b: &str) -> Ordering {
    let mut a = tokens(a).into_iter();
    let mut b = tokens(b).into_iter();
    loop {
        match (a.next(), b.next()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) => {
                let order = match (x.parse::<u64>(), y.parse::<u64>()) {
                    (Ok(x), Ok(y)) => x.cmp(&y),
                    (Ok(_), Err(_)) => Ordering::Greater,
                    (Err(_), Ok(_)) => Ordering::Less,
                    (Err(_), Err(_)) => x.cmp(y),
                };
                if order != Ordering::Equal {
                    return order;
                }
            }
        }
    }
}

/// Alternating digit and non-digit runs, separators dropped.
fn tokens(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = None;
    let mut digits = false;
    for (i, c) in text.char_indices() {
        if !c.is_ascii_alphanumeric() {
            if let Some(s) = start.take() {
                out.push(&text[s..i]);
            }
            continue;
        }
        match start {
            Some(s) if c.is_ascii_digit() != digits => {
                out.push(&text[s..i]);
                start = Some(i);
            }
            Some(_) => {}
            None => start = Some(i),
        }
        digits = c.is_ascii_digit();
    }
    if let Some(s) = start {
        out.push(&text[s..]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_order_numerically_with_prereleases_first() {
        let mut list = vec![
            "0.20.0-beta.9",
            "0.24.0",
            "0.20.0",
            "0.9.1",
            "0.20.0-beta.10",
            "0.20.2-beta.1",
            "0.16.14",
        ];
        list.sort_by(|a, b| compare_versions(a, b));
        assert_eq!(
            list,
            [
                "0.9.1",
                "0.16.14",
                "0.20.0-beta.9",
                "0.20.0-beta.10",
                "0.20.0",
                "0.20.2-beta.1",
                "0.24.0"
            ]
        );
        assert_eq!(
            compare_versions("26.1.2.122", "26.1.0.9"),
            Ordering::Greater
        );
        assert_eq!(
            compare_versions("1.21.1-52.1.0", "1.21.1-52.0.40"),
            Ordering::Greater
        );
    }

    #[test]
    fn loader_versions_are_plain_ids() {
        for good in ["0.16.14", "1.21.1-52.1.0", "21.1.262", "0.15.5+mixin.0.8.7"] {
            assert!(valid_version(good), "{good}");
        }
        for bad in ["", "../x", "1.0/2", "-1", "a b", "1.0?x", &"9".repeat(65)] {
            assert!(!valid_version(bad), "{bad}");
        }
        let json = serde_json::to_string(&LoaderKind::NeoForge).unwrap();
        assert_eq!(json, "\"neoforge\"");
        let json = serde_json::to_string(&LoaderKind::OptiFine).unwrap();
        assert_eq!(json, "\"optifine\"");
    }
}
