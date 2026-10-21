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
pub const MOD_VERSION: &str = "0.5.0";
/// Where the release keeps the jars.
const RELEASE: &str = "https://github.com/kurasis/launcher/releases/download/skins-mod-v0.5.0/";
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
        file: "launcherskins-fabric-26.1-26.3-0.5.0.jar",
        sha256: "d0df8a3095e6708ea103336a8b85487540fd181c95b706017e637cfe8bd25e71",
        size: 33488,
    },
    Jar {
        games: BAND_26,
        target: Target::NeoForge,
        file: "launcherskins-neoforge-26.1-26.3-0.5.0.jar",
        sha256: "47b2dd1fe3556a559dc634e19df69ea86dfd46a434d841cc641544c236a6d3e7",
        size: 32580,
    },
    Jar {
        games: BAND_26,
        target: Target::Forge,
        file: "launcherskins-forge-26.1-26.3-0.5.0.jar",
        sha256: "53d4ae8a8f06f7f054426ccf9e6315cbaa7a6ec81ba4932b5d5cc65ad073cc5f",
        size: 32930,
    },
    Jar {
        games: BAND_26,
        target: Target::Agent,
        file: "launcherskins-agent-26.1-26.3-0.5.0.jar",
        sha256: "15687582d70feb31680685c22e2f499a7c671de7a674bc15941efdab0e877933",
        size: 34756,
    },
    Jar {
        games: &["1.21.11"],
        target: Target::Fabric,
        file: "launcherskins-fabric-1.21.11-0.5.0.jar",
        sha256: "2a26ee0b631aff612a392f4c41dc6ce61dfea2ea555e8bc2e27cb955aa257214",
        size: 33931,
    },
    Jar {
        games: &["1.21.11"],
        target: Target::NeoForge,
        file: "launcherskins-neoforge-1.21.11-0.5.0.jar",
        sha256: "d3c4dee99f3760c45b5c39ad519b2c445a8f166129ab4d34a3c2ec78a3c4d2dc",
        size: 32585,
    },
    Jar {
        games: &["1.21.11"],
        target: Target::Forge,
        file: "launcherskins-forge-1.21.11-0.5.0.jar",
        sha256: "8ca0fceac756d7a5ca54a779a0ff98afd7a9c5eb8c7b26b24d6ab56eecf18ffa",
        size: 32928,
    },
    Jar {
        games: &["1.21.11"],
        target: Target::Agent,
        file: "launcherskins-agent-1.21.11-0.5.0.jar",
        sha256: "8b5a8718131743a610c0aed38958ad54f55548debe49d5608fa2743bd5175542",
        size: 221744,
    },
    Jar {
        games: BAND_1_21_9,
        target: Target::Fabric,
        file: "launcherskins-fabric-1.21.9-1.21.10-0.5.0.jar",
        sha256: "fbf294f4bc8ec919bbe0bc9254715a3996694dd05735df4175210e0c9386a13b",
        size: 33935,
    },
    Jar {
        games: BAND_1_21_9,
        target: Target::NeoForge,
        file: "launcherskins-neoforge-1.21.9-1.21.10-0.5.0.jar",
        sha256: "616ae21cd3fd02378daac0197bc175ce7dd83672287a72e523273b2ffe498869",
        size: 32593,
    },
    Jar {
        games: BAND_1_21_9,
        target: Target::Forge,
        file: "launcherskins-forge-1.21.9-1.21.10-0.5.0.jar",
        sha256: "b9fb672bf860f6f94afc89d5b7648c1e0de6d5400d423ca055c9ff55a94ab548",
        size: 32933,
    },
    Jar {
        games: &["1.21.10"],
        target: Target::Agent,
        file: "launcherskins-agent-1.21.10-0.5.0.jar",
        sha256: "32e2be7ac441f2e0b768faeef29ecf3c412132f80da29e6265f6826c909ba4fe",
        size: 221735,
    },
    Jar {
        games: &["1.21.9"],
        target: Target::Agent,
        file: "launcherskins-agent-1.21.9-0.5.0.jar",
        sha256: "1349f503ef9a6b570df3ebecfe034d3ef33c981cfc3a3bd97e30ce1974cf9db1",
        size: 221736,
    },
    Jar {
        games: BAND_1_21_5,
        target: Target::Fabric,
        file: "launcherskins-fabric-1.21.5-1.21.8-0.5.0.jar",
        sha256: "2e18c11179e181b635ddb9fc72eee1e62e3039f25bc0612693b187955450ab61",
        size: 33915,
    },
    Jar {
        games: BAND_1_21_5,
        target: Target::NeoForge,
        file: "launcherskins-neoforge-1.21.5-1.21.8-0.5.0.jar",
        sha256: "fa17b541313ebb2901c9765798792ff03d3b779e0b07267cb93479c71f992fac",
        size: 32541,
    },
    Jar {
        games: BAND_1_21_5,
        target: Target::Forge,
        file: "launcherskins-forge-1.21.5-1.21.8-0.5.0.jar",
        sha256: "6283433f70408f715aa517c0e7fb2747e515aa1ff830051a7298a32453fed365",
        size: 32881,
    },
    Jar {
        games: &["1.21.8"],
        target: Target::Agent,
        file: "launcherskins-agent-1.21.8-0.5.0.jar",
        sha256: "e3d4466ae7822a64f393491d66638f6e5e17b15827e8a79c2b3bb8515f4b94a8",
        size: 221718,
    },
    Jar {
        games: &["1.21.7"],
        target: Target::Agent,
        file: "launcherskins-agent-1.21.7-0.5.0.jar",
        sha256: "0774de9d66f2ce1b8c84fdd62f4ea2ec96c5908814411e391c95d967f1fa1627",
        size: 221722,
    },
    Jar {
        games: &["1.21.6"],
        target: Target::Agent,
        file: "launcherskins-agent-1.21.6-0.5.0.jar",
        sha256: "1fd5de014a1252033c87365b21f7413df4afaa53dbbb6adae4fe772a120b734a",
        size: 221725,
    },
    Jar {
        games: &["1.21.5"],
        target: Target::Agent,
        file: "launcherskins-agent-1.21.5-0.5.0.jar",
        sha256: "b5880d1296940d61623ab1edcd69e229798a14c19bc4b99973c7781cfeb81230",
        size: 221721,
    },
    Jar {
        games: BAND_1_21_2,
        target: Target::Fabric,
        file: "launcherskins-fabric-1.21.2-1.21.4-0.5.0.jar",
        sha256: "9b5fc08f300203cc2bd75b9e5c00c4fdcff4649f2b0c3a5fb665c5587acaa35b",
        size: 33809,
    },
    Jar {
        games: BAND_1_21_2,
        target: Target::NeoForge,
        file: "launcherskins-neoforge-1.21.2-1.21.4-0.5.0.jar",
        sha256: "2fc6bd5ea70fa09c80ccc02bf0d6e4c467546d6138f3f5a715766331c5b9f856",
        size: 32426,
    },
    Jar {
        games: BAND_1_21_2,
        target: Target::Forge,
        file: "launcherskins-forge-1.21.2-1.21.4-0.5.0.jar",
        sha256: "243b812a20a6b379ccac68b11b59868d6b200569c201a7bf1c7d7e7cbe6a925f",
        size: 32769,
    },
    Jar {
        games: &["1.21.4"],
        target: Target::Agent,
        file: "launcherskins-agent-1.21.4-0.5.0.jar",
        sha256: "2e2cf9f1fbca36de6e2c0227e412b651e2b5e45e222ed84387bb810ea5d84058",
        size: 221617,
    },
    Jar {
        games: &["1.21.3"],
        target: Target::Agent,
        file: "launcherskins-agent-1.21.3-0.5.0.jar",
        sha256: "37b3f08ff7396064c889dec294aa7ecb55356a63808e806d49993ab1a5781fb5",
        size: 221620,
    },
    Jar {
        games: &["1.21.2"],
        target: Target::Agent,
        file: "launcherskins-agent-1.21.2-0.5.0.jar",
        sha256: "b7c7c37f383e1dedb9a34cee155429b98d1812a83e1ad981bd7f04485f0ee3d1",
        size: 221620,
    },
    Jar {
        games: BAND_1_20_2_FABRIC,
        target: Target::Fabric,
        file: "launcherskins-fabric-1.20.2-1.21.1-0.5.0.jar",
        sha256: "e46f8b1f270dcaff78f8208c6f71f94eb2877f12a6e7ca99beefae8829a6adc8",
        size: 33895,
    },
    Jar {
        games: BAND_1_20_6_NEOFORGE,
        target: Target::NeoForge,
        file: "launcherskins-neoforge-1.20.6-1.21.1-0.5.0.jar",
        sha256: "1a9d4080e354e44cd6b9078e072513853d3fd701352cbda5bd2001f72f547b68",
        size: 32521,
    },
    Jar {
        games: &["1.21.1"],
        target: Target::Forge,
        file: "launcherskins-forge-1.21.1-0.5.0.jar",
        sha256: "e4c812d9df48b85e7130ba9390731eafd1ac41c57f111155b7f648852846516e",
        size: 32859,
    },
    Jar {
        games: &["1.21.1"],
        target: Target::Agent,
        file: "launcherskins-agent-1.21.1-0.5.0.jar",
        sha256: "c1d69b38b55973b5e58427cdce3c99e626b033fe9579756fecb4ce562e4c51e1",
        size: 221713,
    },
    Jar {
        games: &["1.21"],
        target: Target::Agent,
        file: "launcherskins-agent-1.21-0.5.0.jar",
        sha256: "48fad0cdcaa8004f7f5c1849a29a9587b42733e72de286fe8cff1c37f3b2d326",
        size: 221713,
    },
    Jar {
        games: &["1.20.6"],
        target: Target::Forge,
        file: "launcherskins-forge-1.20.6-0.5.0.jar",
        sha256: "c5b5c4ac514371ac9602d4bfa70d39e9eae36392df26b9dd5eae0bd5742ffa7a",
        size: 33798,
    },
    Jar {
        games: &["1.20.6"],
        target: Target::Agent,
        file: "launcherskins-agent-1.20.6-0.5.0.jar",
        sha256: "50851426f30211d90aef9f5d04f580ab317bddc86b960665918eeb3ee2bf0196",
        size: 221722,
    },
    Jar {
        games: &["1.20.5"],
        target: Target::Agent,
        file: "launcherskins-agent-1.20.5-0.5.0.jar",
        sha256: "e18729f07433e1f809d9d35022b294608335188e03d169e8dd727df2fe68c0cb",
        size: 221710,
    },
    Jar {
        games: BAND_1_20_2,
        target: Target::NeoForge,
        file: "launcherskins-neoforge-1.20.2-1.20.4-0.5.0.jar",
        sha256: "6b2ee8ff94a7978a8448b24658e1c0a45e257af9620d23c4001776f229d0c1cc",
        size: 33759,
    },
    Jar {
        games: BAND_1_20_2,
        target: Target::Forge,
        file: "launcherskins-forge-1.20.2-1.20.4-0.5.0.jar",
        sha256: "80c037bb1b20436c5e2a86aefe13a3fe34015ac5762a2ca7ec6ff6d14a399641",
        size: 33826,
    },
    Jar {
        games: &["1.20.4"],
        target: Target::Agent,
        file: "launcherskins-agent-1.20.4-0.5.0.jar",
        sha256: "b7051c928f9d7720a6dd08770f92c955ae11fca745a128b26fb90ee05655a899",
        size: 221707,
    },
    Jar {
        games: &["1.20.3"],
        target: Target::Agent,
        file: "launcherskins-agent-1.20.3-0.5.0.jar",
        sha256: "942024d9292a6099b98d2fbc0a469ccf461d0c00dda867ce752940f857c421b7",
        size: 221707,
    },
    Jar {
        games: &["1.20.2"],
        target: Target::Agent,
        file: "launcherskins-agent-1.20.2-0.5.0.jar",
        sha256: "3bb6545bb3e1a95b5b8fc96921dabab29a710f7e29af3f6b914e0c6a555f00a3",
        size: 221720,
    },
    Jar {
        games: BAND_1_15_2_FABRIC,
        target: Target::Fabric,
        file: "launcherskins-fabric-1.15.2-1.20.1-0.5.0.jar",
        sha256: "bf9ae0b512b4eff5557c90c96ed2896689bbd053584d1e620b1ffb1667af8da6",
        size: 34052,
    },
    Jar {
        games: BAND_1_17_FORGE,
        target: Target::Forge,
        file: "launcherskins-forge-1.17.1-1.20.1-0.5.0.jar",
        sha256: "7a88f9fccc2bd96d9b686623b999785ba3486406d2d05ad10aeb43bad2a71510",
        size: 33856,
    },
    Jar {
        games: &["1.16.5"],
        target: Target::Forge,
        file: "launcherskins-forge-1.16.5-0.5.0.jar",
        sha256: "9f62c32d850ba42de5c4b8c0a778084c98b50374317c80dc972e8e9290902b57",
        size: 34020,
    },
    Jar {
        games: &["1.20.1"],
        target: Target::Agent,
        file: "launcherskins-agent-1.20.1-0.5.0.jar",
        sha256: "deff7229fa9b11cb8092a3e433ac520cba474f924c11b7a5e5b7f5c9e2b264c5",
        size: 221810,
    },
    Jar {
        games: &["1.20"],
        target: Target::Agent,
        file: "launcherskins-agent-1.20-0.5.0.jar",
        sha256: "057071b21c26414f442f43914cc9081d87c5692795a3984645ba80dd7b84b038",
        size: 221809,
    },
    Jar {
        games: &["1.19.4"],
        target: Target::Agent,
        file: "launcherskins-agent-1.19.4-0.5.0.jar",
        sha256: "ba1b1837fea8741cefa920a8911669cfd95bb07f64febe70a11668f8a7bd8419",
        size: 221807,
    },
    Jar {
        games: &["1.19.3"],
        target: Target::Agent,
        file: "launcherskins-agent-1.19.3-0.5.0.jar",
        sha256: "e8e0b5d9338ed122e8854a47af4dc70a2e05d243af92b457282e3ba35c50e461",
        size: 221810,
    },
    Jar {
        games: &["1.19.2"],
        target: Target::Agent,
        file: "launcherskins-agent-1.19.2-0.5.0.jar",
        sha256: "8d8ed133ec086bac66a2049d545e71886f450d37f56dc8373cd754a6a53e6539",
        size: 221807,
    },
    Jar {
        games: &["1.19.1"],
        target: Target::Agent,
        file: "launcherskins-agent-1.19.1-0.5.0.jar",
        sha256: "8a18dca94d3481e7707d4573d4e7d6c2cd503208092fb5268461731934af18d3",
        size: 221804,
    },
    Jar {
        games: &["1.19"],
        target: Target::Agent,
        file: "launcherskins-agent-1.19-0.5.0.jar",
        sha256: "978706c6c9a6593025cd68960682589f4ce6554e5ea478d270ad1b17ae4542cf",
        size: 221808,
    },
    Jar {
        games: &["1.18.2"],
        target: Target::Agent,
        file: "launcherskins-agent-1.18.2-0.5.0.jar",
        sha256: "37e8a5a26cbdc2eb37ed0ce34cb6f7ee9aa6c0d67622be3aff37e72385736867",
        size: 221809,
    },
    Jar {
        games: &["1.18.1"],
        target: Target::Agent,
        file: "launcherskins-agent-1.18.1-0.5.0.jar",
        sha256: "64e8ea072aec671000059a0d7d524c818eca376c8ab8f65de50579c6a40d687f",
        size: 221811,
    },
    Jar {
        games: &["1.18"],
        target: Target::Agent,
        file: "launcherskins-agent-1.18-0.5.0.jar",
        sha256: "e31198bf31526cd2ab2aa004d57e016d4f16a3bf7058c38f16dee8825a51ee1f",
        size: 221811,
    },
    Jar {
        games: &["1.17.1"],
        target: Target::Agent,
        file: "launcherskins-agent-1.17.1-0.5.0.jar",
        sha256: "7fabda688707f116da47bce5fa9e4819108d211b9d0de0f8565b737e52b0dbdd",
        size: 221811,
    },
    Jar {
        games: &["1.17"],
        target: Target::Agent,
        file: "launcherskins-agent-1.17-0.5.0.jar",
        sha256: "18a1b3f67b0691b93185c7d4924cfef2e6476a9e215dc58e4832e17044b54ffd",
        size: 221813,
    },
    Jar {
        games: &["1.16.5"],
        target: Target::Agent,
        file: "launcherskins-agent-1.16.5-0.5.0.jar",
        sha256: "9db609f671c7ce148d4c1579465be9285cf8647ea68f54f4040b398c6aebdc2f",
        size: 221719,
    },
    Jar {
        games: &["1.16.4"],
        target: Target::Agent,
        file: "launcherskins-agent-1.16.4-0.5.0.jar",
        sha256: "922d1be79cec7c8a4a27f34a420e651663da00399315b89c6f46dac26f2fe5dd",
        size: 221719,
    },
    Jar {
        games: &["1.16.3"],
        target: Target::Agent,
        file: "launcherskins-agent-1.16.3-0.5.0.jar",
        sha256: "c2acaab6d2a5da8d28e1e752b28b1ab4902c61350c0e78285ad5581f1faf5158",
        size: 221717,
    },
    Jar {
        games: &["1.16.2"],
        target: Target::Agent,
        file: "launcherskins-agent-1.16.2-0.5.0.jar",
        sha256: "57e77b9fc82f286d50eaf1cfbc16a92dfd3a1c583f9ee1bdff59fd6f351b17f9",
        size: 221717,
    },
    Jar {
        games: &["1.16.1"],
        target: Target::Agent,
        file: "launcherskins-agent-1.16.1-0.5.0.jar",
        sha256: "926ba113b8577272d6b466f242bb1e00929afb3dbbdd4b4de1afec8e0fbc9976",
        size: 221714,
    },
    Jar {
        games: &["1.16"],
        target: Target::Agent,
        file: "launcherskins-agent-1.16-0.5.0.jar",
        sha256: "16b74e7e897984750a54eb3f4c660c34fa0845e4619dabe8a0a00e89a4edf047",
        size: 221714,
    },
    Jar {
        games: &["1.15.2"],
        target: Target::Forge,
        file: "launcherskins-forge-1.15.2-0.5.0.jar",
        sha256: "17c043ccc709421b83a12c700bd60fec51ba9a46edf11d32fef6f7a1b0e6ad7c",
        size: 33808,
    },
    Jar {
        games: &["1.15.2"],
        target: Target::Agent,
        file: "launcherskins-agent-1.15.2-0.5.0.jar",
        sha256: "d8c614a5ea74d31bee8bc56de39e1436bf24dd5470e586f8757e5899358f4586",
        size: 221714,
    },
    Jar {
        games: &["1.15", "1.15.1"],
        target: Target::Fabric,
        file: "launcherskins-fabric-1.15-1.15.1-0.5.0.jar",
        sha256: "e5e2a355eb0cebf22e265bcb460895da32118925806eec7aaf1096f5d79267e2",
        size: 34049,
    },
    Jar {
        games: &["1.15.1"],
        target: Target::Agent,
        file: "launcherskins-agent-1.15.1-0.5.0.jar",
        sha256: "524ca9335e5f5396ecfcd054392ef2f1aca2b65fcbfb0cd06bdf292d84ece920",
        size: 221713,
    },
    Jar {
        games: &["1.15"],
        target: Target::Agent,
        file: "launcherskins-agent-1.15-0.5.0.jar",
        sha256: "709cc478249f27361fba157d49cff16d07079be32b0867123564d71549563d21",
        size: 221713,
    },
    Jar {
        games: &["1.14.4"],
        target: Target::Fabric,
        file: "launcherskins-fabric-1.14.4-0.5.0.jar",
        sha256: "33c7a32451efa4f182656b5497418de2c8d6ca9029ee00a93df5ae3fc1818102",
        size: 34046,
    },
    Jar {
        games: &["1.14.4"],
        target: Target::Forge,
        file: "launcherskins-forge-1.14.4-0.5.0.jar",
        sha256: "c659a0c9f07cb7b9be9184f4f12bcb27e4bd3d4865ca7144dd323c3b02b209be",
        size: 33805,
    },
    Jar {
        games: &["1.14.4"],
        target: Target::Agent,
        file: "launcherskins-agent-1.14.4-0.5.0.jar",
        sha256: "9003ac898c1a92b0701659fdd3aed855e63a8586b838a7ca1a1ec2e313392c49",
        size: 221721,
    },
    Jar {
        games: &["1.12.2"],
        target: Target::Forge,
        file: "launcherskins-forge-1.12.2-0.5.0.jar",
        sha256: "6ecd919248b55c1b2d9947dcd8291b94862522f79bd7e788d52a2a2a8c1032fe",
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
