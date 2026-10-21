//! Releases and channels on disk (D-063).
//!
//! ```text
//! <root>/incoming/launcher-<version>-release.tar.gz   uploaded by the admin
//! <root>/releases/<version>/                          files + release.json,
//!                                                     <target>-<bundle>.json
//!                                                     (this version alone,
//!                                                     for a return to it)
//! <root>/channels/<channel>/state.json                current, history, rollout
//! <root>/channels/<channel>/<target>-<bundle>.json    what launchers read
//! <root>/download/index.html                          files for manual download
//! ```
//!
//! The public key is a separate file the deploy writes.
//!
//! Every change is written to a temporary name and renamed into place, so nginx
//! never serves half a file.

use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use base64::Engine as _;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::Digest as _;

/// What every release must hold unless published with `--partial`.
pub const REQUIRED: &[(&str, &str)] = &[
    ("windows-x86_64", "nsis"),
    ("windows-aarch64", "nsis"),
    ("darwin-aarch64", "app"),
    ("darwin-x86_64", "app"),
    ("linux-x86_64", "appimage"),
    ("linux-x86_64", "deb"),
];
/// Install types the launcher only tells about: no signature, no install.
const NOTICE_ONLY: &[&str] = &["deb"];
pub(crate) const CHANNELS: &[&str] = &["stable", "beta"];

/// `release.json` in a release archive, written by the Package workflow.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Release {
    pub version: String,
    #[serde(default)]
    pub pub_date: Option<String>,
    /// Release notes by language tag; `en` is the fallback.
    #[serde(default)]
    pub notes: BTreeMap<String, String>,
    pub platforms: Vec<Platform>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Platform {
    /// The updater's target, `windows-x86_64`.
    pub target: String,
    /// `nsis`, `app`, `appimage`, `deb`.
    pub bundle: String,
    pub file: String,
    pub sha256: String,
    /// The `.sig` file's content; empty for notice-only bundles.
    #[serde(default)]
    pub signature: String,
}

/// A channel's state.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Channel {
    pub current: Option<String>,
    /// Versions published before, oldest first.
    #[serde(default)]
    pub history: Vec<String>,
    pub rollout: u8,
    #[serde(default)]
    pub paused: bool,
    #[serde(default)]
    pub mandatory: bool,
    #[serde(default)]
    pub revoked: Vec<String>,
}

pub struct Options {
    pub channel: String,
    pub rollout: u8,
    pub mandatory: bool,
    pub partial: bool,
}

pub struct Server {
    root: PathBuf,
    base_url: String,
    key: PathBuf,
}

fn version(text: &str) -> Result<semver::Version, String> {
    semver::Version::parse(text).map_err(|_| format!("{text} is not a version like 1.2.3"))
}

/// Plain file names only: a release archive cannot write outside its folder.
fn plain_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 200
        && name != "."
        && name != ".."
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_' | b'+'))
}

/// Writes whole, then renames into place.
pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let dir = path.parent().ok_or("no folder")?;
    fs::create_dir_all(dir).map_err(|e| format!("creating {}: {e}", dir.display()))?;
    let temp = dir.join(format!(
        ".{}.tmp",
        path.file_name().and_then(|n| n.to_str()).unwrap_or("file")
    ));
    fs::write(&temp, bytes).map_err(|e| format!("writing {}: {e}", temp.display()))?;
    fs::rename(&temp, path).map_err(|e| format!("replacing {}: {e}", path.display()))
}

/// Checks a Tauri signature (base64 of a minisign signature) of `data`
/// against a Tauri public key (base64 of a minisign public key), and that the
/// signed trusted comment names `version`.
pub fn verify_signature(
    public_key: &str,
    data: &[u8],
    signature: &str,
    version: &str,
) -> Result<(), String> {
    let b64 = |s: &str| {
        base64::engine::general_purpose::STANDARD
            .decode(s.trim())
            .ok()
            .and_then(|bytes| String::from_utf8(bytes).ok())
    };
    let key_text = b64(public_key).ok_or("the public key is not valid")?;
    let key =
        minisign_verify::PublicKey::decode(&key_text).map_err(|_| "the public key is not valid")?;
    let signature_text = b64(signature).ok_or("the signature is not valid base64")?;
    let signature = minisign_verify::Signature::decode(&signature_text)
        .map_err(|_| "the signature is not valid")?;
    key.verify(data, &signature, false)
        .map_err(|_| "the signature does not match the file")?;
    let signed = signature
        .trusted_comment()
        .split('\t')
        .find_map(|field| field.strip_prefix("version:"));
    match signed {
        Some(v) if v.trim_start_matches('v') == version => Ok(()),
        Some(v) => Err(format!("the file was signed for version {v}")),
        None => Err("the signature does not name a version".to_owned()),
    }
}

impl Server {
    pub fn new(root: PathBuf, base_url: String, key: PathBuf) -> Self {
        Self {
            root,
            base_url: base_url.trim_end_matches('/').to_owned(),
            key,
        }
    }

    pub(crate) fn root(&self) -> &Path {
        &self.root
    }

    pub(crate) fn channel_dir(&self, channel: &str) -> Result<PathBuf, String> {
        if !CHANNELS.contains(&channel) {
            return Err(format!("unknown channel {channel} (stable or beta)"));
        }
        Ok(self.root.join("channels").join(channel))
    }

    fn load(&self, channel: &str) -> Result<Channel, String> {
        let path = self.channel_dir(channel)?.join("state.json");
        match fs::read(&path) {
            Ok(bytes) => {
                serde_json::from_slice(&bytes).map_err(|e| format!("{}: {e}", path.display()))
            }
            Err(_) => Ok(Channel::default()),
        }
    }

    fn save(&self, channel: &str, state: &Channel) -> Result<(), String> {
        let bytes = serde_json::to_vec_pretty(state).map_err(|e| e.to_string())?;
        write_atomic(&self.channel_dir(channel)?.join("state.json"), &bytes)
    }

    fn release(&self, version: &str) -> Result<Release, String> {
        let path = self
            .root
            .join("releases")
            .join(version)
            .join("release.json");
        let bytes = fs::read(&path).map_err(|_| format!("no published release {version}"))?;
        serde_json::from_slice(&bytes).map_err(|e| format!("{}: {e}", path.display()))
    }

    pub(crate) fn public_key(&self) -> Result<String, String> {
        fs::read_to_string(&self.key)
            .map(|k| k.trim().to_owned())
            .ok()
            .filter(|k| !k.is_empty())
            .ok_or_else(|| {
                "no update-key.pub: set the repository variable UPDATE_PUBLIC_KEY and deploy"
                    .to_owned()
            })
    }

    /// Unpacks an archive into `into`, plain files only.
    fn unpack(archive: &Path, into: &Path) -> Result<(), String> {
        let file = fs::File::open(archive).map_err(|_| format!("no {}", archive.display()))?;
        let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(file));
        fs::create_dir_all(into).map_err(|e| e.to_string())?;
        for entry in tar
            .entries()
            .map_err(|e| format!("reading the archive: {e}"))?
        {
            let mut entry = entry.map_err(|e| format!("reading the archive: {e}"))?;
            let kind = entry.header().entry_type();
            let path = entry.path().map_err(|e| e.to_string())?.into_owned();
            let name = path
                .to_str()
                .unwrap_or_default()
                .trim_start_matches("./")
                .to_owned();
            if kind.is_dir() && (name.is_empty() || name == ".") {
                continue;
            }
            if !kind.is_file() || !plain_name(&name) {
                return Err(format!("the archive holds {name:?}, not a plain file"));
            }
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes).map_err(|e| e.to_string())?;
            fs::write(into.join(&name), bytes).map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    /// Checks an unpacked release against the rules in D-063.
    fn check(&self, dir: &Path, wanted: &str, partial: bool) -> Result<Release, String> {
        let release: Release = serde_json::from_slice(
            &fs::read(dir.join("release.json")).map_err(|_| "the archive has no release.json")?,
        )
        .map_err(|e| format!("release.json: {e}"))?;
        if release.version != wanted {
            return Err(format!(
                "the archive is version {}, not {wanted}",
                release.version
            ));
        }
        let key = self.public_key()?;
        for platform in &release.platforms {
            if !plain_name(&platform.file) {
                return Err(format!("bad file name {:?}", platform.file));
            }
            // Both name manifest files and appear on the download page.
            if !plain_name(&platform.target) || !plain_name(&platform.bundle) {
                return Err(format!("bad target or bundle for {:?}", platform.file));
            }
            let bytes = fs::read(dir.join(&platform.file))
                .map_err(|_| format!("{} is listed but not in the archive", platform.file))?;
            if hex::encode(sha2::Sha256::digest(&bytes)) != platform.sha256.to_ascii_lowercase() {
                return Err(format!("{} does not match its checksum", platform.file));
            }
            if !NOTICE_ONLY.contains(&platform.bundle.as_str()) {
                verify_signature(&key, &bytes, &platform.signature, wanted)
                    .map_err(|e| format!("{}: {e}", platform.file))?;
            }
        }
        if !partial {
            let missing: Vec<String> = REQUIRED
                .iter()
                .filter(|(t, b)| {
                    !release
                        .platforms
                        .iter()
                        .any(|p| p.target == *t && p.bundle == *b)
                })
                .map(|(t, b)| format!("{t}/{b}"))
                .collect();
            if !missing.is_empty() {
                return Err(format!(
                    "missing {} (publish with --partial to leave them on the previous version)",
                    missing.join(", ")
                ));
            }
        }
        Ok(release)
    }

    /// Tauri's manifest of `platform` in `release`, plus our fields.
    fn manifest(
        &self,
        release: &Release,
        platform: &Platform,
        state: &Channel,
    ) -> serde_json::Value {
        let version = &release.version;
        let url = format!("{}/releases/{version}/{}", self.base_url, platform.file);
        let notice_only = NOTICE_ONLY.contains(&platform.bundle.as_str());
        json!({
            "version": version,
            "notes": release.notes.get("en").cloned().unwrap_or_default(),
            "pub_date": release.pub_date,
            "url": url,
            "signature": if notice_only { "" } else { platform.signature.as_str() },
            "rollout": if state.paused { 0 } else { state.rollout },
            "mandatory": state.mandatory,
            "revoked": state.revoked,
            "notesByLanguage": release.notes,
            "download": if notice_only { url.clone() } else { format!("{}/download/", self.base_url) },
            "installable": !notice_only,
        })
    }

    /// The manifests of a channel from its state, one per platform of the
    /// current release.
    fn write_manifests(&self, channel: &str, state: &Channel) -> Result<(), String> {
        let Some(current) = &state.current else {
            return Ok(());
        };
        let release = self.release(current)?;
        let dir = self.channel_dir(channel)?;
        for platform in &release.platforms {
            let name = format!("{}-{}.json", platform.target, platform.bundle);
            write_atomic(
                &dir.join(name),
                &serde_json::to_vec_pretty(&self.manifest(&release, platform, state))
                    .unwrap_or_default(),
            )?;
        }
        Ok(())
    }

    /// One manifest per installable platform next to a release's files: a
    /// launcher whose new version does not start returns to the version it
    /// came from with it, whatever the channels say by then.
    fn write_version_manifests(&self, dir: &Path, release: &Release) -> Result<(), String> {
        let whole = Channel {
            rollout: 100,
            ..Channel::default()
        };
        for platform in release
            .platforms
            .iter()
            .filter(|p| !NOTICE_ONLY.contains(&p.bundle.as_str()))
        {
            let path = dir.join(format!("{}-{}.json", platform.target, platform.bundle));
            if !path.exists() {
                write_atomic(
                    &path,
                    &serde_json::to_vec_pretty(&self.manifest(release, platform, &whole))
                        .unwrap_or_default(),
                )?;
            }
        }
        Ok(())
    }

    fn write_download_page(&self) -> Result<(), String> {
        let state = self.load("stable")?;
        let Some(current) = &state.current else {
            return Ok(());
        };
        let release = self.release(current)?;
        let rows: String = release
            .platforms
            .iter()
            .map(|p| {
                format!(
                    "<li><a href=\"../releases/{current}/{f}\">{f}</a> <small>{t} · {b}</small></li>\n",
                    f = p.file,
                    t = p.target,
                    b = p.bundle
                )
            })
            .collect();
        let page = format!(
            "<!doctype html><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width\">\n<title>Launcher {current}</title>\n<h1>Launcher {current}</h1>\n<ul>\n{rows}</ul>\n"
        );
        write_atomic(
            &self.root.join("download").join("index.html"),
            page.as_bytes(),
        )
    }

    pub fn publish(&self, wanted: &str, options: &Options) -> Result<String, String> {
        let new = version(wanted)?;
        if options.channel == "stable" && !new.pre.is_empty() {
            return Err(format!(
                "{wanted} is a pre-release: publish it with --channel beta"
            ));
        }
        let mut state = self.load(&options.channel)?;
        // A revoked version stays revoked: after going back, the channel's
        // current version is older, so only this check stops it coming back.
        if state.revoked.iter().any(|v| v == wanted) {
            return Err(format!(
                "{wanted} was revoked on {}; publish a newer version",
                options.channel
            ));
        }
        if let Some(current) = &state.current
            && version(current)? >= new
        {
            return Err(format!(
                "{} already has {current}; publish a newer version",
                options.channel
            ));
        }
        let archive = self
            .root
            .join("incoming")
            .join(format!("launcher-{wanted}-release.tar.gz"));
        let destination = self.root.join("releases").join(wanted);
        if !destination.exists() {
            let temp = self.root.join("releases").join(format!(".{wanted}.tmp"));
            let _ = fs::remove_dir_all(&temp);
            let checked = Self::unpack(&archive, &temp)
                .and_then(|()| self.check(&temp, wanted, options.partial))
                .and_then(|release| self.write_version_manifests(&temp, &release));
            if let Err(e) = checked {
                let _ = fs::remove_dir_all(&temp);
                return Err(e);
            }
            fs::rename(&temp, &destination).map_err(|e| format!("placing the release: {e}"))?;
        } else {
            // Published to another channel before: checked then.
            let release = self.release(wanted)?;
            self.write_version_manifests(&destination, &release)?;
        }
        if let Some(previous) = state.current.take() {
            state.history.push(previous);
        }
        state.current = Some(wanted.to_owned());
        state.rollout = options.rollout;
        state.paused = false;
        state.mandatory = options.mandatory;
        self.write_manifests(&options.channel, &state)?;
        self.save(&options.channel, &state)?;
        let beta = if options.channel == "stable" {
            self.beta_follows(wanted, &new, &state)?
        } else {
            false
        };
        self.write_download_page()?;
        Ok(format!(
            "{wanted} is current on {} for {}% of installs{}{}",
            options.channel,
            options.rollout,
            if options.mandatory { ", mandatory" } else { "" },
            if beta { "; beta has it too" } else { "" }
        ))
    }

    /// Beta is never behind stable: a stable version newer than beta's becomes
    /// beta's too, with the same rollout.
    fn beta_follows(
        &self,
        wanted: &str,
        new: &semver::Version,
        stable: &Channel,
    ) -> Result<bool, String> {
        let mut beta = self.load("beta")?;
        if let Some(current) = &beta.current
            && version(current)? >= *new
        {
            return Ok(false);
        }
        if let Some(previous) = beta.current.take() {
            beta.history.push(previous);
        }
        beta.current = Some(wanted.to_owned());
        beta.rollout = stable.rollout;
        beta.paused = false;
        beta.mandatory = stable.mandatory;
        self.write_manifests("beta", &beta)?;
        self.save("beta", &beta)?;
        Ok(true)
    }

    fn change(
        &self,
        channel: &str,
        edit: impl FnOnce(&mut Channel) -> Result<(), String>,
    ) -> Result<Channel, String> {
        let mut state = self.load(channel)?;
        if state.current.is_none() {
            return Err(format!("nothing is published on {channel}"));
        }
        edit(&mut state)?;
        self.write_manifests(channel, &state)?;
        self.save(channel, &state)?;
        self.write_download_page()?;
        Ok(state)
    }

    pub fn set_rollout(&self, channel: &str, percent: u8) -> Result<String, String> {
        self.change(channel, |s| {
            s.rollout = percent;
            Ok(())
        })
        .map(|s| {
            format!(
                "{} on {channel}: {percent}% of installs",
                s.current.unwrap_or_default()
            )
        })
    }

    pub fn pause(&self, channel: &str, paused: bool) -> Result<String, String> {
        self.change(channel, |s| {
            s.paused = paused;
            Ok(())
        })
        .map(|_| {
            format!(
                "{channel} {}",
                if paused {
                    "paused: no install is offered the version"
                } else {
                    "resumed"
                }
            )
        })
    }

    /// The channel goes back to the version before; installs on the revoked
    /// one are offered that version although it is older.
    pub fn revoke(&self, channel: &str, wanted: &str) -> Result<String, String> {
        self.change(channel, |s| {
            if s.current.as_deref() != Some(wanted) {
                return Err(format!("{wanted} is not current on {channel}"));
            }
            let previous = s
                .history
                .pop()
                .ok_or("there is no earlier version to go back to")?;
            s.current = Some(previous);
            if !s.revoked.iter().any(|v| v == wanted) {
                s.revoked.push(wanted.to_owned());
            }
            s.rollout = 100;
            s.paused = false;
            // Leaving a broken version is not optional.
            s.mandatory = true;
            Ok(())
        })
        .and_then(|s| {
            let back = s.current.unwrap_or_default();
            let mut beta = self.load("beta")?;
            let follows = channel == "stable" && beta.current.as_deref() == Some(wanted);
            if follows {
                // Beta got the version from stable; it leaves it the same way.
                beta.current = Some(back.clone());
                if !beta.revoked.iter().any(|v| v == wanted) {
                    beta.revoked.push(wanted.to_owned());
                }
                beta.rollout = 100;
                beta.paused = false;
                beta.mandatory = true;
                self.write_manifests("beta", &beta)?;
                self.save("beta", &beta)?;
            }
            Ok(format!(
                "{wanted} revoked; {channel} is back on {back}{}",
                if follows { ", and so is beta" } else { "" }
            ))
        })
    }

    pub fn status(&self) -> Result<String, String> {
        let mut out = String::new();
        for channel in CHANNELS {
            let s = self.load(channel)?;
            out.push_str(&match &s.current {
                None => format!("{channel}: nothing published\n"),
                Some(v) => format!(
                    "{channel}: {v}, {}{}; before: {}; revoked: {}\n",
                    if s.paused {
                        "paused".to_owned()
                    } else {
                        format!("{}% of installs", s.rollout)
                    },
                    if s.mandatory { ", mandatory" } else { "" },
                    if s.history.is_empty() {
                        "-".to_owned()
                    } else {
                        s.history.join(", ")
                    },
                    if s.revoked.is_empty() {
                        "-".to_owned()
                    } else {
                        s.revoked.join(", ")
                    },
                ),
            });
        }
        Ok(out.trim_end().to_owned())
    }
}

#[cfg(test)]
mod tests;
