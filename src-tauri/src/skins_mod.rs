//! Launcher Skins in the game (D-065, stage 8).
//!
//! When an installation starts, the launcher adds its skins mod: the jar for
//! the game version and loader goes into `<game>/mods/` (Fabric, Quilt,
//! NeoForge, Forge), or the Java agent is passed with `-javaagent` (no loader).
//! The jars come from this repository's GitHub release `skins-mod-v<version>`
//! and are pinned here by SHA-256 and size: [`JARS`] is the only list of what
//! may reach a game. A jar is downloaded once into [`MANAGED_DIR`], shared by
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
pub const MOD_VERSION: &str = "0.4.0";
/// Where the release keeps the jars.
const RELEASE: &str = "https://github.com/kurasis/launcher/releases/download/skins-mod-v0.4.0/";
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
    pub fn for_loader(loader: Option<LoaderKind>) -> Self {
        match loader {
            None => Self::Agent,
            Some(LoaderKind::Fabric | LoaderKind::Quilt) => Self::Fabric,
            Some(LoaderKind::NeoForge) => Self::NeoForge,
            Some(LoaderKind::Forge) => Self::Forge,
        }
    }

    /// The target that serves this game version and loader. NeoForge for 1.20.1
    /// is still Forge and runs Forge's jar.
    pub fn for_game(game: &str, loader: Option<LoaderKind>) -> Self {
        match Self::for_loader(loader) {
            Self::NeoForge if game == "1.20.1" => Self::Forge,
            target => target,
        }
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
/// Fabric runs 1.16 to 1.20.1 with intermediary names, Forge 1.17.1 to 1.20.1 with SRG
/// names; neither changes for what the mod uses, so one jar serves each range.
const BAND_1_16_FABRIC: &[&str] = &[
    "1.16", "1.16.1", "1.16.2", "1.16.3", "1.16.4", "1.16.5", "1.17", "1.17.1", "1.18", "1.18.1",
    "1.18.2", "1.19", "1.19.1", "1.19.2", "1.19.3", "1.19.4", "1.20", "1.20.1",
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
    build(Target::Fabric, "1.16-1.20.1", BAND_1_16_FABRIC),
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
    build(Target::Forge, "1.12.2", &["1.12.2"]),
];

impl Build {
    /// The build for this game version and loader, if the mod has one.
    pub fn find(game: &str, loader: Option<LoaderKind>) -> Option<&'static Build> {
        let target = Target::for_game(game, loader);
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

    /// Its place under DATA_ROOT.
    fn managed(&self) -> String {
        format!("{MANAGED_DIR}/{}", self.file)
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
        file: "launcherskins-fabric-26.1-26.3-0.4.0.jar",
        sha256: "ff592875f59fdae7c2e2e27036982be948e6912371e89464e42da03e4ab5a83f",
        size: 33489,
    },
    Jar {
        games: BAND_26,
        target: Target::NeoForge,
        file: "launcherskins-neoforge-26.1-26.3-0.4.0.jar",
        sha256: "93771f6dccdce9c31fb7552ced09119996fc3d0d710b27d6e2cc83404593f7da",
        size: 32580,
    },
    Jar {
        games: BAND_26,
        target: Target::Forge,
        file: "launcherskins-forge-26.1-26.3-0.4.0.jar",
        sha256: "77b46123c74d8a26a040088558fa5991da13bc2d5d3f9e177e9c77daebbb6896",
        size: 32930,
    },
    Jar {
        games: BAND_26,
        target: Target::Agent,
        file: "launcherskins-agent-26.1-26.3-0.4.0.jar",
        sha256: "bc210bd64cc2e3528e07e0021f630a9fef0e8aafe692f3c8d4099ee3a37bf5aa",
        size: 34756,
    },
    Jar {
        games: &["1.21.11"],
        target: Target::Fabric,
        file: "launcherskins-fabric-1.21.11-0.4.0.jar",
        sha256: "7bc1dc1fa0b0039b8c44a5cc6e4b79ea9b8779f1d832d4949b5e66d4c66b8ee3",
        size: 33932,
    },
    Jar {
        games: &["1.21.11"],
        target: Target::NeoForge,
        file: "launcherskins-neoforge-1.21.11-0.4.0.jar",
        sha256: "8797803be357aac2ef6cf3670c49f0f0d30b83b4bdab58db160dd2800a3f5dee",
        size: 32585,
    },
    Jar {
        games: &["1.21.11"],
        target: Target::Forge,
        file: "launcherskins-forge-1.21.11-0.4.0.jar",
        sha256: "78434e3b771b8b8d9ffb0579ca3d1b0f0c8bd1a7134aa79143289f4ab4e8d8de",
        size: 32928,
    },
    Jar {
        games: &["1.21.11"],
        target: Target::Agent,
        file: "launcherskins-agent-1.21.11-0.4.0.jar",
        sha256: "1825770533a112f6b97272a5a15fbab6990c881c222beeca9de4be99830a565c",
        size: 221745,
    },
    Jar {
        games: BAND_1_21_9,
        target: Target::Fabric,
        file: "launcherskins-fabric-1.21.9-1.21.10-0.4.0.jar",
        sha256: "75808530b0784fea90ac5847b739fe2b2be87e8333eb9fade84f2a0354f0cb35",
        size: 33935,
    },
    Jar {
        games: BAND_1_21_9,
        target: Target::NeoForge,
        file: "launcherskins-neoforge-1.21.9-1.21.10-0.4.0.jar",
        sha256: "daf38b6b3634f652ec28114d0a9d9ad43b32818a50df1eef36d6ab513de0a8e9",
        size: 32593,
    },
    Jar {
        games: BAND_1_21_9,
        target: Target::Forge,
        file: "launcherskins-forge-1.21.9-1.21.10-0.4.0.jar",
        sha256: "d351eb2b80aae9643dc719396d4d0ab17d7ee59a686a3c9cc8923b0783723614",
        size: 32934,
    },
    Jar {
        games: &["1.21.10"],
        target: Target::Agent,
        file: "launcherskins-agent-1.21.10-0.4.0.jar",
        sha256: "2ca745cf50e89f75327fb6f393e6b4b466412b86bc195f3d6ab4ab536eabd4e1",
        size: 221736,
    },
    Jar {
        games: &["1.21.9"],
        target: Target::Agent,
        file: "launcherskins-agent-1.21.9-0.4.0.jar",
        sha256: "a74a5da062decdabc9fc9cf4a0da7e937a54db88a0aa45f22440ce3d8354ad23",
        size: 221736,
    },
    Jar {
        games: BAND_1_21_5,
        target: Target::Fabric,
        file: "launcherskins-fabric-1.21.5-1.21.8-0.4.0.jar",
        sha256: "c267dba1a05bc2592f8f1fa0d426346fd5091f3757c14d3d6e9e76ec4e5e05cb",
        size: 33916,
    },
    Jar {
        games: BAND_1_21_5,
        target: Target::NeoForge,
        file: "launcherskins-neoforge-1.21.5-1.21.8-0.4.0.jar",
        sha256: "c58c26980425a7b3bb6236b9e32c39d763efad58eba7e06905b16bb848f37663",
        size: 32541,
    },
    Jar {
        games: BAND_1_21_5,
        target: Target::Forge,
        file: "launcherskins-forge-1.21.5-1.21.8-0.4.0.jar",
        sha256: "e51b899cf298ff40174c71fb6ae1bce3ffe9c95d854586d09504c052453f97e3",
        size: 32881,
    },
    Jar {
        games: &["1.21.8"],
        target: Target::Agent,
        file: "launcherskins-agent-1.21.8-0.4.0.jar",
        sha256: "b7e5dbc8fd6fbfaacc33208f026f8ca1fe3d9b558286bfbadbdd61e15423f56a",
        size: 221719,
    },
    Jar {
        games: &["1.21.7"],
        target: Target::Agent,
        file: "launcherskins-agent-1.21.7-0.4.0.jar",
        sha256: "f9503a1da90927972e2ed52b5272f116fb78dd00f6374df2e41d1740743b6634",
        size: 221723,
    },
    Jar {
        games: &["1.21.6"],
        target: Target::Agent,
        file: "launcherskins-agent-1.21.6-0.4.0.jar",
        sha256: "ad28bb47c23074e084cd0920340db4ca9c0eb5bd8548b164c0d3380b5b187ab2",
        size: 221726,
    },
    Jar {
        games: &["1.21.5"],
        target: Target::Agent,
        file: "launcherskins-agent-1.21.5-0.4.0.jar",
        sha256: "50d9654e59f574a28628be440af20657cce723b1b7ae197ebbff224be405658e",
        size: 221722,
    },
    Jar {
        games: BAND_1_21_2,
        target: Target::Fabric,
        file: "launcherskins-fabric-1.21.2-1.21.4-0.4.0.jar",
        sha256: "3d7614151f8f4867e8148ba67ac83f7e3241e342db6a61d0e905ccbe05b226c4",
        size: 33810,
    },
    Jar {
        games: BAND_1_21_2,
        target: Target::NeoForge,
        file: "launcherskins-neoforge-1.21.2-1.21.4-0.4.0.jar",
        sha256: "a0012c95d3be53711d13fa90dddd719fd9a1bf2ddd448199578c50ac8c344f25",
        size: 32425,
    },
    Jar {
        games: BAND_1_21_2,
        target: Target::Forge,
        file: "launcherskins-forge-1.21.2-1.21.4-0.4.0.jar",
        sha256: "ca5c055b409687214ffc338e526c899e85fc5fe3e08a48ecafc4fdbfcbb02f7f",
        size: 32769,
    },
    Jar {
        games: &["1.21.4"],
        target: Target::Agent,
        file: "launcherskins-agent-1.21.4-0.4.0.jar",
        sha256: "c91c06558936668b91b9a4b4027ed722cf2dc2ab822b1f29e09188f588e14014",
        size: 221618,
    },
    Jar {
        games: &["1.21.3"],
        target: Target::Agent,
        file: "launcherskins-agent-1.21.3-0.4.0.jar",
        sha256: "2e2c8e768d031be6e64cfc0136ab9129d96e11a244232defdfe66067e879d6ba",
        size: 221621,
    },
    Jar {
        games: &["1.21.2"],
        target: Target::Agent,
        file: "launcherskins-agent-1.21.2-0.4.0.jar",
        sha256: "e44fd644f3b93ed9c63fd7a9ab5e94b04832961a8bd0c9311639f6b7fdcad801",
        size: 221620,
    },
    Jar {
        games: BAND_1_20_2_FABRIC,
        target: Target::Fabric,
        file: "launcherskins-fabric-1.20.2-1.21.1-0.4.0.jar",
        sha256: "b06a2a67cc32f128b842a331f41b4eb23d4d47d0f7ac39791f53c5c6c28e76ec",
        size: 33896,
    },
    Jar {
        games: BAND_1_20_6_NEOFORGE,
        target: Target::NeoForge,
        file: "launcherskins-neoforge-1.20.6-1.21.1-0.4.0.jar",
        sha256: "fb4ccde66e03e3e04e15ea0e646eece904c589da816dbeab33fafc17f55ecdd8",
        size: 32521,
    },
    Jar {
        games: &["1.21.1"],
        target: Target::Forge,
        file: "launcherskins-forge-1.21.1-0.4.0.jar",
        sha256: "cb3659d696daec84caddbed23e33893ee3e2eb0b9cef4200456bfacc286a5090",
        size: 32859,
    },
    Jar {
        games: &["1.21.1"],
        target: Target::Agent,
        file: "launcherskins-agent-1.21.1-0.4.0.jar",
        sha256: "1b4b04391a72a17c01511094b4892faecc2577e01ea61930a3b794d7c2caceac",
        size: 221714,
    },
    Jar {
        games: &["1.21"],
        target: Target::Agent,
        file: "launcherskins-agent-1.21-0.4.0.jar",
        sha256: "4eba422c2481a73eb40956e7d6c0e5a98273d6d7ace7b8bb2d2101eb216ad771",
        size: 221714,
    },
    Jar {
        games: &["1.20.6"],
        target: Target::Forge,
        file: "launcherskins-forge-1.20.6-0.4.0.jar",
        sha256: "19e4635d7bd4010b33465e652fb4c4c6448c6fa0f7e72d89aaa65e4e7fd75934",
        size: 33799,
    },
    Jar {
        games: &["1.20.6"],
        target: Target::Agent,
        file: "launcherskins-agent-1.20.6-0.4.0.jar",
        sha256: "c39439390680351e1b29f4952f1ebeef89fbb66d1e361fe4c16e68a91ac6fcc0",
        size: 221723,
    },
    Jar {
        games: &["1.20.5"],
        target: Target::Agent,
        file: "launcherskins-agent-1.20.5-0.4.0.jar",
        sha256: "2ea27aa4fb184c50e065c2a1fe18b2b3c61f2dc67eefaa489cb8e364f7f1b44a",
        size: 221712,
    },
    Jar {
        games: BAND_1_20_2,
        target: Target::NeoForge,
        file: "launcherskins-neoforge-1.20.2-1.20.4-0.4.0.jar",
        sha256: "24e00226c079e21f1957daa9e3678154441ac969d11dbc2738738579bfa8864c",
        size: 33759,
    },
    Jar {
        games: BAND_1_20_2,
        target: Target::Forge,
        file: "launcherskins-forge-1.20.2-1.20.4-0.4.0.jar",
        sha256: "40721cacdcaea2894151ddf3a73fe6caa1c46045dbb2655c9c94c949ea11d248",
        size: 33826,
    },
    Jar {
        games: &["1.20.4"],
        target: Target::Agent,
        file: "launcherskins-agent-1.20.4-0.4.0.jar",
        sha256: "d6075caff06876b9533e259381b0c240107dc25cfb9f2b3fd5b38349aeadf869",
        size: 221707,
    },
    Jar {
        games: &["1.20.3"],
        target: Target::Agent,
        file: "launcherskins-agent-1.20.3-0.4.0.jar",
        sha256: "961ef11b8754c715cf9d3ad205af632f1502264a7820b6ca95c2f4f1cf102754",
        size: 221708,
    },
    Jar {
        games: &["1.20.2"],
        target: Target::Agent,
        file: "launcherskins-agent-1.20.2-0.4.0.jar",
        sha256: "ff7977fdc22d94cfb170fb39fd3860ddd2e01c5b8390e400d7c518580f0bbf0a",
        size: 221721,
    },
    Jar {
        games: BAND_1_16_FABRIC,
        target: Target::Fabric,
        file: "launcherskins-fabric-1.16-1.20.1-0.4.0.jar",
        sha256: "e845c966f7f665722dde4063b705edf6b0e5415f29a79081ed832b25b4fe14cd",
        size: 34049,
    },
    Jar {
        games: BAND_1_17_FORGE,
        target: Target::Forge,
        file: "launcherskins-forge-1.17.1-1.20.1-0.4.0.jar",
        sha256: "3a6a87bbc0b26efd3d4c136b1a31259e32bccc5f66a2b089a1ac38646da05744",
        size: 33856,
    },
    Jar {
        games: &["1.16.5"],
        target: Target::Forge,
        file: "launcherskins-forge-1.16.5-0.4.0.jar",
        sha256: "3a807ea16d8cc3c1894fb3f67975402061db32607394e129ee7e8b4fa5cd5759",
        size: 34021,
    },
    Jar {
        games: &["1.20.1"],
        target: Target::Agent,
        file: "launcherskins-agent-1.20.1-0.4.0.jar",
        sha256: "1b004b29885addea0137b3c19c6a62bf615fe8e1a387c302dd8cde07f6608fb8",
        size: 221811,
    },
    Jar {
        games: &["1.20"],
        target: Target::Agent,
        file: "launcherskins-agent-1.20-0.4.0.jar",
        sha256: "e9f5c5a50fa89f756cb2cc480ffc421fdab9a22e8c9a85d3da43e348371440f2",
        size: 221810,
    },
    Jar {
        games: &["1.19.4"],
        target: Target::Agent,
        file: "launcherskins-agent-1.19.4-0.4.0.jar",
        sha256: "772b431528b12fa2b4d5fe894db621f21a52c44ba942fb5de8ef37078f9161d1",
        size: 221807,
    },
    Jar {
        games: &["1.19.3"],
        target: Target::Agent,
        file: "launcherskins-agent-1.19.3-0.4.0.jar",
        sha256: "3d4eb592a3d2f27086683452a55df18c446e48087a0a5f74f4732a6d7df933b6",
        size: 221810,
    },
    Jar {
        games: &["1.19.2"],
        target: Target::Agent,
        file: "launcherskins-agent-1.19.2-0.4.0.jar",
        sha256: "2b716ff9f4278e55f3b86e4c43913595c7d18fc0b2bce74f23f9063f730bba02",
        size: 221807,
    },
    Jar {
        games: &["1.19.1"],
        target: Target::Agent,
        file: "launcherskins-agent-1.19.1-0.4.0.jar",
        sha256: "b3c820d985cd694658a78e789ea3566f38fb33a31f332ee9400ed78975d89628",
        size: 221805,
    },
    Jar {
        games: &["1.19"],
        target: Target::Agent,
        file: "launcherskins-agent-1.19-0.4.0.jar",
        sha256: "4357008f6e6ffa7351a1192dab637e63a811f4eded56c009f4abce34be553d61",
        size: 221809,
    },
    Jar {
        games: &["1.18.2"],
        target: Target::Agent,
        file: "launcherskins-agent-1.18.2-0.4.0.jar",
        sha256: "2c744b317cd10029368ff65f9eaf90d198dcbdc1824e8d2cc7c955d783600d62",
        size: 221810,
    },
    Jar {
        games: &["1.18.1"],
        target: Target::Agent,
        file: "launcherskins-agent-1.18.1-0.4.0.jar",
        sha256: "2ec8474f404f9d258431f0927600d137f26ceb61f10ef14f23e78248fb6a2ead",
        size: 221811,
    },
    Jar {
        games: &["1.18"],
        target: Target::Agent,
        file: "launcherskins-agent-1.18-0.4.0.jar",
        sha256: "dea9c2b8e9eaaff903e548eb79a52445c9c03922e21fbd7a2d042e3e6f389f42",
        size: 221812,
    },
    Jar {
        games: &["1.17.1"],
        target: Target::Agent,
        file: "launcherskins-agent-1.17.1-0.4.0.jar",
        sha256: "39e19a173b7702deac839fded70344fb96c4e4d5dc7da61cc9f54932b5deab19",
        size: 221811,
    },
    Jar {
        games: &["1.17"],
        target: Target::Agent,
        file: "launcherskins-agent-1.17-0.4.0.jar",
        sha256: "5e7dba886f8de98ceb0fa4af9a323a59253cc4b1e0d0235b04a69f55529d03b5",
        size: 221813,
    },
    Jar {
        games: &["1.16.5"],
        target: Target::Agent,
        file: "launcherskins-agent-1.16.5-0.4.0.jar",
        sha256: "fe4aa3875056f523e41f94bc00b1118843673d940bb6a8ff1780252cfaa6274e",
        size: 221720,
    },
    Jar {
        games: &["1.16.4"],
        target: Target::Agent,
        file: "launcherskins-agent-1.16.4-0.4.0.jar",
        sha256: "e1045b6443c321b6b8aaaf9b5de0e567b925bccef6d3eb649ecbf5d6dbed2c15",
        size: 221720,
    },
    Jar {
        games: &["1.16.3"],
        target: Target::Agent,
        file: "launcherskins-agent-1.16.3-0.4.0.jar",
        sha256: "ac66e3052bf383ff4ec954e47394f8919c55f2ad51eae615a69733b8438e645d",
        size: 221718,
    },
    Jar {
        games: &["1.16.2"],
        target: Target::Agent,
        file: "launcherskins-agent-1.16.2-0.4.0.jar",
        sha256: "f34724a700b5dc14a56a0689a387e0b120bd19682484814cb8d042e3ecd0a570",
        size: 221718,
    },
    Jar {
        games: &["1.16.1"],
        target: Target::Agent,
        file: "launcherskins-agent-1.16.1-0.4.0.jar",
        sha256: "aaa64531af865877c60926e20242adf5dbc6e8712455cda48c64443932a04785",
        size: 221715,
    },
    Jar {
        games: &["1.16"],
        target: Target::Agent,
        file: "launcherskins-agent-1.16-0.4.0.jar",
        sha256: "133e45dd74347b24d9baeb5040c3b7fb31672ab0985d53784587293cece38f64",
        size: 221715,
    },
    Jar {
        games: &["1.12.2"],
        target: Target::Forge,
        file: "launcherskins-forge-1.12.2-0.4.0.jar",
        sha256: "a171ce9ff37be56cd5e8f3a168eb636389bd33bd709a3a4c3576b110868b859f",
        size: 34504,
    },
];

/// The jar for this game version and loader, if there is a pinned one.
pub fn find(game: &str, loader: Option<LoaderKind>) -> Option<&'static Jar> {
    find_in(JARS, game, loader)
}

fn find_in<'a>(table: &'a [Jar], game: &str, loader: Option<LoaderKind>) -> Option<&'a Jar> {
    let target = Target::for_game(game, loader);
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
fn read_verified(path: &Path, jar: &Jar) -> Option<Vec<u8>> {
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
    let expected = Sha256::parse(jar.sha256).ok()?;
    (bytes.len() as u64 == jar.size
        && hex::encode(Sha256Hasher::digest(&bytes)) == expected.as_str())
    .then_some(bytes)
}

/// The jar's verified bytes from [`MANAGED_DIR`], downloading it first when
/// it is missing or damaged and a transport is given.
async fn managed_bytes(
    root: &DataRoot,
    transport: Option<&HttpTransport>,
    jar: &'static Jar,
    cancel: &CancelToken,
) -> Option<Vec<u8>> {
    let path = resolve_managed_path(root, &jar.managed()).ok()?;
    let check = |path: PathBuf| async move {
        tokio::task::spawn_blocking(move || read_verified(&path, jar))
            .await
            .ok()
            .flatten()
    };
    if let Some(bytes) = check(path.clone()).await {
        return Some(bytes);
    }
    let transport = transport?;
    let request = DownloadRequest {
        source: SourcePolicy::SkinsMod,
        url: SourcePolicy::SkinsMod.check(&jar.url()).ok()?,
        path: jar.managed(),
        size: jar.size,
        digest: Digest::Sha256(Sha256::parse(jar.sha256).ok()?),
        label: Some(jar.file.to_owned()),
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
fn sync_mods(mods: &Path, keep: Option<(&Jar, &[u8])>) -> std::io::Result<()> {
    if keep.is_some() {
        std::fs::create_dir_all(mods)?;
    }
    let entries = match std::fs::read_dir(mods) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound && keep.is_none() => return Ok(()),
        Err(e) => return Err(e),
    };
    let keep_name = keep.map(|(jar, _)| jar.file);
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
    let target = mods.join(jar.file);
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
    table: &'static [Jar],
    root: &DataRoot,
    transport: Option<&HttpTransport>,
    setup: &Setup<'_>,
    cancel: &CancelToken,
) -> Prepared {
    let jar = setup
        .enabled
        .then(|| find_in(table, setup.game_version, setup.loader))
        .flatten();
    let bytes = match jar {
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
    let keep = match (setup.loader, jar, bytes.as_ref()) {
        (Some(_), Some(jar), Some(bytes)) => Some((jar, bytes.clone())),
        _ => None,
    };
    let mods = setup.game_dir.join("mods");
    let wanted = keep.is_some();
    let synced = tokio::task::spawn_blocking(move || {
        sync_mods(
            &mods,
            keep.as_ref().map(|(jar, bytes)| (*jar, bytes.as_slice())),
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

#[cfg(test)]
mod tests;
