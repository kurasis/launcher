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
pub const MOD_VERSION: &str = "0.6.0";
/// Where the release keeps the jars.
const RELEASE: &str = "https://github.com/kurasis/launcher/releases/download/skins-mod-v0.6.0/";
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
    // Forge only: 1.8.9 shares 1.12.2's game names; 1.7.10 hooks the player itself.
    build(Target::Forge, "1.8.9", &["1.8.9"]),
    build(Target::Forge, "1.7.10", &["1.7.10"]),
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
        file: "launcherskins-fabric-26.1-26.3-0.6.0.jar",
        sha256: "449c604f12aaa74624ef2e571690a4338ca39edd6ca5ee0d43d63a851d8ba233",
        size: 33527,
    },
    Jar {
        games: BAND_26,
        target: Target::NeoForge,
        file: "launcherskins-neoforge-26.1-26.3-0.6.0.jar",
        sha256: "6c901271a842194bfc2f87a3a6e4592eb1e2ce79410ba8edd8db19c5314cf293",
        size: 32619,
    },
    Jar {
        games: BAND_26,
        target: Target::Forge,
        file: "launcherskins-forge-26.1-26.3-0.6.0.jar",
        sha256: "06a8a0f5a3b1cb46618b417ca4455606ed723cda99e7b12260d2ee8e8d7f4f21",
        size: 32968,
    },
    Jar {
        games: BAND_26,
        target: Target::Agent,
        file: "launcherskins-agent-26.1-26.3-0.6.0.jar",
        sha256: "294a1680169fb2c84608d4c99f5ef4c4eda499ae5ec4ef5e81eaa7544ef180b8",
        size: 34792,
    },
    Jar {
        games: &["1.21.11"],
        target: Target::Fabric,
        file: "launcherskins-fabric-1.21.11-0.6.0.jar",
        sha256: "e3a1b54e4941eb568a51ab6006b045d45e28e37f5490a27097e20cce5ddd7727",
        size: 33962,
    },
    Jar {
        games: &["1.21.11"],
        target: Target::NeoForge,
        file: "launcherskins-neoforge-1.21.11-0.6.0.jar",
        sha256: "768e6bff92d0a11cd89f49cffc1bff576529227f33d3eefbb713874b4671a495",
        size: 32625,
    },
    Jar {
        games: &["1.21.11"],
        target: Target::Forge,
        file: "launcherskins-forge-1.21.11-0.6.0.jar",
        sha256: "cfaf3072fc79b59e2dae938e586a1ac617034d35d304713bfcc90e2209c6c53d",
        size: 32968,
    },
    Jar {
        games: &["1.21.11"],
        target: Target::Agent,
        file: "launcherskins-agent-1.21.11-0.6.0.jar",
        sha256: "982ca7f73df2149a46e0cd7ccd8d6b5790d96dfc57c2e547dcd320c5ea5b8151",
        size: 221773,
    },
    Jar {
        games: BAND_1_21_9,
        target: Target::Fabric,
        file: "launcherskins-fabric-1.21.9-1.21.10-0.6.0.jar",
        sha256: "19ccec2e39ef6d845959489be2e665d9d5ef508164f4eb71cb9a0cfe1142306e",
        size: 33966,
    },
    Jar {
        games: BAND_1_21_9,
        target: Target::NeoForge,
        file: "launcherskins-neoforge-1.21.9-1.21.10-0.6.0.jar",
        sha256: "441f34621390e515af063e180789a0db522b103294d7b5fd3406c86cf8d63b25",
        size: 32633,
    },
    Jar {
        games: BAND_1_21_9,
        target: Target::Forge,
        file: "launcherskins-forge-1.21.9-1.21.10-0.6.0.jar",
        sha256: "00db4f689fad7bf2e0f07c53e3cd88870a780d0c833dfcac8b63c753b7d29860",
        size: 32973,
    },
    Jar {
        games: &["1.21.10"],
        target: Target::Agent,
        file: "launcherskins-agent-1.21.10-0.6.0.jar",
        sha256: "09d64533358b7c833c8ba5ef02e6f0ca333153091b4f3ebdca11f4d849a42dbb",
        size: 221764,
    },
    Jar {
        games: &["1.21.9"],
        target: Target::Agent,
        file: "launcherskins-agent-1.21.9-0.6.0.jar",
        sha256: "c771668a16a13e83684ff3c18f077f25f170909e8c639d1889d12185beaaff49",
        size: 221765,
    },
    Jar {
        games: BAND_1_21_5,
        target: Target::Fabric,
        file: "launcherskins-fabric-1.21.5-1.21.8-0.6.0.jar",
        sha256: "851f1990cc9241fab26d2d4a84c61b388bdb049ef1ae2fca785949bbc7d5f32b",
        size: 33946,
    },
    Jar {
        games: BAND_1_21_5,
        target: Target::NeoForge,
        file: "launcherskins-neoforge-1.21.5-1.21.8-0.6.0.jar",
        sha256: "6451761ac9597cdddda04ab97e034711e7a2f0ff81c0cef5aec122054b72fa8f",
        size: 32581,
    },
    Jar {
        games: BAND_1_21_5,
        target: Target::Forge,
        file: "launcherskins-forge-1.21.5-1.21.8-0.6.0.jar",
        sha256: "e7b9a4c2d4adda6644c58792dcc721329ee5233707452dcde049d5634d39e838",
        size: 32921,
    },
    Jar {
        games: &["1.21.8"],
        target: Target::Agent,
        file: "launcherskins-agent-1.21.8-0.6.0.jar",
        sha256: "8b6c0dbaa98eec1e99a9bbdcd90b7aa609bc09a487e653a66077eea8e737263d",
        size: 221747,
    },
    Jar {
        games: &["1.21.7"],
        target: Target::Agent,
        file: "launcherskins-agent-1.21.7-0.6.0.jar",
        sha256: "ce4d8a5434090dad061a55511332271451ec905747afdb59444e309a6b829622",
        size: 221751,
    },
    Jar {
        games: &["1.21.6"],
        target: Target::Agent,
        file: "launcherskins-agent-1.21.6-0.6.0.jar",
        sha256: "a95425d1beb8d2a5e15a1aa7d49ea667c337c201a26d42dc74346fdb6e591ff3",
        size: 221754,
    },
    Jar {
        games: &["1.21.5"],
        target: Target::Agent,
        file: "launcherskins-agent-1.21.5-0.6.0.jar",
        sha256: "b8d8cfa9a34a62f16c902d11e0e54d637c1e9b6493db2f1a682b99a3dddb185c",
        size: 221750,
    },
    Jar {
        games: BAND_1_21_2,
        target: Target::Fabric,
        file: "launcherskins-fabric-1.21.2-1.21.4-0.6.0.jar",
        sha256: "245946a663e7cd24b4adaa36c6125e88ec74f39b2f882d6d6672015bcc8a553d",
        size: 33841,
    },
    Jar {
        games: BAND_1_21_2,
        target: Target::NeoForge,
        file: "launcherskins-neoforge-1.21.2-1.21.4-0.6.0.jar",
        sha256: "15747459c2c7d39784b403bc3c5ad03aa12d489714ec2feb673ee77073098bf5",
        size: 32466,
    },
    Jar {
        games: BAND_1_21_2,
        target: Target::Forge,
        file: "launcherskins-forge-1.21.2-1.21.4-0.6.0.jar",
        sha256: "342c9adaf697ed5ef7e770a240a85ce86a5cd76372bab0bf6ece4bc32586ded0",
        size: 32809,
    },
    Jar {
        games: &["1.21.4"],
        target: Target::Agent,
        file: "launcherskins-agent-1.21.4-0.6.0.jar",
        sha256: "7b8a66735a6377978b1d0d74705f6201095957510f1e1e8496ae5e4432ece106",
        size: 221646,
    },
    Jar {
        games: &["1.21.3"],
        target: Target::Agent,
        file: "launcherskins-agent-1.21.3-0.6.0.jar",
        sha256: "cfbc51cf98fe5aa915eb74edbce72dee1f7057a17e0e2dd833005509cd199b4d",
        size: 221649,
    },
    Jar {
        games: &["1.21.2"],
        target: Target::Agent,
        file: "launcherskins-agent-1.21.2-0.6.0.jar",
        sha256: "6391f707897a9f08fbdce0f66687e600cdb107e1e2d9222d279bec51cb13d5ca",
        size: 221648,
    },
    Jar {
        games: BAND_1_20_2_FABRIC,
        target: Target::Fabric,
        file: "launcherskins-fabric-1.20.2-1.21.1-0.6.0.jar",
        sha256: "00c25ca1063d5c19ab5d084f585ae4d7b5b3aee3a1b73a0c120ed20efb6408ad",
        size: 33926,
    },
    Jar {
        games: BAND_1_20_6_NEOFORGE,
        target: Target::NeoForge,
        file: "launcherskins-neoforge-1.20.6-1.21.1-0.6.0.jar",
        sha256: "028b7593ab640f172cd389446cbb9bd9134d253f4e03366ec4cab54ff00d10ec",
        size: 32560,
    },
    Jar {
        games: &["1.21.1"],
        target: Target::Forge,
        file: "launcherskins-forge-1.21.1-0.6.0.jar",
        sha256: "645886e2d653fa15680ad8ae5d27f92a7af87f0f62d5f52a9ed7a96d8b5ed668",
        size: 32899,
    },
    Jar {
        games: &["1.21.1"],
        target: Target::Agent,
        file: "launcherskins-agent-1.21.1-0.6.0.jar",
        sha256: "123fc4073359b341927e3059c93e91620c06957af493d55713fca817a6046084",
        size: 221742,
    },
    Jar {
        games: &["1.21"],
        target: Target::Agent,
        file: "launcherskins-agent-1.21-0.6.0.jar",
        sha256: "37f346005241a47359281a203638bdac73cb0e589a24a9fd07df8be47255b52b",
        size: 221742,
    },
    Jar {
        games: &["1.20.6"],
        target: Target::Forge,
        file: "launcherskins-forge-1.20.6-0.6.0.jar",
        sha256: "52cc9a0a9814c77795fbe74b15477c46cbeeae0618fa0fcde7ab9fe17d7b1c64",
        size: 33828,
    },
    Jar {
        games: &["1.20.6"],
        target: Target::Agent,
        file: "launcherskins-agent-1.20.6-0.6.0.jar",
        sha256: "7c7f6d16284c92c23a5e1ca57663b9a897fce375af79232233620b7c378377b9",
        size: 221751,
    },
    Jar {
        games: &["1.20.5"],
        target: Target::Agent,
        file: "launcherskins-agent-1.20.5-0.6.0.jar",
        sha256: "60840c741811878aa12f9a43bb64cda65039faac531d21518755910111964063",
        size: 221740,
    },
    Jar {
        games: BAND_1_20_2,
        target: Target::NeoForge,
        file: "launcherskins-neoforge-1.20.2-1.20.4-0.6.0.jar",
        sha256: "4760ef77ca73edd45a0cedb25d89d49e2b69666dbe9fa726577f50a1d14deca6",
        size: 33790,
    },
    Jar {
        games: BAND_1_20_2,
        target: Target::Forge,
        file: "launcherskins-forge-1.20.2-1.20.4-0.6.0.jar",
        sha256: "b900e76b8269666b380a9f5166a10436d9706604bf2e57f8b528ea3dcbc44b3f",
        size: 33856,
    },
    Jar {
        games: &["1.20.4"],
        target: Target::Agent,
        file: "launcherskins-agent-1.20.4-0.6.0.jar",
        sha256: "76fffef3495c7a0f041aa753296aa0291daf6699c06e2d477d543c2e47103ebf",
        size: 221735,
    },
    Jar {
        games: &["1.20.3"],
        target: Target::Agent,
        file: "launcherskins-agent-1.20.3-0.6.0.jar",
        sha256: "b1a142ce94a1e92c2d43cfb809df51f8872f469b618a0a11856e3deb34bb491c",
        size: 221735,
    },
    Jar {
        games: &["1.20.2"],
        target: Target::Agent,
        file: "launcherskins-agent-1.20.2-0.6.0.jar",
        sha256: "b811c5e6610f468ca0019b8b22279d69167f74faa2843cd8bdc94377872f5899",
        size: 221749,
    },
    Jar {
        games: BAND_1_15_2_FABRIC,
        target: Target::Fabric,
        file: "launcherskins-fabric-1.15.2-1.20.1-0.6.0.jar",
        sha256: "078b45e6d86bc0452d6441a9da63bc4f6cd861e382ef601d6a2ccc8b60e679d4",
        size: 34090,
    },
    Jar {
        games: BAND_1_17_FORGE,
        target: Target::Forge,
        file: "launcherskins-forge-1.17.1-1.20.1-0.6.0.jar",
        sha256: "e2e05f14dfdfb566b38c76652b911f377cffa5510d7e98bc11289a7086260106",
        size: 33892,
    },
    Jar {
        games: &["1.16.5"],
        target: Target::Forge,
        file: "launcherskins-forge-1.16.5-0.6.0.jar",
        sha256: "474ef196c771c0079e049c97529a395a3860f8feaa9aa6abfb73faf7b6059f54",
        size: 34058,
    },
    Jar {
        games: &["1.20.1"],
        target: Target::Agent,
        file: "launcherskins-agent-1.20.1-0.6.0.jar",
        sha256: "67d3a6d5e020b72eca41bbd69d19528fdbe718acd3cf511832ba78adbf61b51d",
        size: 221839,
    },
    Jar {
        games: &["1.20"],
        target: Target::Agent,
        file: "launcherskins-agent-1.20-0.6.0.jar",
        sha256: "768cb20b2dcc429b36d3822e9cc0fc73c2e29a4b6284b41e72e69bae7845c63c",
        size: 221838,
    },
    Jar {
        games: &["1.19.4"],
        target: Target::Agent,
        file: "launcherskins-agent-1.19.4-0.6.0.jar",
        sha256: "6442649d3d70de94dbc176d1adb445348d5258f9cedff1b588ac461d306e9c1d",
        size: 221834,
    },
    Jar {
        games: &["1.19.3"],
        target: Target::Agent,
        file: "launcherskins-agent-1.19.3-0.6.0.jar",
        sha256: "2325f94efd74bdab48fcbeca43cc15b81a2801ae136a0311e6bdfb594d3a11d2",
        size: 221837,
    },
    Jar {
        games: &["1.19.2"],
        target: Target::Agent,
        file: "launcherskins-agent-1.19.2-0.6.0.jar",
        sha256: "1aad47c8b1d0285455fb5dc6e14417af119d4bb7af9b3f6f1f8063049b1c2bcb",
        size: 221834,
    },
    Jar {
        games: &["1.19.1"],
        target: Target::Agent,
        file: "launcherskins-agent-1.19.1-0.6.0.jar",
        sha256: "bc2bfc37c10bafb8aab2f29bea823023dfe1dd96b9a7ed5217ed75b98ae31eb6",
        size: 221832,
    },
    Jar {
        games: &["1.19"],
        target: Target::Agent,
        file: "launcherskins-agent-1.19-0.6.0.jar",
        sha256: "48e1dab495effdf6838cb4c477c728f5f4f001bdcd883b1a92cefcea8292c513",
        size: 221837,
    },
    Jar {
        games: &["1.18.2"],
        target: Target::Agent,
        file: "launcherskins-agent-1.18.2-0.6.0.jar",
        sha256: "99007798382375a82c7b5df9f81327e479756f864788a2a6ec7a21fbba12e950",
        size: 221838,
    },
    Jar {
        games: &["1.18.1"],
        target: Target::Agent,
        file: "launcherskins-agent-1.18.1-0.6.0.jar",
        sha256: "b5b9ed593068759da40af5539076cbe0e6fa1ea7151617727f4516f6688dcf8c",
        size: 221839,
    },
    Jar {
        games: &["1.18"],
        target: Target::Agent,
        file: "launcherskins-agent-1.18-0.6.0.jar",
        sha256: "1a791a7994a972bf8d2cc2afaf741fefcf47a5c38884948496ac7bec95545f2d",
        size: 221840,
    },
    Jar {
        games: &["1.17.1"],
        target: Target::Agent,
        file: "launcherskins-agent-1.17.1-0.6.0.jar",
        sha256: "0a46b57687b351f8f4f7e412c38e8bd0c1098c5840bd10f8c6243d72df8330e2",
        size: 221840,
    },
    Jar {
        games: &["1.17"],
        target: Target::Agent,
        file: "launcherskins-agent-1.17-0.6.0.jar",
        sha256: "e3f30451ddf5a56808217644dcb52eb6e656bc014a9e8690b1ca849a582ec3ce",
        size: 221842,
    },
    Jar {
        games: &["1.16.5"],
        target: Target::Agent,
        file: "launcherskins-agent-1.16.5-0.6.0.jar",
        sha256: "c06849fda323fd7c0091b286300ad2b9fd8dac2c5c8118aade9aeb5fdb236b3e",
        size: 221755,
    },
    Jar {
        games: &["1.16.4"],
        target: Target::Agent,
        file: "launcherskins-agent-1.16.4-0.6.0.jar",
        sha256: "f3e6d4fd86c7123e9cd4e6f9936d0331ca09a294d0c20f5f9dbc838a8ced7f59",
        size: 221756,
    },
    Jar {
        games: &["1.16.3"],
        target: Target::Agent,
        file: "launcherskins-agent-1.16.3-0.6.0.jar",
        sha256: "5075f7e6610d71109078fdbae25671efe905444bb69fd7abd931d9681af64d5d",
        size: 221754,
    },
    Jar {
        games: &["1.16.2"],
        target: Target::Agent,
        file: "launcherskins-agent-1.16.2-0.6.0.jar",
        sha256: "dc810ee8e987fc9019c22b9543fb29cdbb27824d6af0dcf4bf172169dfaaf2cb",
        size: 221754,
    },
    Jar {
        games: &["1.16.1"],
        target: Target::Agent,
        file: "launcherskins-agent-1.16.1-0.6.0.jar",
        sha256: "6e21e566d8ca648bd5307b5900481869eff496955394dfe071bc2a64f64dc9be",
        size: 221751,
    },
    Jar {
        games: &["1.16"],
        target: Target::Agent,
        file: "launcherskins-agent-1.16-0.6.0.jar",
        sha256: "9d12c2e75fe3708fe42b85a55f794e6c47ffe8e28739160cb139f4049c11ebc4",
        size: 221751,
    },
    Jar {
        games: &["1.15.2"],
        target: Target::Forge,
        file: "launcherskins-forge-1.15.2-0.6.0.jar",
        sha256: "6a68b969aea19531027593944ea26c13eee4e8f46e042aada8c024ff04266049",
        size: 33847,
    },
    Jar {
        games: &["1.15.2"],
        target: Target::Agent,
        file: "launcherskins-agent-1.15.2-0.6.0.jar",
        sha256: "8ce63f1ef8e55748f7c429f11293e11cf50c05357233219a9109f2a6ab91537d",
        size: 221750,
    },
    Jar {
        games: &["1.15", "1.15.1"],
        target: Target::Fabric,
        file: "launcherskins-fabric-1.15-1.15.1-0.6.0.jar",
        sha256: "88f6cd3f1d7dbeecfec6314a132b6a80fdd75cf31e60fe7e621dc2e3e75bbd94",
        size: 34087,
    },
    Jar {
        games: &["1.15.1"],
        target: Target::Agent,
        file: "launcherskins-agent-1.15.1-0.6.0.jar",
        sha256: "f9154dbb0bb8300f27aa852a947d525839d7d5245cf12983dc5126fb3745bfcb",
        size: 221749,
    },
    Jar {
        games: &["1.15"],
        target: Target::Agent,
        file: "launcherskins-agent-1.15-0.6.0.jar",
        sha256: "7b3171b4df0f19b3c26a4772e509a312f2aa81dd078205162e12aa262a3da275",
        size: 221750,
    },
    Jar {
        games: &["1.14.4"],
        target: Target::Fabric,
        file: "launcherskins-fabric-1.14.4-0.6.0.jar",
        sha256: "de93ff99462f988910a38e40e6edd67a02f4e0c1d363046018ca58e4017b32ad",
        size: 34084,
    },
    Jar {
        games: &["1.14.4"],
        target: Target::Forge,
        file: "launcherskins-forge-1.14.4-0.6.0.jar",
        sha256: "16dcbdafcc7a260557fcbb02b65fc99ca7e7f40d369ccd3905e4f92fac87e4c2",
        size: 33843,
    },
    Jar {
        games: &["1.14.4"],
        target: Target::Agent,
        file: "launcherskins-agent-1.14.4-0.6.0.jar",
        sha256: "1d88ad1b5c14b7a17ffc9e0e7a316400e8f108e0510320e1455378b3845ac0c5",
        size: 221758,
    },
    Jar {
        games: &["1.12.2"],
        target: Target::Forge,
        file: "launcherskins-forge-1.12.2-0.6.0.jar",
        sha256: "d493c414c38099d400fb0bb12042dd82fb8aae0d54d204b926f52f4b72325ee0",
        size: 34565,
    },
    Jar {
        games: &["1.8.9"],
        target: Target::Forge,
        file: "launcherskins-forge-1.8.9-0.6.0.jar",
        sha256: "3f0e42be7ea387646b4f2f86c1945d692f7b7185fd4e4906f3168a009dcd59a7",
        size: 34097,
    },
    Jar {
        games: &["1.7.10"],
        target: Target::Forge,
        file: "launcherskins-forge-1.7.10-0.6.0.jar",
        sha256: "7583120d24cd0511171190fc9db6ca09e723c1d9648d0a50d13087f679aa0e13",
        size: 34558,
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
