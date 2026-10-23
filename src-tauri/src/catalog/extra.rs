//! Versions Mojang's list does not have (D-073): the April Fools versions,
//! 2.0, Combat Tests, experimental snapshots, the builds Mojang replaced and the
//! old builds the archives keep. Our update server has their signed list, their
//! version files and their game jars, and the launcher gets them from there
//! only ([`SourcePolicy::ExtraVersions`]); libraries, assets and Java are
//! Mojang's, as for every version.
//!
//! CI signs the list with the launcher's update key; the trusted comment names
//! the kind `versions` and a serial that grows with every list. The launcher
//! asks for it with Mojang's catalog, takes it only when the signature verifies
//! with the built-in public key and its serial is not lower than the list it
//! has, and keeps it next to the catalog. A build without the key shows none of
//! these versions. The list pins each version file by its SHA-1, and each
//! version file pins its jars.

use std::collections::HashSet;

use base64::Engine as _;
use serde::{Deserialize, Serialize};

use super::manifest::CatalogEntry;
use crate::install::descriptor::{MAX_DESCRIPTOR_BYTES, VERSION_TYPES};
use crate::net::SourcePolicy;
use crate::storage::paths::{SafeName, Sha1};

/// The signed list.
pub const URL: &str = "https://upd.lapi.sh/versions/manifest.json";
/// Where the version files are, by SHA-1.
const META_URL: &str = "https://upd.lapi.sh/versions/meta/";
/// What every list's signature names, so no other signed file passes as one.
const KIND: &str = "versions";
/// Bounds, as the server's: the list, and the file with its signature.
const MAX_LIST_BYTES: usize = 1024 * 1024;
pub const MAX_BYTES: usize = 2 * MAX_LIST_BYTES;
const MAX_VERSIONS: usize = 2048;
const MAX_TEXT_CHARS: usize = 64;
const GROUPS: &[&str] = &[
    "joke",
    "unobfuscated",
    "experimental",
    "combat",
    "first-build",
    "early-release",
    "beta",
    "alpha",
    "infdev",
    "indev",
    "classic",
];
const VARIANTS: &[&str] = &["first-build"];
/// What the picker may say about a build (services/upd has the same).
const NOTES: &[&str] = &["crashes-at-start"];

#[derive(Deserialize)]
struct Envelope {
    list: String,
    signature: String,
}

/// A verified list.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct List {
    kind: String,
    pub serial: u64,
    /// In the order the picker shows them: the April Fools versions newest
    /// first, then the rest newest first.
    pub versions: Vec<Listed>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Listed {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub title: Option<String>,
    pub group: String,
    #[serde(default)]
    pub variant: Option<String>,
    #[serde(default)]
    pub note: Option<String>,
    #[serde(rename = "type")]
    pub version_type: String,
    pub release_time: String,
    pub server: bool,
    /// Of the version file.
    pub sha1: String,
    pub size: u64,
}

/// How the picker shows a version of this list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Special {
    /// `2.0 Blue`, `Combat Test 8c`, `20w14∞`.
    pub name: String,
    /// The update's name, for April Fools versions: `Craftmine Update`.
    pub title: Option<String>,
    /// `joke`, `combat`, `first-build`, … ([`GROUPS`]).
    pub group: String,
    /// `first-build`: an earlier build of a version, replaced under its name.
    pub variant: Option<String>,
    /// `crashes-at-start`: the build ends with an error before its title screen,
    /// in the game itself ([`NOTES`]).
    pub note: Option<String>,
    /// Has a server jar.
    pub server: bool,
}

/// Shown text: no control characters and nothing that changes how the text
/// around it reads (bidi formatting, zero-width).
fn text_ok(text: &str) -> bool {
    let count = text.chars().count();
    (1..=MAX_TEXT_CHARS).contains(&count)
        && text.trim() == text
        && !text.chars().any(|c| {
            c.is_control()
                || matches!(
                    c,
                    '\u{061C}'
                        | '\u{200B}'..='\u{200F}'
                        | '\u{202A}'..='\u{202E}'
                        | '\u{2066}'..='\u{2069}'
                        | '\u{FEFF}'
                )
        })
}

impl Listed {
    fn sane(&self) -> bool {
        SafeName::parse(&self.id).is_ok()
            && text_ok(&self.name)
            && self.title.as_deref().is_none_or(text_ok)
            && GROUPS.contains(&self.group.as_str())
            && self
                .variant
                .as_deref()
                .is_none_or(|v| VARIANTS.contains(&v))
            && self.note.as_deref().is_none_or(|n| NOTES.contains(&n))
            && VERSION_TYPES.contains(&self.version_type.as_str())
            && self.release_time.len() <= 40
            && chrono::DateTime::parse_from_rfc3339(&self.release_time).is_ok()
            && Sha1::parse(&self.sha1).is_ok()
            && self.sha1.bytes().all(|b| !b.is_ascii_uppercase())
            && (1..=MAX_DESCRIPTOR_BYTES as u64).contains(&self.size)
    }

    /// The catalog entry: the version file on our server, pinned by its SHA-1.
    pub fn entry(&self) -> Option<CatalogEntry> {
        Some(CatalogEntry {
            id: self.id.clone(),
            version_type: self.version_type.clone(),
            url: format!("{META_URL}{}.json", self.sha1),
            sha1: Sha1::parse(&self.sha1).ok()?,
            release_time: Some(self.release_time.clone()),
            special: Some(self.special()),
        })
    }

    pub fn special(&self) -> Special {
        Special {
            name: self.name.clone(),
            title: self.title.clone(),
            group: self.group.clone(),
            variant: self.variant.clone(),
            note: self.note.clone(),
            server: self.server,
        }
    }
}

impl List {
    fn sane(&self) -> bool {
        // Folder names: Windows does not tell `A` from `a`.
        let mut ids = HashSet::new();
        self.kind == KIND
            && !self.versions.is_empty()
            && self.versions.len() <= MAX_VERSIONS
            && self
                .versions
                .iter()
                .all(|v| v.sane() && ids.insert(v.id.to_ascii_lowercase()))
    }
}

/// The list in `bytes` when its signature verifies with `public_key` (Tauri's
/// base64 form) and names this kind and the list's serial.
pub fn verify(public_key: &str, bytes: &[u8]) -> Option<List> {
    if bytes.len() > MAX_BYTES {
        return None;
    }
    let envelope: Envelope = serde_json::from_slice(bytes).ok()?;
    if envelope.list.len() > MAX_LIST_BYTES {
        return None;
    }
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
    let named =
        field("kind:") == Some(KIND) && field("serial:") == Some(list.serial.to_string().as_str());
    (named && list.sane()).then_some(list)
}

/// Whether a version file's address is on our server (its jars are then
/// there too).
pub fn ours(url: &str) -> bool {
    SourcePolicy::ExtraVersions.check(url).is_ok()
}

#[cfg(test)]
pub(crate) mod tests;
