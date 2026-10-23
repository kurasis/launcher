//! Minecraft versions that Mojang's version list does not have (D-073): the
//! April Fools versions and every older or test build, in our own signed list
//! with their version files and game jars, all served from here.
//!
//! `data/extra-versions.json` in the repository holds every version with its
//! version file (written by `scripts/extra-versions.mjs`). The Versions
//! workflow fetches every game jar from where it came from, checks it against
//! its SHA-1 and for viruses, and only then publishes: `versions-list` (CI)
//! checks the data and signs the list with the update key, which never comes
//! here; `versions-files` puts the uploaded files in place; `versions` puts
//! the list in place once every file it names is. Launchers check the
//! signature, then each version file and jar by its SHA-1.
//!
//! ```text
//! <root>/incoming/versions/SHA1SUMS             uploaded by CI, with
//! <root>/incoming/versions/meta/<sha1>.json     version files and
//! <root>/incoming/versions/files/<sha1>.jar     the jars not here yet
//! <root>/incoming/versions.json                 the signed list, by CI
//! <root>/versions/manifest.json                 what launchers read
//! <root>/versions/meta/<sha1>.json              version files
//! <root>/versions/files/<sha1>.jar              game jars
//! ```

use std::collections::HashSet;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use base64::Engine as _;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha1::Digest as _;

use crate::release::{Server, write_atomic};

/// The kind every list's signature names, so no other signed file passes as one.
pub const KIND: &str = "versions";
/// The list's name, also in the launcher's address: `/versions/manifest.json`.
pub const FILE: &str = "manifest.json";
/// Where launchers download the game jars; every version file names them here.
pub const FILES_URL: &str = "https://upd.lapi.sh/versions/files/";
/// Hosts a version file may name besides ours: Mojang's libraries, assets and
/// log configurations, as for every version.
const MOJANG_HOSTS: &[&str] = &[
    "piston-meta.mojang.com",
    "piston-data.mojang.com",
    "launcher.mojang.com",
    "launchermeta.mojang.com",
    "libraries.minecraft.net",
    "resources.download.minecraft.net",
];
/// Where CI may fetch a game jar from.
const ORIGINS: &[&str] = &[
    "piston-data.mojang.com",
    "launcher.mojang.com",
    "files.betacraft.uk",
    "vault.omniarchive.uk",
];
pub const GROUPS: &[&str] = &[
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
const TYPES: &[&str] = &["release", "snapshot", "old_beta", "old_alpha"];
/// What the launcher may say about a build (the launcher has the same).
pub const NOTES: &[&str] = &["crashes-at-start"];
/// Bounds; the launcher has the same.
pub const MAX_BYTES: u64 = 1024 * 1024;
const MAX_VERSIONS: usize = 2048;
pub const MAX_DESCRIPTOR_BYTES: u64 = 8 * 1024 * 1024;
pub const MAX_JAR_BYTES: u64 = 256 * 1024 * 1024;
const MAX_SUMS_BYTES: u64 = 1024 * 1024;
const MAX_DATA_BYTES: u64 = 64 * 1024 * 1024;

/// What is signed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct List {
    pub kind: String,
    /// Grows with every list signed (the signing time in seconds): launchers
    /// take no list with a lower one than they have.
    pub serial: u64,
    /// In the order the launcher shows them: the April Fools versions newest
    /// first, then the rest newest first.
    pub versions: Vec<Listed>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Listed {
    /// The version's id: its folder name and `--version`.
    pub id: String,
    /// How it is shown: `20w14∞`, `Combat Test 8c`.
    pub name: String,
    /// The update's name, for April Fools versions: `Craftmine Update`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub group: String,
    /// `first-build`: an earlier build of a version, replaced under its name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variant: Option<String>,
    /// `crashes-at-start`: the build ends with an error before its title screen.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    #[serde(rename = "type")]
    pub version_type: String,
    pub release_time: String,
    /// Has a server jar.
    pub server: bool,
    /// The version file, served at `/versions/meta/<sha1>.json`.
    pub sha1: String,
    pub size: u64,
}

/// What is served: the list's exact bytes and their signature.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Envelope {
    pub list: String,
    /// Base64 of a minisign signature, as Tauri writes them.
    pub signature: String,
}

/// `data/extra-versions.json`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Data {
    schema_version: u64,
    versions: Vec<DataVersion>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DataVersion {
    id: String,
    name: String,
    #[serde(default)]
    title: Option<String>,
    group: String,
    #[serde(default)]
    variant: Option<String>,
    #[serde(default)]
    note: Option<String>,
    #[serde(rename = "type")]
    version_type: String,
    release_time: String,
    server: bool,
    sha1: String,
    files: Vec<DataFile>,
    descriptor: String,
}

#[derive(Debug, Deserialize)]
struct DataFile {
    kind: String,
    sha1: String,
    size: u64,
    origin: String,
}

pub fn sha1_ok(text: &str) -> bool {
    text.len() == 40
        && text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn sha1_hex(bytes: &[u8]) -> String {
    hex::encode(sha1::Sha1::digest(bytes))
}

/// A version id as a folder name everywhere: letters, digits, `.`, `_`, `-`
/// and inner spaces.
pub fn id_ok(id: &str) -> bool {
    (1..=64).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-' | b' '))
        && !id.starts_with([' ', '.'])
        && !id.ends_with([' ', '.'])
        && !windows_reserved(id)
}

/// A name Windows keeps for a device, which the launcher refuses as a folder
/// name too.
fn windows_reserved(id: &str) -> bool {
    let stem = id
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ((stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.len() == 4
            && stem.as_bytes()[3].is_ascii_digit())
}

/// Shown text: 1 to 64 characters, no control characters and nothing that
/// changes how the text around it reads (bidi formatting, zero-width).
fn text_ok(text: &str) -> bool {
    let count = text.chars().count();
    (1..=64).contains(&count)
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
        && text.trim() == text
}

fn time_ok(text: &str) -> bool {
    text.len() <= 40 && chrono::DateTime::parse_from_rfc3339(text).is_ok()
}

impl Listed {
    fn check(&self) -> Result<(), String> {
        let ok = id_ok(&self.id)
            && text_ok(&self.name)
            && self.title.as_deref().is_none_or(text_ok)
            && GROUPS.contains(&self.group.as_str())
            && self.variant.as_deref().is_none_or(|v| v == "first-build")
            && self.note.as_deref().is_none_or(|n| NOTES.contains(&n))
            && TYPES.contains(&self.version_type.as_str())
            && time_ok(&self.release_time)
            && sha1_ok(&self.sha1)
            && (1..=MAX_DESCRIPTOR_BYTES).contains(&self.size);
        if ok {
            Ok(())
        } else {
            Err(format!("a version entry is not valid ({})", self.id))
        }
    }
}

impl List {
    /// Every field within bounds and of the expected shape, ids unique.
    pub fn check(&self) -> Result<(), String> {
        if self.kind != KIND {
            return Err(format!("not a version list ({})", self.kind));
        }
        if self.versions.is_empty() || self.versions.len() > MAX_VERSIONS {
            return Err(format!(
                "{} versions (1 to {MAX_VERSIONS})",
                self.versions.len()
            ));
        }
        let mut ids = HashSet::new();
        for version in &self.versions {
            version.check()?;
            // Folder names: Windows does not tell `A` from `a`.
            if !ids.insert(version.id.to_ascii_lowercase()) {
                return Err(format!("{} is listed twice", version.id));
            }
        }
        Ok(())
    }
}

fn host_of(url: &str) -> Option<&str> {
    let rest = url.strip_prefix("https://")?;
    let host = rest.split('/').next()?;
    (!host.is_empty() && !host.contains([':', '@', '?', '#'])).then_some(host)
}

/// Every URL a version file names.
fn urls(descriptor: &Value) -> Vec<&str> {
    let mut found = Vec::new();
    fn text<'a>(value: &'a Value, pointer: &str) -> Option<&'a str> {
        value.pointer(pointer).and_then(Value::as_str)
    }
    for library in descriptor["libraries"].as_array().into_iter().flatten() {
        found.extend(text(library, "/downloads/artifact/url"));
        for native in library
            .pointer("/downloads/classifiers")
            .and_then(Value::as_object)
            .into_iter()
            .flatten()
        {
            found.extend(text(native.1, "/url"));
        }
    }
    found.extend(text(descriptor, "/assetIndex/url"));
    found.extend(text(descriptor, "/logging/client/file/url"));
    found
}

/// The game jars a version file names: `(kind, sha1, size)`, each at
/// [`FILES_URL`]`<sha1>.jar`.
pub fn jars(descriptor: &Value) -> Result<Vec<(String, String, u64)>, String> {
    let downloads = descriptor["downloads"]
        .as_object()
        .ok_or("a version file without downloads")?;
    let mut found = Vec::new();
    for (kind, download) in downloads {
        if kind != "client" && kind != "server" {
            return Err(format!("a version file with a {kind} download"));
        }
        let sha1 = download["sha1"].as_str().unwrap_or_default();
        let size = download["size"].as_u64().unwrap_or_default();
        let url = download["url"].as_str().unwrap_or_default();
        if !sha1_ok(sha1)
            || !(1..=MAX_JAR_BYTES).contains(&size)
            || url != format!("{FILES_URL}{sha1}.jar")
        {
            return Err(format!("a {kind} download that is not ours"));
        }
        found.push((kind.clone(), sha1.to_owned(), size));
    }
    if !found.iter().any(|(kind, _, _)| kind == "client") {
        return Err("a version file without a client".to_owned());
    }
    Ok(found)
}

/// One version of the data: its version file as published, checked.
fn check_data_version(version: &DataVersion) -> Result<Value, String> {
    let id = &version.id;
    if sha1_hex(version.descriptor.as_bytes()) != version.sha1 {
        return Err(format!("{id}: the version file does not match its SHA-1"));
    }
    let descriptor: Value = serde_json::from_str(&version.descriptor)
        .map_err(|_| format!("{id}: the version file is not JSON"))?;
    if descriptor["id"].as_str() != Some(id.as_str())
        || descriptor["type"].as_str() != Some(version.version_type.as_str())
        || descriptor.get("inheritsFrom").is_some()
    {
        return Err(format!(
            "{id}: the version file names another version or type"
        ));
    }
    let jars = jars(&descriptor).map_err(|e| format!("{id}: {e}"))?;
    if jars.iter().any(|(kind, _, _)| kind == "server") != version.server
        || jars.len() != version.files.len()
    {
        return Err(format!("{id}: the jars do not match the version file"));
    }
    for (kind, sha1, size) in &jars {
        let listed = version
            .files
            .iter()
            .find(|f| f.kind == *kind && f.sha1 == *sha1 && f.size == *size);
        let origin_ok =
            listed.is_some_and(|f| host_of(&f.origin).is_some_and(|host| ORIGINS.contains(&host)));
        if !origin_ok {
            return Err(format!("{id}: the {kind} jar has no known origin"));
        }
    }
    for url in urls(&descriptor) {
        if !host_of(url).is_some_and(|host| MOJANG_HOSTS.contains(&host)) {
            return Err(format!("{id}: the version file names another host"));
        }
    }
    Ok(descriptor)
}

/// A version file as published: its SHA-1 and bytes.
pub type Meta = (String, Vec<u8>);

/// The list of `data` (`data/extra-versions.json`) and every version file in
/// it as published, all checked.
pub fn build(data: &[u8], serial: u64) -> Result<(List, Vec<Meta>), String> {
    if data.len() as u64 > MAX_DATA_BYTES {
        return Err("the data is too large".to_owned());
    }
    let data: Data =
        serde_json::from_slice(data).map_err(|e| format!("extra-versions.json: {e}"))?;
    if data.schema_version != 1 {
        return Err(format!("schema version {}", data.schema_version));
    }
    let mut list = List {
        kind: KIND.to_owned(),
        serial,
        versions: Vec::new(),
    };
    let mut meta = Vec::new();
    for version in &data.versions {
        check_data_version(version)?;
        list.versions.push(Listed {
            id: version.id.clone(),
            name: version.name.clone(),
            title: version.title.clone(),
            group: version.group.clone(),
            variant: version.variant.clone(),
            note: version.note.clone(),
            version_type: version.version_type.clone(),
            release_time: version.release_time.clone(),
            server: version.server,
            sha1: version.sha1.clone(),
            size: version.descriptor.len() as u64,
        });
        meta.push((
            version.sha1.clone(),
            version.descriptor.clone().into_bytes(),
        ));
    }
    list.check()?;
    Ok((list, meta))
}

/// The SHA1SUMS of a published list: each version file and each game jar it
/// names, as `<sha1>  meta/<sha1>.json` and `<sha1>  files/<sha1>.jar`.
pub fn sums(meta: &[Meta]) -> Result<String, String> {
    let mut lines = Vec::new();
    let mut seen = HashSet::new();
    for (sha1, bytes) in meta {
        lines.push(format!("{sha1}  meta/{sha1}.json"));
        let descriptor: Value = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
        for (_, jar, _) in jars(&descriptor)? {
            if seen.insert(jar.clone()) {
                lines.push(format!("{jar}  files/{jar}.jar"));
            }
        }
    }
    Ok(lines.join("\n") + "\n")
}

/// Signs `list`; the trusted comment names the kind and the serial.
pub fn sign(list: &List, key: &minisign::SecretKey) -> Result<Envelope, String> {
    list.check()?;
    let text = serde_json::to_string(list).map_err(|e| e.to_string())?;
    if text.len() as u64 > MAX_BYTES {
        return Err("the list is too large".to_owned());
    }
    let trusted = format!(
        "timestamp:{}\tfile:{FILE}\tkind:{KIND}\tserial:{}",
        chrono::Utc::now().timestamp(),
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
/// base64 form) and names this kind and the list's serial.
pub fn verify(public_key: &str, envelope: &[u8]) -> Result<List, String> {
    if envelope.len() as u64 > MAX_BYTES * 2 {
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
    if field("kind:").as_deref() != Some(KIND) || field("serial:") != Some(list.serial.to_string())
    {
        return Err("the signature does not name this list".to_owned());
    }
    Ok(list)
}

/// `(sha1, path)` for every line of a SHA1SUMS that names a version file or
/// a jar as [`sums`] writes them; any other line is refused.
fn parse_sums(text: &str) -> Result<Vec<(String, String)>, String> {
    let mut entries = Vec::new();
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        let (sum, path) = line
            .split_once("  ")
            .ok_or("SHA1SUMS has a line it cannot read")?;
        let ok = sha1_ok(sum)
            && (path == format!("meta/{sum}.json") || path == format!("files/{sum}.jar"));
        if !ok {
            return Err("SHA1SUMS names a file that is not a version file or jar".to_owned());
        }
        entries.push((sum.to_owned(), path.to_owned()));
    }
    Ok(entries)
}

/// The SHA-1 of a file, read in pieces; `None` past `max` bytes or unreadable.
fn file_sha1(path: &Path, max: u64) -> Option<(String, u64)> {
    let mut file = fs::File::open(path).ok()?;
    let mut hasher = sha1::Sha1::new();
    let mut buffer = vec![0; 1 << 20];
    let mut total = 0u64;
    loop {
        let n = file.read(&mut buffer).ok()?;
        if n == 0 {
            break;
        }
        total += n as u64;
        if total > max {
            return None;
        }
        hasher.update(&buffer[..n]);
    }
    Some((hex::encode(hasher.finalize()), total))
}

fn read_bounded(path: &Path, max: u64) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    fs::File::open(path)
        .map_err(|_| format!("no {}", path.display()))?
        .take(max + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("reading {}: {e}", path.display()))?;
    if bytes.len() as u64 > max {
        return Err(format!("{} is too large", path.display()));
    }
    Ok(bytes)
}

impl Server {
    fn versions_dir(&self) -> PathBuf {
        self.root().join("versions")
    }

    /// The list served now, if any.
    fn versions_current(&self) -> Result<Option<List>, String> {
        let path = self.versions_dir().join(FILE);
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

    /// The SHA-1 of every jar in place, one per line, so CI uploads only the
    /// others.
    pub fn versions_have(&self) -> Result<String, String> {
        let dir = self.versions_dir().join("files");
        let mut have: Vec<String> = match fs::read_dir(&dir) {
            Ok(entries) => entries
                .filter_map(|e| e.ok()?.file_name().into_string().ok())
                .filter_map(|name| name.strip_suffix(".jar").map(str::to_owned))
                .filter(|sum| sha1_ok(sum))
                .collect(),
            Err(_) => Vec::new(),
        };
        have.sort();
        Ok(have.join("\n"))
    }

    /// Puts the files of `incoming/versions/` in place: every file SHA1SUMS
    /// names is either uploaded there, checked against its SHA-1 and copied in
    /// place, or in place already. A published file never changes.
    pub fn install_versions_files(&self) -> Result<String, String> {
        let incoming = self.root().join("incoming").join("versions");
        let sums = read_bounded(&incoming.join("SHA1SUMS"), MAX_SUMS_BYTES)?;
        let entries = parse_sums(&String::from_utf8_lossy(&sums))?;
        if entries.is_empty() {
            return Err("SHA1SUMS names no file".to_owned());
        }
        let target = self.versions_dir();
        // Every uploaded file is checked before any is moved.
        let mut moves = Vec::new();
        let mut kept = 0;
        for (sum, path) in &entries {
            let max = if path.starts_with("meta/") {
                MAX_DESCRIPTOR_BYTES
            } else {
                MAX_JAR_BYTES
            };
            let source = incoming.join(path);
            let place = target.join(path);
            if source.exists() {
                if file_sha1(&source, max).map(|(s, _)| s).as_deref() != Some(sum.as_str()) {
                    return Err(format!("{path} does not match SHA1SUMS"));
                }
                if place.exists() {
                    kept += 1;
                } else {
                    moves.push((sum, path, max, source, place));
                }
            } else if place.exists() {
                kept += 1;
            } else {
                return Err(format!("{path} was not uploaded and is not in place"));
            }
        }
        let new = moves.len();
        for (sum, path, max, source, place) in moves {
            let dir = place.parent().ok_or("no folder")?;
            fs::create_dir_all(dir).map_err(|e| format!("creating {}: {e}", dir.display()))?;
            // Copied, not moved: the upload folder is CI's, and CI empties it.
            let temp = dir.join(format!(".{sum}.tmp"));
            fs::copy(&source, &temp).map_err(|e| format!("copying {path}: {e}"))?;
            // Checked again as copied: CI could change the upload meanwhile.
            if file_sha1(&temp, max).map(|(s, _)| s).as_deref() != Some(sum.as_str()) {
                let _ = fs::remove_file(&temp);
                return Err(format!("{path} changed while it was put in place"));
            }
            fs::rename(&temp, &place).map_err(|e| format!("putting {path} in place: {e}"))?;
        }
        Ok(format!(
            "versions: {} files, {new} new, {kept} in place",
            entries.len()
        ))
    }

    /// The files `list` names that are not in place: each version file with
    /// its SHA-1, and each jar it names.
    fn versions_missing(&self, list: &List) -> Result<Vec<String>, String> {
        let dir = self.versions_dir();
        let mut missing = Vec::new();
        for version in &list.versions {
            let path = dir.join("meta").join(format!("{}.json", version.sha1));
            let Ok(bytes) = read_bounded(&path, MAX_DESCRIPTOR_BYTES) else {
                missing.push(format!("meta/{}.json", version.sha1));
                continue;
            };
            if sha1_hex(&bytes) != version.sha1 || bytes.len() as u64 != version.size {
                return Err(format!(
                    "the version file of {} in place is not the listed one",
                    version.id
                ));
            }
            let descriptor: Value = serde_json::from_slice(&bytes)
                .map_err(|_| format!("the version file of {} is not JSON", version.id))?;
            for (_, sha1, size) in jars(&descriptor).map_err(|e| format!("{}: {e}", version.id))? {
                let jar = dir.join("files").join(format!("{sha1}.jar"));
                if !fs::metadata(&jar).is_ok_and(|m| m.len() == size) {
                    missing.push(format!("files/{sha1}.jar"));
                }
            }
        }
        Ok(missing)
    }

    /// Puts `incoming/versions.json` in place after checking it against the
    /// public key, that its serial is newer than the list served now and that
    /// every file it names is in place (`versions-files`).
    pub fn install_versions(&self) -> Result<String, String> {
        let incoming = self.root().join("incoming").join("versions.json");
        let bytes = read_bounded(&incoming, MAX_BYTES * 2)?;
        let list = verify(&self.public_key()?, &bytes)?;
        let missing = self.versions_missing(&list)?;
        if !missing.is_empty() {
            return Err(format!(
                "{} files of the list are not in place: run versions-files first",
                missing.len()
            ));
        }
        if let Some(current) = self.versions_current()?
            && current.serial >= list.serial
        {
            return Err(format!(
                "a list as new or newer is served (serial {})",
                current.serial
            ));
        }
        write_atomic(&self.versions_dir().join(FILE), &bytes)?;
        let _ = fs::remove_file(&incoming);
        Ok(format!(
            "versions: {} listed (serial {})",
            list.versions.len(),
            list.serial
        ))
    }

    /// One line for `status`.
    pub fn versions_status(&self) -> Result<String, String> {
        Ok(match self.versions_current()? {
            Some(list) => format!(
                "versions: {} listed (serial {}), {} files not in place",
                list.versions.len(),
                list.serial,
                self.versions_missing(&list)?.len()
            ),
            None => "versions: no list".to_owned(),
        })
    }
}

#[cfg(test)]
mod tests;
