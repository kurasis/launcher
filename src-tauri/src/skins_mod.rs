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
pub const MOD_VERSION: &str = "0.2.1";
/// Where the release keeps the jars.
const RELEASE: &str = "https://github.com/kurasis/launcher/releases/download/skins-mod-v0.2.1/";
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
    build(Target::Fabric, "1.20.1", &["1.20.1"]),
    // NeoForge for 1.20.1 is still Forge and runs Forge's jar.
    build(Target::Forge, "1.20.1", &["1.20.1"]),
    build(Target::Agent, "1.20.1", &["1.20.1"]),
    build(Target::Fabric, "1.18.2", &["1.18.2"]),
    build(Target::Forge, "1.18.2", &["1.18.2"]),
    build(Target::Agent, "1.18.2", &["1.18.2"]),
    build(Target::Fabric, "1.16.5", &["1.16.5"]),
    build(Target::Forge, "1.16.5", &["1.16.5"]),
    build(Target::Agent, "1.16.5", &["1.16.5"]),
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
        file: "launcherskins-fabric-26.1-26.3-0.2.1.jar",
        sha256: "cbdca91112cb9a67d445ab739dbae46847f46c476d99833354c74be62f54c393",
        size: 33489,
    },
    Jar {
        games: BAND_26,
        target: Target::NeoForge,
        file: "launcherskins-neoforge-26.1-26.3-0.2.1.jar",
        sha256: "9883d75166e9a09fecd7db182d2559ebcb7a146bca0efed62a9b2518e9b74d02",
        size: 32580,
    },
    Jar {
        games: BAND_26,
        target: Target::Forge,
        file: "launcherskins-forge-26.1-26.3-0.2.1.jar",
        sha256: "20abdabf5a707a6b4336f98e12381b93376632991761fe6160fbbd9e53b5795c",
        size: 32929,
    },
    Jar {
        games: BAND_26,
        target: Target::Agent,
        file: "launcherskins-agent-26.1-26.3-0.2.1.jar",
        sha256: "42f37cf4cb0dad7f328ae7176877e762a93266a89ac3f8bab2a067b9c13862fc",
        size: 34757,
    },
    Jar {
        games: &["1.21.11"],
        target: Target::Fabric,
        file: "launcherskins-fabric-1.21.11-0.2.1.jar",
        sha256: "d07b6b4ba7551055814d1aed09bc20a42919a94e53118574863b6da9e9d118a6",
        size: 33932,
    },
    Jar {
        games: &["1.21.11"],
        target: Target::NeoForge,
        file: "launcherskins-neoforge-1.21.11-0.2.1.jar",
        sha256: "75c97f7cc48ff6919b18e9a401fd9f04e22d50543fd6e1ad9f7290d3e145e23a",
        size: 32584,
    },
    Jar {
        games: &["1.21.11"],
        target: Target::Forge,
        file: "launcherskins-forge-1.21.11-0.2.1.jar",
        sha256: "a226661eba2e4857f255f94ff64b82ccf21b39cb681a121e8704173b1e9524e1",
        size: 32928,
    },
    Jar {
        games: &["1.21.11"],
        target: Target::Agent,
        file: "launcherskins-agent-1.21.11-0.2.1.jar",
        sha256: "39c0ca64f9f0a6b3f1b64cce713bcd23e357e5054c72225a725cd422403af97f",
        size: 221747,
    },
    Jar {
        games: BAND_1_21_9,
        target: Target::Fabric,
        file: "launcherskins-fabric-1.21.9-1.21.10-0.2.1.jar",
        sha256: "d8200328d7e21afae742cfb792efb3e7c11ce89b6534b700d9011f2ae2ced11b",
        size: 33936,
    },
    Jar {
        games: BAND_1_21_9,
        target: Target::NeoForge,
        file: "launcherskins-neoforge-1.21.9-1.21.10-0.2.1.jar",
        sha256: "8751c1a2b10a6edb99cb100c3bb638e14eaa140051264cb4b8863acf1fcf8e54",
        size: 32592,
    },
    Jar {
        games: BAND_1_21_9,
        target: Target::Forge,
        file: "launcherskins-forge-1.21.9-1.21.10-0.2.1.jar",
        sha256: "bd7d2ecbc565932e256d3cf8304d769ff804d0ad8a998d0482c0e438792acdef",
        size: 32933,
    },
    Jar {
        games: &["1.21.10"],
        target: Target::Agent,
        file: "launcherskins-agent-1.21.10-0.2.1.jar",
        sha256: "e8ca315d8625497faad99d6fb088a8efab28d2ffc1d7a4aaff69ee2fa8f48bf6",
        size: 221738,
    },
    Jar {
        games: &["1.21.9"],
        target: Target::Agent,
        file: "launcherskins-agent-1.21.9-0.2.1.jar",
        sha256: "f1d725b1e2dda786f657c7f6bb1517e7d2c13133a413a90b2eef47dfcda01010",
        size: 221739,
    },
    Jar {
        games: BAND_1_21_5,
        target: Target::Fabric,
        file: "launcherskins-fabric-1.21.5-1.21.8-0.2.1.jar",
        sha256: "4c22560aabbe5c26d9b9326caa5f166213b7aa6c48b58b03a36ab0767e09d8f2",
        size: 33916,
    },
    Jar {
        games: BAND_1_21_5,
        target: Target::NeoForge,
        file: "launcherskins-neoforge-1.21.5-1.21.8-0.2.1.jar",
        sha256: "3f2e8244fc1ec8d6db0e97001f95683d7aef7226f1918a9d12b26246f3f83a3e",
        size: 32541,
    },
    Jar {
        games: BAND_1_21_5,
        target: Target::Forge,
        file: "launcherskins-forge-1.21.5-1.21.8-0.2.1.jar",
        sha256: "ec6cc1673c317de2ad4e3dc4bdc297cec9bfb8a441aad38ee45d42205488348a",
        size: 32881,
    },
    Jar {
        games: &["1.21.8"],
        target: Target::Agent,
        file: "launcherskins-agent-1.21.8-0.2.1.jar",
        sha256: "02d563fe680674918580e8657919f3bafc0abc57f2826e651e0497a2812c972c",
        size: 221721,
    },
    Jar {
        games: &["1.21.7"],
        target: Target::Agent,
        file: "launcherskins-agent-1.21.7-0.2.1.jar",
        sha256: "25eb51189b3339f951d8b7922f2d09ecabcbb1b342d1e044dc5fbe4208f37017",
        size: 221725,
    },
    Jar {
        games: &["1.21.6"],
        target: Target::Agent,
        file: "launcherskins-agent-1.21.6-0.2.1.jar",
        sha256: "d049d88eb5aa262f2e52d5402fa6ece49082a00100f0490c045cb746abba6312",
        size: 221728,
    },
    Jar {
        games: &["1.21.5"],
        target: Target::Agent,
        file: "launcherskins-agent-1.21.5-0.2.1.jar",
        sha256: "fd3529fea8ad7e8d9c966eae9b8d105833b4fb9d4def202cb3fca407eed66201",
        size: 221724,
    },
    Jar {
        games: BAND_1_21_2,
        target: Target::Fabric,
        file: "launcherskins-fabric-1.21.2-1.21.4-0.2.1.jar",
        sha256: "ab45a1f60dc2f567512be793c9774f16c922c2e2d0675928a5997a80b864ca83",
        size: 33812,
    },
    Jar {
        games: BAND_1_21_2,
        target: Target::NeoForge,
        file: "launcherskins-neoforge-1.21.2-1.21.4-0.2.1.jar",
        sha256: "820a8d7994e3bc840a8b6d274e87809ad0362b271e358daf0adb0320c27694ae",
        size: 32425,
    },
    Jar {
        games: BAND_1_21_2,
        target: Target::Forge,
        file: "launcherskins-forge-1.21.2-1.21.4-0.2.1.jar",
        sha256: "286a99a90c3c62d13aa569f229a9e83e66cf45eb34e25a892adb9e13b2e1195f",
        size: 32769,
    },
    Jar {
        games: &["1.21.4"],
        target: Target::Agent,
        file: "launcherskins-agent-1.21.4-0.2.1.jar",
        sha256: "de995fedafb664c5e889049428c3405350299b598b1b89b440c1605527576620",
        size: 221620,
    },
    Jar {
        games: &["1.21.3"],
        target: Target::Agent,
        file: "launcherskins-agent-1.21.3-0.2.1.jar",
        sha256: "0f9f505055ff318da15980e519b4c832d724198976e98ac411a1d50776cd6bf0",
        size: 221623,
    },
    Jar {
        games: &["1.21.2"],
        target: Target::Agent,
        file: "launcherskins-agent-1.21.2-0.2.1.jar",
        sha256: "38da5654312547c2701410853c80a626263d112f9c627783bb6188196cc2a4bf",
        size: 221623,
    },
    Jar {
        games: BAND_1_21,
        target: Target::Fabric,
        file: "launcherskins-fabric-1.21-1.21.1-0.2.1.jar",
        sha256: "4c4a18a3cb73d2ff750351270c24579a06b0f46309dd716298e4a632f9bf2155",
        size: 33900,
    },
    Jar {
        games: BAND_1_21,
        target: Target::NeoForge,
        file: "launcherskins-neoforge-1.21-1.21.1-0.2.1.jar",
        sha256: "c21f9d02b36dc456835df9c221bd3fef967d3bbdf0471f75c09196c476177172",
        size: 32530,
    },
    Jar {
        games: &["1.21.1"],
        target: Target::Forge,
        file: "launcherskins-forge-1.21.1-0.2.1.jar",
        sha256: "76c9d1a27dcf9f14ecb0fe0e77a27e41f3ede56629eca580d414355923fa2162",
        size: 32871,
    },
    Jar {
        games: &["1.21.1"],
        target: Target::Agent,
        file: "launcherskins-agent-1.21.1-0.2.1.jar",
        sha256: "46cad8247f0861f798e78fd25d2c17fe97bb21b9efc97f77988d86eff4be7c7e",
        size: 221714,
    },
    Jar {
        games: &["1.21"],
        target: Target::Agent,
        file: "launcherskins-agent-1.21-0.2.1.jar",
        sha256: "d2b65eae75340471097ba321aee286e2702c6bec2d0baa915028baf55df8ae26",
        size: 221714,
    },
    Jar {
        games: &["1.20.1"],
        target: Target::Fabric,
        file: "launcherskins-fabric-1.20.1-0.2.1.jar",
        sha256: "09356331607690f6f3dd43df7fd51ff7d3ca43586bc6a8c09d780a45098b2c10",
        size: 34049,
    },
    Jar {
        games: &["1.20.1"],
        target: Target::Forge,
        file: "launcherskins-forge-1.20.1-0.2.1.jar",
        sha256: "405b3d15e4af1a381aef456a96b16bceb70ae2695d77126f85202da855968184",
        size: 33852,
    },
    Jar {
        games: &["1.20.1"],
        target: Target::Agent,
        file: "launcherskins-agent-1.20.1-0.2.1.jar",
        sha256: "d3421c0f1b3cf6f5ac69a3c2a00e3564a4dae30cf1a24dad48b456bc1ed5683c",
        size: 221813,
    },
    Jar {
        games: &["1.18.2"],
        target: Target::Fabric,
        file: "launcherskins-fabric-1.18.2-0.2.1.jar",
        sha256: "6942056ad08e2a39630f15b1fa4c6c696529f746a00e00e4e53910ff4a0206d1",
        size: 34049,
    },
    Jar {
        games: &["1.18.2"],
        target: Target::Forge,
        file: "launcherskins-forge-1.18.2-0.2.1.jar",
        sha256: "eecd6ec816659d35c9638b2149c21d656950a0763fda6ec4838679469c2b1f61",
        size: 33851,
    },
    Jar {
        games: &["1.18.2"],
        target: Target::Agent,
        file: "launcherskins-agent-1.18.2-0.2.1.jar",
        sha256: "f8cdae53ff657d4ba3799592bf2f61fde7185b0f32a4474e7404e3b813e32f22",
        size: 221812,
    },
    Jar {
        games: &["1.16.5"],
        target: Target::Fabric,
        file: "launcherskins-fabric-1.16.5-0.2.1.jar",
        sha256: "997e23c335283394ba79da3bfd65aeec630e9d6102dbe95748e6dfff7159e2e5",
        size: 34048,
    },
    Jar {
        games: &["1.16.5"],
        target: Target::Forge,
        file: "launcherskins-forge-1.16.5-0.2.1.jar",
        sha256: "9c63acddf21173200f6932805a48e859364cb69b8b0409289fd304ecb9804225",
        size: 34021,
    },
    Jar {
        games: &["1.16.5"],
        target: Target::Agent,
        file: "launcherskins-agent-1.16.5-0.2.1.jar",
        sha256: "e6297bd682d2891d16aa2070d6418054bae93efcade2b6b24e6876ab830a7454",
        size: 221722,
    },
    Jar {
        games: &["1.12.2"],
        target: Target::Forge,
        file: "launcherskins-forge-1.12.2-0.2.1.jar",
        sha256: "250c98d35a3f53385791d6e52bc998ee740a4e91a225d0618ed117c9d9274b94",
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
