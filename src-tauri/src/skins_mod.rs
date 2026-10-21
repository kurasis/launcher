//! Launcher Skins in the game (D-065, stage 8).
//!
//! When an installation starts, the launcher adds its skins mod: the jar for
//! the game version and loader goes into `<game>/mods/` (Fabric, Quilt,
//! NeoForge, Forge), or the Java agent is passed with `-javaagent` (no loader).
//! The jars come from this repository's GitHub release `skins-mod-v<version>`
//! and are pinned by SHA-256 and size: here in [`JARS`], or in the signed list
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
pub const MOD_VERSION: &str = "0.6.1";
/// Where the release keeps the jars.
const RELEASE: &str = "https://github.com/kurasis/launcher/releases/download/skins-mod-v0.6.1/";
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
    /// Its download address in the release.
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
        file: "launcherskins-fabric-26.1-26.3-0.6.1.jar",
        sha256: "43568a0e3a623233a7753b65f9a872844248662c7a687c52605316780d629f70",
        size: 33529,
    },
    Jar {
        games: BAND_26,
        target: Target::NeoForge,
        file: "launcherskins-neoforge-26.1-26.3-0.6.1.jar",
        sha256: "1a0a412c64b99e662667a2a39df84678d7be1e9ff218a4b0f600147ef8f29bb6",
        size: 32618,
    },
    Jar {
        games: BAND_26,
        target: Target::Forge,
        file: "launcherskins-forge-26.1-26.3-0.6.1.jar",
        sha256: "a74659588e9f1ccab246dcbd2e88b64ed9eaec6bbfce11cf62b2103676c3a379",
        size: 32967,
    },
    Jar {
        games: BAND_26,
        target: Target::Agent,
        file: "launcherskins-agent-26.1-26.3-0.6.1.jar",
        sha256: "9c46f223f06ed69f5aca54ef8728bf7ddd44c9b6428488527ee6b61ae470bcd6",
        size: 34794,
    },
    Jar {
        games: &["1.21.11"],
        target: Target::Fabric,
        file: "launcherskins-fabric-1.21.11-0.6.1.jar",
        sha256: "2d6291b4de8e640643f009592114559cbc35a347fed5e37acb704e059c18e800",
        size: 33962,
    },
    Jar {
        games: &["1.21.11"],
        target: Target::NeoForge,
        file: "launcherskins-neoforge-1.21.11-0.6.1.jar",
        sha256: "8486e2234e6330bf961af3dd3485586cf66720e5cd9d27361da2976527ebcd8e",
        size: 32625,
    },
    Jar {
        games: &["1.21.11"],
        target: Target::Forge,
        file: "launcherskins-forge-1.21.11-0.6.1.jar",
        sha256: "d4c40900cfb59968e7c6b29c68727ff00691d127ecd7aa6312b3b4787a0285e0",
        size: 32968,
    },
    Jar {
        games: &["1.21.11"],
        target: Target::Agent,
        file: "launcherskins-agent-1.21.11-0.6.1.jar",
        sha256: "075b36b60a1c6170a1d737445cdd061ebaf48d1eeadc5592d7aae518222f3514",
        size: 221775,
    },
    Jar {
        games: BAND_1_21_9,
        target: Target::Fabric,
        file: "launcherskins-fabric-1.21.9-1.21.10-0.6.1.jar",
        sha256: "106bbeba3a3f191cbefa443d605235fa7b0612e0e1dd5091b29f719432532a4c",
        size: 33966,
    },
    Jar {
        games: BAND_1_21_9,
        target: Target::NeoForge,
        file: "launcherskins-neoforge-1.21.9-1.21.10-0.6.1.jar",
        sha256: "b58bd306d2474394a8c9ac29a54a955cd965bad85e29f33118910d1e6913c287",
        size: 32633,
    },
    Jar {
        games: BAND_1_21_9,
        target: Target::Forge,
        file: "launcherskins-forge-1.21.9-1.21.10-0.6.1.jar",
        sha256: "a25ddd9405e7db82ec527098076c4386102da90999e2bb5bb442fe240892e180",
        size: 32973,
    },
    Jar {
        games: &["1.21.10"],
        target: Target::Agent,
        file: "launcherskins-agent-1.21.10-0.6.1.jar",
        sha256: "929e842e00659469e27e3aa8b8e6a310038a06d82119d036f0570c21ff00379b",
        size: 221766,
    },
    Jar {
        games: &["1.21.9"],
        target: Target::Agent,
        file: "launcherskins-agent-1.21.9-0.6.1.jar",
        sha256: "47e4d9b680c1153ba9acd8733f8fab4a8a0872460102b6f9fb399dcf05f55e86",
        size: 221767,
    },
    Jar {
        games: BAND_1_21_5,
        target: Target::Fabric,
        file: "launcherskins-fabric-1.21.5-1.21.8-0.6.1.jar",
        sha256: "1787abf15dc4244d45e0e0283908b651fb33ac183fff1f52a562c6e851639fea",
        size: 33946,
    },
    Jar {
        games: BAND_1_21_5,
        target: Target::NeoForge,
        file: "launcherskins-neoforge-1.21.5-1.21.8-0.6.1.jar",
        sha256: "1cb87023cc0860e4a95690e0ee40cabd74daf9ec2748993e0cb18901e33cb94d",
        size: 32581,
    },
    Jar {
        games: BAND_1_21_5,
        target: Target::Forge,
        file: "launcherskins-forge-1.21.5-1.21.8-0.6.1.jar",
        sha256: "f2bf867b5315c5601f66534e94c047ed2266f1993a8eaa2000db35fee8091724",
        size: 32922,
    },
    Jar {
        games: &["1.21.8"],
        target: Target::Agent,
        file: "launcherskins-agent-1.21.8-0.6.1.jar",
        sha256: "560fdbcc99a560ecd367105764beeb22c18ccc5b9ee266989bda340c1abfb5af",
        size: 221749,
    },
    Jar {
        games: &["1.21.7"],
        target: Target::Agent,
        file: "launcherskins-agent-1.21.7-0.6.1.jar",
        sha256: "2c6014100d3941f639e4260ae7468568a56d273a65d70ea52c0238193c6ed75a",
        size: 221753,
    },
    Jar {
        games: &["1.21.6"],
        target: Target::Agent,
        file: "launcherskins-agent-1.21.6-0.6.1.jar",
        sha256: "bc53e68d746e35e11b1fa49509e82efad45e3c32605ff79e35bdaa7974621e2d",
        size: 221756,
    },
    Jar {
        games: &["1.21.5"],
        target: Target::Agent,
        file: "launcherskins-agent-1.21.5-0.6.1.jar",
        sha256: "a629e101dfd59d14872aad0dc63f7567b43ac7da14da2d22f3efc2772a2e9b76",
        size: 221752,
    },
    Jar {
        games: BAND_1_21_2,
        target: Target::Fabric,
        file: "launcherskins-fabric-1.21.2-1.21.4-0.6.1.jar",
        sha256: "1d96d3a4d594b9a6c78b21bc3adc5257ef4b239c93a4cafe257262b79663aab5",
        size: 33842,
    },
    Jar {
        games: BAND_1_21_2,
        target: Target::NeoForge,
        file: "launcherskins-neoforge-1.21.2-1.21.4-0.6.1.jar",
        sha256: "1fb4dbbb7c7906bc247eaac7a7c8dc537dada0a7c50ef47b581ea91cbe2c5400",
        size: 32466,
    },
    Jar {
        games: BAND_1_21_2,
        target: Target::Forge,
        file: "launcherskins-forge-1.21.2-1.21.4-0.6.1.jar",
        sha256: "1fb418f892376b5e94ec44f550d5867d582a6333575a2d860d1ca955c026f63f",
        size: 32810,
    },
    Jar {
        games: &["1.21.4"],
        target: Target::Agent,
        file: "launcherskins-agent-1.21.4-0.6.1.jar",
        sha256: "53f5591a8b585ef3c3507a1e19da326379d2109319b6e26d833e6826a496d8db",
        size: 221648,
    },
    Jar {
        games: &["1.21.3"],
        target: Target::Agent,
        file: "launcherskins-agent-1.21.3-0.6.1.jar",
        sha256: "b095f075948e59d3214dfd7e684ab6168f3bfa97c4d4768d2eca73e5e4cf8d32",
        size: 221651,
    },
    Jar {
        games: &["1.21.2"],
        target: Target::Agent,
        file: "launcherskins-agent-1.21.2-0.6.1.jar",
        sha256: "ef97d472e08be094f62ae7248cfbda04d38cecbd2649b1f386e957cd49e0f017",
        size: 221650,
    },
    Jar {
        games: BAND_1_20_2_FABRIC,
        target: Target::Fabric,
        file: "launcherskins-fabric-1.20.2-1.21.1-0.6.1.jar",
        sha256: "ad297108e58c3d717cb754f904b700583c1de3293b298603a75e506165d4d400",
        size: 33928,
    },
    Jar {
        games: BAND_1_20_6_NEOFORGE,
        target: Target::NeoForge,
        file: "launcherskins-neoforge-1.20.6-1.21.1-0.6.1.jar",
        sha256: "48c6e99e4f53ee14b7db00b50ffd1c70e472707db2e6860e6f2941bcc1814922",
        size: 32560,
    },
    Jar {
        games: &["1.21.1"],
        target: Target::Forge,
        file: "launcherskins-forge-1.21.1-0.6.1.jar",
        sha256: "806898648736d4cc95c9db3820f5e2077f6e523505236e754493ce30edc379ec",
        size: 32899,
    },
    Jar {
        games: &["1.21.1"],
        target: Target::Agent,
        file: "launcherskins-agent-1.21.1-0.6.1.jar",
        sha256: "04dbd7c23389bc5461247adc1fde9777c5615d14b4fe864789ad16eee9e5e101",
        size: 221744,
    },
    Jar {
        games: &["1.21"],
        target: Target::Agent,
        file: "launcherskins-agent-1.21-0.6.1.jar",
        sha256: "f9274f7fea0d3c5e7abdf23b8f7d9d71bcc786096dca6730f592cfeddff4e482",
        size: 221744,
    },
    Jar {
        games: &["1.20.6"],
        target: Target::Forge,
        file: "launcherskins-forge-1.20.6-0.6.1.jar",
        sha256: "a2db7dee29be504dcb938b22e987aed6720b9d3e675de1978b2b55a999bd817c",
        size: 33828,
    },
    Jar {
        games: &["1.20.6"],
        target: Target::Agent,
        file: "launcherskins-agent-1.20.6-0.6.1.jar",
        sha256: "b119419776b8a736e327f4f44ee88404a3820f3a65c925ee0057d632a761aa58",
        size: 221752,
    },
    Jar {
        games: &["1.20.5"],
        target: Target::Agent,
        file: "launcherskins-agent-1.20.5-0.6.1.jar",
        sha256: "dc0fdad80e97ec79bae0a70e0d17cac2c77cbc830cc98f6cb177ef5f72107fbd",
        size: 221742,
    },
    Jar {
        games: BAND_1_20_2,
        target: Target::NeoForge,
        file: "launcherskins-neoforge-1.20.2-1.20.4-0.6.1.jar",
        sha256: "70bb1bfafbb8267009db008735eef714a8045025d74bb112443c1a3c817e468b",
        size: 33790,
    },
    Jar {
        games: BAND_1_20_2,
        target: Target::Forge,
        file: "launcherskins-forge-1.20.2-1.20.4-0.6.1.jar",
        sha256: "ea261ab17277deb5327d20e01a457a6bc3aca444f2f412a353c4e9fde37a428c",
        size: 33857,
    },
    Jar {
        games: &["1.20.4"],
        target: Target::Agent,
        file: "launcherskins-agent-1.20.4-0.6.1.jar",
        sha256: "ff9633ba31d181cf4205985a06601159dc75464177359e87b44d00c14d867301",
        size: 221738,
    },
    Jar {
        games: &["1.20.3"],
        target: Target::Agent,
        file: "launcherskins-agent-1.20.3-0.6.1.jar",
        sha256: "3d31b5b7f636f8de851741fe01510ef9246fb19471b37fb99d765134a0d3321d",
        size: 221738,
    },
    Jar {
        games: &["1.20.2"],
        target: Target::Agent,
        file: "launcherskins-agent-1.20.2-0.6.1.jar",
        sha256: "19fd89987289f6b3949f70681786736449eb04528e8f758b5e3b1dfcfc52d945",
        size: 221751,
    },
    Jar {
        games: BAND_1_15_2_FABRIC,
        target: Target::Fabric,
        file: "launcherskins-fabric-1.15.2-1.20.1-0.6.1.jar",
        sha256: "6b9bc45b1fe3d7ff66c6ab7a2b0d17b56642b3b55b74558d12948684b9dd24f7",
        size: 34092,
    },
    Jar {
        games: BAND_1_17_FORGE,
        target: Target::Forge,
        file: "launcherskins-forge-1.17.1-1.20.1-0.6.1.jar",
        sha256: "019c76f41cbd973d28fe958e20ec313ec52762dcc0d0ec4de0df8eb4fdfb95e4",
        size: 33892,
    },
    Jar {
        games: &["1.16.5"],
        target: Target::Forge,
        file: "launcherskins-forge-1.16.5-0.6.1.jar",
        sha256: "c4e12dd4b6f5a907331bbc455b1e4c9674ad6f6673908ffd1ef0589091353219",
        size: 34058,
    },
    Jar {
        games: &["1.20.1"],
        target: Target::Agent,
        file: "launcherskins-agent-1.20.1-0.6.1.jar",
        sha256: "e7fba8c4d3dcbb6e2c1765b52375c349fa3f6afbca887604fa7adb4f30446639",
        size: 221841,
    },
    Jar {
        games: &["1.20"],
        target: Target::Agent,
        file: "launcherskins-agent-1.20-0.6.1.jar",
        sha256: "b6c7d02d3c6384bda425fac8bafd62483ede1fca265a3ae0c02d73d25e86f685",
        size: 221841,
    },
    Jar {
        games: &["1.19.4"],
        target: Target::Agent,
        file: "launcherskins-agent-1.19.4-0.6.1.jar",
        sha256: "eef64b2e64d683ab793f07c74b369083abec1da73ef72e611f873f2ff18e4aa0",
        size: 221837,
    },
    Jar {
        games: &["1.19.3"],
        target: Target::Agent,
        file: "launcherskins-agent-1.19.3-0.6.1.jar",
        sha256: "d2e9fbfbe84697e9780c45575b12e20505ed8fec218b5a477c90e7d775340e84",
        size: 221840,
    },
    Jar {
        games: &["1.19.2"],
        target: Target::Agent,
        file: "launcherskins-agent-1.19.2-0.6.1.jar",
        sha256: "b5ac0dd7fb371173762aa872dc11ba45d1bb9f5d90b0e5c62d17ef18e7e70d51",
        size: 221837,
    },
    Jar {
        games: &["1.19.1"],
        target: Target::Agent,
        file: "launcherskins-agent-1.19.1-0.6.1.jar",
        sha256: "33ec3aae5656e75fcfd4899beaf16bf290637f492ab28afaa923419089fcab67",
        size: 221835,
    },
    Jar {
        games: &["1.19"],
        target: Target::Agent,
        file: "launcherskins-agent-1.19-0.6.1.jar",
        sha256: "b5057876a6c4a88b7c229b02f5cb175825b27e886a4bb2d5be41f56e244df9f6",
        size: 221839,
    },
    Jar {
        games: &["1.18.2"],
        target: Target::Agent,
        file: "launcherskins-agent-1.18.2-0.6.1.jar",
        sha256: "843c20abb2d915007329dcb61562f1de06664960a2f5fe4acb199eebce86ac99",
        size: 221840,
    },
    Jar {
        games: &["1.18.1"],
        target: Target::Agent,
        file: "launcherskins-agent-1.18.1-0.6.1.jar",
        sha256: "e400f4291310e9cf38b63228fa0f298bf795fe0531997d5eac4c47c3a793a021",
        size: 221841,
    },
    Jar {
        games: &["1.18"],
        target: Target::Agent,
        file: "launcherskins-agent-1.18-0.6.1.jar",
        sha256: "d74a304f2f62ae70c21ef1e8202d82bc1dab81f283738a91bac8337b1b2a07a1",
        size: 221842,
    },
    Jar {
        games: &["1.17.1"],
        target: Target::Agent,
        file: "launcherskins-agent-1.17.1-0.6.1.jar",
        sha256: "620c89ad897ffd2363fd56dcead8903086614a745d68d6e637862ae884aeb969",
        size: 221842,
    },
    Jar {
        games: &["1.17"],
        target: Target::Agent,
        file: "launcherskins-agent-1.17-0.6.1.jar",
        sha256: "eceb4c7e03f7fbec1b95d0266d641d77a6fa380b0fe4353a4d5931f6b07b7758",
        size: 221844,
    },
    Jar {
        games: &["1.16.5"],
        target: Target::Agent,
        file: "launcherskins-agent-1.16.5-0.6.1.jar",
        sha256: "bbfe8ff196b6c3965b99f36fa7f79f5c7f2b582d85618a458db1ef2a5840193b",
        size: 221758,
    },
    Jar {
        games: &["1.16.4"],
        target: Target::Agent,
        file: "launcherskins-agent-1.16.4-0.6.1.jar",
        sha256: "7cb77475e441facc80ac77c1d70be7fd38cdff04be7485748ab7dcaa4efed4e7",
        size: 221758,
    },
    Jar {
        games: &["1.16.3"],
        target: Target::Agent,
        file: "launcherskins-agent-1.16.3-0.6.1.jar",
        sha256: "b737822dac52f6ad15dbd3a4b0840bb553c8e01bec423c5a795a19ca6252f443",
        size: 221756,
    },
    Jar {
        games: &["1.16.2"],
        target: Target::Agent,
        file: "launcherskins-agent-1.16.2-0.6.1.jar",
        sha256: "745bd7a96e7b38ec224a69aa836da43dcc5e18810ec38c1fc27d8da1f6a4d87d",
        size: 221756,
    },
    Jar {
        games: &["1.16.1"],
        target: Target::Agent,
        file: "launcherskins-agent-1.16.1-0.6.1.jar",
        sha256: "3bc3b9dcd29d48d22af002c03958aea24339a3ad1bfbc0c59d6bf2e8b418ac13",
        size: 221752,
    },
    Jar {
        games: &["1.16"],
        target: Target::Agent,
        file: "launcherskins-agent-1.16-0.6.1.jar",
        sha256: "8d8aa5f0caa4d68080c4c51ea6f24219a13ae92625544ed0b61f1c8bb54b5c94",
        size: 221753,
    },
    Jar {
        games: &["1.15.2"],
        target: Target::Forge,
        file: "launcherskins-forge-1.15.2-0.6.1.jar",
        sha256: "9ec2f847f1939e332ea1b83fd654e526bd7c61d22ebe5cd359ab20d7664e8d78",
        size: 33848,
    },
    Jar {
        games: &["1.15.2"],
        target: Target::Agent,
        file: "launcherskins-agent-1.15.2-0.6.1.jar",
        sha256: "c31b653b471b2442379c7b5951558a4e5c99e3dd3bc51f9b9995b0cf6b6608a6",
        size: 221751,
    },
    Jar {
        games: &["1.15", "1.15.1"],
        target: Target::Fabric,
        file: "launcherskins-fabric-1.15-1.15.1-0.6.1.jar",
        sha256: "b829f3e1de36765c4386b3fa106f53d88ec02eeedf994bffd89458e00019dff7",
        size: 34090,
    },
    Jar {
        games: &["1.15.1"],
        target: Target::Agent,
        file: "launcherskins-agent-1.15.1-0.6.1.jar",
        sha256: "0e6ca3f62fb113006bf283b4b7157646485da5ebc3a6951ecf516f89c1420497",
        size: 221752,
    },
    Jar {
        games: &["1.15"],
        target: Target::Agent,
        file: "launcherskins-agent-1.15-0.6.1.jar",
        sha256: "7bf767118833cd21f921dabb17f60ea2a024d68598330da8f9088f850c3e2e84",
        size: 221752,
    },
    Jar {
        games: &["1.14.4"],
        target: Target::Fabric,
        file: "launcherskins-fabric-1.14.4-0.6.1.jar",
        sha256: "9133901fb09c2e07f8288f208d88f2378e4efdd3def9eebb0fc02f938f45fc70",
        size: 34085,
    },
    Jar {
        games: &["1.14.4"],
        target: Target::Forge,
        file: "launcherskins-forge-1.14.4-0.6.1.jar",
        sha256: "8c571692612471460c21eabc8c923d45eeb71ad034e6a0f9f484af15f657faa6",
        size: 33844,
    },
    Jar {
        games: &["1.14.4"],
        target: Target::Agent,
        file: "launcherskins-agent-1.14.4-0.6.1.jar",
        sha256: "92c2299a998073d769a745c280dfbb169d28cb8e4805a60b950b202cf3445fb7",
        size: 221760,
    },
    Jar {
        games: &["1.12.2"],
        target: Target::Forge,
        file: "launcherskins-forge-1.12.2-0.6.1.jar",
        sha256: "ab55f8c47341a1e3ef91e68275b0d5c292d0ded34ea7796c5bf13de2488660a1",
        size: 34565,
    },
    Jar {
        games: &["1.8.9"],
        target: Target::Forge,
        file: "launcherskins-forge-1.8.9-0.6.1.jar",
        sha256: "566f739a30b333ac9e4df30d2b0111767a04251e189a4d7787448a6b61b3790b",
        size: 34097,
    },
    Jar {
        games: &["1.7.10"],
        target: Target::Forge,
        file: "launcherskins-forge-1.7.10-0.6.1.jar",
        sha256: "53a8f20f0d8c74b419d178bb6f2acc77eedf4ca25e252a24a1f7c6a3bcd552e1",
        size: 34558,
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
