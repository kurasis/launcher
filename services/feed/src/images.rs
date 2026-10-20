//! Pictures for the feed, fetched by the service and kept on its disk: each
//! one is decoded, scaled down and saved again as a JPEG, so a broken or
//! hostile file never reaches a player, and players never ask YouTube,
//! Twitch or Mojang for anything.
//!
//! Three kinds, each in its own folder of `data/`:
//! - `img/<sha256>.jpg`: article pictures, named by their content, kept
//!   while the feed refers to them;
//! - `yt/<video id>.jpg`: a YouTube video's preview, fetched the first time a
//!   launcher asks for it;
//! - `tw/<login>.jpg`: a live stream's preview, fetched again after
//!   `STREAM_TTL`.

use std::collections::HashSet;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use image::ImageDecoder;
use sha2::{Digest, Sha256};

use crate::net;

/// The largest file fetched.
const MAX_FILE: usize = 8 * 1024 * 1024;
/// The largest picture decoded, in pixels per side.
const MAX_SIDE: u32 = 8192;
/// Memory one decode may take.
const MAX_ALLOC: u64 = 160 * 1024 * 1024;
const QUALITY: u8 = 82;
/// A stream preview is fetched again after this.
pub const STREAM_TTL: Duration = Duration::from_secs(10 * 60);
/// Unused article pictures, video and stream previews older than these go.
const KEEP_ARTICLE: Duration = Duration::from_secs(3 * 24 * 3600);
const KEEP_VIDEO: Duration = Duration::from_secs(30 * 24 * 3600);
const KEEP_STREAM: Duration = Duration::from_secs(24 * 3600);

/// Picture widths: article pictures and previews.
pub const WIDE: u32 = 1280;
pub const PREVIEW: u32 = 480;

pub fn sha_name(name: &str) -> bool {
    name.len() == 68
        && name.ends_with(".jpg")
        && name[..64]
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

pub fn video_id(id: &str) -> bool {
    id.len() == 11
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

pub fn twitch_login(login: &str) -> bool {
    (3..=25).contains(&login.len())
        && login
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

/// Decodes a picture of any of our formats, scales it to at most `width`
/// pixels wide and gives it back as a JPEG.
pub fn reencode(bytes: &[u8], width: u32) -> Result<Vec<u8>, String> {
    let reader = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| e.to_string())?;
    let format = reader.format().ok_or("not a picture")?;
    if !matches!(
        format,
        image::ImageFormat::Jpeg
            | image::ImageFormat::Png
            | image::ImageFormat::WebP
            | image::ImageFormat::Gif
    ) {
        return Err("not a picture we take".to_owned());
    }
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_SIDE);
    limits.max_image_height = Some(MAX_SIDE);
    limits.max_alloc = Some(MAX_ALLOC);
    let mut decoder = reader.into_decoder().map_err(|e| e.to_string())?;
    decoder.set_limits(limits).map_err(|e| e.to_string())?;
    let (w, h) = decoder.dimensions();
    if w == 0 || h == 0 || w > MAX_SIDE || h > MAX_SIDE {
        return Err("picture too large".to_owned());
    }
    let picture = image::DynamicImage::from_decoder(decoder).map_err(|e| e.to_string())?;
    let picture = if picture.width() > width {
        let height = (u64::from(picture.height()) * u64::from(width) / u64::from(picture.width()))
            .max(1) as u32;
        picture.resize_exact(width, height, image::imageops::FilterType::Triangle)
    } else {
        picture
    };
    let rgb = picture.to_rgb8();
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, QUALITY)
        .encode_image(&rgb)
        .map_err(|e| e.to_string())?;
    Ok(out)
}

pub struct Images {
    dir: PathBuf,
    http: reqwest::Client,
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())
}

fn age(path: &Path) -> Option<Duration> {
    let modified = std::fs::metadata(path).ok()?.modified().ok()?;
    SystemTime::now().duration_since(modified).ok()
}

impl Images {
    pub fn new(dir: &Path, http: reqwest::Client) -> Result<Self, String> {
        for sub in ["img", "yt", "tw"] {
            std::fs::create_dir_all(dir.join(sub)).map_err(|e| e.to_string())?;
        }
        Ok(Self {
            dir: dir.to_owned(),
            http,
        })
    }

    async fn fetch(&self, url: &str, width: u32) -> Result<Vec<u8>, String> {
        if !url.starts_with("https://") {
            return Err("not https".to_owned());
        }
        let bytes = net::get(&self.http, url, MAX_FILE).await?;
        tokio::task::spawn_blocking(move || reencode(&bytes, width))
            .await
            .map_err(|e| e.to_string())?
    }

    /// An article picture: fetched, kept by content, named `img/<sha>.jpg`.
    pub async fn article(&self, url: &str, width: u32) -> Result<String, String> {
        let jpeg = self.fetch(url, width).await?;
        let name = format!("{}.jpg", hex::encode(Sha256::digest(&jpeg)));
        let path = self.dir.join("img").join(&name);
        if !path.exists() {
            write_atomic(&path, &jpeg)?;
        }
        Ok(format!("img/{name}"))
    }

    /// The stored file of a public picture path (`img/…`, `yt/…`, `tw/…`),
    /// fetching a preview first when it is not there or too old.
    pub async fn file(
        &self,
        kind: &str,
        name: &str,
        stream_url: Option<String>,
    ) -> Option<PathBuf> {
        match kind {
            "img" if sha_name(name) => {
                let path = self.dir.join("img").join(name);
                path.exists().then_some(path)
            }
            "yt" => {
                let id = name.strip_suffix(".jpg").filter(|id| video_id(id))?;
                let path = self.dir.join("yt").join(name);
                if path.exists() {
                    return Some(path);
                }
                // The 16:9 preview, without the black bars of `hqdefault`.
                let url = format!("https://i.ytimg.com/vi/{id}/mqdefault.jpg");
                let jpeg = self.fetch(&url, PREVIEW).await.ok()?;
                write_atomic(&path, &jpeg).ok()?;
                Some(path)
            }
            "tw" => {
                let login = name.strip_suffix(".jpg").filter(|l| twitch_login(l))?;
                let path = self.dir.join("tw").join(format!("{login}.jpg"));
                if age(&path).is_some_and(|a| a < STREAM_TTL) {
                    return Some(path);
                }
                // Only streams the feed or a player's own list lately had
                // have a preview address.
                let url = stream_url?;
                match self.fetch(&url, PREVIEW).await {
                    Ok(jpeg) => {
                        write_atomic(&path, &jpeg).ok()?;
                        Some(path)
                    }
                    Err(_) => path.exists().then_some(path),
                }
            }
            _ => None,
        }
    }

    /// Removes pictures no one needs: article pictures not in `used` and
    /// older than a few days, old video and stream previews.
    pub fn clean(&self, used: &HashSet<String>) -> usize {
        let mut removed = 0;
        for (sub, keep) in [
            ("img", KEEP_ARTICLE),
            ("yt", KEEP_VIDEO),
            ("tw", KEEP_STREAM),
        ] {
            let Ok(entries) = std::fs::read_dir(self.dir.join(sub)) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                let name = format!("{sub}/{}", entry.file_name().to_string_lossy());
                if used.contains(&name) {
                    continue;
                }
                if age(&path).is_some_and(|a| a > keep) && std::fs::remove_file(&path).is_ok() {
                    removed += 1;
                }
            }
        }
        removed
    }
}
