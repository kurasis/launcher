//! DATA_ROOT layout and canonical cache keys (docs/spec/02 "Data directories",
//! doc 10 "Identity and storage contract").
//!
//! Every path is derived from validated identifiers. Display names, URLs and Maven
//! coordinates never become path components, so no input can escape DATA_ROOT.

use std::fmt;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use uuid::Uuid;

use crate::error::{ErrorCode, LauncherError};

/// Java's classpath separator on this platform. A DATA_ROOT that contains it cannot
/// be represented unambiguously in a classpath (doc 05 "path boundaries").
pub const CLASSPATH_SEPARATOR: char = if cfg!(windows) { ';' } else { ':' };

/// A validated, absolute application data root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataRoot {
    root: PathBuf,
}

impl DataRoot {
    /// Validates a candidate root. The directory does not need to exist yet.
    pub fn new(root: PathBuf) -> Result<Self, LauncherError> {
        if !root.is_absolute() {
            return Err(LauncherError::new(
                ErrorCode::UnsupportedDataPath,
                "The launcher data directory must be an absolute path.",
            ));
        }
        let Some(text) = root.to_str() else {
            return Err(LauncherError::new(
                ErrorCode::UnsupportedPathEncoding,
                "The launcher data directory path cannot be represented as Unicode text, \
                 which Java requires for game paths.",
            ));
        };
        // Windows drive prefixes contain ':', which is not the Windows separator.
        if text.contains(CLASSPATH_SEPARATOR) {
            return Err(LauncherError::templated(
                ErrorCode::UnsupportedDataPath,
                "The launcher data directory contains the character {character}, which Java \
                 cannot use in a classpath. Use a directory without it.",
                crate::error::params([("character", CLASSPATH_SEPARATOR.into())]),
            ));
        }
        if text.contains('\0') {
            return Err(LauncherError::new(
                ErrorCode::UnsupportedDataPath,
                "The launcher data directory path is invalid.",
            ));
        }
        Ok(Self { root })
    }

    pub fn path(&self) -> &Path {
        &self.root
    }

    pub fn settings(&self) -> PathBuf {
        self.root.join("settings.json")
    }

    pub fn settings_backup(&self) -> PathBuf {
        self.root.join("settings.json.bak")
    }

    pub fn accounts(&self) -> PathBuf {
        self.root.join("accounts.json")
    }

    /// Traffic the launcher downloaded, by month (D-051).
    pub fn traffic(&self) -> PathBuf {
        self.root.join("traffic.json")
    }

    pub fn compatibility(&self) -> PathBuf {
        self.root.join("compatibility.json")
    }

    pub fn locks_dir(&self) -> PathBuf {
        self.root.join("locks")
    }

    pub fn supervisor_lock(&self) -> PathBuf {
        self.locks_dir().join("supervisor.lock")
    }

    pub fn instances_dir(&self) -> PathBuf {
        self.root.join("instances")
    }

    pub fn instance_dir(&self, id: InstanceId) -> PathBuf {
        self.instances_dir().join(id.to_string())
    }

    pub fn instance_file(&self, id: InstanceId) -> PathBuf {
        self.instance_dir(id).join("instance.json")
    }

    pub fn install_lock(&self, id: InstanceId) -> PathBuf {
        self.instance_dir(id).join("install.lock.json")
    }

    pub fn game_dir(&self, id: InstanceId) -> PathBuf {
        self.instance_dir(id).join("game")
    }

    pub fn run_dir(&self, id: InstanceId, run: Uuid) -> PathBuf {
        self.instance_dir(id).join("runs").join(run.to_string())
    }

    pub fn catalog(&self) -> PathBuf {
        self.root.join("metadata").join("catalog.json")
    }

    pub fn catalog_receipt(&self) -> PathBuf {
        self.root.join("metadata").join("catalog.receipt.json")
    }

    /// The signed list of versions Mojang's list does not have (D-073).
    pub fn extra_versions(&self) -> PathBuf {
        self.root.join("metadata").join("extra-versions.json")
    }

    pub fn extra_versions_receipt(&self) -> PathBuf {
        self.root
            .join("metadata")
            .join("extra-versions.receipt.json")
    }

    pub fn version_descriptor(&self, version: &SafeName, sha1: &Sha1) -> PathBuf {
        self.root
            .join("metadata")
            .join("versions")
            .join(version.as_str())
            .join(format!("{sha1}.json"))
    }

    pub fn asset_index_source(&self, sha1: &Sha1) -> PathBuf {
        self.root
            .join("metadata")
            .join("asset-indexes")
            .join(format!("{sha1}.json"))
    }

    #[cfg(test)]
    pub fn client_jar(&self, sha1: &Sha1) -> PathBuf {
        self.shared()
            .join("clients")
            .join(sha1.as_str())
            .join("client.jar")
    }

    pub fn library(&self, sha1: &Sha1) -> PathBuf {
        self.shared()
            .join("libraries")
            .join(sha1.as_str())
            .join("artifact.jar")
    }

    /// Maven-layout library tree for mod-loader installations (D-039): Forge and
    /// NeoForge find libraries and patched game files by their Maven path.
    pub fn maven_root(&self) -> PathBuf {
        self.shared().join("maven")
    }

    /// A resolved mod-loader manifest, stored under its SHA-1 (D-039).
    pub fn loader_manifest(&self, sha1: &Sha1) -> PathBuf {
        self.root
            .join("metadata")
            .join("loaders")
            .join(format!("{sha1}.json"))
    }

    /// The launcher's tweaker for games before 1.6, named by a prefix of its
    /// SHA-256 (hex).
    pub fn legacy_tweaker(&self, digest: &str) -> PathBuf {
        self.shared()
            .join("launcher")
            .join(format!("legacy-tweaker-{digest}.jar"))
    }

    pub fn assets_root(&self) -> PathBuf {
        self.shared().join("assets")
    }

    pub fn asset_object(&self, sha1: &Sha1) -> PathBuf {
        self.assets_root()
            .join("objects")
            .join(&sha1.as_str()[..2])
            .join(sha1.as_str())
    }

    pub fn asset_index_projection(&self, index_id: &SafeName) -> PathBuf {
        self.assets_root()
            .join("indexes")
            .join(format!("{}.json", index_id.as_str()))
    }

    pub fn runtimes_dir(&self) -> PathBuf {
        self.root.join("runtimes")
    }

    pub fn runtime_dir(&self, key: &RuntimeKey) -> PathBuf {
        self.runtimes_dir()
            .join(key.provider.as_str())
            .join(format!("{}-{}", key.os.as_str(), key.arch.as_str()))
            .join(key.major.to_string())
            .join(key.image_type.as_str())
            .join(key.archive_sha256.as_str())
    }

    pub fn jobs_dir(&self) -> PathBuf {
        self.root.join("jobs")
    }

    pub fn job_receipt(&self, job: Uuid) -> PathBuf {
        self.jobs_dir().join(format!("{job}.json"))
    }

    pub fn downloads_dir(&self) -> PathBuf {
        self.root.join("downloads")
    }

    /// Job-owned staging directory; removed when the job ends.
    pub fn download_staging(&self, job: Uuid) -> PathBuf {
        self.downloads_dir().join(job.to_string())
    }

    pub fn download_part(&self, job: Uuid, algorithm: &str, digest: &str) -> PathBuf {
        self.download_staging(job)
            .join(format!("{algorithm}-{digest}.part"))
    }

    /// Small copies of screenshots for the gallery; can be deleted at any time.
    pub fn thumbnails_dir(&self) -> PathBuf {
        self.root.join("cache").join("thumbnails")
    }

    /// Copies of the start page feed, its notes and pictures (D-067); can be
    /// deleted at any time.
    pub fn feed_cache_dir(&self) -> PathBuf {
        self.root.join("cache").join("feed")
    }

    /// The YouTube channels a player added to the feed (D-067). Kept on this
    /// computer only.
    pub fn feed_channels(&self) -> PathBuf {
        self.root.join("feed-channels.json")
    }

    /// The feed's videos a player opened, to show them fainter (D-067).
    /// Kept on this computer only.
    pub fn feed_watched(&self) -> PathBuf {
        self.root.join("feed-watched.json")
    }

    /// The launcher's log and its failure records.
    pub fn logs_dir(&self) -> PathBuf {
        self.root.join("logs")
    }

    /// One folder per game run.
    pub fn runs_dir(&self) -> PathBuf {
        self.logs_dir().join("runs")
    }

    pub fn run_logs(&self, run: Uuid) -> PathBuf {
        self.runs_dir().join(run.to_string())
    }

    /// Worlds hosted by this launcher (D-068). Not game files: a reset of game
    /// files keeps them.
    pub fn worlds_dir(&self) -> PathBuf {
        self.root.join("worlds")
    }

    /// Backups of the hosted worlds, in a store of their own (W9). Not a
    /// world id, so the world list passes it by.
    pub fn world_backups_dir(&self) -> PathBuf {
        self.worlds_dir().join("backups")
    }

    pub fn world_dir(&self, id: WorldId) -> PathBuf {
        self.worlds_dir().join(id.to_string())
    }

    pub fn world_file(&self, id: WorldId) -> PathBuf {
        self.world_dir(id).join("world.json")
    }

    /// The server's working directory: `server.properties`, `whitelist.json`,
    /// `eula.txt` and the save in `world/`.
    pub fn world_server_dir(&self, id: WorldId) -> PathBuf {
        self.world_dir(id).join("server")
    }

    /// The mods of a world with mods, each file under its SHA-256 (W12).
    pub fn world_pack_dir(&self, id: WorldId) -> PathBuf {
        self.world_dir(id).join("pack")
    }

    /// A Mojang server JAR, shared by every world of its version.
    pub fn server_jar(&self, sha1: &Sha1) -> PathBuf {
        self.shared()
            .join("servers")
            .join(sha1.as_str())
            .join("server.jar")
    }

    /// Scratch space for one run of a loader's server-side processors, beside
    /// the server JARs it reads.
    pub fn server_processing_dir(&self, run: Uuid) -> PathBuf {
        self.shared()
            .join("servers")
            .join(format!("processing-{run}"))
    }

    fn shared(&self) -> PathBuf {
        self.root.join("shared")
    }
}

/// [`DataRoot::server_jar`] relative to DATA_ROOT, for a download request.
pub fn server_jar_relative(sha1: &Sha1) -> String {
    format!("shared/servers/{sha1}/server.jar")
}

/// Generated instance identifier. Only canonical lowercase hyphenated UUIDs parse,
/// so an identifier can never contain path separators or `..`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct InstanceId(Uuid);

impl InstanceId {
    pub fn generate() -> Self {
        Self(Uuid::new_v4())
    }
}

impl fmt::Display for InstanceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.hyphenated().fmt(f)
    }
}

impl FromStr for InstanceId {
    type Err = LauncherError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let invalid = || LauncherError::invalid_input("The installation identifier is invalid.");
        if s.len() != 36 {
            return Err(invalid());
        }
        let uuid = Uuid::parse_str(s).map_err(|_| invalid())?;
        if uuid.hyphenated().to_string() != s {
            return Err(invalid());
        }
        Ok(Self(uuid))
    }
}

impl Serialize for InstanceId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for InstanceId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        text.parse()
            .map_err(|e: LauncherError| serde::de::Error::custom(e.message))
    }
}

/// Generated world identifier, canonical like [`InstanceId`] (D-068).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct WorldId(Uuid);

impl WorldId {
    pub fn generate() -> Self {
        Self(Uuid::new_v4())
    }

    pub fn uuid(self) -> Uuid {
        self.0
    }
}

impl fmt::Display for WorldId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.hyphenated().fmt(f)
    }
}

impl FromStr for WorldId {
    type Err = LauncherError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let invalid = || LauncherError::invalid_input("The world identifier is invalid.");
        if s.len() != 36 {
            return Err(invalid());
        }
        let uuid = Uuid::parse_str(s).map_err(|_| invalid())?;
        if uuid.hyphenated().to_string() != s {
            return Err(invalid());
        }
        Ok(Self(uuid))
    }
}

impl Serialize for WorldId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for WorldId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        text.parse()
            .map_err(|e: LauncherError| serde::de::Error::custom(e.message))
    }
}

macro_rules! hex_digest {
    ($name:ident, $len:expr, $label:expr) => {
        #[doc = concat!("A normalized lowercase ", $label, " digest.")]
        #[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $name(String);

        impl $name {
            pub const HEX_LEN: usize = $len;

            /// Accepts upper- or lowercase hex of the exact length; stores lowercase.
            pub fn parse(text: &str) -> Result<Self, LauncherError> {
                if text.len() == $len && text.bytes().all(|b| b.is_ascii_hexdigit()) {
                    Ok(Self(text.to_ascii_lowercase()))
                } else {
                    Err(LauncherError::new(
                        ErrorCode::UnsupportedVersionMetadata,
                        concat!("A ", $label, " digest in metadata is malformed."),
                    ))
                }
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl Serialize for $name {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_str(&self.0)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let text = String::deserialize(deserializer)?;
                Self::parse(&text).map_err(|e| serde::de::Error::custom(e.message))
            }
        }
    };
}

hex_digest!(Sha1, 40, "SHA-1");
hex_digest!(Sha256, 64, "SHA-256");

/// A short identifier that is safe as a single path component: version IDs, asset
/// index IDs and logging configuration file names from metadata.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub struct SafeName(String);

impl SafeName {
    pub const MAX_LEN: usize = 64;

    pub fn parse(text: &str) -> Result<Self, LauncherError> {
        let valid = !text.is_empty()
            && text.len() <= Self::MAX_LEN
            && text != "."
            && text != ".."
            && !text.starts_with('.')
            // Windows drops trailing dots and spaces from names.
            && !text.starts_with(' ')
            && !text.ends_with([' ', '.'])
            // Spaces occur in old pre-release IDs such as "1.14 Pre-Release 5".
            && text
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-' | b' '));
        if valid && !is_windows_reserved(text) {
            Ok(Self(text.to_owned()))
        } else {
            Err(LauncherError::new(
                ErrorCode::UnsupportedVersionMetadata,
                "An identifier in metadata is not a safe file name.",
            ))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for SafeName {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::parse(&text).map_err(|e| serde::de::Error::custom(e.message))
    }
}

fn is_windows_reserved(text: &str) -> bool {
    let stem = text
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ((stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.len() == 4
            && stem.as_bytes()[3].is_ascii_digit())
}

/// Target operating system for runtime and artifact selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TargetOs {
    Windows,
    Mac,
    Linux,
}

impl TargetOs {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Windows => "windows",
            Self::Mac => "mac",
            Self::Linux => "linux",
        }
    }
}

/// Target CPU architecture using Adoptium naming.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TargetArch {
    X64,
    Aarch64,
}

impl TargetArch {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::X64 => "x64",
            Self::Aarch64 => "aarch64",
        }
    }
}

/// Java image type of a runtime package.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ImageType {
    Jre,
    Jdk,
}

impl ImageType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Jre => "jre",
            Self::Jdk => "jdk",
        }
    }
}

/// Where private Java runtimes come from (D-036).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum JavaProvider {
    /// Mojang's own runtimes, the ones the official launcher uses.
    #[default]
    Mojang,
    /// Eclipse Temurin from Adoptium.
    Temurin,
}

impl JavaProvider {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Mojang => "mojang",
            Self::Temurin => "temurin",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "mojang" => Some(Self::Mojang),
            "temurin" => Some(Self::Temurin),
            _ => None,
        }
    }

    /// Keys written before Mojang runtimes existed are Temurin's.
    fn legacy() -> Self {
        Self::Temurin
    }
}

/// Identity of an extracted private Java runtime (doc 10, R2-01): two packages with
/// the same release label but a different image type or archive digest never share
/// a directory. For Mojang runtimes the digest is the SHA-256 of the component
/// manifest, which pins every file.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeKey {
    #[serde(default = "JavaProvider::legacy")]
    pub provider: JavaProvider,
    pub os: TargetOs,
    pub arch: TargetArch,
    pub major: u16,
    pub image_type: ImageType,
    pub archive_sha256: Sha256,
}

/// Returns true when `candidate` is lexically inside `root`. Used as a defence in
/// depth check for paths assembled from validated components.
pub fn is_within(root: &Path, candidate: &Path) -> bool {
    use std::path::Component;
    let Ok(relative) = candidate.strip_prefix(root) else {
        return false;
    };
    relative
        .components()
        .all(|component| matches!(component, Component::Normal(_)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root() -> DataRoot {
        let base = if cfg!(windows) {
            PathBuf::from(r"C:\Users\Test User\AppData\Local\app.launcher")
        } else {
            PathBuf::from("/home/test user/.local/share/app.launcher")
        };
        DataRoot::new(base).unwrap()
    }

    const SHA1: &str = "499680d659b7a0ccb2fa9a809c345c5baf2ace7d";

    #[test]
    fn rejects_relative_and_classpath_separator_roots() {
        assert_eq!(
            DataRoot::new(PathBuf::from("relative/dir"))
                .unwrap_err()
                .code,
            ErrorCode::UnsupportedDataPath
        );
        let bad = if cfg!(windows) {
            r"C:\games;old\launcher"
        } else {
            "/home/a:b/launcher"
        };
        assert_eq!(
            DataRoot::new(PathBuf::from(bad)).unwrap_err().code,
            ErrorCode::UnsupportedDataPath
        );
    }

    #[test]
    fn accepts_unicode_and_spaces() {
        let text = if cfg!(windows) {
            r"C:\Users\Jürgen Ölsen\データ"
        } else {
            "/home/jürgen ölsen/データ"
        };
        assert!(DataRoot::new(PathBuf::from(text)).is_ok());
    }

    #[test]
    fn instance_ids_only_accept_canonical_uuids() {
        let id = InstanceId::generate();
        assert_eq!(id.to_string().parse::<InstanceId>().unwrap(), id);
        for bad in [
            "",
            "..",
            "../../etc/passwd",
            "0F1E2D3C-4B5A-6978-8796-A5B4C3D2E1F0",
            "0f1e2d3c4b5a69788796a5b4c3d2e1f0",
            "{0f1e2d3c-4b5a-6978-8796-a5b4c3d2e1f0}",
            "0f1e2d3c-4b5a-6978-8796-a5b4c3d2e1f0/..",
            "urn:uuid:0f1e2d3c-4b5a-6978-8796-a5b4c3d2e1f0",
        ] {
            assert!(bad.parse::<InstanceId>().is_err(), "{bad}");
        }
    }

    #[test]
    fn instance_paths_stay_inside_root() {
        let root = root();
        let id = InstanceId::generate();
        for path in [
            root.instance_file(id),
            root.install_lock(id),
            root.game_dir(id),
            root.run_dir(id, Uuid::new_v4()),
        ] {
            assert!(is_within(root.path(), &path), "{}", path.display());
        }
    }

    #[test]
    fn canonical_content_paths_use_digests_not_names() {
        let root = root();
        let sha1 = Sha1::parse(SHA1).unwrap();
        let client = root.client_jar(&sha1);
        assert!(client.ends_with(Path::new("shared/clients").join(SHA1).join("client.jar")));
        let library = root.library(&sha1);
        assert!(
            library.ends_with(
                Path::new("shared/libraries")
                    .join(SHA1)
                    .join("artifact.jar")
            )
        );
        let object = root.asset_object(&sha1);
        assert!(object.ends_with(Path::new("shared/assets/objects/49").join(SHA1)));
        for path in [client, library, object] {
            assert!(is_within(root.path(), &path));
        }
    }

    #[test]
    fn runtime_key_separates_image_type_and_archive() {
        let root = root();
        let digest = Sha256::parse(&"ab".repeat(32)).unwrap();
        let jre = RuntimeKey {
            provider: JavaProvider::Temurin,
            os: TargetOs::Linux,
            arch: TargetArch::X64,
            major: 21,
            image_type: ImageType::Jre,
            archive_sha256: digest.clone(),
        };
        let jdk = RuntimeKey {
            image_type: ImageType::Jdk,
            ..jre.clone()
        };
        let other = RuntimeKey {
            archive_sha256: Sha256::parse(&"cd".repeat(32)).unwrap(),
            ..jre.clone()
        };
        let mojang = RuntimeKey {
            provider: JavaProvider::Mojang,
            ..jre.clone()
        };
        let paths = [
            root.runtime_dir(&jre),
            root.runtime_dir(&jdk),
            root.runtime_dir(&other),
            root.runtime_dir(&mojang),
        ];
        assert_ne!(paths[0], paths[1]);
        assert_ne!(paths[0], paths[2]);
        assert!(
            paths[0]
                .ends_with(Path::new("runtimes/temurin/linux-x64/21/jre").join("ab".repeat(32)))
        );
        assert!(
            paths[3].ends_with(Path::new("runtimes/mojang/linux-x64/21/jre").join("ab".repeat(32)))
        );
    }

    #[test]
    fn runtime_keys_without_a_provider_are_temurin() {
        let key: RuntimeKey = serde_json::from_value(serde_json::json!({
            "os": "linux",
            "arch": "x64",
            "major": 21,
            "imageType": "jre",
            "archiveSha256": "ab".repeat(32),
        }))
        .unwrap();
        assert_eq!(key.provider, JavaProvider::Temurin);
        assert_eq!(JavaProvider::default(), JavaProvider::Mojang);
    }

    #[test]
    fn digests_are_validated_and_normalized() {
        assert_eq!(Sha1::parse(&SHA1.to_uppercase()).unwrap().as_str(), SHA1);
        for bad in [
            "",
            "abc",
            &SHA1[..39],
            &format!("{SHA1}0"),
            &SHA1.replace('d', "g"),
        ] {
            assert!(Sha1::parse(bad).is_err(), "{bad}");
        }
        assert!(Sha256::parse(SHA1).is_err());
    }

    #[test]
    fn safe_names_reject_traversal_and_reserved_names() {
        for good in [
            "1.21.1",
            "26.3",
            "17",
            "client-1.12.xml",
            "26.4-snapshot-4",
            "1.14 Pre-Release 5",
        ] {
            assert!(SafeName::parse(good).is_ok(), "{good}");
        }
        for bad in [
            "",
            ".",
            "..",
            "../x",
            "a/b",
            r"a\b",
            ".hidden",
            "CON",
            "nul.txt",
            "COM1",
            " a",
            "a ",
            "a.",
            "a\tb",
            "x:y",
            &"a".repeat(65),
        ] {
            assert!(SafeName::parse(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn is_within_rejects_escapes() {
        let root = Path::new("/data/root");
        assert!(is_within(root, Path::new("/data/root/a/b")));
        assert!(!is_within(root, Path::new("/data/root/../x")));
        assert!(!is_within(root, Path::new("/data/rootx/a")));
        assert!(!is_within(root, Path::new("/other")));
    }
}
