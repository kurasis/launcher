//! `lapi-upd pack`: signs a version's files and builds its release archive
//! (D-063). Run by the Package workflow, which has the signing key; the server
//! never does.
//!
//! The key is Tauri's (`tauri signer generate`): `TAURI_SIGNING_PRIVATE_KEY`
//! (base64 of a minisign secret key) and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`.
//! Each signature's trusted comment names the version, which the launcher
//! requires, so an old signed file cannot be announced as a new version.

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::Path;

use base64::Engine as _;
use sha2::Digest as _;

use crate::release::{Platform, Release};

/// A Tauri signature of `data` naming `version`.
pub fn sign(
    key: &minisign::SecretKey,
    data: &[u8],
    file: &str,
    version: &str,
) -> Result<String, String> {
    let trusted = format!(
        "timestamp:{}\tfile:{file}\tversion:{version}",
        chrono::Utc::now().timestamp()
    );
    let signature = minisign::sign(
        None,
        key,
        data,
        Some(&trusted),
        Some("signature from tauri secret key"),
    )
    .map_err(|e| format!("signing {file}: {e}"))?;
    Ok(base64::engine::general_purpose::STANDARD.encode(signature.to_string()))
}

/// Reads Tauri's private key from its base64 form.
pub fn secret_key(encoded: &str, password: &str) -> Result<minisign::SecretKey, String> {
    let text = base64::engine::general_purpose::STANDARD
        .decode(encoded.trim())
        .ok()
        .and_then(|b| String::from_utf8(b).ok())
        .ok_or("TAURI_SIGNING_PRIVATE_KEY is not a Tauri key")?;
    minisign::SecretKeyBox::from_string(&text)
        .and_then(|b| b.into_secret_key(Some(password.to_owned())))
        .map_err(|_| "the signing key or its password is wrong".to_owned())
}

/// `<target>:<bundle>:<path>` arguments, signed (but `deb`), into
/// `out`: the files under their own names and `release.json`.
pub fn pack(
    version: &str,
    out: &Path,
    notes: BTreeMap<String, String>,
    items: &[String],
    key: &minisign::SecretKey,
) -> Result<String, String> {
    semver::Version::parse(version).map_err(|_| format!("{version} is not a version"))?;
    let mut release = Release {
        version: version.to_owned(),
        pub_date: Some(chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)),
        notes,
        platforms: Vec::new(),
    };
    let gz = flate2::write::GzEncoder::new(
        fs::File::create(out).map_err(|e| format!("creating {}: {e}", out.display()))?,
        flate2::Compression::default(),
    );
    let mut tar = tar::Builder::new(gz);
    let mut add = |name: &str, bytes: &[u8]| -> Result<(), String> {
        let mut header = tar::Header::new_gnu();
        header.set_size(bytes.len() as u64);
        header.set_mode(0o644);
        header.set_mtime(0);
        tar.append_data(&mut header, name, bytes)
            .map_err(|e| e.to_string())
    };
    for item in items {
        let mut parts = item.splitn(3, ':');
        let (Some(target), Some(bundle), Some(path)) = (parts.next(), parts.next(), parts.next())
        else {
            return Err(format!("{item}: expected <target>:<bundle>:<path>"));
        };
        let path = Path::new(path);
        let file = path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| format!("{item}: no file name"))?
            .to_owned();
        let bytes = fs::read(path).map_err(|e| format!("reading {}: {e}", path.display()))?;
        let signature = if bundle == "deb" {
            String::new()
        } else {
            sign(key, &bytes, &file, version)?
        };
        release.platforms.push(Platform {
            target: target.to_owned(),
            bundle: bundle.to_owned(),
            file: file.clone(),
            sha256: hex::encode(sha2::Sha256::digest(&bytes)),
            signature,
        });
        add(&file, &bytes)?;
    }
    let manifest = serde_json::to_vec_pretty(&release).map_err(|e| e.to_string())?;
    add("release.json", &manifest)?;
    tar.into_inner()
        .and_then(|gz| gz.finish())
        .and_then(|mut f| f.flush())
        .map_err(|e| format!("writing {}: {e}", out.display()))?;
    Ok(format!(
        "{} with {} files",
        out.display(),
        release.platforms.len()
    ))
}
