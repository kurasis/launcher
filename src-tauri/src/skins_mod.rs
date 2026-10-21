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
pub const MOD_VERSION: &str = "0.3.0";
/// Where the release keeps the jars.
const RELEASE: &str = "https://github.com/kurasis/launcher/releases/download/skins-mod-v0.3.0/";
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

const BAND_1_21: &[&str] = &["1.21", "1.21.1"];
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
    build(Target::Fabric, "1.21-1.21.1", BAND_1_21),
    build(Target::NeoForge, "1.21-1.21.1", BAND_1_21),
    build(Target::Forge, "1.21.1", &["1.21.1"]),
    build(Target::Agent, "1.21.1", &["1.21.1"]),
    build(Target::Agent, "1.21", &["1.21"]),
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
        file: "launcherskins-fabric-26.1-26.3-0.3.0.jar",
        sha256: "1861bd86bdef010a909fdb0e56eec1a847b2a0bc711a03fd09c97957fde2e4d6",
        size: 33488,
    },
    Jar {
        games: BAND_26,
        target: Target::NeoForge,
        file: "launcherskins-neoforge-26.1-26.3-0.3.0.jar",
        sha256: "eb5cac4f5af81d7d867cc950696a21d9100fa2f0ffc34d582bb436ec10db7354",
        size: 32580,
    },
    Jar {
        games: BAND_26,
        target: Target::Forge,
        file: "launcherskins-forge-26.1-26.3-0.3.0.jar",
        sha256: "0b155dcbcf9bb0ac43129cdc5339798942b20f9d6c3e2d7c16ea5b40eca5a660",
        size: 32929,
    },
    Jar {
        games: BAND_26,
        target: Target::Agent,
        file: "launcherskins-agent-26.1-26.3-0.3.0.jar",
        sha256: "1f77cf5b5f8d781ee3b95ae421160fcd1c32ce6af47099870b8005a6193d533a",
        size: 34756,
    },
    Jar {
        games: &["1.21.11"],
        target: Target::Fabric,
        file: "launcherskins-fabric-1.21.11-0.3.0.jar",
        sha256: "35a30561c76da690800afc0508825b6b480a3603197fdf75f3ba89ff45f37eb8",
        size: 33932,
    },
    Jar {
        games: &["1.21.11"],
        target: Target::NeoForge,
        file: "launcherskins-neoforge-1.21.11-0.3.0.jar",
        sha256: "5f477d0f87d927e240a33ed8e85fbef57bddfc54f2ca9ddb56850b20fff6e682",
        size: 32585,
    },
    Jar {
        games: &["1.21.11"],
        target: Target::Forge,
        file: "launcherskins-forge-1.21.11-0.3.0.jar",
        sha256: "5003c59394d2393fd00f83ff0338719c43e034e1c5b0f90e872f7f55fc331803",
        size: 32928,
    },
    Jar {
        games: &["1.21.11"],
        target: Target::Agent,
        file: "launcherskins-agent-1.21.11-0.3.0.jar",
        sha256: "6b66b572a2abeae96eec125629003ca44955eeeceace0f6d18e1f9093f047e16",
        size: 221745,
    },
    Jar {
        games: BAND_1_21_9,
        target: Target::Fabric,
        file: "launcherskins-fabric-1.21.9-1.21.10-0.3.0.jar",
        sha256: "f72d06d07bc7c12623ead32bd9543b2ac527884abea6f9b10236a2cf5e86598f",
        size: 33935,
    },
    Jar {
        games: BAND_1_21_9,
        target: Target::NeoForge,
        file: "launcherskins-neoforge-1.21.9-1.21.10-0.3.0.jar",
        sha256: "7270012023155b05a241ff0ef2aa1318cfc267dfec11d2802d68ac6ff8469a99",
        size: 32593,
    },
    Jar {
        games: BAND_1_21_9,
        target: Target::Forge,
        file: "launcherskins-forge-1.21.9-1.21.10-0.3.0.jar",
        sha256: "b68276b0522fbf5c18d0a2b4ebef965aacef5360c303aefa5bbbc34aaef82f30",
        size: 32934,
    },
    Jar {
        games: &["1.21.10"],
        target: Target::Agent,
        file: "launcherskins-agent-1.21.10-0.3.0.jar",
        sha256: "24a7b20d0a7008ade5b125a5304fd367c995856e1221f824fffea6aba9f96cdc",
        size: 221736,
    },
    Jar {
        games: &["1.21.9"],
        target: Target::Agent,
        file: "launcherskins-agent-1.21.9-0.3.0.jar",
        sha256: "8d96158d9e98dfb6831db1581f30db950f810e09d89ccb317e8fbd7987df6472",
        size: 221736,
    },
    Jar {
        games: BAND_1_21_5,
        target: Target::Fabric,
        file: "launcherskins-fabric-1.21.5-1.21.8-0.3.0.jar",
        sha256: "d38c002989d22c58104a7c77820181cde2fe5f2c030df75531bd803332b21ca3",
        size: 33916,
    },
    Jar {
        games: BAND_1_21_5,
        target: Target::NeoForge,
        file: "launcherskins-neoforge-1.21.5-1.21.8-0.3.0.jar",
        sha256: "21490047f09e945720485989cee0bcd0be54d202e00767d8e0349bc014e5c826",
        size: 32541,
    },
    Jar {
        games: BAND_1_21_5,
        target: Target::Forge,
        file: "launcherskins-forge-1.21.5-1.21.8-0.3.0.jar",
        sha256: "383a15fa5692f6af8c07d0d6c4ec0094b9d7d98f8a5129e1e6f8bbf7d50eeda2",
        size: 32881,
    },
    Jar {
        games: &["1.21.8"],
        target: Target::Agent,
        file: "launcherskins-agent-1.21.8-0.3.0.jar",
        sha256: "6ecc5c235aa0505bb36ff7cbb5ffff81a99e89e119d97162efa726d9a48ddb1d",
        size: 221719,
    },
    Jar {
        games: &["1.21.7"],
        target: Target::Agent,
        file: "launcherskins-agent-1.21.7-0.3.0.jar",
        sha256: "74e87882c781264af7589c6b94c5478fb97a4390515f8297c90f6c1a0512d126",
        size: 221723,
    },
    Jar {
        games: &["1.21.6"],
        target: Target::Agent,
        file: "launcherskins-agent-1.21.6-0.3.0.jar",
        sha256: "ac0535a5b6710e8f6912f0b3c9f2a531c9ecf7a03fa78b761ca925e2b3af1f9a",
        size: 221726,
    },
    Jar {
        games: &["1.21.5"],
        target: Target::Agent,
        file: "launcherskins-agent-1.21.5-0.3.0.jar",
        sha256: "fb58e1a21ee17b714d4d8bd589afcd2d87b5c5b1e3aef62e48c957c1c304f800",
        size: 221722,
    },
    Jar {
        games: BAND_1_21_2,
        target: Target::Fabric,
        file: "launcherskins-fabric-1.21.2-1.21.4-0.3.0.jar",
        sha256: "2f8f9268b105bd1cb7ecc1fd348c55b8553e8ae135fa4a00bb9ceeae44c14d70",
        size: 33811,
    },
    Jar {
        games: BAND_1_21_2,
        target: Target::NeoForge,
        file: "launcherskins-neoforge-1.21.2-1.21.4-0.3.0.jar",
        sha256: "2545099081a5fc08bb8e0c8e547f519f2fb8cb4347b8723d1dd740a4c3d39298",
        size: 32426,
    },
    Jar {
        games: BAND_1_21_2,
        target: Target::Forge,
        file: "launcherskins-forge-1.21.2-1.21.4-0.3.0.jar",
        sha256: "cd531f20858429b32545b4779560362d8fdebd982a0706ce1a0d65d468a8af9a",
        size: 32769,
    },
    Jar {
        games: &["1.21.4"],
        target: Target::Agent,
        file: "launcherskins-agent-1.21.4-0.3.0.jar",
        sha256: "81a324f39f10aeeabb9e0bc1593f267e08e19c24e2a4ff6bc33728fc3c37d340",
        size: 221618,
    },
    Jar {
        games: &["1.21.3"],
        target: Target::Agent,
        file: "launcherskins-agent-1.21.3-0.3.0.jar",
        sha256: "7ac1ce038f39c65b355f187fa12b70fa942335a068fb7b7bfea6f89777f2aefb",
        size: 221621,
    },
    Jar {
        games: &["1.21.2"],
        target: Target::Agent,
        file: "launcherskins-agent-1.21.2-0.3.0.jar",
        sha256: "99b2ce256b5bc5e39be2c3e7f0443375cada01ea2edf38f7cb2f91718c46b80a",
        size: 221620,
    },
    Jar {
        games: BAND_1_21,
        target: Target::Fabric,
        file: "launcherskins-fabric-1.21-1.21.1-0.3.0.jar",
        sha256: "f0f39c39db5a8de0084c6d23ee9c650561d34b02383299f6800489ef4d8f51da",
        size: 33897,
    },
    Jar {
        games: BAND_1_21,
        target: Target::NeoForge,
        file: "launcherskins-neoforge-1.21-1.21.1-0.3.0.jar",
        sha256: "336ee908f524a7f0734f9bea2bf0110e85b5a901a4a9c7b377f7840bd09478fb",
        size: 32530,
    },
    Jar {
        games: &["1.21.1"],
        target: Target::Forge,
        file: "launcherskins-forge-1.21.1-0.3.0.jar",
        sha256: "048f2c8030c9ae5e9fdcc4c639d101f73424c2c93cdd4a44e682955fc9df42f3",
        size: 32871,
    },
    Jar {
        games: &["1.21.1"],
        target: Target::Agent,
        file: "launcherskins-agent-1.21.1-0.3.0.jar",
        sha256: "7e5c37cd3c25fa4b489d7ed4807a227cbdae0ab8f33ce2e66c5a2acd1374f926",
        size: 221712,
    },
    Jar {
        games: &["1.21"],
        target: Target::Agent,
        file: "launcherskins-agent-1.21-0.3.0.jar",
        sha256: "e5714ce309049b5685fb26a380df70ef5d76051b49338622b5aebe6a921cdad4",
        size: 221712,
    },
    Jar {
        games: BAND_1_16_FABRIC,
        target: Target::Fabric,
        file: "launcherskins-fabric-1.16-1.20.1-0.3.0.jar",
        sha256: "300b1856057828cdbc5ec2509f6bebb3d7fecefc3224b3a9f47859b6bda30aeb",
        size: 34050,
    },
    Jar {
        games: BAND_1_17_FORGE,
        target: Target::Forge,
        file: "launcherskins-forge-1.17.1-1.20.1-0.3.0.jar",
        sha256: "a8951da35c6c660a79ec3dbc49931f5613b3f34f673c623aef65d2234f8d7847",
        size: 33856,
    },
    Jar {
        games: &["1.16.5"],
        target: Target::Forge,
        file: "launcherskins-forge-1.16.5-0.3.0.jar",
        sha256: "53a7e5d91984c128cd934bc9c8bbd22eb83fae751d56a3f98f69169d0f59261f",
        size: 34020,
    },
    Jar {
        games: &["1.20.1"],
        target: Target::Agent,
        file: "launcherskins-agent-1.20.1-0.3.0.jar",
        sha256: "0d545c467cb92917fbac7da07bf17f57ba125da24e8d5f3d35ab871b633250b1",
        size: 221811,
    },
    Jar {
        games: &["1.20"],
        target: Target::Agent,
        file: "launcherskins-agent-1.20-0.3.0.jar",
        sha256: "d439a04957d1544369ba03d63ece55c7104d89a1049044796d3201bd67c3d1f1",
        size: 221810,
    },
    Jar {
        games: &["1.19.4"],
        target: Target::Agent,
        file: "launcherskins-agent-1.19.4-0.3.0.jar",
        sha256: "94e30d252803f5edaa462f67ad4681a76f23b7d7224b7d3d554aaf3d33d43e3e",
        size: 221807,
    },
    Jar {
        games: &["1.19.3"],
        target: Target::Agent,
        file: "launcherskins-agent-1.19.3-0.3.0.jar",
        sha256: "ccec46f2c69363c73bc9ece8d5960ecf8961005edae88399561437e681153dd5",
        size: 221810,
    },
    Jar {
        games: &["1.19.2"],
        target: Target::Agent,
        file: "launcherskins-agent-1.19.2-0.3.0.jar",
        sha256: "74ab3ab4f6cd0da59539f09f6873ea2beea0dfde00dc4e003ef40ddac6632926",
        size: 221807,
    },
    Jar {
        games: &["1.19.1"],
        target: Target::Agent,
        file: "launcherskins-agent-1.19.1-0.3.0.jar",
        sha256: "df4b55624bed1104cd6c50d4162265cffecbaf8f0a34516d9885f9fa65612fde",
        size: 221805,
    },
    Jar {
        games: &["1.19"],
        target: Target::Agent,
        file: "launcherskins-agent-1.19-0.3.0.jar",
        sha256: "b1b533e87945688e6c2758a6e008d02d552450a9538da3991855bae1e7110d1e",
        size: 221809,
    },
    Jar {
        games: &["1.18.2"],
        target: Target::Agent,
        file: "launcherskins-agent-1.18.2-0.3.0.jar",
        sha256: "fddcfcec14dc5cc6853c1b678b7eaa5f16234a55cc8035dcc853c17cd496faad",
        size: 221810,
    },
    Jar {
        games: &["1.18.1"],
        target: Target::Agent,
        file: "launcherskins-agent-1.18.1-0.3.0.jar",
        sha256: "f4eb604e81a12991b3ad53539812656f86c8d86af461dc6f2b4b0ca587bc74aa",
        size: 221811,
    },
    Jar {
        games: &["1.18"],
        target: Target::Agent,
        file: "launcherskins-agent-1.18-0.3.0.jar",
        sha256: "902ae4ad3c93ad7a5afccbf6a76518b988710a34497d83de3fca8eda86714566",
        size: 221812,
    },
    Jar {
        games: &["1.17.1"],
        target: Target::Agent,
        file: "launcherskins-agent-1.17.1-0.3.0.jar",
        sha256: "a9afc6d9c89ed1c2087fd33e450efd7995cf73adf7c3b0e44e576cfcedd95e13",
        size: 221811,
    },
    Jar {
        games: &["1.17"],
        target: Target::Agent,
        file: "launcherskins-agent-1.17-0.3.0.jar",
        sha256: "e4cc1d66db9ab9a5c50dcf48497a8262f3f1ec8014cff61be467b6e1563aea20",
        size: 221813,
    },
    Jar {
        games: &["1.16.5"],
        target: Target::Agent,
        file: "launcherskins-agent-1.16.5-0.3.0.jar",
        sha256: "9b70e11c35867e5312f9dd9747af6927285572617fd762bb3fc5b2d2235d3485",
        size: 221720,
    },
    Jar {
        games: &["1.16.4"],
        target: Target::Agent,
        file: "launcherskins-agent-1.16.4-0.3.0.jar",
        sha256: "fd14b721152f44a1fb81561c281aa07d24113bc2dca456434a3708c5596510a0",
        size: 221720,
    },
    Jar {
        games: &["1.16.3"],
        target: Target::Agent,
        file: "launcherskins-agent-1.16.3-0.3.0.jar",
        sha256: "432eb6a5150293dc05f6288186407025a35e465975d3f5e2fdf924f7389bc9e6",
        size: 221718,
    },
    Jar {
        games: &["1.16.2"],
        target: Target::Agent,
        file: "launcherskins-agent-1.16.2-0.3.0.jar",
        sha256: "4d1db10075eb16495ea8b95a8e2414f226cf1d2e930677bd35ca016262fe9057",
        size: 221718,
    },
    Jar {
        games: &["1.16.1"],
        target: Target::Agent,
        file: "launcherskins-agent-1.16.1-0.3.0.jar",
        sha256: "30d1b12cdfa72de041c5bbd325c0e1b415f1f46162dc049e90d8e5038cc77a81",
        size: 221715,
    },
    Jar {
        games: &["1.16"],
        target: Target::Agent,
        file: "launcherskins-agent-1.16-0.3.0.jar",
        sha256: "eaf5bd98e9c1c1d772056573a4fe576e62fba862427e224964887ec40165c22e",
        size: 221715,
    },
    Jar {
        games: &["1.12.2"],
        target: Target::Forge,
        file: "launcherskins-forge-1.12.2-0.3.0.jar",
        sha256: "31c36e08ed1c9a1ff24438fb0dc1edb89791e0bdba92cb015bbb55b8982970d2",
        size: 34503,
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
fn is_ours(name: &str) -> bool {
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
