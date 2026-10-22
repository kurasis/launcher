//! Launcher Skins in the game (D-065, stage 8).
//!
//! When an installation starts, the launcher adds its skins mod: the jar for
//! the game version and loader goes into `<game>/mods/` (Fabric, Quilt,
//! NeoForge, Forge), or the Java agent is passed with `-javaagent` (no loader).
//! The jars are this repository's GitHub release `skins-mod-v<version>`, which
//! players download only from upd.lapi.sh, never from GitHub, and are pinned by
//! SHA-256 and size: here in [`JARS`], or in the signed list
//! on upd.lapi.sh ([`list`]), which brings a newer mod version without a new
//! launcher. Those two are the only lists of what may reach a game. A jar is downloaded once into [`MANAGED_DIR`], shared by
//! every installation, and checked again each time before it is used.
//!
//! Nothing here can stop a launch: a jar that cannot be had means the game
//! starts without it. In `mods/` only the launcher's own files
//! (`launcherskins-*.jar`) are ever written or removed. Nothing about the
//! player is logged.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

use sha2::{Digest as _, Sha256 as Sha256Hasher};
use uuid::Uuid;

use crate::download::transport::HttpTransport;
use crate::download::{
    CancelToken, Digest, DownloadRequest, Executor, ExecutorConfig, NoProgress,
    resolve_managed_path,
};
use crate::loader::LoaderKind;
use crate::net::SourcePolicy;
use crate::storage::paths::{DataRoot, Sha256};

/// The mod's version these jars are (`mod/VERSION`).
pub const MOD_VERSION: &str = "0.6.3";
/// Where players get a release's jars: `<RELEASES><version>/<file>` on our
/// update server, which CI fills from the GitHub release (D-065).
const RELEASES: &str = "https://upd.lapi.sh/skins-mod/";
/// The jars of [`MOD_VERSION`].
const RELEASE: &str = "https://upd.lapi.sh/skins-mod/0.6.3/";
/// Verified jars, relative to DATA_ROOT; shared by every installation.
pub const MANAGED_DIR: &str = "shared/launcherskins";
/// Every jar of ours starts with this and ends with [`JAR_SUFFIX`].
const PREFIX: &str = "launcherskins-";
const JAR_SUFFIX: &str = ".jar";
/// No jar of ours is anywhere near this; a larger entry is refused.
pub const MAX_JAR_BYTES: u64 = 16 * 1024 * 1024;
/// The longest a launch waits for a missing jar before it starts without it.
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(45);

/// What a jar plugs into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// Fabric; the same jar runs on Quilt.
    Fabric,
    NeoForge,
    Forge,
    /// The Java agent, for the game without a loader.
    Agent,
}

impl Target {
    /// None for OptiFine on its own, which takes no mods.
    pub fn for_loader(loader: Option<LoaderKind>) -> Option<Self> {
        Some(match loader {
            None => Self::Agent,
            Some(LoaderKind::Fabric | LoaderKind::Quilt) => Self::Fabric,
            Some(LoaderKind::NeoForge) => Self::NeoForge,
            Some(LoaderKind::Forge) => Self::Forge,
            Some(LoaderKind::OptiFine) => return None,
        })
    }

    /// The target that serves this game version and loader. NeoForge for 1.20.1
    /// is still Forge and runs Forge's jar.
    pub fn for_game(game: &str, loader: Option<LoaderKind>) -> Option<Self> {
        Some(match Self::for_loader(loader)? {
            Self::NeoForge if game == "1.20.1" => Self::Forge,
            target => target,
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Fabric => "fabric",
            Self::NeoForge => "neoforge",
            Self::Forge => "forge",
            Self::Agent => "agent",
        }
    }
}

/// Which build of the mod serves which game versions, by target: one jar each,
/// named `launcherskins-<target>-<label>-<mod version>.jar`. Releases whose game
/// code is the same share a build (mod/settings.gradle). The Java agent of a
/// release with obfuscated names (before 26.1) is built for that release alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Build {
    pub target: Target,
    pub label: &'static str,
    pub games: &'static [&'static str],
}

const fn build(target: Target, label: &'static str, games: &'static [&'static str]) -> Build {
    Build {
        target,
        label,
        games,
    }
}

/// From 1.20.2 the skin is a PlayerSkin: Fabric (intermediary names) runs one jar from
/// 1.20.2 to 1.21.1, NeoForge (Mojang names) one from 1.20.6.
const BAND_1_20_2_FABRIC: &[&str] = &[
    "1.20.2", "1.20.3", "1.20.4", "1.20.5", "1.20.6", "1.21", "1.21.1",
];
const BAND_1_20_6_NEOFORGE: &[&str] = &["1.20.6", "1.21", "1.21.1"];
const BAND_1_20_2: &[&str] = &["1.20.2", "1.20.3", "1.20.4"];
const BAND_1_21_2: &[&str] = &["1.21.2", "1.21.3", "1.21.4"];
const BAND_1_21_5: &[&str] = &["1.21.5", "1.21.6", "1.21.7", "1.21.8"];
const BAND_1_21_9: &[&str] = &["1.21.9", "1.21.10"];
/// Fabric runs 1.15.2 to 1.20.1 with intermediary names, Forge 1.17.1 to 1.20.1 with SRG
/// names; neither changes for what the mod uses, so one jar serves each range.
const BAND_1_15_2_FABRIC: &[&str] = &[
    "1.15.2", "1.16", "1.16.1", "1.16.2", "1.16.3", "1.16.4", "1.16.5", "1.17", "1.17.1", "1.18",
    "1.18.1", "1.18.2", "1.19", "1.19.1", "1.19.2", "1.19.3", "1.19.4", "1.20", "1.20.1",
];
const BAND_1_17_FORGE: &[&str] = &[
    "1.17.1", "1.18", "1.18.1", "1.18.2", "1.19", "1.19.1", "1.19.2", "1.19.3", "1.19.4", "1.20",
    "1.20.1",
];
const BAND_26: &[&str] = &["26.1", "26.1.1", "26.1.2", "26.2", "26.3"];

pub const BUILDS: &[Build] = &[
    build(Target::Fabric, "26.1-26.3", BAND_26),
    build(Target::NeoForge, "26.1-26.3", BAND_26),
    build(Target::Forge, "26.1-26.3", BAND_26),
    build(Target::Agent, "26.1-26.3", BAND_26),
    build(Target::Fabric, "1.21.11", &["1.21.11"]),
    build(Target::NeoForge, "1.21.11", &["1.21.11"]),
    build(Target::Forge, "1.21.11", &["1.21.11"]),
    build(Target::Agent, "1.21.11", &["1.21.11"]),
    build(Target::Fabric, "1.21.9-1.21.10", BAND_1_21_9),
    build(Target::NeoForge, "1.21.9-1.21.10", BAND_1_21_9),
    build(Target::Forge, "1.21.9-1.21.10", BAND_1_21_9),
    build(Target::Agent, "1.21.10", &["1.21.10"]),
    build(Target::Agent, "1.21.9", &["1.21.9"]),
    build(Target::Fabric, "1.21.5-1.21.8", BAND_1_21_5),
    build(Target::NeoForge, "1.21.5-1.21.8", BAND_1_21_5),
    build(Target::Forge, "1.21.5-1.21.8", BAND_1_21_5),
    build(Target::Agent, "1.21.8", &["1.21.8"]),
    build(Target::Agent, "1.21.7", &["1.21.7"]),
    build(Target::Agent, "1.21.6", &["1.21.6"]),
    build(Target::Agent, "1.21.5", &["1.21.5"]),
    build(Target::Fabric, "1.21.2-1.21.4", BAND_1_21_2),
    build(Target::NeoForge, "1.21.2-1.21.4", BAND_1_21_2),
    build(Target::Forge, "1.21.2-1.21.4", BAND_1_21_2),
    build(Target::Agent, "1.21.4", &["1.21.4"]),
    build(Target::Agent, "1.21.3", &["1.21.3"]),
    build(Target::Agent, "1.21.2", &["1.21.2"]),
    build(Target::Fabric, "1.20.2-1.21.1", BAND_1_20_2_FABRIC),
    build(Target::NeoForge, "1.20.6-1.21.1", BAND_1_20_6_NEOFORGE),
    build(Target::Forge, "1.21.1", &["1.21.1"]),
    build(Target::Agent, "1.21.1", &["1.21.1"]),
    build(Target::Agent, "1.21", &["1.21"]),
    build(Target::Forge, "1.20.6", &["1.20.6"]),
    build(Target::Agent, "1.20.6", &["1.20.6"]),
    build(Target::Agent, "1.20.5", &["1.20.5"]),
    // Forge runs 1.20.2 to 1.20.4 with SRG names, which keep their numbers across them.
    build(Target::NeoForge, "1.20.2-1.20.4", BAND_1_20_2),
    build(Target::Forge, "1.20.2-1.20.4", BAND_1_20_2),
    build(Target::Agent, "1.20.4", &["1.20.4"]),
    build(Target::Agent, "1.20.3", &["1.20.3"]),
    build(Target::Agent, "1.20.2", &["1.20.2"]),
    build(Target::Fabric, "1.15.2-1.20.1", BAND_1_15_2_FABRIC),
    // NeoForge for 1.20.1 is still Forge and runs Forge's jar.
    build(Target::Forge, "1.17.1-1.20.1", BAND_1_17_FORGE),
    build(Target::Forge, "1.16.5", &["1.16.5"]),
    build(Target::Agent, "1.20.1", &["1.20.1"]),
    build(Target::Agent, "1.20", &["1.20"]),
    build(Target::Agent, "1.19.4", &["1.19.4"]),
    build(Target::Agent, "1.19.3", &["1.19.3"]),
    build(Target::Agent, "1.19.2", &["1.19.2"]),
    build(Target::Agent, "1.19.1", &["1.19.1"]),
    build(Target::Agent, "1.19", &["1.19"]),
    build(Target::Agent, "1.18.2", &["1.18.2"]),
    build(Target::Agent, "1.18.1", &["1.18.1"]),
    build(Target::Agent, "1.18", &["1.18"]),
    build(Target::Agent, "1.17.1", &["1.17.1"]),
    build(Target::Agent, "1.17", &["1.17"]),
    build(Target::Agent, "1.16.5", &["1.16.5"]),
    build(Target::Agent, "1.16.4", &["1.16.4"]),
    build(Target::Agent, "1.16.3", &["1.16.3"]),
    build(Target::Agent, "1.16.2", &["1.16.2"]),
    build(Target::Agent, "1.16.1", &["1.16.1"]),
    build(Target::Agent, "1.16", &["1.16"]),
    // Before 1.15.2 a texture is registered through another method; Forge 1.14.4 and
    // 1.15.2 have no Mixin (a coremod) and differ in SRG names.
    build(Target::Forge, "1.15.2", &["1.15.2"]),
    build(Target::Agent, "1.15.2", &["1.15.2"]),
    build(Target::Fabric, "1.15-1.15.1", &["1.15", "1.15.1"]),
    build(Target::Agent, "1.15.1", &["1.15.1"]),
    build(Target::Agent, "1.15", &["1.15"]),
    build(Target::Fabric, "1.14.4", &["1.14.4"]),
    build(Target::Forge, "1.14.4", &["1.14.4"]),
    build(Target::Agent, "1.14.4", &["1.14.4"]),
    build(Target::Forge, "1.12.2", &["1.12.2"]),
    // Forge only: 1.8.9 shares 1.12.2's game names; 1.7.10 hooks the player itself.
    build(Target::Forge, "1.8.9", &["1.8.9"]),
    build(Target::Forge, "1.7.10", &["1.7.10"]),
];

impl Build {
    /// The build for this game version and loader, if the mod has one.
    pub fn find(game: &str, loader: Option<LoaderKind>) -> Option<&'static Build> {
        let target = Target::for_game(game, loader)?;
        BUILDS
            .iter()
            .find(|b| b.target == target && b.games.contains(&game))
    }

    /// Its jar's name for a mod version.
    pub fn file(&self, mod_version: &str) -> String {
        format!(
            "{PREFIX}{}-{}-{mod_version}{JAR_SUFFIX}",
            self.target.as_str(),
            self.label
        )
    }
}

/// The jar a launch uses: from [`JARS`] or from the signed [`list::List`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pick {
    pub file: String,
    pub sha256: String,
    pub size: u64,
    pub url: String,
}

impl Pick {
    fn of(jar: &Jar) -> Self {
        Self {
            file: jar.file.to_owned(),
            sha256: jar.sha256.to_owned(),
            size: jar.size,
            url: jar.url(),
        }
    }

    /// Its place under DATA_ROOT.
    fn managed(&self) -> String {
        format!("{MANAGED_DIR}/{}", self.file)
    }
}

/// The jar for this game version and loader: the signed list's when it is of
/// a mod version not older than the built-in one (or goes back on purpose),
/// else the built-in one. A list going back gives only its own jars.
pub fn choose(
    table: &[Jar],
    listed: Option<&list::List>,
    game: &str,
    loader: Option<LoaderKind>,
) -> Option<Pick> {
    let builtin = || find_in(table, game, loader).map(Pick::of);
    let Some(list) = listed else {
        return builtin();
    };
    let newer = match (
        semver::Version::parse(&list.version),
        semver::Version::parse(MOD_VERSION),
    ) {
        (Ok(listed), Ok(built_in)) => listed >= built_in,
        _ => false,
    };
    if !newer && !list.rollback {
        return builtin();
    }
    let target = Target::for_game(game, loader)?;
    let from_list = list.find(game, target).map(|jar| Pick {
        file: jar.file.clone(),
        sha256: jar.sha256.clone(),
        size: jar.size,
        url: list.url(jar),
    });
    if list.rollback {
        from_list
    } else {
        from_list.or_else(builtin)
    }
}

/// One jar of the release.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Jar {
    /// The exact game versions it is for, as in [`BUILDS`].
    pub games: &'static [&'static str],
    pub target: Target,
    /// Its name in the release, in `mods/` and in [`MANAGED_DIR`].
    pub file: &'static str,
    /// Lowercase hex, from the release's SHA256SUMS.
    pub sha256: &'static str,
    pub size: u64,
}

impl Jar {
    /// Its download address.
    pub fn url(&self) -> String {
        format!("{RELEASE}{}", self.file)
    }

    /// Whether the entry is filled in and sane: a SHA-256, a size within
    /// bounds and a name of ours. Placeholders are not.
    pub fn pinned(&self) -> bool {
        self.sha256.len() == Sha256::HEX_LEN
            && self
                .sha256
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            && (1..=MAX_JAR_BYTES).contains(&self.size)
            && is_ours(self.file)
            && self
                .file
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'-'))
    }
}

/// Every jar the launcher may add to a game: one per entry of [`BUILDS`], in
/// the same order, each for the game versions of its build (the Fabric jar
/// also serves Quilt). Game versions match exactly; other versions get nothing.
///
/// `sha256` (lowercase hex) and `size` (bytes) come from the release
/// skins-mod-v0.2.0: its SHA256SUMS and the jars' sizes. A new mod version
/// is a new release and a new table; an entry that is not filled in is never used.
pub const JARS: &[Jar] = &[
    Jar {
        games: BAND_26,
        target: Target::Fabric,
        file: "launcherskins-fabric-26.1-26.3-0.6.3.jar",
        sha256: "8eb20251e6a0de49f0b8fd099ef08872a95724aa6f9adaace8a8137f1784f883",
        size: 33528,
    },
    Jar {
        games: BAND_26,
        target: Target::NeoForge,
        file: "launcherskins-neoforge-26.1-26.3-0.6.3.jar",
        sha256: "951cff53af653cbc326f133d780452a9a78a2eb1cab5acdeb414a730cf5418d0",
        size: 32619,
    },
    Jar {
        games: BAND_26,
        target: Target::Forge,
        file: "launcherskins-forge-26.1-26.3-0.6.3.jar",
        sha256: "2a7c718e75e7509947e14907934de9efde52846ea1e4477b5e8a057f751db82a",
        size: 32968,
    },
    Jar {
        games: BAND_26,
        target: Target::Agent,
        file: "launcherskins-agent-26.1-26.3-0.6.3.jar",
        sha256: "c531b62523a4a79f10e6d4ce41308b0e71c756800150c39ee8a454632324a229",
        size: 34793,
    },
    Jar {
        games: &["1.21.11"],
        target: Target::Fabric,
        file: "launcherskins-fabric-1.21.11-0.6.3.jar",
        sha256: "20ee412c0c51d547ea6adf352e2d5ed7feed71e914cc141d7a2ad1694d190164",
        size: 33963,
    },
    Jar {
        games: &["1.21.11"],
        target: Target::NeoForge,
        file: "launcherskins-neoforge-1.21.11-0.6.3.jar",
        sha256: "bd5ece0f97e5e8ccfb615b015c0ee6f5dcdee6384f275ddb52125f68d05fa2d4",
        size: 32625,
    },
    Jar {
        games: &["1.21.11"],
        target: Target::Forge,
        file: "launcherskins-forge-1.21.11-0.6.3.jar",
        sha256: "9102e14e056cb6f3425f0b6ed6084eaff0b8d40829f00dbc6a0bfcf80eee1866",
        size: 32968,
    },
    Jar {
        games: &["1.21.11"],
        target: Target::Agent,
        file: "launcherskins-agent-1.21.11-0.6.3.jar",
        sha256: "2efbab985a05d18c6ac7a63ec0cb438ab385e614b2dea98c80f9738e20674d98",
        size: 221775,
    },
    Jar {
        games: BAND_1_21_9,
        target: Target::Fabric,
        file: "launcherskins-fabric-1.21.9-1.21.10-0.6.3.jar",
        sha256: "8111f7b80f80d0bf6e5869d7ce72bd9a770ebbbb6ffd9b5bf63378cf5a2e5753",
        size: 33967,
    },
    Jar {
        games: BAND_1_21_9,
        target: Target::NeoForge,
        file: "launcherskins-neoforge-1.21.9-1.21.10-0.6.3.jar",
        sha256: "954f3933061618af8db250e350ceff5a5e48ef33eeb809bbcfbf3ff09d77fa20",
        size: 32634,
    },
    Jar {
        games: BAND_1_21_9,
        target: Target::Forge,
        file: "launcherskins-forge-1.21.9-1.21.10-0.6.3.jar",
        sha256: "b7793f06b68b8c50d2511b34f67b5863d52219030ae29dadf20dbad6415317af",
        size: 32974,
    },
    Jar {
        games: &["1.21.10"],
        target: Target::Agent,
        file: "launcherskins-agent-1.21.10-0.6.3.jar",
        sha256: "c9ee39dc64b6a18c97b3638fef78a5da4b2e8c19b9a32bf240dd136ae81d83bd",
        size: 221766,
    },
    Jar {
        games: &["1.21.9"],
        target: Target::Agent,
        file: "launcherskins-agent-1.21.9-0.6.3.jar",
        sha256: "cd4afb0b2cde6494a91099789597aa252d65eb5c0c9825cf1fc7ea0fba71b2ec",
        size: 221766,
    },
    Jar {
        games: BAND_1_21_5,
        target: Target::Fabric,
        file: "launcherskins-fabric-1.21.5-1.21.8-0.6.3.jar",
        sha256: "28f4d760079a87f603a0c9fbeb5ce3cd89311fd2882f32139caac475cd28bf66",
        size: 33947,
    },
    Jar {
        games: BAND_1_21_5,
        target: Target::NeoForge,
        file: "launcherskins-neoforge-1.21.5-1.21.8-0.6.3.jar",
        sha256: "76745041eb7e00b9c4ffd31d83308fa64e7a3d37607f4a43ff94402e17e262d0",
        size: 32582,
    },
    Jar {
        games: BAND_1_21_5,
        target: Target::Forge,
        file: "launcherskins-forge-1.21.5-1.21.8-0.6.3.jar",
        sha256: "3f6f6caefa73ef3ca13b7cbf8d76f391797f61557354be3c3da7f535df6ef375",
        size: 32922,
    },
    Jar {
        games: &["1.21.8"],
        target: Target::Agent,
        file: "launcherskins-agent-1.21.8-0.6.3.jar",
        sha256: "904c3b215c145fc330f70c5d0cb854627ffe9e3e8c2d8e7b9ca9a727587953f6",
        size: 221749,
    },
    Jar {
        games: &["1.21.7"],
        target: Target::Agent,
        file: "launcherskins-agent-1.21.7-0.6.3.jar",
        sha256: "4596041e7889016f9604dad3535855f1224305c1c031f31a940361fa78c2149a",
        size: 221753,
    },
    Jar {
        games: &["1.21.6"],
        target: Target::Agent,
        file: "launcherskins-agent-1.21.6-0.6.3.jar",
        sha256: "b55c30b63e2e1ad621af4dea490ede1c5984612e20374e546f15d452a6d34e0a",
        size: 221756,
    },
    Jar {
        games: &["1.21.5"],
        target: Target::Agent,
        file: "launcherskins-agent-1.21.5-0.6.3.jar",
        sha256: "cadbe15c9fd19c662cf48c5790c5da69e7b8692b8ca95e79366ba2e7d5c87b1b",
        size: 221752,
    },
    Jar {
        games: BAND_1_21_2,
        target: Target::Fabric,
        file: "launcherskins-fabric-1.21.2-1.21.4-0.6.3.jar",
        sha256: "e0edf912da0480c55ece358a388bdc9383de99b64dd1f41be7f06541aa1ccc47",
        size: 33843,
    },
    Jar {
        games: BAND_1_21_2,
        target: Target::NeoForge,
        file: "launcherskins-neoforge-1.21.2-1.21.4-0.6.3.jar",
        sha256: "6c15641d817fc8c6e2d4ecd4371c6d81a321aa20bf86851624ab191fa41a4672",
        size: 32466,
    },
    Jar {
        games: BAND_1_21_2,
        target: Target::Forge,
        file: "launcherskins-forge-1.21.2-1.21.4-0.6.3.jar",
        sha256: "ca510bb4e2df665fb7f1364ae30d9e3e69bdc1b99e65fbf1206003973e5ca250",
        size: 32810,
    },
    Jar {
        games: &["1.21.4"],
        target: Target::Agent,
        file: "launcherskins-agent-1.21.4-0.6.3.jar",
        sha256: "7103db1b3442b1740e8d80b35ff1663f459971bc46a625c9e5737dc79cfad230",
        size: 221648,
    },
    Jar {
        games: &["1.21.3"],
        target: Target::Agent,
        file: "launcherskins-agent-1.21.3-0.6.3.jar",
        sha256: "6b025baba99ea56bdf85f3b454314dd0673abbdb0988a9cc605f2029f76d1e1f",
        size: 221649,
    },
    Jar {
        games: &["1.21.2"],
        target: Target::Agent,
        file: "launcherskins-agent-1.21.2-0.6.3.jar",
        sha256: "f9df821407afdd861c9bbc2e39826fba2f3a71ff6f9d273988b632174e5f3858",
        size: 221651,
    },
    Jar {
        games: BAND_1_20_2_FABRIC,
        target: Target::Fabric,
        file: "launcherskins-fabric-1.20.2-1.21.1-0.6.3.jar",
        sha256: "cdc8fe086375a7e1573146b2058edf13f6cee3f6a7b1a9552c961f943dc26d3b",
        size: 33927,
    },
    Jar {
        games: BAND_1_20_6_NEOFORGE,
        target: Target::NeoForge,
        file: "launcherskins-neoforge-1.20.6-1.21.1-0.6.3.jar",
        sha256: "bff11c7af9aa28b227398a8981b80f729b3a34f61f613745c93194142273d643",
        size: 32560,
    },
    Jar {
        games: &["1.21.1"],
        target: Target::Forge,
        file: "launcherskins-forge-1.21.1-0.6.3.jar",
        sha256: "9cfdbf4eeefb5c2387d0db5aa7599ddc7e7f351156d19ac6cb83646077d6833f",
        size: 32900,
    },
    Jar {
        games: &["1.21.1"],
        target: Target::Agent,
        file: "launcherskins-agent-1.21.1-0.6.3.jar",
        sha256: "46c28dfeba6b239aacb17c7f8f7b2228ebad9181a8943eb00f8ae87f53e3e7b9",
        size: 221744,
    },
    Jar {
        games: &["1.21"],
        target: Target::Agent,
        file: "launcherskins-agent-1.21-0.6.3.jar",
        sha256: "420d1c879ee46bbae8169e8bcb7e82c18c6d3416d949760638c9c68edffde922",
        size: 221743,
    },
    Jar {
        games: &["1.20.6"],
        target: Target::Forge,
        file: "launcherskins-forge-1.20.6-0.6.3.jar",
        sha256: "12f01dac0144fd931c819b37fe70a3508cffb27529c1fc2a2bf120fb395f2af8",
        size: 33828,
    },
    Jar {
        games: &["1.20.6"],
        target: Target::Agent,
        file: "launcherskins-agent-1.20.6-0.6.3.jar",
        sha256: "0b0abfc4b41801e10885923a18d4debb16c77f396423986656e4bb8a5c794bed",
        size: 221752,
    },
    Jar {
        games: &["1.20.5"],
        target: Target::Agent,
        file: "launcherskins-agent-1.20.5-0.6.3.jar",
        sha256: "83687482fecc9c98c3bc868a4d5372a4431d69892ff8a0830fcdf1f7bd768a8f",
        size: 221742,
    },
    Jar {
        games: BAND_1_20_2,
        target: Target::NeoForge,
        file: "launcherskins-neoforge-1.20.2-1.20.4-0.6.3.jar",
        sha256: "902765f1058b8a58687b58ea268427c5d1152764c967cb3ea93bba117b7b11c8",
        size: 33883,
    },
    Jar {
        games: BAND_1_20_2,
        target: Target::Forge,
        file: "launcherskins-forge-1.20.2-1.20.4-0.6.3.jar",
        sha256: "960d091bbbe463d078c1e19d66e7f855eb7cc83d20727a77c31965fed57202b0",
        size: 33857,
    },
    Jar {
        games: &["1.20.4"],
        target: Target::Agent,
        file: "launcherskins-agent-1.20.4-0.6.3.jar",
        sha256: "69d9276a3b418ebf47e0a26319073228f3f96433d810fbb3cabefe35a552eb43",
        size: 221738,
    },
    Jar {
        games: &["1.20.3"],
        target: Target::Agent,
        file: "launcherskins-agent-1.20.3-0.6.3.jar",
        sha256: "1a16d507220b2f246d86f05dab1ca7fb16629f6042463b9c7ef22c8701214261",
        size: 221736,
    },
    Jar {
        games: &["1.20.2"],
        target: Target::Agent,
        file: "launcherskins-agent-1.20.2-0.6.3.jar",
        sha256: "8887c3368b6cc078dd5f81d27be1a2e6a9a353b8bcb19bba09c7e73b6a8501b3",
        size: 221751,
    },
    Jar {
        games: BAND_1_15_2_FABRIC,
        target: Target::Fabric,
        file: "launcherskins-fabric-1.15.2-1.20.1-0.6.3.jar",
        sha256: "e7c82645c5338d51499b6e5c6b4c6bc4d7eb88263282a5496fb460feeaff534d",
        size: 34091,
    },
    Jar {
        games: BAND_1_17_FORGE,
        target: Target::Forge,
        file: "launcherskins-forge-1.17.1-1.20.1-0.6.3.jar",
        sha256: "b2a07bc0f7af20e667f8a1d3c10c92e4087714326a28f2c0c11d20bc5202530b",
        size: 33892,
    },
    Jar {
        games: &["1.16.5"],
        target: Target::Forge,
        file: "launcherskins-forge-1.16.5-0.6.3.jar",
        sha256: "a4692a41d30d4da0352927bb128a6874948e4f7137da1f2db49a23444c64f6f0",
        size: 34058,
    },
    Jar {
        games: &["1.20.1"],
        target: Target::Agent,
        file: "launcherskins-agent-1.20.1-0.6.3.jar",
        sha256: "b689f2b9d1218b07d0ff5afb9d5c5222d68a759a6c42dcc88a75b61da927e17c",
        size: 221841,
    },
    Jar {
        games: &["1.20"],
        target: Target::Agent,
        file: "launcherskins-agent-1.20-0.6.3.jar",
        sha256: "74263cf06c2dc6c2fb323f71060da2ead81c55f508d0a3fe888de59a3eb83fc3",
        size: 221840,
    },
    Jar {
        games: &["1.19.4"],
        target: Target::Agent,
        file: "launcherskins-agent-1.19.4-0.6.3.jar",
        sha256: "9d93c6ba6fb891f4bd92ef7729876317784e0232493f61c72d1ae21e6c4f58b3",
        size: 221837,
    },
    Jar {
        games: &["1.19.3"],
        target: Target::Agent,
        file: "launcherskins-agent-1.19.3-0.6.3.jar",
        sha256: "109f636a8e7307461c30455a463ed50c955efb56e03e7070122e0b23080a0cf0",
        size: 221840,
    },
    Jar {
        games: &["1.19.2"],
        target: Target::Agent,
        file: "launcherskins-agent-1.19.2-0.6.3.jar",
        sha256: "e430c04faf549fe56b035eb6c704ce84f480b62d845058980756591f84e4378b",
        size: 221837,
    },
    Jar {
        games: &["1.19.1"],
        target: Target::Agent,
        file: "launcherskins-agent-1.19.1-0.6.3.jar",
        sha256: "8de038a1bd999c1cc34ed361266a036f50dd9a43d6c76b7afff0adb01ddefeb2",
        size: 221835,
    },
    Jar {
        games: &["1.19"],
        target: Target::Agent,
        file: "launcherskins-agent-1.19-0.6.3.jar",
        sha256: "d0611495c32028d8960701feb70e66343839b929ede6ea15636ab666a0ab08f7",
        size: 221838,
    },
    Jar {
        games: &["1.18.2"],
        target: Target::Agent,
        file: "launcherskins-agent-1.18.2-0.6.3.jar",
        sha256: "e31d112f1aece05f7eb48b4483e7cb08ebe297b5883f786e9c764c0943821fd4",
        size: 221840,
    },
    Jar {
        games: &["1.18.1"],
        target: Target::Agent,
        file: "launcherskins-agent-1.18.1-0.6.3.jar",
        sha256: "7a832d4313012a1a809290dc669adaa18e2af247da6313f7d925f7d013b14d6e",
        size: 221841,
    },
    Jar {
        games: &["1.18"],
        target: Target::Agent,
        file: "launcherskins-agent-1.18-0.6.3.jar",
        sha256: "b349852fa06e0610ead85309312441568d0bd2e19323cff380591926dfced614",
        size: 221842,
    },
    Jar {
        games: &["1.17.1"],
        target: Target::Agent,
        file: "launcherskins-agent-1.17.1-0.6.3.jar",
        sha256: "359d1406a3d10497ac1b5fa30fd6a315844c6dc5cffb0583d67df54de7163049",
        size: 221842,
    },
    Jar {
        games: &["1.17"],
        target: Target::Agent,
        file: "launcherskins-agent-1.17-0.6.3.jar",
        sha256: "9d13bbc35c1a7eeed62234e6bf3affebb8f83440d0bf177db45e06ff6da70347",
        size: 221844,
    },
    Jar {
        games: &["1.16.5"],
        target: Target::Agent,
        file: "launcherskins-agent-1.16.5-0.6.3.jar",
        sha256: "0a551056fa2b15d2a9654f0fc2204346fb9f3aabe02f866d98cc514ef4fa85e3",
        size: 221757,
    },
    Jar {
        games: &["1.16.4"],
        target: Target::Agent,
        file: "launcherskins-agent-1.16.4-0.6.3.jar",
        sha256: "3afcbba09548bc0a7e61020c01d2645706beaef858bafe65eed89f8a7b87b6a7",
        size: 221758,
    },
    Jar {
        games: &["1.16.3"],
        target: Target::Agent,
        file: "launcherskins-agent-1.16.3-0.6.3.jar",
        sha256: "76d62b821032d0100696fc4410274c32f7c167ec114959c3fcddfc793b5ddf01",
        size: 221753,
    },
    Jar {
        games: &["1.16.2"],
        target: Target::Agent,
        file: "launcherskins-agent-1.16.2-0.6.3.jar",
        sha256: "660d4ec2bb27ac16bcf5d33afa63312982228bdd2e51904d2928713c9adc19bb",
        size: 221756,
    },
    Jar {
        games: &["1.16.1"],
        target: Target::Agent,
        file: "launcherskins-agent-1.16.1-0.6.3.jar",
        sha256: "e0a8b90c787b8de1c01c381673048db04a2610c2c6df6481445a58916924086f",
        size: 221753,
    },
    Jar {
        games: &["1.16"],
        target: Target::Agent,
        file: "launcherskins-agent-1.16-0.6.3.jar",
        sha256: "1cd24be51377a2514565b65f9279a9fe4823f7d179cb878fec1b23161602482e",
        size: 221753,
    },
    Jar {
        games: &["1.15.2"],
        target: Target::Forge,
        file: "launcherskins-forge-1.15.2-0.6.3.jar",
        sha256: "070b139082270c038e99eb621ff5a8ddbf02c79580542a40724d32cb2a335147",
        size: 33848,
    },
    Jar {
        games: &["1.15.2"],
        target: Target::Agent,
        file: "launcherskins-agent-1.15.2-0.6.3.jar",
        sha256: "8eec3a44b38dd7e1985c1a570b7f97de378631ed3cd2f89df4d627098a1b9212",
        size: 221751,
    },
    Jar {
        games: &["1.15", "1.15.1"],
        target: Target::Fabric,
        file: "launcherskins-fabric-1.15-1.15.1-0.6.3.jar",
        sha256: "4b87cc5db4b051b694056bb997abfc75aa0fd96f483e2113b6b9fc9b5f1e9f5f",
        size: 34088,
    },
    Jar {
        games: &["1.15.1"],
        target: Target::Agent,
        file: "launcherskins-agent-1.15.1-0.6.3.jar",
        sha256: "459110a740f7faccef88bda895be94d74ebedc835031c779f6e0fa19b4490465",
        size: 221751,
    },
    Jar {
        games: &["1.15"],
        target: Target::Agent,
        file: "launcherskins-agent-1.15-0.6.3.jar",
        sha256: "f099660e918d8016c1450b015af99777e5c82c2cde1e031fa11464be04349541",
        size: 221752,
    },
    Jar {
        games: &["1.14.4"],
        target: Target::Fabric,
        file: "launcherskins-fabric-1.14.4-0.6.3.jar",
        sha256: "b4cdd9d34f04280930cd6b92b69a65d91803945f36c33ee99454e2a7b8855a19",
        size: 34085,
    },
    Jar {
        games: &["1.14.4"],
        target: Target::Forge,
        file: "launcherskins-forge-1.14.4-0.6.3.jar",
        sha256: "f94fea0467052d964d30abbca41f103f00ea2e6179327f13a04e479d8d436802",
        size: 33844,
    },
    Jar {
        games: &["1.14.4"],
        target: Target::Agent,
        file: "launcherskins-agent-1.14.4-0.6.3.jar",
        sha256: "edff6f8fe4993e294090976315a58db3845c53d4f0f2fbbf54443903b05e3149",
        size: 221760,
    },
    Jar {
        games: &["1.12.2"],
        target: Target::Forge,
        file: "launcherskins-forge-1.12.2-0.6.3.jar",
        sha256: "450976ca73cc0fbe331542a8760991a3263de91bd5aa6ea40b00258e37a015f9",
        size: 34566,
    },
    Jar {
        games: &["1.8.9"],
        target: Target::Forge,
        file: "launcherskins-forge-1.8.9-0.6.3.jar",
        sha256: "7b394fc6e0d4fb7c9ace0969fc65506afb454b6b8d574a327111746138421344",
        size: 34098,
    },
    Jar {
        games: &["1.7.10"],
        target: Target::Forge,
        file: "launcherskins-forge-1.7.10-0.6.3.jar",
        sha256: "66903eb2b84af0ea070008c667c3c733ad7988beeaf02c465e50c0d701d7a4ae",
        size: 34559,
    },
];

/// The jar for this game version and loader, if there is a pinned one.
pub fn find(game: &str, loader: Option<LoaderKind>) -> Option<&'static Jar> {
    find_in(JARS, game, loader)
}

fn find_in<'a>(table: &'a [Jar], game: &str, loader: Option<LoaderKind>) -> Option<&'a Jar> {
    let target = Target::for_game(game, loader)?;
    table
        .iter()
        .find(|j| j.games.contains(&game) && j.target == target && j.pinned())
}

/// `launcherskins-*.jar`, in any case (Windows names are case-insensitive).
pub fn is_ours(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.len() > PREFIX.len() + JAR_SUFFIX.len()
        && lower.starts_with(PREFIX)
        && lower.ends_with(JAR_SUFFIX)
}

/// A temporary file an interrupted write of ours left (see `storage::atomic`).
fn is_our_temp(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.starts_with(".launcherskins-") && lower.contains(".jar.tmp-")
}

/// The API address a test build passes to the mod
/// (`LAUNCHER_SKINS_URL`, as in `skins::base`). A release build passes none
/// and the mod uses its own default, `https://skins.lapi.sh`.
pub fn mod_api() -> Option<&'static str> {
    option_env!("LAUNCHER_SKINS_URL").and_then(api_value)
}

/// An http(s) URL of printable ASCII, so it is one JVM argument.
fn api_value(raw: &str) -> Option<&str> {
    let raw = raw.trim();
    let url = url::Url::parse(raw).ok()?;
    (matches!(url.scheme(), "https" | "http")
        && url.host_str().is_some()
        && raw.len() <= 256
        && raw.bytes().all(|b| b.is_ascii_graphic()))
    .then_some(raw)
}

/// The file at `path`, if it is a regular file with the jar's size and hash.
/// The bytes returned are the ones checked.
fn read_verified(path: &Path, jar: &Pick) -> Option<Vec<u8>> {
    let metadata = std::fs::symlink_metadata(path).ok()?;
    if !metadata.is_file() || metadata.len() != jar.size {
        return None;
    }
    let mut bytes = Vec::with_capacity(usize::try_from(jar.size).ok()?);
    std::fs::File::open(path)
        .ok()?
        .take(jar.size + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    let expected = Sha256::parse(&jar.sha256).ok()?;
    (bytes.len() as u64 == jar.size
        && hex::encode(Sha256Hasher::digest(&bytes)) == expected.as_str())
    .then_some(bytes)
}

/// The jar's verified bytes from [`MANAGED_DIR`], downloading it first when
/// it is missing or damaged and a transport is given.
async fn managed_bytes(
    root: &DataRoot,
    transport: Option<&HttpTransport>,
    jar: &Pick,
    cancel: &CancelToken,
) -> Option<Vec<u8>> {
    let path = resolve_managed_path(root, &jar.managed()).ok()?;
    let check = |path: PathBuf| {
        let jar = jar.clone();
        async move {
            tokio::task::spawn_blocking(move || read_verified(&path, &jar))
                .await
                .ok()
                .flatten()
        }
    };
    if let Some(bytes) = check(path.clone()).await {
        return Some(bytes);
    }
    let transport = transport?;
    let request = DownloadRequest {
        source: SourcePolicy::SkinsMod,
        url: SourcePolicy::SkinsMod.check(&jar.url).ok()?,
        path: jar.managed(),
        size: jar.size,
        digest: Digest::Sha256(Sha256::parse(&jar.sha256).ok()?),
        label: Some(jar.file.clone()),
    };
    let executor = Executor::new(transport.clone(), root.clone(), ExecutorConfig::default());
    let job = Uuid::new_v4();
    let run = executor.run(job, vec![request], cancel, &NoProgress);
    if tokio::time::timeout(DOWNLOAD_TIMEOUT, run).await.is_err() {
        // Cut off mid-run: its staging folder is not cleaned up by the run.
        let _ = tokio::fs::remove_dir_all(root.download_staging(job)).await;
        return None;
    }
    // Checked again: what the game gets is what was verified.
    check(path).await
}

/// Makes `mods` hold exactly `keep` of the launcher's jars: our other jars
/// (old versions, other loaders) and our stale temporary files are removed,
/// `keep` is written atomically unless the same file is there already.
/// Directories and every file not named like ours are left alone; `mods` is
/// created only to hold `keep`.
fn sync_mods(mods: &Path, keep: Option<(&Pick, &[u8])>) -> std::io::Result<()> {
    if keep.is_some() {
        std::fs::create_dir_all(mods)?;
    }
    let entries = match std::fs::read_dir(mods) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound && keep.is_none() => return Ok(()),
        Err(e) => return Err(e),
    };
    let keep_name = keep.map(|(jar, _)| jar.file.as_str());
    let mut failed = None;
    for entry in entries {
        let entry = entry?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if Some(name) == keep_name || !(is_ours(name) || is_our_temp(name)) {
            continue;
        }
        // Not followed: a link is removed itself, a directory never.
        let path = entry.path();
        match std::fs::symlink_metadata(&path) {
            Ok(m) if m.is_dir() => continue,
            Ok(_) => {}
            Err(e) => {
                failed.get_or_insert(e);
                continue;
            }
        }
        if let Err(e) = std::fs::remove_file(&path) {
            failed.get_or_insert(e);
        }
    }
    let Some((jar, bytes)) = keep else {
        return failed.map_or(Ok(()), Err);
    };
    let target = mods.join(&jar.file);
    if let Some(error) = failed {
        // Two versions of the mod side by side would stop the loader; better
        // none of ours this time.
        if std::fs::symlink_metadata(&target).is_ok_and(|m| !m.is_dir()) {
            let _ = std::fs::remove_file(&target);
        }
        return Err(error);
    }
    if read_verified(&target, jar).is_some() {
        return Ok(());
    }
    if std::fs::symlink_metadata(&target).is_ok_and(|m| m.file_type().is_symlink()) {
        std::fs::remove_file(&target)?;
    }
    crate::storage::atomic::write(&target, bytes)
}

/// What the launch needs to know about the installation.
pub struct Setup<'a> {
    pub game_version: &'a str,
    pub loader: Option<LoaderKind>,
    /// The installation's switch.
    pub enabled: bool,
    pub game_dir: &'a Path,
    /// The signed list kept for the player's update channel, checked.
    pub listed: Option<&'a list::List>,
}

/// The skins mod for one launch.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Prepared {
    /// The mod is in `mods/`, or the agent below is passed.
    pub active: bool,
    /// The verified agent jar for `-javaagent` (no loader).
    pub agent: Option<PathBuf>,
    /// A fixed message for the log when the game starts without the mod
    /// although it should have it.
    pub problem: Option<&'static str>,
}

/// Brings the installation's `mods/` and the agent in line with its switch.
/// Never fails: problems end up in [`Prepared::problem`].
pub async fn prepare(
    root: &DataRoot,
    transport: Option<&HttpTransport>,
    setup: &Setup<'_>,
    cancel: &CancelToken,
) -> Prepared {
    prepare_with(JARS, root, transport, setup, cancel).await
}

async fn prepare_with(
    table: &[Jar],
    root: &DataRoot,
    transport: Option<&HttpTransport>,
    setup: &Setup<'_>,
    cancel: &CancelToken,
) -> Prepared {
    let jar = setup
        .enabled
        .then(|| choose(table, setup.listed, setup.game_version, setup.loader))
        .flatten();
    let bytes = match &jar {
        Some(jar) => managed_bytes(root, transport, jar, cancel).await,
        None => None,
    };
    let mut prepared = Prepared::default();
    // A cancelled launch leaves `mods/` as it was.
    if cancel.is_cancelled() {
        return prepared;
    }
    if jar.is_some() && bytes.is_none() {
        prepared.problem =
            Some("Launcher Skins could not be downloaded; the game starts without it");
    }
    // `mods/` holds our jar only with a loader; otherwise ours are removed.
    let keep = match (setup.loader, &jar, bytes.as_ref()) {
        (Some(_), Some(jar), Some(bytes)) => Some((jar.clone(), bytes.clone())),
        _ => None,
    };
    let mods = setup.game_dir.join("mods");
    let wanted = keep.is_some();
    let synced = tokio::task::spawn_blocking(move || {
        sync_mods(
            &mods,
            keep.as_ref().map(|(jar, bytes)| (jar, bytes.as_slice())),
        )
    })
    .await;
    match synced {
        Ok(Ok(())) => prepared.active = wanted,
        _ => {
            prepared.problem = Some(if wanted {
                "Launcher Skins could not be put into the mods folder; the game starts without it"
            } else {
                "An old copy of Launcher Skins could not be removed from the mods folder"
            });
        }
    }
    if setup.loader.is_none()
        && let (Some(jar), Some(_)) = (jar, bytes)
    {
        match resolve_managed_path(root, &jar.managed()) {
            // `-javaagent:<jar>=<options>`: a path with `=` would be cut there.
            Ok(path) if path.to_str().is_some_and(|p| !p.contains('=')) => {
                prepared.agent = Some(path);
                prepared.active = true;
            }
            _ => {
                prepared.problem = Some(
                    "Launcher Skins cannot start from a data folder whose path contains '='; \
                     the game starts without it",
                );
            }
        }
    }
    prepared
}

pub mod list;

#[cfg(test)]
mod tests;
