//! The signed list of Launcher Skins jars (D-065, "Updates without a launcher
//! update"): which jar of which release serves which game versions.
//!
//! CI builds the list from a release's jars and `mod/builds.json` and signs it
//! with the update key (`skins-mod-list`); the server only checks it and puts
//! it in place (`skins-mod`). Launchers read
//! `/v1/<channel>/skins-mod-list.json`, check the signature themselves, and take
//! no list older (by serial) than one they already have.
//!
//! ```text
//! <root>/incoming/skins-mod-<channel>.json           uploaded by CI
//! <root>/channels/<channel>/skins-mod-list.json      what launchers read
//! ```

use std::fs;
use std::io::Read;
use std::path::Path;

use base64::Engine as _;
use serde::{Deserialize, Serialize};
use sha2::Digest as _;

use crate::release::{CHANNELS, Server, write_atomic};

/// The kind every list's signature names, so no other signed file passes as one.
pub const KIND: &str = "skins-mod";
/// Its name in a channel's folder and in the launcher's address.
pub const FILE: &str = "skins-mod-list.json";
const TARGETS: &[&str] = &["fabric", "neoforge", "forge", "agent"];
/// Bounds for what a list may hold; the launcher has the same.
pub const MAX_BYTES: u64 = 512 * 1024;
const MAX_JARS: usize = 256;
const MAX_GAMES: usize = 128;
const MAX_JAR_BYTES: u64 = 16 * 1024 * 1024;

/// What is signed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct List {
    pub kind: String,
    pub channel: String,
    /// Grows with every list signed (the signing time in seconds): launchers
    /// take no list with a lower one than they have.
    pub serial: u64,
    /// The mod version; the jars are on the GitHub release `skins-mod-v<version>`,
    /// and players get them from mirror.lapi.sh's copy (D-065).
    pub version: String,
    /// Set when the list goes back to an older mod version: launchers then take
    /// it over the version built into them, even a newer one.
    #[serde(default)]
    pub rollback: bool,
    pub jars: Vec<Jar>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Jar {
    pub target: String,
    pub games: Vec<String>,
    pub file: String,
    pub sha256: String,
    pub size: u64,
}

/// What is served: the list's exact bytes and their signature.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Envelope {
    pub list: String,
    /// Base64 of a minisign signature, as Tauri writes them.
    pub signature: String,
}

/// One build of the mod in `mod/builds.json`.
#[derive(Debug, Deserialize)]
struct Build {
    target: String,
    label: String,
    games: Vec<String>,
}

fn plain(text: &str, max: usize, extra: &[u8]) -> bool {
    !text.is_empty()
        && text.len() <= max
        && text
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || extra.contains(&b))
}

/// A mod version as releases are named: `0.6.1`.
pub fn version_ok(version: &str) -> bool {
    let parts: Vec<&str> = version.split('.').collect();
    version.len() <= 32
        && parts.len() == 3
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
}

impl List {
    /// Every field within bounds and of the expected shape.
    pub fn check(&self) -> Result<(), String> {
        if self.kind != KIND {
            return Err(format!("not a skins mod list ({})", self.kind));
        }
        if !CHANNELS.contains(&self.channel.as_str()) {
            return Err(format!("unknown channel {}", self.channel));
        }
        if !version_ok(&self.version) {
            return Err(format!("{} is not a mod version", self.version));
        }
        if self.jars.is_empty() || self.jars.len() > MAX_JARS {
            return Err(format!("{} jars (1 to {MAX_JARS})", self.jars.len()));
        }
        for jar in &self.jars {
            let name_ok = plain(&jar.file, 128, b".-")
                && jar.file.starts_with("launcherskins-")
                && jar.file.ends_with(".jar");
            let games_ok = !jar.games.is_empty()
                && jar.games.len() <= MAX_GAMES
                && jar.games.iter().all(|g| plain(g, 16, b".-"));
            let sum_ok = jar.sha256.len() == 64
                && jar.sha256.bytes().all(|b| b.is_ascii_hexdigit())
                && jar.sha256 == jar.sha256.to_ascii_lowercase();
            if !name_ok
                || !games_ok
                || !sum_ok
                || !TARGETS.contains(&jar.target.as_str())
                || !(1..=MAX_JAR_BYTES).contains(&jar.size)
            {
                return Err(format!("a jar entry is not valid ({})", jar.file));
            }
        }
        Ok(())
    }
}

/// The list for release `version`: every build of `builds` (`mod/builds.json`)
/// with its jar's sum from `sums` (the release's SHA256SUMS) and size, each jar
/// read from `jars` and checked against its sum.
pub fn build(
    version: &str,
    channel: &str,
    serial: u64,
    rollback: bool,
    builds: &[u8],
    sums: &str,
    jars: &Path,
) -> Result<List, String> {
    let builds: Vec<Build> =
        serde_json::from_slice(builds).map_err(|e| format!("builds.json: {e}"))?;
    let sums: Vec<(String, String)> = sums
        .lines()
        .filter_map(|l| {
            let (sum, name) = l.split_once(char::is_whitespace)?;
            Some((
                name.trim().trim_start_matches('*').to_owned(),
                sum.to_owned(),
            ))
        })
        .collect();
    let mut list = List {
        kind: KIND.to_owned(),
        channel: channel.to_owned(),
        serial,
        version: version.to_owned(),
        rollback,
        jars: Vec::new(),
    };
    for b in &builds {
        let file = format!("launcherskins-{}-{}-{version}.jar", b.target, b.label);
        let sum = sums
            .iter()
            .find(|(name, _)| *name == file)
            .map(|(_, sum)| sum.to_ascii_lowercase())
            .ok_or_else(|| format!("{file} is not in SHA256SUMS"))?;
        let bytes = fs::read(jars.join(&file)).map_err(|e| format!("reading {file}: {e}"))?;
        if hex::encode(sha2::Sha256::digest(&bytes)) != sum {
            return Err(format!("{file} does not match SHA256SUMS"));
        }
        list.jars.push(Jar {
            target: b.target.clone(),
            games: b.games.clone(),
            file,
            sha256: sum,
            size: bytes.len() as u64,
        });
    }
    let listed: Vec<&str> = list.jars.iter().map(|j| j.file.as_str()).collect();
    let extra: Vec<&str> = sums
        .iter()
        .map(|(name, _)| name.as_str())
        .filter(|name| name.ends_with(".jar") && !listed.contains(name))
        .collect();
    if !extra.is_empty() {
        return Err(format!(
            "release jars not in mod/builds.json: {}",
            extra.join(", ")
        ));
    }
    list.check()?;
    Ok(list)
}

/// Signs `list`; the trusted comment names the kind, the channel and the serial.
pub fn sign(list: &List, key: &minisign::SecretKey) -> Result<Envelope, String> {
    list.check()?;
    let text = serde_json::to_string(list).map_err(|e| e.to_string())?;
    let trusted = format!(
        "timestamp:{}\tfile:{FILE}\tkind:{KIND}\tchannel:{}\tserial:{}",
        chrono::Utc::now().timestamp(),
        list.channel,
        list.serial
    );
    let signature = minisign::sign(
        None,
        key,
        text.as_bytes(),
        Some(&trusted),
        Some("signature from tauri secret key"),
    )
    .map_err(|e| format!("signing the list: {e}"))?;
    Ok(Envelope {
        list: text,
        signature: base64::engine::general_purpose::STANDARD.encode(signature.to_string()),
    })
}

/// The list in `envelope` when its signature verifies with `public_key` (Tauri's
/// base64 form) and names the same kind, channel and serial as the list.
pub fn verify(public_key: &str, envelope: &[u8], channel: &str) -> Result<List, String> {
    if envelope.len() as u64 > MAX_BYTES {
        return Err("the list is too large".to_owned());
    }
    let envelope: Envelope =
        serde_json::from_slice(envelope).map_err(|_| "the list is not valid JSON")?;
    let b64 = |s: &str| {
        base64::engine::general_purpose::STANDARD
            .decode(s.trim())
            .ok()
            .and_then(|bytes| String::from_utf8(bytes).ok())
    };
    let key = b64(public_key)
        .and_then(|t| minisign_verify::PublicKey::decode(&t).ok())
        .ok_or("the public key is not valid")?;
    let signature = b64(&envelope.signature)
        .and_then(|t| minisign_verify::Signature::decode(&t).ok())
        .ok_or("the signature is not valid")?;
    key.verify(envelope.list.as_bytes(), &signature, false)
        .map_err(|_| "the signature does not match the list")?;
    let field = |name: &str| {
        signature
            .trusted_comment()
            .split('\t')
            .find_map(|f| f.strip_prefix(name).map(str::to_owned))
    };
    let list: List =
        serde_json::from_str(&envelope.list).map_err(|_| "the signed list is not valid")?;
    list.check()?;
    if field("kind:").as_deref() != Some(KIND)
        || field("channel:").as_deref() != Some(list.channel.as_str())
        || field("serial:") != Some(list.serial.to_string())
    {
        return Err("the signature does not name this list".to_owned());
    }
    if list.channel != channel {
        return Err(format!("the list is for {}, not {channel}", list.channel));
    }
    Ok(list)
}

impl Server {
    fn skins_mod_path(&self, channel: &str) -> Result<std::path::PathBuf, String> {
        Ok(self.channel_dir(channel)?.join(FILE))
    }

    /// The list a channel serves now, if any.
    fn skins_mod_current(&self, channel: &str) -> Result<Option<List>, String> {
        let path = self.skins_mod_path(channel)?;
        match fs::read(&path) {
            Ok(bytes) => {
                let envelope: Envelope = serde_json::from_slice(&bytes)
                    .map_err(|e| format!("{}: {e}", path.display()))?;
                serde_json::from_str(&envelope.list)
                    .map(Some)
                    .map_err(|e| format!("{}: {e}", path.display()))
            }
            Err(_) => Ok(None),
        }
    }

    /// Puts `incoming/skins-mod-<channel>.json` in place after checking it
    /// against the public key and that its serial is newer than the channel's.
    pub fn install_skins_mod(&self, channel: &str) -> Result<String, String> {
        let path = self.skins_mod_path(channel)?;
        let incoming = self
            .root()
            .join("incoming")
            .join(format!("skins-mod-{channel}.json"));
        let mut bytes = Vec::new();
        fs::File::open(&incoming)
            .map_err(|_| format!("no {}", incoming.display()))?
            .take(MAX_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| format!("reading {}: {e}", incoming.display()))?;
        let list = verify(&self.public_key()?, &bytes, channel)?;
        if let Some(current) = self.skins_mod_current(channel)?
            && current.serial >= list.serial
        {
            return Err(format!(
                "{channel} has a list as new or newer (serial {})",
                current.serial
            ));
        }
        write_atomic(&path, &bytes)?;
        let _ = fs::remove_file(&incoming);
        Ok(format!(
            "skins mod {} on {channel}{} ({} jars, serial {})",
            list.version,
            if list.rollback { ", a rollback" } else { "" },
            list.jars.len(),
            list.serial
        ))
    }

    /// One line per channel for `status`.
    pub fn skins_mod_status(&self) -> Result<String, String> {
        let mut out = String::new();
        for channel in CHANNELS {
            out.push_str(&match self.skins_mod_current(channel)? {
                Some(l) => format!(
                    "skins mod on {channel}: {}{} (serial {})\n",
                    l.version,
                    if l.rollback { ", a rollback" } else { "" },
                    l.serial
                ),
                None => format!("skins mod on {channel}: no list\n"),
            });
        }
        Ok(out.trim_end().to_owned())
    }
}

#[cfg(test)]
mod tests;
