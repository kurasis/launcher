//! What a mod's jar says about itself, read the way the loaders read it at
//! start (D-066): the mods it provides, what it needs and what it breaks, from
//! `fabric.mod.json`, `quilt.mod.json`, `META-INF/neoforge.mods.toml`,
//! `META-INF/mods.toml` and `mcmod.info`, nested jars included. [`check`]
//! then says whether a set of jars starts on a game version, loader and Java,
//! and why not.
//!
//! - Shared by the launcher (its install plans, the check before a launch,
//!   world servers) and the catalog service, which reads every jar it keeps.
//! - Jars come from anywhere: every size, count and depth is bounded, and
//!   anything unreadable is left out rather than guessed.
//! - Where a version cannot be compared (`${version}`, a scheme of its own),
//!   the answer is "can't tell", which [`check`] takes as fine: a false alarm
//!   would stop a set that starts.

mod check;
mod jar;
mod version;

pub use check::{Jar, Problem, Setup, check, for_loader};
pub use jar::{read, read_bytes};
pub use version::{allows, compare};

use serde::{Deserialize, Serialize};

/// The loader a descriptor is written for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Loader {
    Fabric,
    Quilt,
    Forge,
    NeoForge,
}

impl Loader {
    /// Forge and NeoForge write ranges as Maven does (`[1.21.1,)`); Fabric
    /// and Quilt as predicates (`>=0.16 <1`, `~1.21`).
    pub fn maven(self) -> bool {
        matches!(self, Self::Forge | Self::NeoForge)
    }

    /// The mod ID the loader itself goes by.
    pub fn id(self) -> &'static str {
        match self {
            Self::Fabric => "fabricloader",
            Self::Quilt => "quilt_loader",
            Self::Forge => "forge",
            Self::NeoForge => "neoforge",
        }
    }
}

/// Where a mod runs, or where a need applies.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    #[default]
    Both,
    Client,
    Server,
}

impl Side {
    fn is_both(&self) -> bool {
        *self == Self::Both
    }

    /// Whether something of this side counts where `here` runs.
    pub fn applies(self, here: Side) -> bool {
        self == Self::Both || here == Self::Both || self == here
    }
}

fn is_false(b: &bool) -> bool {
    !b
}

/// One descriptor of a jar: what it gives a loader. A jar made for several
/// loaders has one for each.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub loader: Loader,
    /// The mods in the jar: its own first, then those of jars inside it.
    pub mods: Vec<Provided>,
    /// What those mods need to start.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub needs: Vec<Need>,
    /// What they do not start with.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub breaks: Vec<Need>,
}

impl Manifest {
    /// Whether it is one [`read`] could give: every ID, version and range as
    /// the reader cleans them and within its bounds. For manifests that come
    /// from elsewhere, such as the catalog's answers.
    pub fn is_clean(&self) -> bool {
        jar::clean(self)
    }
}

/// A mod a jar gives, under its ID.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Provided {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// Inside another jar (Fabric's `jars`, Jar-in-Jar): the loader keeps the
    /// newest of such copies, so a second one is no clash.
    #[serde(default, skip_serializing_if = "is_false")]
    pub nested: bool,
    #[serde(default, skip_serializing_if = "Side::is_both")]
    pub side: Side,
}

/// A mod another one needs, or does not start with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Need {
    /// The mod that says so.
    pub by: String,
    pub id: String,
    /// Any one of these, in the loader's own words; none: any version.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub versions: Vec<String>,
    #[serde(default, skip_serializing_if = "Side::is_both")]
    pub side: Side,
}

#[cfg(test)]
mod tests;
