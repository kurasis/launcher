//! The signed list of Launcher Skins jars at upd.lapi.sh (D-065, "Updates
//! without a launcher update"): a new mod version reaches players without a new
//! launcher.
//!
//! CI signs each list with the launcher's update key; the trusted comment names
//! the kind `skins-mod`, the channel and a serial that grows with every list.
//! The launcher asks its update channel's list at start and every 6 hours
//! (with the update check), takes it only when the signature verifies with
//! the built-in public key and its serial is not lower than the list it has,
//! and keeps it in [`super::MANAGED_DIR`]. A launch reads the kept list again
//! and checks it again; without one, or with a damaged one, the jars built into
//! the launcher ([`super::JARS`]) are used. Nothing here stops a launch, and
//! nothing about the player is sent: the request is a plain GET of one file.

use std::io::Read as _;
use std::path::PathBuf;

use base64::Engine as _;
use serde::Deserialize;

use super::{MANAGED_DIR, MAX_JAR_BYTES, RELEASES, Target, is_ours};
use crate::download::resolve_managed_path;
use crate::net::{PublicClient, SourcePolicy};
use crate::storage::paths::{DataRoot, Sha256};
use crate::update::UpdateChannel;

/// What every list's signature names.
const KIND: &str = "skins-mod";
/// Its name on upd.lapi.sh: `/v1/<channel>/skins-mod-list.json`.
pub const FILE: &str = "skins-mod-list.json";
/// Bounds for what a list may hold, as the server's.
pub const MAX_BYTES: u64 = 512 * 1024;
const MAX_JARS: usize = 256;
const MAX_GAMES: usize = 128;

#[derive(Deserialize)]
struct Envelope {
    list: String,
    signature: String,
}

/// A verified list.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct List {
    kind: String,
    pub channel: String,
    pub serial: u64,
    /// The mod version, `0.6.2`.
    pub version: String,
    /// Going back to an older mod version: taken over the built-in jars even
    /// when those are newer.
    #[serde(default)]
    pub rollback: bool,
    pub jars: Vec<ListedJar>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ListedJar {
    pub target: String,
    pub games: Vec<String>,
    pub file: String,
    pub sha256: String,
    pub size: u64,
}

fn plain(text: &str, max: usize) -> bool {
    !text.is_empty()
        && text.len() <= max
        && text
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'-'))
}

/// `0.6.2`: three numbers, as the mod's releases are named.
fn version_ok(version: &str) -> bool {
    version.len() <= 32
        && version.split('.').count() == 3
        && version
            .split('.')
            .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
}

impl ListedJar {
    fn sane(&self) -> bool {
        plain(&self.file, 128)
            && is_ours(&self.file)
            && Sha256::parse(&self.sha256).is_ok()
            && self.sha256.bytes().all(|b| !b.is_ascii_uppercase())
            && (1..=MAX_JAR_BYTES).contains(&self.size)
            && ["fabric", "neoforge", "forge", "agent"].contains(&self.target.as_str())
            && !self.games.is_empty()
            && self.games.len() <= MAX_GAMES
            && self.games.iter().all(|g| plain(g, 16))
    }
}

impl List {
    fn sane(&self) -> bool {
        self.kind == KIND
            && version_ok(&self.version)
            && !self.jars.is_empty()
            && self.jars.len() <= MAX_JARS
            && self.jars.iter().all(ListedJar::sane)
    }

    /// The listed jar for this game version and target.
    pub fn find(&self, game: &str, target: Target) -> Option<&ListedJar> {
        self.jars
            .iter()
            .find(|j| j.target == target.as_str() && j.games.iter().any(|g| g == game))
    }

    /// A listed jar's download address.
    pub fn url(&self, jar: &ListedJar) -> String {
        format!("{RELEASES}{}/{}", self.version, jar.file)
    }
}

/// The list in `bytes` when its signature verifies with `public_key` (Tauri's
/// base64 form) and names this kind, the channel and the list's serial.
pub fn verify(public_key: &str, bytes: &[u8], channel: UpdateChannel) -> Option<List> {
    if bytes.len() as u64 > MAX_BYTES {
        return None;
    }
    let envelope: Envelope = serde_json::from_slice(bytes).ok()?;
    let b64 = |s: &str| {
        base64::engine::general_purpose::STANDARD
            .decode(s.trim())
            .ok()
            .and_then(|b| String::from_utf8(b).ok())
    };
    let key = minisign_verify::PublicKey::decode(&b64(public_key)?).ok()?;
    let signature = minisign_verify::Signature::decode(&b64(&envelope.signature)?).ok()?;
    key.verify(envelope.list.as_bytes(), &signature, false)
        .ok()?;
    let list: List = serde_json::from_str(&envelope.list).ok()?;
    let field = |name: &str| {
        signature
            .trusted_comment()
            .split('\t')
            .find_map(|f| f.strip_prefix(name))
    };
    let named = field("kind:") == Some(KIND)
        && field("channel:") == Some(channel.path())
        && field("serial:") == Some(list.serial.to_string().as_str());
    (named && list.channel == channel.path() && list.sane()).then_some(list)
}

fn kept_path(root: &DataRoot, channel: UpdateChannel) -> Option<PathBuf> {
    resolve_managed_path(root, &format!("{MANAGED_DIR}/list-{}.json", channel.path())).ok()
}

/// The list kept for `channel`, checked again.
pub fn kept(root: &DataRoot, public_key: &str, channel: UpdateChannel) -> Option<List> {
    let path = kept_path(root, channel)?;
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .ok()?
        .take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    verify(public_key, &bytes, channel)
}

/// What [`take`] did with a fetched list.
#[derive(Debug, PartialEq, Eq)]
pub enum Taken {
    /// Kept: a newer list than before, of this mod version.
    New(String),
    /// The list kept already.
    Same,
    /// An older list than the one kept: someone serves an old file.
    Older,
    /// Not signed with the update key for this channel, or not a valid list.
    Refused,
}

/// Keeps `bytes` as the channel's list when it verifies and is not older
/// than the one kept.
pub fn take(root: &DataRoot, public_key: &str, channel: UpdateChannel, bytes: &[u8]) -> Taken {
    let Some(list) = verify(public_key, bytes, channel) else {
        return Taken::Refused;
    };
    match kept(root, public_key, channel) {
        Some(k) if k.serial > list.serial => return Taken::Older,
        Some(k) if k.serial == list.serial => return Taken::Same,
        _ => {}
    }
    let Some(path) = kept_path(root, channel) else {
        return Taken::Refused;
    };
    let written = path
        .parent()
        .is_some_and(|dir| std::fs::create_dir_all(dir).is_ok())
        && crate::storage::atomic::write(&path, bytes).is_ok();
    if written {
        Taken::New(list.version)
    } else {
        Taken::Refused
    }
}

/// The list's address for `channel` on the update server, if the policy
/// allows that server (a test build's own server is not asked).
fn address(base: &str, channel: UpdateChannel) -> Option<url::Url> {
    let raw = format!(
        "{}/v1/{}/{FILE}",
        base.trim_end_matches('/'),
        channel.path()
    );
    SourcePolicy::SkinsModList.check(&raw).ok()
}

/// Asks the update server for the channel's list and keeps it as [`take`]
/// says. `None` when it could not be asked or there is none (204).
pub async fn refresh(
    root: &DataRoot,
    client: &PublicClient,
    base: &str,
    public_key: &str,
    channel: UpdateChannel,
) -> Option<Taken> {
    let url = address(base, channel)?;
    let response = client
        .get(&url)
        .ok()?
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .await
        .ok()?;
    if response.status() != reqwest::StatusCode::OK {
        return None;
    }
    if response.content_length().is_some_and(|n| n > MAX_BYTES) {
        return Some(Taken::Refused);
    }
    let mut bytes = Vec::new();
    let mut response = response;
    while let Some(chunk) = response.chunk().await.ok()? {
        bytes.extend_from_slice(&chunk);
        if bytes.len() as u64 > MAX_BYTES {
            return Some(Taken::Refused);
        }
    }
    let root = root.clone();
    tokio::task::spawn_blocking({
        let key = public_key.to_owned();
        move || take(&root, &key, channel, &bytes)
    })
    .await
    .ok()
}

#[cfg(test)]
mod tests;
