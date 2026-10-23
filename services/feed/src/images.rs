//! Pictures for the feed, fetched by the service and kept on its disk: each
//! one is decoded, scaled down and saved again as a JPEG, so a broken or
//! hostile file never reaches a player, and players never ask YouTube,
//! Twitch or Mojang for anything.
//!
//! Six kinds, each in its own folder of `data/`:
//! - `img/<sha256>.jpg`: article pictures, named by their content, kept
//!   while the feed refers to them;
//! - `yt/<video id>.jpg`: a YouTube video's preview (16:9), fetched the first
//!   time a launcher asks for it, from the largest picture YouTube has;
//! - `ys/<video id>.jpg`: the same for a Short, upright (9:16);
//! - `tw/<login>.jpg`: a live stream's preview, fetched again after
//!   `STREAM_TTL`;
//! - `ya/<channel id>.jpg`, `ta/<login>.jpg`: a YouTube or Twitch channel's
//!   picture (square), fetched again after `AVATAR_TTL`.

use std::collections::HashSet;
use std::future::Future;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use image::ImageDecoder;
use sha2::{Digest, Sha256};

use crate::net;
use crate::youtube::channel_id;

/// The largest file fetched.
const MAX_FILE: usize = 8 * 1024 * 1024;
/// The largest picture decoded, in pixels per side.
const MAX_SIDE: u32 = 8192;
/// Memory one decode may take.
const MAX_ALLOC: u64 = 160 * 1024 * 1024;
const QUALITY: u8 = 85;
/// A stream preview is fetched again after this.
pub const STREAM_TTL: Duration = Duration::from_secs(10 * 60);
/// A channel's picture is fetched again after this.
pub const AVATAR_TTL: Duration = Duration::from_secs(7 * 24 * 3600);
/// Unused article pictures, previews and channel pictures older than these go.
const KEEP_ARTICLE: Duration = Duration::from_secs(3 * 24 * 3600);
const KEEP_VIDEO: Duration = Duration::from_secs(30 * 24 * 3600);
const KEEP_STREAM: Duration = Duration::from_secs(24 * 3600);
const KEEP_AVATAR: Duration = Duration::from_secs(30 * 24 * 3600);

/// Picture widths: article pictures, previews (960×540), upright Short
/// previews (540×960) and channel pictures.
pub const WIDE: u32 = 1280;
pub const PREVIEW: u32 = 960;
pub const TALL: u32 = 540;
pub const AVATAR: u32 = 96;

/// Where a video's 16:9 preview is looked for, best first: 1280×720, then
/// the 4:3 ones (640×480, 480×360) without their black bars, then 320×180.
const PREVIEW_SOURCES: [&str; 4] = [
    "hq720.jpg",
    "sddefault.jpg",
    "hqdefault.jpg",
    "mqdefault.jpg",
];

/// Where a Short's upright preview is looked for, best first: YouTube's own
/// upright picture (1080×1920), then the middle of its 16:9 previews, which
/// show the Short between blurred sides.
const TALL_SOURCES: [&str; 3] = ["oar2.jpg", "hq720.jpg", "hqdefault.jpg"];

/// A stored video preview narrower than this (made from YouTube's smallest
/// pictures, as all were before 2026-10-23) is made again when it is older
/// than `REDO_AFTER`, until a larger one is found.
const REDO_NARROWER: u32 = 400;
const REDO_AFTER: Duration = Duration::from_secs(24 * 3600);

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

/// Decodes a picture of any of our formats, cuts the middle of it to the
/// `ratio` of width to height when one is given, scales it to at most
/// `width` pixels wide and gives it back as a JPEG.
pub fn reencode(bytes: &[u8], width: u32, ratio: Option<(u32, u32)>) -> Result<Vec<u8>, String> {
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
    let mut picture = image::DynamicImage::from_decoder(decoder).map_err(|e| e.to_string())?;
    if let Some(ratio) = ratio {
        let (x, y, w, h) = middle(picture.width(), picture.height(), ratio);
        picture = picture.crop_imm(x, y, w, h);
    }
    let picture = if picture.width() > width {
        let height = (u64::from(picture.height()) * u64::from(width) / u64::from(picture.width()))
            .max(1) as u32;
        picture.resize_exact(width, height, image::imageops::FilterType::Lanczos3)
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

/// The middle part (x, y, width, height) of a `width`×`height` picture with
/// the `ratio` of width to height, as large as fits.
pub fn middle(width: u32, height: u32, (rw, rh): (u32, u32)) -> (u32, u32, u32, u32) {
    let (w, h) = (u64::from(width), u64::from(height));
    let (rw, rh) = (u64::from(rw.max(1)), u64::from(rh.max(1)));
    // Too wide: full height; otherwise full width.
    let (cw, ch) = if w * rh > h * rw {
        ((h * rw / rh).max(1), h)
    } else {
        (w, (w * rh / rw).max(1))
    };
    (
        ((w - cw) / 2) as u32,
        ((h - ch) / 2) as u32,
        cw as u32,
        ch as u32,
    )
}

/// The width and height of a stored picture, read from its header.
pub fn size_of(path: &Path) -> Option<(u32, u32)> {
    image::ImageReader::open(path)
        .ok()?
        .with_guessed_format()
        .ok()?
        .into_dimensions()
        .ok()
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

/// Whether a stored video preview is a narrow one due to be made again.
pub fn redo(path: &Path) -> bool {
    age(path).is_some_and(|a| a > REDO_AFTER)
        && size_of(path).is_some_and(|(width, _)| width < REDO_NARROWER)
}

/// Marks a file as just made, so a preview that could not be made again is
/// tried only after `REDO_AFTER` once more.
fn touch(path: &Path) {
    if let Ok(file) = std::fs::File::options().write(true).open(path) {
        file.set_modified(SystemTime::now()).ok();
    }
}

impl Images {
    pub fn new(dir: &Path, http: reqwest::Client) -> Result<Self, String> {
        for sub in ["img", "yt", "ys", "tw", "ya", "ta"] {
            std::fs::create_dir_all(dir.join(sub)).map_err(|e| e.to_string())?;
        }
        Ok(Self {
            dir: dir.to_owned(),
            http,
        })
    }

    async fn fetch(
        &self,
        url: &str,
        width: u32,
        ratio: Option<(u32, u32)>,
    ) -> Result<Vec<u8>, String> {
        if !url.starts_with("https://") {
            return Err("not https".to_owned());
        }
        let bytes = net::get(&self.http, url, MAX_FILE).await?;
        tokio::task::spawn_blocking(move || reencode(&bytes, width, ratio))
            .await
            .map_err(|e| e.to_string())?
    }

    /// The width and height of an article picture (`img/<sha>.jpg`).
    pub fn size(&self, path: &str) -> Option<(u32, u32)> {
        let name = path.strip_prefix("img/").filter(|n| sha_name(n))?;
        size_of(&self.dir.join("img").join(name))
    }

    /// An article picture: fetched, kept by content, named `img/<sha>.jpg`.
    pub async fn article(&self, url: &str, width: u32) -> Result<String, String> {
        let jpeg = self.fetch(url, width, None).await?;
        let name = format!("{}.jpg", hex::encode(Sha256::digest(&jpeg)));
        let path = self.dir.join("img").join(&name);
        if !path.exists() {
            write_atomic(&path, &jpeg)?;
        }
        Ok(format!("img/{name}"))
    }

    /// The stored file of a public picture path (`img/…`, `yt/…`, `ys/…`,
    /// `tw/…`), fetching a preview first when it is not there or too old.
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
            "yt" | "ys" => {
                let id = name.strip_suffix(".jpg").filter(|id| video_id(id))?;
                let path = self.dir.join(kind).join(name);
                let stored = path.exists();
                if stored && !redo(&path) {
                    return Some(path);
                }
                let (sources, width, ratio) = if kind == "yt" {
                    (&PREVIEW_SOURCES[..], PREVIEW, (16, 9))
                } else {
                    (&TALL_SOURCES[..], TALL, (9, 16))
                };
                for source in sources {
                    let url = format!("https://i.ytimg.com/vi/{id}/{source}");
                    if let Ok(jpeg) = self.fetch(&url, width, Some(ratio)).await {
                        write_atomic(&path, &jpeg).ok()?;
                        return Some(path);
                    }
                }
                if stored {
                    touch(&path);
                }
                stored.then_some(path)
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
                match self.fetch(&url, PREVIEW, None).await {
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

    /// A channel's picture, `ya/<channel id>.jpg` or `ta/<login>.jpg`: the
    /// stored one while fresh, else fetched from where `source` says (asked
    /// only then); the old one when that fails.
    pub async fn avatar(
        &self,
        kind: &str,
        name: &str,
        source: impl Future<Output = Option<String>>,
    ) -> Option<PathBuf> {
        let stem = name.strip_suffix(".jpg")?;
        let named = match kind {
            "ya" => channel_id(stem),
            "ta" => twitch_login(stem),
            _ => false,
        };
        if !named {
            return None;
        }
        let path = self.dir.join(kind).join(name);
        if age(&path).is_some_and(|a| a < AVATAR_TTL) {
            return Some(path);
        }
        let fetched = match source.await {
            Some(url) => self.fetch(&url, AVATAR, Some((1, 1))).await.ok(),
            None => None,
        };
        match fetched {
            Some(jpeg) => {
                write_atomic(&path, &jpeg).ok()?;
                Some(path)
            }
            None => path.exists().then_some(path),
        }
    }

    /// Removes pictures no one needs: article pictures not in `used` and
    /// older than a few days, old previews and channel pictures.
    pub fn clean(&self, used: &HashSet<String>) -> usize {
        let mut removed = 0;
        for (sub, keep) in [
            ("img", KEEP_ARTICLE),
            ("yt", KEEP_VIDEO),
            ("ys", KEEP_VIDEO),
            ("tw", KEEP_STREAM),
            ("ya", KEEP_AVATAR),
            ("ta", KEEP_AVATAR),
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
