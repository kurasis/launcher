//! Screenshot gallery across installations (D-050).
//!
//! Minecraft saves screenshots as PNG files in `screenshots/` of each game
//! folder. The gallery lists them by installation, serves them to the webview
//! through the `gallery:` protocol (full size, or a cached thumbnail), and can
//! delete one or show it in the file manager. The webview names a screenshot
//! only by installation ID and file name; Rust resolves and checks the path.

use std::fs;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::{ErrorCode, LauncherError, LauncherResult};
use crate::state::AppState;
use crate::storage::paths::{DataRoot, InstanceId};

/// The protocol the webview loads screenshots from.
pub const PROTOCOL: &str = "gallery";
/// Thumbnail width in pixels: sharp at about 240 CSS pixels on a 2× display.
const THUMB_WIDTH: u32 = 480;
/// Most memory a screenshot may take decoded (an 8K RGBA image is 132 MiB).
const MAX_DECODED_BYTES: usize = 256 * 1024 * 1024;

/// One screenshot file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Screenshot {
    pub file: String,
    pub taken_at: DateTime<Utc>,
    pub bytes: u64,
    /// From the PNG header; `None` if it cannot be read.
    pub width: Option<u32>,
    pub height: Option<u32>,
}

/// The screenshots of one installation, newest first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstanceScreenshots {
    pub instance_id: InstanceId,
    pub screenshots: Vec<Screenshot>,
}

/// Every installation's screenshots, the installation with the newest first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Gallery {
    pub installations: Vec<InstanceScreenshots>,
    pub total_bytes: u64,
}

/// A file name Minecraft could have written: a plain `.png` name, nothing that
/// leaves the folder.
pub fn valid_file_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 255
        && !name.starts_with('.')
        && !name.contains(['/', '\\', ':'])
        && !name.chars().any(char::is_control)
        && name.to_ascii_lowercase().ends_with(".png")
}

fn invalid() -> LauncherError {
    LauncherError::invalid_input("This screenshot name is invalid.")
}

fn missing() -> LauncherError {
    LauncherError::new(ErrorCode::InvalidInput, "This screenshot no longer exists.")
}

fn screenshots_dir(root: &DataRoot, id: InstanceId) -> PathBuf {
    root.game_dir(id).join("screenshots")
}

/// The path of a screenshot that exists as a regular file (not a link).
pub fn resolve(root: &DataRoot, id: InstanceId, file: &str) -> LauncherResult<PathBuf> {
    if !valid_file_name(file) {
        return Err(invalid());
    }
    let path = screenshots_dir(root, id).join(file);
    match fs::symlink_metadata(&path) {
        Ok(meta) if meta.is_file() => Ok(path),
        _ => Err(missing()),
    }
}

/// Width and height from a PNG header, without decoding the image.
fn png_size(path: &Path) -> Option<(u32, u32)> {
    let mut header = [0u8; 24];
    fs::File::open(path).ok()?.read_exact(&mut header).ok()?;
    if &header[..8] != b"\x89PNG\r\n\x1a\n" || &header[12..16] != b"IHDR" {
        return None;
    }
    let width = u32::from_be_bytes(header[16..20].try_into().ok()?);
    let height = u32::from_be_bytes(header[20..24].try_into().ok()?);
    Some((width, height))
}

/// The screenshots of one game folder, newest first.
pub fn screenshots(root: &DataRoot, id: InstanceId) -> Vec<Screenshot> {
    let Ok(entries) = fs::read_dir(screenshots_dir(root, id)) else {
        return Vec::new();
    };
    let mut found: Vec<Screenshot> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let file = entry.file_name().to_str()?.to_owned();
            let meta = entry.metadata().ok()?;
            // `DirEntry::metadata` does not follow links, so links are skipped.
            if !valid_file_name(&file) || !meta.is_file() {
                return None;
            }
            let size = png_size(&entry.path());
            Some(Screenshot {
                file,
                taken_at: meta.modified().ok()?.into(),
                bytes: meta.len(),
                width: size.map(|s| s.0),
                height: size.map(|s| s.1),
            })
        })
        .collect();
    found.sort_by(|a, b| b.taken_at.cmp(&a.taken_at).then(b.file.cmp(&a.file)));
    found
}

fn data_root(state: &AppState) -> LauncherResult<DataRoot> {
    state.data_root().ok_or_else(|| {
        LauncherError::new(
            ErrorCode::StorageUnavailable,
            "Launcher data is unavailable.",
        )
    })
}

fn known(state: &AppState, id: InstanceId) -> LauncherResult<()> {
    state.instance(id).map(|_| ()).ok_or_else(|| {
        LauncherError::new(
            ErrorCode::InstanceNotFound,
            "The installation no longer exists.",
        )
    })
}

/// Every installation's screenshots. Thumbnails of screenshots that are gone
/// are removed from the cache.
pub fn list(state: &AppState) -> LauncherResult<Gallery> {
    let root = data_root(state)?;
    let mut installations: Vec<InstanceScreenshots> = state
        .instance_ids()
        .into_iter()
        .map(|id| InstanceScreenshots {
            instance_id: id,
            screenshots: screenshots(&root, id),
        })
        .filter(|i| !i.screenshots.is_empty())
        .collect();
    installations.sort_by(|a, b| {
        let newest = |i: &InstanceScreenshots| i.screenshots.first().map(|s| s.taken_at);
        newest(b).cmp(&newest(a))
    });
    let mut keep = std::collections::HashSet::new();
    for group in &installations {
        for shot in &group.screenshots {
            let path = screenshots_dir(&root, group.instance_id).join(&shot.file);
            if let Ok(meta) = fs::metadata(&path) {
                keep.insert(thumbnail_name(group.instance_id, &shot.file, &meta));
            }
        }
    }
    prune_thumbnails(&root, &keep);
    let total_bytes = installations
        .iter()
        .flat_map(|i| &i.screenshots)
        .map(|s| s.bytes)
        .sum();
    Ok(Gallery {
        installations,
        total_bytes,
    })
}

/// Deletes a screenshot and its thumbnail.
pub fn delete(state: &AppState, id: InstanceId, file: &str) -> LauncherResult<()> {
    known(state, id)?;
    let root = data_root(state)?;
    let path = resolve(&root, id, file)?;
    let meta = fs::metadata(&path).map_err(|_| missing())?;
    let thumb = root.thumbnails_dir().join(thumbnail_name(id, file, &meta));
    fs::remove_file(&path).map_err(|e| LauncherError::from_io(&e, "Deleting a screenshot"))?;
    let _ = fs::remove_file(thumb);
    state.console().info(format!("Screenshot {file} deleted"));
    Ok(())
}

/// The path of a screenshot of a known installation, for opening it.
pub fn path(state: &AppState, id: InstanceId, file: &str) -> LauncherResult<PathBuf> {
    known(state, id)?;
    resolve(&data_root(state)?, id, file)
}

/// Thumbnails are named by the screenshot and its size and time, so a changed
/// file gets a new one.
fn thumbnail_name(id: InstanceId, file: &str, meta: &fs::Metadata) -> String {
    let modified = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_nanos());
    let key = format!("{id}\n{file}\n{}\n{modified}", meta.len());
    format!("{}.png", hex::encode(Sha256::digest(key.as_bytes())))
}

fn prune_thumbnails(root: &DataRoot, keep: &std::collections::HashSet<String>) {
    let Ok(entries) = fs::read_dir(root.thumbnails_dir()) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let name = entry.file_name();
        if !name.to_str().is_some_and(|n| keep.contains(n)) {
            let _ = fs::remove_file(entry.path());
        }
    }
}

/// What the `gallery:` protocol serves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Size {
    Full,
    Thumbnail,
}

/// Parses a protocol path: `/full/<instance ID>/<file>` or
/// `/thumb/<instance ID>/<file>`, the file name percent-encoded.
pub fn parse_request(path: &str) -> Option<(Size, InstanceId, String)> {
    let mut parts = path.trim_start_matches('/').split('/');
    let size = match parts.next()? {
        "full" => Size::Full,
        "thumb" => Size::Thumbnail,
        _ => return None,
    };
    let id: InstanceId = parts.next()?.parse().ok()?;
    let file = percent_encoding::percent_decode_str(parts.next()?)
        .decode_utf8()
        .ok()?
        .into_owned();
    if parts.next().is_some() || !valid_file_name(&file) {
        return None;
    }
    Some((size, id, file))
}

/// The PNG bytes for a protocol request, from a known installation only.
pub fn serve(state: &AppState, size: Size, id: InstanceId, file: &str) -> LauncherResult<Vec<u8>> {
    let path = path(state, id, file)?;
    match size {
        Size::Full => {
            fs::read(&path).map_err(|e| LauncherError::from_io(&e, "Reading a screenshot"))
        }
        Size::Thumbnail => thumbnail(&data_root(state)?, id, file, &path),
    }
}

/// A cached thumbnail, made on first use.
fn thumbnail(root: &DataRoot, id: InstanceId, file: &str, path: &Path) -> LauncherResult<Vec<u8>> {
    let meta = fs::metadata(path).map_err(|_| missing())?;
    let dir = root.thumbnails_dir();
    let cached = dir.join(thumbnail_name(id, file, &meta));
    if let Ok(bytes) = fs::read(&cached) {
        return Ok(bytes);
    }
    let bytes = make_thumbnail(path, THUMB_WIDTH)?;
    // The cache is only a speed-up: failing to write it is not an error.
    if fs::create_dir_all(&dir).is_ok() {
        let partial = cached.with_extension("part");
        if fs::write(&partial, &bytes).is_ok() && fs::rename(&partial, &cached).is_err() {
            let _ = fs::remove_file(&partial);
        }
    }
    Ok(bytes)
}

fn unreadable() -> LauncherError {
    LauncherError::new(
        ErrorCode::InvalidInput,
        "This screenshot is not a PNG image the launcher can read.",
    )
}

/// Decodes a PNG and scales it down to `width` (never up) as an RGB PNG.
pub fn make_thumbnail(path: &Path, width: u32) -> LauncherResult<Vec<u8>> {
    let file = fs::File::open(path).map_err(|_| missing())?;
    let mut decoder = png::Decoder::new_with_limits(
        BufReader::new(file),
        png::Limits {
            bytes: MAX_DECODED_BYTES,
        },
    );
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().map_err(|_| unreadable())?;
    let mut pixels = vec![0; reader.output_buffer_size().ok_or_else(unreadable)?];
    let info = reader.next_frame(&mut pixels).map_err(|_| unreadable())?;
    let channels = match info.color_type {
        png::ColorType::Grayscale => 1,
        png::ColorType::GrayscaleAlpha => 2,
        png::ColorType::Rgb => 3,
        png::ColorType::Rgba => 4,
        png::ColorType::Indexed => return Err(unreadable()),
    };
    let image = Image {
        pixels: &pixels,
        width: info.width as usize,
        height: info.height as usize,
        stride: info.line_size,
        channels,
    };
    let target_width = info.width.min(width).max(1);
    let target_height = ((u64::from(info.height) * u64::from(target_width))
        / u64::from(info.width.max(1)))
    .max(1) as u32;
    let rgb = image.shrink(target_width as usize, target_height as usize);
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, target_width, target_height);
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder
        .write_header()
        .map_err(|_| LauncherError::internal("A thumbnail could not be made."))?;
    writer
        .write_image_data(&rgb)
        .and_then(|()| writer.finish())
        .map_err(|_| LauncherError::internal("A thumbnail could not be made."))?;
    Ok(out)
}

/// Decoded 8-bit pixels.
struct Image<'a> {
    pixels: &'a [u8],
    width: usize,
    height: usize,
    stride: usize,
    channels: usize,
}

impl Image<'_> {
    /// Averages each block of source pixels into one RGB pixel.
    fn shrink(&self, width: usize, height: usize) -> Vec<u8> {
        let mut out = Vec::with_capacity(width * height * 3);
        for ty in 0..height {
            let y0 = ty * self.height / height;
            let y1 = ((ty + 1) * self.height / height).max(y0 + 1);
            for tx in 0..width {
                let x0 = tx * self.width / width;
                let x1 = ((tx + 1) * self.width / width).max(x0 + 1);
                let mut sum = [0u64; 3];
                let mut count = 0u64;
                for y in y0..y1 {
                    let row = &self.pixels[y * self.stride..];
                    for x in x0..x1 {
                        let p = &row[x * self.channels..];
                        let rgb = if self.channels < 3 {
                            [p[0]; 3]
                        } else {
                            [p[0], p[1], p[2]]
                        };
                        for (total, value) in sum.iter_mut().zip(rgb) {
                            *total += u64::from(value);
                        }
                        count += 1;
                    }
                }
                out.extend(sum.map(|total| (total / count) as u8));
            }
        }
        out
    }
}

#[cfg(test)]
mod tests;
