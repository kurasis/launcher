//! The launcher account's skin and cape at skins.lapi.sh (D-065, stage 2).
//!
//! The player picks a PNG in a native dialog; the core reads it (never more
//! than its kind may weigh), checks it against the service's rules and keeps
//! it in memory until the player uploads it or picks another. The interface
//! never sends a path: it names the kind, and the core uploads what was
//! picked for it. The service stays the authority; the checks here only say
//! what is wrong before anything is sent.
//!
//! Textures are shown from `GET /v1/textures/{sha256}`, fetched here (the
//! window's CSP loads no remote image) and checked against their hash.
//! Nothing about the player goes into the log.

use std::collections::VecDeque;
use std::io::{Cursor, Read};
use std::path::Path;
use std::sync::{Arc, Mutex};

use base64::Engine as _;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Manager, Runtime};

use crate::launcher_account::{AccountProblem, BodyError, LauncherAccount, body_within};
use crate::state::AppState;
use crate::sync::lock;

const BASE: &str = "https://skins.lapi.sh";
/// A profile answer: two textures' descriptions.
const MAX_PROFILE_BYTES: usize = 64 * 1024;

/// Largest file per kind, as the service takes it.
pub const SKIN_MAX_BYTES: usize = 2 * 1024 * 1024;
pub const CAPE_MAX_BYTES: usize = 1024 * 1024;
/// Widths of a frame: standard and HD (×2, ×4, ×8).
pub const WIDTHS: [u32; 4] = [64, 128, 256, 512];
/// Frames of an animation, a vertical strip.
pub const MAX_FRAMES: u32 = 32;
/// No image larger than this is taken, whatever its shape.
pub const MAX_PIXELS: u64 = 16_777_216;
/// Duration of a frame, in milliseconds.
pub const FRAME_MS_MIN: u32 = 50;
pub const FRAME_MS_MAX: u32 = 10_000;
/// What the decoder may allocate for itself; the image buffer is bounded by
/// the size rules.
const DECODER_BYTES: usize = 4 * 1024 * 1024;
/// Textures kept for the preview (two worn, a few recent).
const TEXTURE_CACHE: usize = 6;
/// The longest file name shown for a picked file.
const NAME_CHARS: usize = 80;

const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";

/// skins.lapi.sh, or the server a test build was made for
/// (`LAUNCHER_SKINS_URL` at build time; release builds never set it).
fn base() -> &'static str {
    #[cfg(test)]
    if let Some(local) = tests::LOCAL_SERVER.with(std::cell::Cell::get) {
        return local;
    }
    option_env!("LAUNCHER_SKINS_URL")
        .filter(|u| !u.trim().is_empty())
        .unwrap_or(BASE)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Skin,
    Cape,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Skin => "skin",
            Self::Cape => "cape",
        }
    }

    pub fn max_bytes(self) -> usize {
        match self {
            Self::Skin => SKIN_MAX_BYTES,
            Self::Cape => CAPE_MAX_BYTES,
        }
    }

    fn slot(self) -> usize {
        match self {
            Self::Skin => 0,
            Self::Cape => 1,
        }
    }
}

/// What went wrong, for the interface to say in the player's language. The
/// service's own refusals keep its codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SkinsProblem {
    /// No launcher account, or its sign-in ended.
    SignedOut,
    Network,
    /// The picked file could not be read.
    Unreadable,
    /// Not a PNG, or a broken one.
    BadPng,
    /// A size the rules do not take.
    BadSize,
    /// More than 32 frames, or not the number the file has.
    BadFrames,
    /// A frame duration outside 50–10 000 ms, or more than one per frame.
    BadFrameMs,
    BadModel,
    /// Heavier than the kind may be.
    TooLarge,
    /// Ten uploads an hour.
    RateLimited,
    /// Nothing picked for that kind, or a request the core refuses to make.
    Invalid,
    Server,
}

fn problem_of(status: u16, body: &Value) -> SkinsProblem {
    match (status, body["error"].as_str().unwrap_or_default()) {
        (401, _) => SkinsProblem::SignedOut,
        (429, _) => SkinsProblem::RateLimited,
        (_, "bad_png") => SkinsProblem::BadPng,
        (_, "bad_size") => SkinsProblem::BadSize,
        (_, "bad_frames") => SkinsProblem::BadFrames,
        (_, "bad_frame_ms") => SkinsProblem::BadFrameMs,
        (_, "bad_model") => SkinsProblem::BadModel,
        (413, _) | (_, "too_large") => SkinsProblem::TooLarge,
        (400 | 404, _) => SkinsProblem::Invalid,
        _ => SkinsProblem::Server,
    }
}

/// A worn texture, as the interface shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Worn {
    /// SHA-256 of the PNG the service keeps; what `skins_texture` fetches.
    pub hash: String,
    /// Skins only: the slim (3-pixel) arms.
    pub slim: bool,
    pub width: u32,
    pub height: u32,
    pub frames: u32,
    /// One per frame; empty for a single frame.
    pub frame_ms: Vec<u32>,
}

/// What the account wears.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Wardrobe {
    pub skin: Option<Worn>,
    pub cape: Option<Worn>,
}

#[derive(Deserialize)]
struct TextureAnswer {
    hash: String,
    #[serde(default)]
    model: Option<String>,
    width: u32,
    height: u32,
    frames: u32,
    #[serde(default)]
    frame_ms: Vec<u32>,
}

#[derive(Deserialize)]
struct ProfileAnswer {
    #[serde(default)]
    skin: Option<TextureAnswer>,
    #[serde(default)]
    cape: Option<TextureAnswer>,
}

/// The service's answer, checked: a texture that breaks the rules is not
/// one it would have kept.
fn wardrobe_of(value: Value) -> Result<Wardrobe, SkinsProblem> {
    let answer: ProfileAnswer = serde_json::from_value(value).map_err(|_| SkinsProblem::Server)?;
    let worn = |kind: Kind, texture: Option<TextureAnswer>| -> Result<Option<Worn>, SkinsProblem> {
        let Some(t) = texture else {
            return Ok(None);
        };
        if !is_hash(&t.hash)
            || frames_of(kind, t.width, t.height) != Ok(t.frames)
            || t.frame_ms.len() > MAX_FRAMES as usize
        {
            return Err(SkinsProblem::Server);
        }
        Ok(Some(Worn {
            hash: t.hash,
            slim: kind == Kind::Skin && t.model.as_deref() == Some("slim"),
            width: t.width,
            height: t.height,
            frames: t.frames,
            frame_ms: t
                .frame_ms
                .into_iter()
                .map(|ms| ms.clamp(FRAME_MS_MIN, FRAME_MS_MAX))
                .collect(),
        }))
    };
    Ok(Wardrobe {
        skin: worn(Kind::Skin, answer.skin)?,
        cape: worn(Kind::Cape, answer.cape)?,
    })
}

/// A texture's name: 64 lowercase hex digits.
pub fn is_hash(text: &str) -> bool {
    text.len() == 64 && text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// The frames of an image of this size, if its shape is one the service
/// takes: a frame is `w`×`w` for a skin (or `w`×`w/2`, the layout before 1.8,
/// alone) and `w`×`w/2` for a cape, stacked into a vertical strip of 1–32.
pub fn frames_of(kind: Kind, width: u32, height: u32) -> Result<u32, SkinsProblem> {
    if u64::from(width) * u64::from(height) > MAX_PIXELS || !WIDTHS.contains(&width) {
        return Err(SkinsProblem::BadSize);
    }
    // The layout before 1.8 (64×32, or HD like 512×256) is one frame only: a strip of
    // them would read the same as a strip of square frames.
    if kind == Kind::Skin && u64::from(height) * 2 == u64::from(width) {
        return Ok(1);
    }
    let frame = match kind {
        Kind::Skin => width,
        Kind::Cape => width / 2,
    };
    if height == 0 || !height.is_multiple_of(frame) {
        return Err(SkinsProblem::BadSize);
    }
    let frames = height / frame;
    if frames > MAX_FRAMES {
        return Err(SkinsProblem::BadFrames);
    }
    Ok(frames)
}

/// The size a PNG's header gives, read from the bytes alone: the signature,
/// then IHDR as the first chunk with sound fields.
pub fn header_size(bytes: &[u8]) -> Result<(u32, u32), SkinsProblem> {
    let bad = SkinsProblem::BadPng;
    // Signature (8), IHDR length (4), type (4), data (13), CRC (4).
    if bytes.len() < 33 || !bytes.starts_with(PNG_SIGNATURE) {
        return Err(bad);
    }
    if bytes[8..12] != [0, 0, 0, 13] || &bytes[12..16] != b"IHDR" {
        return Err(bad);
    }
    let word =
        |at: usize| u32::from_be_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]);
    let (width, height) = (word(16), word(20));
    let (depth, color, compression, filter, interlace) =
        (bytes[24], bytes[25], bytes[26], bytes[27], bytes[28]);
    let sound_depth = match color {
        0 => matches!(depth, 1 | 2 | 4 | 8 | 16),
        3 => matches!(depth, 1 | 2 | 4 | 8),
        2 | 4 | 6 => matches!(depth, 8 | 16),
        _ => false,
    };
    if !sound_depth || compression != 0 || filter != 0 || interlace > 1 {
        return Err(bad);
    }
    Ok((width, height))
}

/// A picked file's shape, once it passed every check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shape {
    pub width: u32,
    pub height: u32,
    pub frames: u32,
}

/// Checks a file against the rules of its kind: its weight, its header, its
/// shape, then the whole image (so a broken or cut-off file is caught here,
/// not after the upload).
pub fn check(kind: Kind, bytes: &[u8]) -> Result<Shape, SkinsProblem> {
    if bytes.len() > kind.max_bytes() {
        return Err(SkinsProblem::TooLarge);
    }
    let (width, height) = header_size(bytes)?;
    let frames = frames_of(kind, width, height)?;
    decodes(bytes, width, height).ok_or(SkinsProblem::BadPng)?;
    Ok(Shape {
        width,
        height,
        frames,
    })
}

/// The image decodes to its end, with the decoder's limits set first.
fn decodes(bytes: &[u8], width: u32, height: u32) -> Option<()> {
    let mut decoder = png::Decoder::new_with_limits(
        Cursor::new(bytes),
        png::Limits {
            bytes: DECODER_BYTES,
        },
    );
    decoder.set_transformations(
        png::Transformations::EXPAND | png::Transformations::ALPHA | png::Transformations::STRIP_16,
    );
    decoder.set_ignore_text_chunk(true);
    decoder.set_ignore_iccp_chunk(true);
    let mut reader = decoder.read_info().ok()?;
    if reader.info().width != width || reader.info().height != height {
        return None;
    }
    let size = reader.output_buffer_size()?;
    // Four bytes a pixel at most, the pixel count bounded by the rules.
    if size > (MAX_PIXELS as usize) * 4 {
        return None;
    }
    let mut buffer = vec![0; size];
    reader.next_frame(&mut buffer).ok()?;
    reader.finish().ok()?;
    Some(())
}

/// The query of an upload: `model` (skins), `frames` and, for an
/// animation, `frame_ms` (one for every frame, or one per frame). Only
/// digits and the words `default` and `slim` ever go into it.
pub fn upload_query(
    kind: Kind,
    slim: bool,
    frames: u32,
    frame_ms: Option<&[u32]>,
) -> Result<String, SkinsProblem> {
    if !(1..=MAX_FRAMES).contains(&frames) {
        return Err(SkinsProblem::BadFrames);
    }
    let mut parts = Vec::new();
    if kind == Kind::Skin {
        parts.push(format!("model={}", if slim { "slim" } else { "default" }));
    }
    parts.push(format!("frames={frames}"));
    if frames > 1
        && let Some(list) = frame_ms
    {
        if list.is_empty()
            || (list.len() != 1 && list.len() != frames as usize)
            || list
                .iter()
                .any(|ms| !(FRAME_MS_MIN..=FRAME_MS_MAX).contains(ms))
        {
            return Err(SkinsProblem::BadFrameMs);
        }
        let list: Vec<String> = list.iter().map(u32::to_string).collect();
        parts.push(format!("frame_ms={}", list.join(",")));
    }
    Ok(parts.join("&"))
}

/// A picked file's name as shown: plain text, short.
fn shown_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy())
        .unwrap_or_default()
        .chars()
        .filter(|c| !c.is_control())
        .take(NAME_CHARS)
        .collect()
}

/// Reads a file, never more than `max` bytes of it.
pub fn read_within(path: &Path, max: usize) -> Result<Vec<u8>, SkinsProblem> {
    let file = std::fs::File::open(path).map_err(|_| SkinsProblem::Unreadable)?;
    let meta = file.metadata().map_err(|_| SkinsProblem::Unreadable)?;
    if !meta.is_file() {
        return Err(SkinsProblem::Unreadable);
    }
    if meta.len() > max as u64 {
        return Err(SkinsProblem::TooLarge);
    }
    let mut bytes = Vec::with_capacity(usize::try_from(meta.len()).unwrap_or(max).min(max));
    file.take(max as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| SkinsProblem::Unreadable)?;
    if bytes.len() > max {
        return Err(SkinsProblem::TooLarge);
    }
    Ok(bytes)
}

fn data_url(png: &[u8]) -> String {
    format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(png)
    )
}

/// A file the player picked and has not uploaded yet.
struct Picked {
    bytes: Vec<u8>,
    shape: Shape,
}

/// What the interface shows of a picked file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PickedView {
    pub kind: Kind,
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub frames: u32,
    /// The file as a `data:` URL, for the preview.
    pub preview: String,
}

pub struct Skins {
    http: reqwest::Client,
    /// The picked file per kind (skin, cape).
    picked: Mutex<[Option<Picked>; 2]>,
    /// Recently shown textures as `data:` URLs, by hash.
    textures: Mutex<VecDeque<(String, String)>>,
}

impl Skins {
    pub fn new(app_version: &str) -> Result<Self, String> {
        let http = crate::launcher_account::our_client(app_version, base())?;
        Ok(Self {
            http,
            picked: Mutex::new([None, None]),
            textures: Mutex::new(VecDeque::new()),
        })
    }

    /// Forgets the picked files (signed out).
    pub fn forget(&self) {
        *lock(&self.picked) = [None, None];
    }

    async fn token<R: Runtime>(&self, app: &AppHandle<R>) -> Result<String, SkinsProblem> {
        let account = Arc::clone(&app.state::<Arc<LauncherAccount>>());
        match account.access_token(&app.state::<AppState>()).await {
            Ok(token) => Ok(token.expose().to_owned()),
            Err(AccountProblem::Network) => Err(SkinsProblem::Network),
            // The account service failing or busy is not a sign-out: try again.
            Err(AccountProblem::Server | AccountProblem::TooMany) => Err(SkinsProblem::Server),
            Err(_) => Err(SkinsProblem::SignedOut),
        }
    }

    /// Calls the API with the account's token; the answer's JSON.
    async fn call(
        &self,
        token: &str,
        method: reqwest::Method,
        path: &str,
        png: Option<Vec<u8>>,
    ) -> Result<Value, SkinsProblem> {
        let mut request = self
            .http
            .request(method, format!("{}{path}", base()))
            .bearer_auth(token)
            .header(reqwest::header::ACCEPT, "application/json");
        if let Some(png) = png {
            request = request
                .header(reqwest::header::CONTENT_TYPE, "image/png")
                .body(png);
        }
        let response = request.send().await.map_err(|_| SkinsProblem::Network)?;
        let status = response.status().as_u16();
        let bytes = body_within(response, MAX_PROFILE_BYTES)
            .await
            .map_err(|e| match e {
                BodyError::Network => SkinsProblem::Network,
                BodyError::TooLarge => SkinsProblem::Server,
            })?;
        let value: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        if (200..300).contains(&status) {
            Ok(value)
        } else {
            Err(problem_of(status, &value))
        }
    }

    /// What the account wears now.
    pub async fn mine<R: Runtime>(&self, app: &AppHandle<R>) -> Result<Wardrobe, SkinsProblem> {
        let token = self.token(app).await?;
        self.mine_with(&token).await
    }

    async fn mine_with(&self, token: &str) -> Result<Wardrobe, SkinsProblem> {
        wardrobe_of(
            self.call(token, reqwest::Method::GET, "/v1/me", None)
                .await?,
        )
    }

    /// Reads and checks a file the player picked, and keeps it for upload.
    pub async fn pick(
        &self,
        kind: Kind,
        path: std::path::PathBuf,
    ) -> Result<PickedView, SkinsProblem> {
        let name = shown_name(&path);
        let (bytes, shape) = tauri::async_runtime::spawn_blocking(move || {
            let bytes = read_within(&path, kind.max_bytes())?;
            let shape = check(kind, &bytes)?;
            Ok::<_, SkinsProblem>((bytes, shape))
        })
        .await
        .map_err(|_| SkinsProblem::Unreadable)??;
        let view = PickedView {
            kind,
            name,
            width: shape.width,
            height: shape.height,
            frames: shape.frames,
            preview: data_url(&bytes),
        };
        lock(&self.picked)[kind.slot()] = Some(Picked { bytes, shape });
        Ok(view)
    }

    /// Uploads what was picked for `kind`; it is forgotten once the service
    /// took it.
    pub async fn upload<R: Runtime>(
        &self,
        app: &AppHandle<R>,
        kind: Kind,
        slim: bool,
        frame_ms: Option<&[u32]>,
    ) -> Result<Wardrobe, SkinsProblem> {
        let token = self.token(app).await?;
        self.upload_with(&token, kind, slim, frame_ms).await
    }

    async fn upload_with(
        &self,
        token: &str,
        kind: Kind,
        slim: bool,
        frame_ms: Option<&[u32]>,
    ) -> Result<Wardrobe, SkinsProblem> {
        let (bytes, shape) = {
            let picked = lock(&self.picked);
            let picked = picked[kind.slot()].as_ref().ok_or(SkinsProblem::Invalid)?;
            (picked.bytes.clone(), picked.shape)
        };
        let query = upload_query(kind, slim, shape.frames, frame_ms)?;
        let answer = self
            .call(
                token,
                reqwest::Method::PUT,
                &format!("/v1/me/{}?{query}", kind.as_str()),
                Some(bytes),
            )
            .await?;
        let wardrobe = wardrobe_of(answer)?;
        lock(&self.picked)[kind.slot()] = None;
        Ok(wardrobe)
    }

    /// Takes the skin or cape off.
    pub async fn remove<R: Runtime>(
        &self,
        app: &AppHandle<R>,
        kind: Kind,
    ) -> Result<Wardrobe, SkinsProblem> {
        let token = self.token(app).await?;
        wardrobe_of(
            self.call(
                &token,
                reqwest::Method::DELETE,
                &format!("/v1/me/{}", kind.as_str()),
                None,
            )
            .await?,
        )
    }

    /// A worn texture as a `data:` URL; it must be the PNG its hash names.
    pub async fn texture(&self, hash: &str) -> Result<String, SkinsProblem> {
        if !is_hash(hash) {
            return Err(SkinsProblem::Invalid);
        }
        if let Some((_, url)) = lock(&self.textures).iter().find(|(h, _)| h == hash) {
            return Ok(url.clone());
        }
        let response = self
            .http
            .get(format!("{}/v1/textures/{hash}", base()))
            .send()
            .await
            .map_err(|_| SkinsProblem::Network)?;
        if response.status().as_u16() != 200 {
            return Err(SkinsProblem::Server);
        }
        let png = body_within(response, SKIN_MAX_BYTES)
            .await
            .map_err(|e| match e {
                BodyError::Network => SkinsProblem::Network,
                BodyError::TooLarge => SkinsProblem::Server,
            })?;
        if !png.starts_with(PNG_SIGNATURE) || hex::encode(Sha256::digest(&png)) != hash {
            return Err(SkinsProblem::Server);
        }
        let url = data_url(&png);
        let mut cache = lock(&self.textures);
        if cache.len() >= TEXTURE_CACHE {
            cache.pop_front();
        }
        cache.push_back((hash.to_owned(), url.clone()));
        Ok(url)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    thread_local! {
        /// A local skins server for one test thread (others keep the real
        /// address).
        pub static LOCAL_SERVER: std::cell::Cell<Option<&'static str>> =
            const { std::cell::Cell::new(None) };
    }

    /// A PNG of this size, RGBA, every pixel the same.
    fn png_of(width: u32, height: u32) -> Vec<u8> {
        let mut out = Vec::new();
        let mut encoder = png::Encoder::new(&mut out, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        writer
            .write_image_data(&vec![120; (width * height * 4) as usize])
            .unwrap();
        writer.finish().unwrap();
        out
    }

    /// Only the header of a PNG claiming this size (no image data).
    fn header_only(width: u32, height: u32) -> Vec<u8> {
        let mut out = PNG_SIGNATURE.to_vec();
        out.extend_from_slice(&[0, 0, 0, 13]);
        out.extend_from_slice(b"IHDR");
        out.extend_from_slice(&width.to_be_bytes());
        out.extend_from_slice(&height.to_be_bytes());
        out.extend_from_slice(&[8, 6, 0, 0, 0]);
        out.extend_from_slice(&[0, 0, 0, 0]);
        out
    }

    #[test]
    fn good_skins_standard_hd_and_strips() {
        for (w, h, frames) in [
            (64, 64, 1),
            (64, 32, 1),
            (128, 64, 1),
            (256, 128, 1),
            (512, 256, 1),
            (128, 128, 1),
            (256, 256, 1),
            (512, 512, 1),
            (64, 64 * 4, 4),
            (64, 64 * 32, 32),
            (128, 128 * 8, 8),
        ] {
            assert_eq!(
                check(Kind::Skin, &png_of(w, h)),
                Ok(Shape {
                    width: w,
                    height: h,
                    frames
                }),
                "{w}×{h}"
            );
        }
    }

    #[test]
    fn good_capes_standard_hd_and_strips() {
        for (w, h, frames) in [
            (64, 32, 1),
            (128, 64, 1),
            (512, 256, 1),
            (64, 64, 2),
            (64, 32 * 32, 32),
        ] {
            assert_eq!(
                check(Kind::Cape, &png_of(w, h)).map(|s| s.frames),
                Ok(frames),
                "{w}×{h}"
            );
        }
    }

    #[test]
    fn bad_sizes_are_refused_before_decoding() {
        for (w, h) in [
            (65, 65),
            (64, 48),
            (64, 100),
            (64, 96),
            (32, 32),
            (1024, 1024),
            (128, 32),
            (512, 128),
            (0, 0),
            (64, 0),
            (30_000, 30_000),
            (u32::MAX, u32::MAX),
        ] {
            assert_eq!(
                check(Kind::Skin, &header_only(w, h)),
                Err(SkinsProblem::BadSize),
                "{w}×{h}"
            );
        }
        assert_eq!(
            check(Kind::Skin, &header_only(64, 64 * 33)),
            Err(SkinsProblem::BadFrames)
        );
        assert_eq!(
            check(Kind::Cape, &header_only(64, 32 * 33)),
            Err(SkinsProblem::BadFrames)
        );
        assert_eq!(
            check(Kind::Cape, &header_only(64, 48)),
            Err(SkinsProblem::BadSize)
        );
        // A skin's shape is not a cape's, past the one both share.
        assert_eq!(frames_of(Kind::Cape, 64, 96), Ok(3));
        assert_eq!(frames_of(Kind::Skin, 64, 96), Err(SkinsProblem::BadSize));
        assert_eq!(frames_of(Kind::Skin, 512, 512 * 32), Ok(32));
        // The old layout alone: two of them stacked are one square frame.
        assert_eq!(frames_of(Kind::Skin, 512, 256), Ok(1));
        assert_eq!(
            frames_of(Kind::Skin, 512, 256 * 3),
            Err(SkinsProblem::BadSize)
        );
    }

    #[test]
    fn junk_is_not_a_png() {
        for junk in [
            Vec::new(),
            b"hello".to_vec(),
            b"GIF89a\x01\x00\x01\x00".to_vec(),
            b"<svg onload=alert(1)>".to_vec(),
            PNG_SIGNATURE.to_vec(),
            vec![0u8; 4096],
            vec![0xff; 100],
        ] {
            assert_eq!(check(Kind::Skin, &junk), Err(SkinsProblem::BadPng));
        }
        // A sound signature, then something other than IHDR.
        let mut other = header_only(64, 64);
        other[12..16].copy_from_slice(b"tEXt");
        assert_eq!(check(Kind::Skin, &other), Err(SkinsProblem::BadPng));
        // An IHDR with fields no PNG has.
        for (at, value) in [(24, 7), (25, 5), (26, 1), (27, 1), (28, 2)] {
            let mut bad = header_only(64, 64);
            bad[at] = value;
            assert_eq!(check(Kind::Skin, &bad), Err(SkinsProblem::BadPng), "{at}");
        }
    }

    #[test]
    fn a_cut_off_or_damaged_png_is_caught_here() {
        let good = png_of(64, 64);
        for len in [33, 40, good.len() / 2, good.len() - 12] {
            assert_eq!(
                check(Kind::Skin, &good[..len]),
                Err(SkinsProblem::BadPng),
                "{len}"
            );
        }
        // The header alone: a size we take, no image.
        assert_eq!(
            check(Kind::Skin, &header_only(64, 64)),
            Err(SkinsProblem::BadPng)
        );
        // A header that lies about the size the image data has.
        let mut lying = good.clone();
        lying[16..20].copy_from_slice(&128u32.to_be_bytes());
        lying[20..24].copy_from_slice(&128u32.to_be_bytes());
        assert_eq!(check(Kind::Skin, &lying), Err(SkinsProblem::BadPng));
    }

    #[test]
    fn heavy_files_are_refused() {
        let mut heavy = png_of(64, 64);
        heavy.resize(SKIN_MAX_BYTES + 1, 0);
        assert_eq!(check(Kind::Skin, &heavy), Err(SkinsProblem::TooLarge));
        heavy.truncate(CAPE_MAX_BYTES + 1);
        assert_eq!(check(Kind::Cape, &heavy), Err(SkinsProblem::TooLarge));

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("big.png");
        std::fs::write(&path, vec![0u8; SKIN_MAX_BYTES + 1]).unwrap();
        assert_eq!(
            read_within(&path, SKIN_MAX_BYTES),
            Err(SkinsProblem::TooLarge)
        );
        std::fs::write(&path, png_of(64, 64)).unwrap();
        assert_eq!(read_within(&path, SKIN_MAX_BYTES), Ok(png_of(64, 64)));
        assert_eq!(
            read_within(dir.path(), SKIN_MAX_BYTES),
            Err(SkinsProblem::Unreadable)
        );
        assert_eq!(
            read_within(&dir.path().join("missing.png"), SKIN_MAX_BYTES),
            Err(SkinsProblem::Unreadable)
        );
    }

    #[test]
    fn queries_carry_only_numbers_and_the_model() {
        assert_eq!(
            upload_query(Kind::Skin, false, 1, None),
            Ok("model=default&frames=1".into())
        );
        assert_eq!(
            upload_query(Kind::Skin, true, 1, Some(&[300])),
            Ok("model=slim&frames=1".into()),
            "one frame has no duration"
        );
        assert_eq!(
            upload_query(Kind::Skin, true, 4, Some(&[120])),
            Ok("model=slim&frames=4&frame_ms=120".into())
        );
        assert_eq!(
            upload_query(Kind::Cape, true, 3, Some(&[50, 10_000, 200])),
            Ok("frames=3&frame_ms=50,10000,200".into()),
            "a cape has no model"
        );
        assert_eq!(
            upload_query(Kind::Cape, false, 2, None),
            Ok("frames=2".into())
        );
        for (frames, list) in [
            (4, vec![49]),
            (4, vec![10_001]),
            (4, vec![0]),
            (4, vec![u32::MAX]),
            (4, vec![]),
            (4, vec![100, 100]),
            (2, vec![100; 33]),
        ] {
            assert_eq!(
                upload_query(Kind::Skin, false, frames, Some(&list)),
                Err(SkinsProblem::BadFrameMs),
                "{list:?}"
            );
        }
        for frames in [0, 33, u32::MAX] {
            assert_eq!(
                upload_query(Kind::Skin, false, frames, None),
                Err(SkinsProblem::BadFrames)
            );
        }
    }

    #[test]
    fn refusals_become_problems() {
        let body = |e: &str| json!({ "error": e });
        assert_eq!(
            problem_of(401, &body("unauthorized")),
            SkinsProblem::SignedOut
        );
        assert_eq!(
            problem_of(429, &body("rate_limited")),
            SkinsProblem::RateLimited
        );
        assert_eq!(problem_of(400, &body("bad_png")), SkinsProblem::BadPng);
        assert_eq!(problem_of(400, &body("bad_size")), SkinsProblem::BadSize);
        assert_eq!(
            problem_of(400, &body("bad_frames")),
            SkinsProblem::BadFrames
        );
        assert_eq!(
            problem_of(400, &body("bad_frame_ms")),
            SkinsProblem::BadFrameMs
        );
        assert_eq!(problem_of(400, &body("bad_model")), SkinsProblem::BadModel);
        assert_eq!(problem_of(413, &body("too_large")), SkinsProblem::TooLarge);
        // nginx refusing the body says it in HTML.
        assert_eq!(problem_of(413, &Value::Null), SkinsProblem::TooLarge);
        assert_eq!(problem_of(400, &body("bad_query")), SkinsProblem::Invalid);
        assert_eq!(problem_of(503, &body("unavailable")), SkinsProblem::Server);
        assert_eq!(problem_of(500, &Value::Null), SkinsProblem::Server);
        assert_eq!(problem_of(502, &body("<b>x</b>")), SkinsProblem::Server);
    }

    #[test]
    fn answers_are_checked_before_they_are_shown() {
        let hash = "a".repeat(64);
        let wardrobe = wardrobe_of(json!({
            "nick": "Steve",
            "skin": { "hash": hash, "model": "slim", "width": 64, "height": 256,
                      "frames": 4, "frame_ms": [100, 200, 300, 400], "url": "https://x" },
            "cape": null,
        }))
        .unwrap();
        let skin = wardrobe.skin.unwrap();
        assert!(skin.slim);
        assert_eq!(skin.frames, 4);
        assert_eq!(skin.frame_ms, [100, 200, 300, 400]);
        assert!(wardrobe.cape.is_none());
        // A cape is never slim, whatever the answer says.
        let cape = wardrobe_of(json!({
            "skin": null,
            "cape": { "hash": hash, "model": "slim", "width": 64, "height": 32, "frames": 1 },
        }))
        .unwrap()
        .cape
        .unwrap();
        assert!(!cape.slim);
        assert_eq!(
            wardrobe_of(json!({ "nick": "x", "skin": null, "cape": null })),
            Ok(Wardrobe::default())
        );
        for bad in [
            json!({ "skin": { "hash": "../../etc/passwd", "width": 64, "height": 64, "frames": 1 } }),
            json!({ "skin": { "hash": hash.to_uppercase(), "width": 64, "height": 64, "frames": 1 } }),
            json!({ "skin": { "hash": hash, "width": 64, "height": 64, "frames": 2 } }),
            json!({ "skin": { "hash": hash, "width": 4096, "height": 4096, "frames": 1 } }),
            json!({ "skin": { "hash": hash, "width": 64, "height": 128, "frames": 2,
                              "frame_ms": vec![100; 40] } }),
            json!({ "skin": "x" }),
            json!("<html>"),
            Value::Null,
        ] {
            assert_eq!(wardrobe_of(bad.clone()), Err(SkinsProblem::Server), "{bad}");
        }
    }

    #[test]
    fn names_are_plain_and_short() {
        assert_eq!(shown_name(Path::new("/x/my skin.png")), "my skin.png");
        assert_eq!(shown_name(Path::new("/x/a\u{1b}[2Jb\n.png")), "a[2Jb.png");
        assert_eq!(
            shown_name(Path::new(&format!("/x/{}.png", "n".repeat(500))))
                .chars()
                .count(),
            NAME_CHARS
        );
    }

    /// Answers each request with `status` and `body`; keeps each request's
    /// head and body.
    async fn local_server(
        status: u16,
        body: Value,
    ) -> (String, Arc<Mutex<Vec<(String, Vec<u8>)>>>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let kept = Arc::clone(&requests);
        let answer = body.to_string();
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                let kept = Arc::clone(&kept);
                let answer = answer.clone();
                tokio::spawn(async move {
                    let mut request = Vec::new();
                    let mut buffer = [0u8; 16 * 1024];
                    loop {
                        let n = socket.read(&mut buffer).await.unwrap_or(0);
                        if n == 0 {
                            return;
                        }
                        request.extend_from_slice(&buffer[..n]);
                        let Some(end) = request.windows(4).position(|w| w == b"\r\n\r\n") else {
                            continue;
                        };
                        let head = String::from_utf8_lossy(&request[..end]).into_owned();
                        let length = head
                            .lines()
                            .find_map(|l| {
                                l.to_ascii_lowercase()
                                    .strip_prefix("content-length:")
                                    .map(|v| v.trim().parse::<usize>().unwrap_or(0))
                            })
                            .unwrap_or(0);
                        if request.len() - end - 4 >= length {
                            lock(&kept).push((head, request[end + 4..].to_vec()));
                            break;
                        }
                    }
                    let reply = format!(
                        "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{answer}",
                        answer.len()
                    );
                    let _ = socket.write_all(reply.as_bytes()).await;
                });
            }
        });
        (address, requests)
    }

    #[tokio::test]
    async fn an_upload_sends_the_picked_png_with_its_query() {
        let hash = "b".repeat(64);
        let (address, requests) = local_server(
            200,
            json!({ "nick": "Steve", "cape": null, "skin": {
                "hash": hash, "model": "default", "width": 64, "height": 128,
                "frames": 2, "frame_ms": [250, 250] } }),
        )
        .await;
        LOCAL_SERVER.with(|s| s.set(Some(Box::leak(address.into_boxed_str()))));
        let skins = Skins::new("test").unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("strip.png");
        let png = png_of(64, 128);
        std::fs::write(&path, &png).unwrap();

        // Nothing picked yet: nothing is sent.
        assert_eq!(
            skins.upload_with("token", Kind::Skin, false, None).await,
            Err(SkinsProblem::Invalid)
        );
        let picked = skins.pick(Kind::Skin, path).await.unwrap();
        assert_eq!((picked.frames, picked.name.as_str()), (2, "strip.png"));
        assert!(picked.preview.starts_with("data:image/png;base64,"));
        // A cape was not picked: the skin's file is not sent as one.
        assert_eq!(
            skins.upload_with("token", Kind::Cape, false, None).await,
            Err(SkinsProblem::Invalid)
        );
        let wardrobe = skins
            .upload_with("token", Kind::Skin, false, Some(&[250]))
            .await
            .unwrap();
        LOCAL_SERVER.with(|s| s.set(None));
        assert_eq!(wardrobe.skin.unwrap().hash, hash);
        let requests = lock(&requests).clone();
        assert_eq!(requests.len(), 1);
        let (head, body) = &requests[0];
        assert!(
            head.starts_with("PUT /v1/me/skin?model=default&frames=2&frame_ms=250 HTTP/1.1"),
            "{head}"
        );
        let head = head.to_ascii_lowercase();
        assert!(head.contains("content-type: image/png"), "{head}");
        assert!(head.contains("authorization: bearer token"), "{head}");
        assert_eq!(body, &png);
        // Uploaded: forgotten.
        assert_eq!(
            skins.upload_with("token", Kind::Skin, false, None).await,
            Err(SkinsProblem::Invalid)
        );
    }

    #[tokio::test]
    async fn a_refused_upload_keeps_the_pick_and_says_why() {
        let (address, _) = local_server(429, json!({ "error": "rate_limited" })).await;
        LOCAL_SERVER.with(|s| s.set(Some(Box::leak(address.into_boxed_str()))));
        let skins = Skins::new("test").unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cape.png");
        std::fs::write(&path, png_of(64, 32)).unwrap();
        skins.pick(Kind::Cape, path).await.unwrap();
        let first = skins.upload_with("token", Kind::Cape, false, None).await;
        let again = skins.upload_with("token", Kind::Cape, false, None).await;
        LOCAL_SERVER.with(|s| s.set(None));
        assert_eq!(first, Err(SkinsProblem::RateLimited));
        assert_eq!(again, Err(SkinsProblem::RateLimited), "still picked");
    }

    #[tokio::test]
    async fn a_bad_pick_replaces_nothing_and_textures_need_a_hash() {
        let skins = Skins::new("test").unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x.png");
        std::fs::write(&path, b"<script>alert(1)</script>").unwrap();
        assert_eq!(
            skins.pick(Kind::Skin, path).await.map(|v| v.frames),
            Err(SkinsProblem::BadPng)
        );
        assert!(lock(&skins.picked)[0].is_none());
        for hash in ["", "../../v1/me", &"A".repeat(64), &"a".repeat(65)] {
            assert_eq!(skins.texture(hash).await, Err(SkinsProblem::Invalid));
        }
    }
}
