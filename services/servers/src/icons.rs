//! Server icons: decoded with limits, made 64×64 and written again as PNG,
//! kept by their SHA-256 in `data/icons`. Nothing a server sent reaches a
//! player as it was.

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

/// The game's icons are 64×64; a picture the owner gives may be larger.
pub const SIZE: u32 = 64;
pub const MAX_SIDE: u32 = 1024;
pub const MAX_BYTES: usize = 1024 * 1024;

pub fn sha_name(name: &str) -> bool {
    name.len() == 64 && name.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// A PNG of a square picture, as 64×64 PNG bytes.
pub fn clean(bytes: &[u8]) -> Result<Vec<u8>, String> {
    if bytes.len() > MAX_BYTES {
        return Err("the icon is larger than 1 MB".to_owned());
    }
    if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Err("the icon is not a PNG".to_owned());
    }
    let mut reader =
        image::ImageReader::with_format(std::io::Cursor::new(bytes), image::ImageFormat::Png);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_SIDE);
    limits.max_image_height = Some(MAX_SIDE);
    limits.max_alloc = Some(16 * 1024 * 1024);
    reader.limits(limits);
    let picture = reader
        .decode()
        .map_err(|e| format!("the icon does not decode: {e}"))?;
    if picture.width() != picture.height() || picture.width() < 16 {
        return Err("the icon must be square, at least 16×16".to_owned());
    }
    let picture = if picture.width() == SIZE {
        picture.to_rgba8()
    } else {
        image::imageops::resize(
            &picture.to_rgba8(),
            SIZE,
            SIZE,
            image::imageops::FilterType::Triangle,
        )
    };
    let mut out = Vec::new();
    image::DynamicImage::ImageRgba8(picture)
        .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
        .map_err(|e| e.to_string())?;
    Ok(out)
}

/// The icon in a status answer: `data:image/png;base64,…`.
pub fn from_status(favicon: &str) -> Result<Vec<u8>, String> {
    use base64::Engine;
    let data = favicon
        .strip_prefix("data:image/png;base64,")
        .ok_or_else(|| "the server's icon is not a PNG data URL".to_owned())?;
    let data: String = data.chars().filter(|c| !c.is_whitespace()).collect();
    if data.len() > MAX_BYTES * 4 / 3 + 4 {
        return Err("the server's icon is too large".to_owned());
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data)
        .map_err(|_| "the server's icon is not base64".to_owned())?;
    clean(&bytes)
}

pub struct Icons {
    dir: PathBuf,
}

impl Icons {
    pub fn new(data: &Path) -> Result<Self, String> {
        let dir = data.join("icons");
        std::fs::create_dir_all(&dir).map_err(|e| format!("creating {}: {e}", dir.display()))?;
        Ok(Self { dir })
    }

    /// Keeps cleaned PNG bytes; their SHA-256 names them.
    pub fn keep(&self, png: &[u8]) -> Result<String, String> {
        let sha = hex::encode(Sha256::digest(png));
        let path = self.dir.join(format!("{sha}.png"));
        if !path.exists() {
            let part = self.dir.join(format!(".{sha}.part"));
            std::fs::write(&part, png).map_err(|e| e.to_string())?;
            std::fs::rename(&part, &path).map_err(|e| e.to_string())?;
        }
        Ok(sha)
    }

    pub fn read(&self, sha: &str) -> Option<Vec<u8>> {
        if !sha_name(sha) {
            return None;
        }
        std::fs::read(self.dir.join(format!("{sha}.png"))).ok()
    }

    /// Removes icons no ad uses any more.
    pub fn clean(&self, used: &[String]) {
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            let keep = name
                .strip_suffix(".png")
                .is_some_and(|sha| used.iter().any(|u| u == sha));
            if !keep {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
}
