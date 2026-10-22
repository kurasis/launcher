//! The signed list of Launcher Skins jars (D-065, "Updates without a launcher
//! update"): which jar of which release serves which game versions.
//!
//! CI builds the list from a release's jars and `mod/builds.json` and signs it
//! with the update key (`skins-mod-list`); the server only checks it and puts
//! it in place (`skins-mod`). Launchers read
//! `/v1/<channel>/skins-mod-list.json`, check the signature themselves, and take
//! no list older (by serial) than one they already have.
//!
//! Players download the jars only from here, never from GitHub (D-065):
//! CI uploads a release's jars and its SHA256SUMS, `skins-mod-jars` checks them
//! and puts them in place, and a list goes in place only once all its jars are.
//!
//! ```text
//! <root>/incoming/skins-mod-<channel>.json           uploaded by CI
//! <root>/incoming/skins-mod-<version>/               SHA256SUMS and jars, by CI
//! <root>/channels/<channel>/skins-mod-list.json      what launchers read
//! <root>/skins-mod/<version>/<jar>                   what launchers download
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
/// The folder under the root that holds each version's jars, as launchers ask
/// for them: `/skins-mod/<version>/<file>`.
const JARS: &str = "skins-mod";
/// The largest SHA256SUMS taken.
const MAX_SUMS_BYTES: u64 = 64 * 1024;
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
    /// The mod version; the jars are the GitHub release `skins-mod-v<version>`,
    /// served at `/skins-mod/<version>/<file>`.
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

/// A jar of ours: `launcherskins-<target>-<label>-<version>.jar`.
fn jar_name_ok(file: &str) -> bool {
    plain(file, 128, b".-") && file.starts_with("launcherskins-") && file.ends_with(".jar")
}

fn sum_ok(sum: &str) -> bool {
    sum.len() == 64
        && sum
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(sha2::Sha256::digest(bytes))
}

/// `(file, sum)` for every line of a SHA256SUMS (`<sum>  <file>`, or `*<file>`).
fn parse_sums(sums: &str) -> Vec<(String, String)> {
    sums.lines()
        .filter_map(|l| {
            let (sum, name) = l.split_once(char::is_whitespace)?;
            Some((
                name.trim().trim_start_matches('*').to_owned(),
                sum.to_ascii_lowercase(),
            ))
        })
        .collect()
}

/// A file's bytes, refused past `max`.
fn read_bounded(path: &Path, max: u64) -> Result<Vec<u8>, String> {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file");
    let mut bytes = Vec::new();
    fs::File::open(path)
        .map_err(|_| format!("no {name}"))?
        .take(max + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("reading {name}: {e}"))?;
    if bytes.len() as u64 > max {
        return Err(format!("{name} is too large"));
    }
    Ok(bytes)
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
            let games_ok = !jar.games.is_empty()
                && jar.games.len() <= MAX_GAMES
                && jar.games.iter().all(|g| plain(g, 16, b".-"));
            if !jar_name_ok(&jar.file)
                || !games_ok
                || !sum_ok(&jar.sha256)
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
    let sums = parse_sums(sums);
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
            .map(|(_, sum)| sum.clone())
            .ok_or_else(|| format!("{file} is not in SHA256SUMS"))?;
        let bytes = fs::read(jars.join(&file)).map_err(|e| format!("reading {file}: {e}"))?;
        if sha256_hex(&bytes) != sum {
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

    /// Where the jars of `version` are served from.
    fn jars_dir(&self, version: &str) -> Result<std::path::PathBuf, String> {
        if !version_ok(version) {
            return Err(format!("{version} is not a mod version"));
        }
        Ok(self.root().join(JARS).join(version))
    }

    /// The jars of `list` that are not in place with their size and sum.
    fn missing_jars<'a>(&self, list: &'a List) -> Result<Vec<&'a str>, String> {
        let dir = self.jars_dir(&list.version)?;
        Ok(list
            .jars
            .iter()
            .filter(|jar| {
                !read_bounded(&dir.join(&jar.file), MAX_JAR_BYTES).is_ok_and(|bytes| {
                    bytes.len() as u64 == jar.size && sha256_hex(&bytes) == jar.sha256
                })
            })
            .map(|jar| jar.file.as_str())
            .collect())
    }

    /// Puts the jars of `version` in place from `incoming/skins-mod-<version>/`
    /// (the release's SHA256SUMS and its jars, uploaded by CI). Every jar is
    /// checked against its sum before any is written. A jar already in place
    /// stays; one in place with other bytes is refused, since a published file
    /// never changes at its address.
    pub fn install_skins_mod_jars(&self, version: &str) -> Result<String, String> {
        let dir = self.jars_dir(version)?;
        let incoming = self
            .root()
            .join("incoming")
            .join(format!("skins-mod-{version}"));
        let sums = read_bounded(&incoming.join("SHA256SUMS"), MAX_SUMS_BYTES)?;
        let jars: Vec<(String, String)> = parse_sums(&String::from_utf8_lossy(&sums))
            .into_iter()
            .filter(|(file, _)| file.ends_with(".jar"))
            .collect();
        if jars.is_empty() || jars.len() > MAX_JARS {
            return Err(format!(
                "{} jars in SHA256SUMS (1 to {MAX_JARS})",
                jars.len()
            ));
        }
        let mut new = Vec::new();
        for (file, sum) in &jars {
            if !jar_name_ok(file) || !sum_ok(sum) {
                return Err("SHA256SUMS names a file that is not a jar of the mod".to_owned());
            }
            let bytes = read_bounded(&incoming.join(file), MAX_JAR_BYTES)?;
            if sha256_hex(&bytes) != *sum {
                return Err(format!("{file} does not match SHA256SUMS"));
            }
            match fs::read(dir.join(file)) {
                Ok(have) if sha256_hex(&have) == *sum => {}
                Ok(_) => return Err(format!("{file} is already published with other bytes")),
                Err(_) => new.push((file, sum)),
            }
        }
        for (file, sum) in &new {
            let bytes = read_bounded(&incoming.join(file), MAX_JAR_BYTES)?;
            if sha256_hex(&bytes) != **sum {
                return Err(format!("{file} changed while it was put in place"));
            }
            write_atomic(&dir.join(file), &bytes)?;
        }
        Ok(format!(
            "skins mod {version}: {} jars, {} new",
            jars.len(),
            new.len()
        ))
    }

    /// Puts `incoming/skins-mod-<channel>.json` in place after checking it
    /// against the public key, that its serial is newer than the channel's and
    /// that every jar it names is in place (`skins-mod-jars`).
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
        let missing = self.missing_jars(&list)?;
        if !missing.is_empty() {
            return Err(format!(
                "{} of {} jars of skins mod {} are not in place: run skins-mod-jars {} first",
                missing.len(),
                list.jars.len(),
                list.version,
                list.version
            ));
        }
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
                    "skins mod on {channel}: {}{} (serial {}, {} of {} jars in place)\n",
                    l.version,
                    if l.rollback { ", a rollback" } else { "" },
                    l.serial,
                    l.jars.len() - self.missing_jars(&l)?.len(),
                    l.jars.len()
                ),
                None => format!("skins mod on {channel}: no list\n"),
            });
        }
        Ok(out.trim_end().to_owned())
    }
}

#[cfg(test)]
mod tests;
