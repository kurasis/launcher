//! The launcher account's skin and cape at skins.lapi.sh (D-065, stage 2).
//!
//! The player picks a PNG, GIF or animated PNG in a native dialog, or drops
//! one on the page (which sends its name and bytes); the core reads it
//! (never more than an import may weigh), makes an animation one PNG strip
//! (`skins_import`), checks it against the service's rules and keeps it in
//! memory until the player uploads it or picks another. The interface never
//! sends a path: it names the kind, and the core uploads what was picked for
//! it. The service stays the authority; the checks here only say what is
//! wrong before anything is sent.
//!
//! Textures are shown from `GET /v1/textures/{sha256}`, fetched here (the
//! window's CSP loads no remote image) and checked against their hash.
//! Other players' heads and skins are looked up by nick (`POST /v1/profiles`,
//! `GET /v1/profile/{nick}`). Nothing about a player, a nick included, goes
//! into the log.

pub mod heads;

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::io::{Cursor, Read};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures_util::StreamExt as _;

use base64::Engine as _;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Manager, Runtime};

use crate::launcher_account::{AccountProblem, BodyError, LauncherAccount, body_within};
use crate::skins_import::{self, IMPORT_MAX_BYTES, Imported, Note, Source};
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
pub(crate) const DECODER_BYTES: usize = 4 * 1024 * 1024;
/// Textures kept for the preview, as `data:` URLs: enough for a wardrobe's
/// grid, never more than this many characters in all.
const TEXTURES_KEPT: usize = 64;
const TEXTURES_KEPT_CHARS: usize = 48 * 1024 * 1024;
/// Most nicks in one `skins_heads`, and in one `POST /v1/profiles`.
pub const MAX_HEAD_NICKS: usize = 200;
const PROFILES_AT_ONCE: usize = 50;
/// A batch of profiles: 50 of them with two textures each.
const MAX_PROFILES_BYTES: usize = 256 * 1024;
/// How long a nick's lookup holds: the service's own cache time.
const LOOKUP_HOLDS: Duration = Duration::from_secs(60);
const LOOKUPS_KEPT: usize = 2048;
/// Heads kept by their skin's hash (a few kilobytes each).
const HEADS_KEPT: usize = 512;
/// Skins fetched at once for heads.
const HEADS_AT_ONCE: usize = 4;
/// The longest file name shown for a picked file.
const NAME_CHARS: usize = 80;
/// The longest name a dropped file may have.
pub const DROPPED_NAME_BYTES: usize = 1024;
/// What a dropped file's name ends with (its content says what it is).
const DROPPED_ENDINGS: [&str; 3] = [".png", ".gif", ".apng"];
/// A picture of the character to save: at most this wide and high, and
/// this heavy.
pub const PICTURE_MAX_SIDE: u32 = 2048;
pub const PICTURE_MAX_BYTES: usize = 16 * 1024 * 1024;
/// The longest name offered for a saved picture, before `.png`.
const PICTURE_NAME_CHARS: usize = 32;

pub(crate) const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";

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
    /// Heavier than the kind may be, or than a file to import.
    TooLarge,
    /// An animation of more than 256 frames, or too many for its size.
    TooManyFrames,
    /// Ten uploads an hour.
    RateLimited,
    /// Nothing picked for that kind, or a request the core refuses to make.
    Invalid,
    /// A picture could not be saved.
    NotSaved,
    /// Nobody with that nick wears a skin (or a cape) of ours.
    NotFound,
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

/// A launcher account's nick, as the service takes it: 3–16 Latin letters,
/// digits and `_`. Nothing else is ever sent as one.
pub fn is_nick(text: &str) -> bool {
    (3..=16).contains(&text.len()) && text.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
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

/// A worn texture as the service may serve it: the PNG its hash names, in a
/// size a skin or a cape can have. The rail draws the player's head from it at
/// every start, so a larger image is never handed to the page to decode.
fn check_worn(png: &[u8], hash: &str) -> Result<(), SkinsProblem> {
    if !png.starts_with(PNG_SIGNATURE) || hex::encode(Sha256::digest(png)) != hash {
        return Err(SkinsProblem::Server);
    }
    let (width, height) = header_size(png).map_err(|_| SkinsProblem::Server)?;
    if frames_of(Kind::Skin, width, height).is_err()
        && frames_of(Kind::Cape, width, height).is_err()
    {
        return Err(SkinsProblem::Server);
    }
    Ok(())
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
    plain_name(
        &path
            .file_name()
            .map(|n| n.to_string_lossy())
            .unwrap_or_default(),
    )
}

/// A dropped file's name as shown, without any folders before it.
fn dropped_name(name: &str) -> String {
    plain_name(name.rsplit(['/', '\\']).next().unwrap_or_default())
}

/// A name without control characters or bidi formatting (which could show
/// `gnp.exe` as `exe.png`), short.
fn plain_name(name: &str) -> String {
    name.chars()
        .filter(|c| !c.is_control() && !is_bidi_format(*c))
        .take(NAME_CHARS)
        .collect()
}

fn is_bidi_format(c: char) -> bool {
    matches!(
        c,
        '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}'
    )
}

/// A dropped file is taken only by the name of a picture it may be.
fn is_picture_name(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    DROPPED_ENDINGS.iter().any(|ending| name.ends_with(ending))
}

/// Characters base64 needs for `bytes` bytes.
pub const fn base64_len(bytes: usize) -> usize {
    bytes.div_ceil(3) * 4
}

/// A picture of the character the page drew (base64 of a PNG, without the
/// `data:` prefix): a whole PNG of at most 2048×2048, checked before
/// anything is written. Not a skin: no skin rule applies.
pub fn picture(data: &str) -> Result<Vec<u8>, SkinsProblem> {
    let bad = SkinsProblem::Invalid;
    if data.len() > base64_len(PICTURE_MAX_BYTES) {
        return Err(bad);
    }
    let png = base64::engine::general_purpose::STANDARD
        .decode(data)
        .map_err(|_| bad)?;
    if png.len() > PICTURE_MAX_BYTES {
        return Err(bad);
    }
    let (width, height) = header_size(&png).map_err(|_| bad)?;
    let sides = 1..=PICTURE_MAX_SIDE;
    if !sides.contains(&width) || !sides.contains(&height) {
        return Err(bad);
    }
    decodes(&png, width, height).ok_or(bad)?;
    Ok(png)
}

/// The name the save dialog offers for a picture: letters, digits, `_` and
/// `-` of `name` (or `skin`), then `.png`.
pub fn picture_file_name(name: &str) -> String {
    let stem: String = name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
        .take(PICTURE_NAME_CHARS)
        .collect();
    format!("{}.png", if stem.is_empty() { "skin" } else { &stem })
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

/// A file the player picked and has not uploaded yet: the PNG to send.
struct Picked {
    bytes: Vec<u8>,
    shape: Shape,
    /// The file's own durations, one per frame; empty for a single frame.
    frame_ms: Vec<u32>,
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
    /// The file's durations, one per frame; empty for a single frame.
    pub frame_ms: Vec<u32>,
    pub source: Source,
    /// What the import changed.
    pub notes: Vec<Note>,
    /// The weight of the PNG to upload.
    pub bytes: usize,
    /// The PNG to upload (the strip, for a GIF or an animated PNG) as a
    /// `data:` URL, for the preview.
    pub preview: String,
    /// A skin taken by nick: whether its owner wears it with slim arms.
    /// None for a file, whose model the player chooses.
    pub slim: Option<bool>,
}

pub struct Skins {
    http: reqwest::Client,
    /// The picked file per kind (skin, cape).
    picked: Mutex<[Option<Picked>; 2]>,
    /// Recently shown textures as `data:` URLs, by hash; the last used last.
    textures: Mutex<VecDeque<(String, String)>>,
    /// Nicks looked up for heads (in lowercase): when, and their skin's hash.
    lookups: Mutex<HashMap<String, (Instant, Option<String>)>>,
    /// Heads as `data:` URLs, by their skin's hash.
    heads: Mutex<VecDeque<(String, String)>>,
}

impl Skins {
    pub fn new(app_version: &str) -> Result<Self, String> {
        let http = crate::launcher_account::our_client(app_version, base())?;
        Ok(Self {
            http,
            picked: Mutex::new([None, None]),
            textures: Mutex::new(VecDeque::new()),
            lookups: Mutex::new(HashMap::new()),
            heads: Mutex::new(VecDeque::new()),
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
        let imported = tauri::async_runtime::spawn_blocking(move || {
            let bytes = read_within(&path, IMPORT_MAX_BYTES)?;
            skins_import::import(kind, &bytes)
        })
        .await
        .map_err(|_| SkinsProblem::Unreadable)??;
        Ok(self.keep(kind, name, imported, None))
    }

    /// Checks a file dropped on the page, by its name and bytes, and keeps
    /// it for upload as a picked one.
    pub async fn take(
        &self,
        kind: Kind,
        name: &str,
        bytes: Vec<u8>,
    ) -> Result<PickedView, SkinsProblem> {
        if name.len() > DROPPED_NAME_BYTES {
            return Err(SkinsProblem::Invalid);
        }
        if !is_picture_name(name) {
            return Err(SkinsProblem::BadPng);
        }
        if bytes.len() > IMPORT_MAX_BYTES {
            return Err(SkinsProblem::TooLarge);
        }
        let name = dropped_name(name);
        let imported =
            tauri::async_runtime::spawn_blocking(move || skins_import::import(kind, &bytes))
                .await
                .map_err(|_| SkinsProblem::Unreadable)??;
        Ok(self.keep(kind, name, imported, None))
    }

    /// The skin or cape another player wears, by nick, kept for upload as a
    /// picked one: the texture the service keeps, checked against its hash,
    /// with its owner's frame durations and, for a skin, arms.
    pub async fn by_nick(&self, kind: Kind, nick: &str) -> Result<PickedView, SkinsProblem> {
        if !is_nick(nick) {
            return Err(SkinsProblem::Invalid);
        }
        let response = self
            .http
            .get(format!("{}/v1/profile/{nick}", base()))
            .header(reqwest::header::ACCEPT, "application/json")
            .send()
            .await
            .map_err(|_| SkinsProblem::Network)?;
        let status = response.status().as_u16();
        if status == 404 {
            return Err(SkinsProblem::NotFound);
        }
        let bytes = body_within(response, MAX_PROFILE_BYTES)
            .await
            .map_err(|e| match e {
                BodyError::Network => SkinsProblem::Network,
                BodyError::TooLarge => SkinsProblem::Server,
            })?;
        if status != 200 {
            return Err(SkinsProblem::Server);
        }
        let value: Value = serde_json::from_slice(&bytes).map_err(|_| SkinsProblem::Server)?;
        let wardrobe = wardrobe_of(value)?;
        let worn = match kind {
            Kind::Skin => wardrobe.skin,
            Kind::Cape => wardrobe.cape,
        }
        .ok_or(SkinsProblem::NotFound)?;
        let png = self.texture_png(&worn.hash).await?;
        let mut imported =
            tauri::async_runtime::spawn_blocking(move || skins_import::import(kind, &png))
                .await
                .map_err(|_| SkinsProblem::Unreadable)??;
        // What the service keeps is a strip: its durations come with the profile.
        if imported.shape.frames != worn.frames {
            return Err(SkinsProblem::Server);
        }
        if worn.frames > 1 && worn.frame_ms.len() == worn.frames as usize {
            imported.frame_ms = worn.frame_ms;
        }
        let slim = (kind == Kind::Skin).then_some(worn.slim);
        Ok(self.keep(kind, nick.to_owned(), imported, slim))
    }

    fn keep(&self, kind: Kind, name: String, imported: Imported, slim: Option<bool>) -> PickedView {
        let Imported {
            png,
            shape,
            frame_ms,
            source,
            notes,
        } = imported;
        let view = PickedView {
            kind,
            name,
            width: shape.width,
            height: shape.height,
            frames: shape.frames,
            frame_ms: frame_ms.clone(),
            source,
            notes,
            bytes: png.len(),
            preview: data_url(&png),
            slim,
        };
        lock(&self.picked)[kind.slot()] = Some(Picked {
            bytes: png,
            shape,
            frame_ms,
        });
        view
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
        let (bytes, shape, own_ms) = {
            let picked = lock(&self.picked);
            let picked = picked[kind.slot()].as_ref().ok_or(SkinsProblem::Invalid)?;
            (picked.bytes.clone(), picked.shape, picked.frame_ms.clone())
        };
        // No durations from the interface: the file's own, if it has them.
        let frame_ms = frame_ms.or((!own_ms.is_empty()).then_some(own_ms.as_slice()));
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
        {
            let mut cache = lock(&self.textures);
            if let Some(at) = cache.iter().position(|(h, _)| h == hash) {
                let entry = cache.remove(at).expect("found above");
                let url = entry.1.clone();
                cache.push_back(entry);
                return Ok(url);
            }
        }
        let url = data_url(&self.texture_png(hash).await?);
        let mut cache = lock(&self.textures);
        let mut chars: usize = cache.iter().map(|(_, u)| u.len()).sum();
        while !cache.is_empty()
            && (cache.len() >= TEXTURES_KEPT || chars + url.len() > TEXTURES_KEPT_CHARS)
        {
            chars -= cache.pop_front().map_or(0, |(_, u)| u.len());
        }
        cache.push_back((hash.to_owned(), url.clone()));
        Ok(url)
    }

    /// The PNG a hash names, from the service; it must be that PNG.
    async fn texture_png(&self, hash: &str) -> Result<Vec<u8>, SkinsProblem> {
        if !is_hash(hash) {
            return Err(SkinsProblem::Server);
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
        check_worn(&png, hash)?;
        Ok(png)
    }

    /// The heads of these players, by nick as asked: a square PNG as a
    /// `data:` URL, or none for a nick nobody has, a player without a skin
    /// of ours, or a skin that could not be fetched now. A nick that cannot
    /// be one is never sent and has none.
    pub async fn heads(
        &self,
        nicks: &[String],
    ) -> Result<BTreeMap<String, Option<String>>, SkinsProblem> {
        if nicks.len() > MAX_HEAD_NICKS {
            return Err(SkinsProblem::Invalid);
        }
        let hashes = self.skins_of(nicks).await?;
        let mut wanted: Vec<String> = hashes.values().flatten().cloned().collect();
        wanted.sort();
        wanted.dedup();
        let missing: Vec<String> = {
            let heads = lock(&self.heads);
            wanted
                .into_iter()
                .filter(|h| !heads.iter().any(|(k, _)| k == h))
                .collect()
        };
        let made: Vec<(String, String)> = futures_util::stream::iter(missing)
            .map(|hash| async move {
                let png = self.texture_png(&hash).await.ok()?;
                let head = tauri::async_runtime::spawn_blocking(move || heads::head(&png))
                    .await
                    .ok()?
                    .ok()?;
                Some((hash, data_url(&head)))
            })
            .buffer_unordered(HEADS_AT_ONCE)
            .filter_map(std::future::ready)
            .collect()
            .await;
        let heads = {
            let mut heads = lock(&self.heads);
            for entry in made {
                if heads.len() >= HEADS_KEPT {
                    heads.pop_front();
                }
                heads.push_back(entry);
            }
            heads.clone()
        };
        Ok(hashes
            .into_iter()
            .map(|(nick, hash)| {
                let head = hash.and_then(|h| {
                    heads
                        .iter()
                        .find(|(k, _)| *k == h)
                        .map(|(_, url)| url.clone())
                });
                (nick, head)
            })
            .collect())
    }

    /// The hash of the skin each nick wears, from lookups of the last minute
    /// or from the service, 50 nicks a request.
    async fn skins_of(
        &self,
        nicks: &[String],
    ) -> Result<BTreeMap<String, Option<String>>, SkinsProblem> {
        let now = Instant::now();
        let mut out = BTreeMap::new();
        let mut ask: Vec<&str> = Vec::new();
        {
            let lookups = lock(&self.lookups);
            for nick in nicks {
                if out.contains_key(nick) {
                    continue;
                }
                if !is_nick(nick) {
                    out.insert(nick.clone(), None);
                    continue;
                }
                match lookups.get(&nick.to_ascii_lowercase()) {
                    Some((at, hash)) if now.duration_since(*at) < LOOKUP_HOLDS => {
                        out.insert(nick.clone(), hash.clone());
                    }
                    _ if !ask.iter().any(|n| n.eq_ignore_ascii_case(nick)) => ask.push(nick),
                    _ => {}
                }
            }
        }
        for batch in ask.chunks(PROFILES_AT_ONCE) {
            let response = self
                .http
                .post(format!("{}/v1/profiles", base()))
                .header(reqwest::header::ACCEPT, "application/json")
                .json(&serde_json::json!({ "nicks": batch }))
                .send()
                .await
                .map_err(|_| SkinsProblem::Network)?;
            let status = response.status().as_u16();
            let bytes = body_within(response, MAX_PROFILES_BYTES)
                .await
                .map_err(|e| match e {
                    BodyError::Network => SkinsProblem::Network,
                    BodyError::TooLarge => SkinsProblem::Server,
                })?;
            if status == 429 {
                return Err(SkinsProblem::RateLimited);
            }
            if status != 200 {
                return Err(SkinsProblem::Server);
            }
            let value: Value = serde_json::from_slice(&bytes).map_err(|_| SkinsProblem::Server)?;
            let profiles = value["profiles"].as_object().ok_or(SkinsProblem::Server)?;
            let mut lookups = lock(&self.lookups);
            if lookups.len() + batch.len() > LOOKUPS_KEPT {
                lookups.retain(|_, (at, _)| now.duration_since(*at) < LOOKUP_HOLDS);
                if lookups.len() + batch.len() > LOOKUPS_KEPT {
                    lookups.clear();
                }
            }
            for &nick in batch {
                // A profile that breaks the rules is no skin.
                let hash = profiles
                    .get(nick)
                    .filter(|p| !p.is_null())
                    .and_then(|p| wardrobe_of(p.clone()).ok())
                    .and_then(|w| w.skin)
                    .map(|s| s.hash);
                lookups.insert(nick.to_ascii_lowercase(), (now, hash.clone()));
                out.insert(nick.to_owned(), hash);
            }
        }
        // Asked twice in other cases: both get the answer.
        for nick in nicks {
            if !out.contains_key(nick) {
                let hash = lock(&self.lookups)
                    .get(&nick.to_ascii_lowercase())
                    .and_then(|(_, h)| h.clone());
                out.insert(nick.clone(), hash);
            }
        }
        Ok(out)
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
        // Bidi formatting cannot make one ending read as another.
        assert_eq!(
            shown_name(Path::new("/x/skin\u{202E}gnp.exe")),
            "skingnp.exe"
        );
        assert_eq!(
            plain_name(
                "a\u{200E}b\u{200F}c\u{202A}d\u{202B}e\u{202C}f\u{202D}g\u{202E}h\u{2066}i\u{2067}j\u{2068}k\u{2069}l"
            ),
            "abcdefghijkl"
        );
        assert_eq!(plain_name("Скин 皮肤 ✓.png"), "Скин 皮肤 ✓.png");
        assert_eq!(dropped_name("C:\\Users\\x\\my.png"), "my.png");
        assert_eq!(dropped_name("../../a\u{2066}b.gif"), "ab.gif");
        assert_eq!(
            dropped_name(&format!("{}.gif", "n".repeat(500)))
                .chars()
                .count(),
            NAME_CHARS
        );
    }

    /// A GIF of full frames in turns of black, red, green and blue, one per
    /// delay (hundredths of a second).
    fn gif_of(width: u16, height: u16, delays: &[u16]) -> Vec<u8> {
        let palette = [0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0, 255];
        let mut encoder = gif::Encoder::new(Vec::new(), width, height, &palette).unwrap();
        for (i, &delay) in delays.iter().enumerate() {
            let pixels = vec![(i % 4) as u8; usize::from(width) * usize::from(height)];
            encoder
                .write_frame(&gif::Frame {
                    delay,
                    width,
                    height,
                    buffer: pixels.into(),
                    ..gif::Frame::default()
                })
                .unwrap();
        }
        encoder.into_inner().unwrap()
    }

    #[tokio::test]
    async fn dropped_and_picked_animations_become_strips() {
        let skins = Skins::new("test").unwrap();
        let gif = gif_of(64, 64, &[10, 20]);
        for name in [
            "skin.jpg",
            "skin.svg",
            "skin",
            "skin.png.exe",
            "skin.gif\u{200E}",
            "",
        ] {
            assert_eq!(
                skins.take(Kind::Skin, name, gif.clone()).await,
                Err(SkinsProblem::BadPng),
                "{name}"
            );
        }
        assert_eq!(
            skins.take(Kind::Skin, &"a".repeat(2000), gif.clone()).await,
            Err(SkinsProblem::Invalid)
        );
        assert_eq!(
            skins.take(Kind::Cape, "x.png", b"<svg/>".to_vec()).await,
            Err(SkinsProblem::BadPng)
        );
        assert_eq!(
            skins
                .take(Kind::Cape, "x.gif", vec![0; IMPORT_MAX_BYTES + 1])
                .await,
            Err(SkinsProblem::TooLarge)
        );
        assert!(lock(&skins.picked).iter().all(Option::is_none));
        for name in ["a.apng", "B.GIF", "c.Png"] {
            assert!(is_picture_name(name), "{name}");
        }

        // The content says what a file is, whatever its name.
        let view = skins
            .take(Kind::Skin, "C:\\x\\My\u{202E}fig.PNG", gif.clone())
            .await
            .unwrap();
        assert_eq!(view.name, "Myfig.PNG");
        assert_eq!(view.source, Source::Gif);
        assert_eq!((view.width, view.height, view.frames), (64, 128, 2));
        assert_eq!(view.frame_ms, [100, 200]);
        assert!(view.notes.is_empty());
        {
            let picked = lock(&skins.picked);
            let picked = picked[0].as_ref().unwrap();
            assert_eq!(view.bytes, picked.bytes.len());
            assert_eq!(view.preview, data_url(&picked.bytes));
            assert_eq!(picked.frame_ms, [100, 200]);
        }
        let mut keys: Vec<String> = serde_json::to_value(&view)
            .unwrap()
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect();
        keys.sort();
        assert_eq!(
            keys,
            [
                "bytes", "frameMs", "frames", "height", "kind", "name", "notes", "preview", "slim",
                "source", "width"
            ]
        );

        // Picked in the dialog, by its path.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cape.gif");
        std::fs::write(&path, gif_of(64, 32, &[5, 5, 5])).unwrap();
        let view = skins.pick(Kind::Cape, path).await.unwrap();
        assert_eq!((view.name.as_str(), view.frames), ("cape.gif", 3));
        assert_eq!(view.frame_ms, [50, 50, 50]);
        let path = dir.path().join("big.gif");
        std::fs::write(&path, vec![0u8; IMPORT_MAX_BYTES + 1]).unwrap();
        assert_eq!(
            skins.pick(Kind::Cape, path).await,
            Err(SkinsProblem::TooLarge)
        );
    }

    #[tokio::test]
    async fn an_animation_is_uploaded_with_its_own_durations() {
        let (address, requests) = local_server(200, json!({ "skin": null, "cape": null })).await;
        LOCAL_SERVER.with(|s| s.set(Some(Box::leak(address.into_boxed_str()))));
        let skins = Skins::new("test").unwrap();
        let gif = gif_of(64, 64, &[10, 20]);
        let first = skins.take(Kind::Skin, "a.gif", gif.clone()).await.unwrap();
        skins
            .upload_with("token", Kind::Skin, true, None)
            .await
            .unwrap();
        // Durations from the interface win.
        skins.take(Kind::Skin, "a.gif", gif).await.unwrap();
        skins
            .upload_with("token", Kind::Skin, false, Some(&[300]))
            .await
            .unwrap();
        LOCAL_SERVER.with(|s| s.set(None));
        let requests = lock(&requests).clone();
        assert_eq!(requests.len(), 2);
        assert!(
            requests[0]
                .0
                .starts_with("PUT /v1/me/skin?model=slim&frames=2&frame_ms=100,200 HTTP/1.1"),
            "{}",
            requests[0].0
        );
        assert!(
            requests[1]
                .0
                .starts_with("PUT /v1/me/skin?model=default&frames=2&frame_ms=300 HTTP/1.1"),
            "{}",
            requests[1].0
        );
        // The strip is sent, not the GIF.
        assert!(requests[0].1.starts_with(PNG_SIGNATURE));
        assert_eq!(requests[0].1.len(), first.bytes);
    }

    #[test]
    fn pictures_to_save_are_whole_pngs_of_a_bounded_size() {
        let encode = |bytes: &[u8]| base64::engine::general_purpose::STANDARD.encode(bytes);
        // Not a skin's size: a picture has no skin rules.
        let good = png_of(300, 200);
        assert_eq!(picture(&encode(&good)), Ok(good.clone()));
        let wide = png_of(2048, 2);
        assert_eq!(picture(&encode(&wide)), Ok(wide));
        for bad in [
            String::new(),
            "not base64!".to_owned(),
            encode(b"<svg onload=alert(1)>"),
            encode(&header_only(4096, 4096)),
            encode(&header_only(2049, 10)),
            encode(&header_only(10, 2049)),
            encode(&header_only(0, 0)),
            encode(&header_only(64, 64)),
            encode(&good[..good.len() / 2]),
            encode(&good[..good.len() - 12]),
            format!("data:image/png;base64,{}", encode(&good)),
            "A".repeat(base64_len(PICTURE_MAX_BYTES) + 4),
        ] {
            assert_eq!(picture(&bad), Err(SkinsProblem::Invalid));
        }
        assert_eq!(base64_len(0), 0);
        assert_eq!(base64_len(1), 4);
        assert_eq!(base64_len(3), 4);
        assert_eq!(base64_len(4), 8);
        assert_eq!(picture_file_name("Steve"), "Steve.png");
        assert_eq!(picture_file_name("my_skin-2"), "my_skin-2.png");
        assert_eq!(picture_file_name("../../etc/passwd"), "etcpasswd.png");
        assert_eq!(picture_file_name("con.png\u{202E}"), "conpng.png");
        assert_eq!(picture_file_name(""), "skin.png");
        assert_eq!(picture_file_name("Скин"), "skin.png");
        assert_eq!(
            picture_file_name(&"n".repeat(100)),
            format!("{}.png", "n".repeat(32))
        );
    }

    #[test]
    fn new_problems_have_their_codes() {
        assert_eq!(
            serde_json::to_value([
                SkinsProblem::TooManyFrames,
                SkinsProblem::NotSaved,
                SkinsProblem::NotFound
            ])
            .unwrap(),
            json!(["too_many_frames", "not_saved", "not_found"])
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

    /// Answers by path: the first route whose path begins the request's
    /// gets its status, type and body; others get 404. Keeps each request's
    /// head and body.
    async fn routed_server(
        routes: Vec<(String, u16, &'static str, Vec<u8>)>,
    ) -> (String, Arc<Mutex<Vec<(String, Vec<u8>)>>>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let kept = Arc::clone(&requests);
        let routes = Arc::new(routes);
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                let kept = Arc::clone(&kept);
                let routes = Arc::clone(&routes);
                tokio::spawn(async move {
                    let mut request = Vec::new();
                    let mut buffer = [0u8; 16 * 1024];
                    let head = loop {
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
                            lock(&kept).push((head.clone(), request[end + 4..].to_vec()));
                            break head;
                        }
                    };
                    let path = head.split(' ').nth(1).unwrap_or_default().to_owned();
                    let (status, kind, body) = routes
                        .iter()
                        .find(|(prefix, ..)| path.starts_with(prefix.as_str()))
                        .map_or((404, "application/json", b"{}".to_vec()), |(_, s, k, b)| {
                            (*s, *k, b.clone())
                        });
                    let mut reply = format!(
                        "HTTP/1.1 {status} X\r\ncontent-type: {kind}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                        body.len()
                    )
                    .into_bytes();
                    reply.extend_from_slice(&body);
                    let _ = socket.write_all(&reply).await;
                });
            }
        });
        (address, requests)
    }

    fn sha(png: &[u8]) -> String {
        hex::encode(Sha256::digest(png))
    }

    fn profile(hash: &str, slim: bool, height: u32, frame_ms: &[u32]) -> Value {
        json!({ "nick": "x", "cape": null, "skin": {
            "hash": hash, "model": if slim { "slim" } else { "default" }, "width": 64,
            "height": height, "frames": height / 64, "frame_ms": frame_ms } })
    }

    #[tokio::test]
    async fn heads_are_looked_up_in_batches_and_kept() {
        let steve = png_of(64, 64);
        let alex = png_of(64, 128);
        let (steve_hash, alex_hash) = (sha(&steve), sha(&alex));
        let lying = "c".repeat(64);
        let profiles = json!({ "profiles": {
            "Steve": profile(&steve_hash, false, 64, &[]),
            "Alex": profile(&alex_hash, true, 128, &[100, 100]),
            "Bob": profile(&steve_hash, false, 64, &[]),
            "Lies": profile(&lying, false, 64, &[]),
            "Nobody": null,
            "Junky": { "skin": { "hash": "../../v1/me", "width": 64, "height": 64, "frames": 1 } },
        } });
        let (address, requests) = routed_server(vec![
            (
                "/v1/profiles".into(),
                200,
                "application/json",
                profiles.to_string().into_bytes(),
            ),
            (
                format!("/v1/textures/{steve_hash}"),
                200,
                "image/png",
                steve,
            ),
            (format!("/v1/textures/{alex_hash}"), 200, "image/png", alex),
            (
                format!("/v1/textures/{lying}"),
                200,
                "image/png",
                png_of(64, 64),
            ),
        ])
        .await;
        LOCAL_SERVER.with(|s| s.set(Some(Box::leak(address.into_boxed_str()))));
        let skins = Skins::new("test").unwrap();
        let nicks: Vec<String> = [
            "Steve", "Alex", "Bob", "Lies", "Nobody", "Junky", "../x", "Steve",
        ]
        .map(String::from)
        .to_vec();
        let heads = skins.heads(&nicks).await.unwrap();
        let again = skins.heads(&nicks[..3]).await.unwrap();
        LOCAL_SERVER.with(|s| s.set(None));

        assert_eq!(heads.len(), 7);
        let steve_head = heads["Steve"].clone().unwrap();
        assert!(steve_head.starts_with("data:image/png;base64,"));
        assert_eq!(heads["Bob"].as_deref(), Some(steve_head.as_str()));
        assert!(heads["Alex"].is_some());
        for none in ["Lies", "Nobody", "Junky", "../x"] {
            assert_eq!(heads[none], None, "{none}");
        }
        assert_eq!(again["Steve"].as_deref(), Some(steve_head.as_str()));
        let requests = lock(&requests).clone();
        let posts: Vec<_> = requests
            .iter()
            .filter(|(h, _)| h.starts_with("POST "))
            .collect();
        // Asked once, junk left out; the second call is answered from what is kept.
        assert_eq!(posts.len(), 1);
        let asked: Value = serde_json::from_slice(&posts[0].1).unwrap();
        assert_eq!(
            asked,
            json!({ "nicks": ["Steve", "Alex", "Bob", "Lies", "Nobody", "Junky"] })
        );
        // Each texture once: Bob wears Steve's.
        assert_eq!(requests.len(), 4);
        // A nick goes nowhere but the request's body.
        assert!(requests.iter().all(|(h, _)| !h.contains("Steve")));

        assert_eq!(
            skins
                .heads(&vec!["Steve".to_owned(); MAX_HEAD_NICKS + 1])
                .await,
            Err(SkinsProblem::Invalid)
        );
    }

    #[tokio::test]
    async fn many_nicks_go_fifty_at_a_time() {
        let (address, requests) = routed_server(vec![(
            "/v1/profiles".into(),
            200,
            "application/json",
            br#"{"profiles":{}}"#.to_vec(),
        )])
        .await;
        LOCAL_SERVER.with(|s| s.set(Some(Box::leak(address.into_boxed_str()))));
        let skins = Skins::new("test").unwrap();
        let nicks: Vec<String> = (0..120).map(|i| format!("player_{i}")).collect();
        let heads = skins.heads(&nicks).await.unwrap();
        LOCAL_SERVER.with(|s| s.set(None));
        assert_eq!(heads.len(), 120);
        assert!(heads.values().all(Option::is_none));
        let sizes: Vec<usize> = lock(&requests)
            .iter()
            .map(|(_, body)| {
                serde_json::from_slice::<Value>(body).unwrap()["nicks"]
                    .as_array()
                    .unwrap()
                    .len()
            })
            .collect();
        assert_eq!(sizes, [50, 50, 20]);
    }

    #[tokio::test]
    async fn a_skin_is_taken_by_nick_with_its_arms_and_durations() {
        let strip = png_of(64, 128);
        let hash = sha(&strip);
        let (address, requests) = routed_server(vec![
            (
                "/v1/profile/Alex".into(),
                200,
                "application/json",
                profile(&hash, true, 128, &[120, 240])
                    .to_string()
                    .into_bytes(),
            ),
            (
                "/v1/profile/Liar".into(),
                200,
                "application/json",
                profile(&"d".repeat(64), true, 64, &[])
                    .to_string()
                    .into_bytes(),
            ),
            (
                format!("/v1/textures/{hash}"),
                200,
                "image/png",
                strip.clone(),
            ),
            (
                format!("/v1/textures/{}", "d".repeat(64)),
                200,
                "image/png",
                strip.clone(),
            ),
            (
                "/v1/me/skin".into(),
                200,
                "application/json",
                br#"{"skin":null,"cape":null}"#.to_vec(),
            ),
        ])
        .await;
        LOCAL_SERVER.with(|s| s.set(Some(Box::leak(address.into_boxed_str()))));
        let skins = Skins::new("test").unwrap();
        let view = skins.by_nick(Kind::Skin, "Alex").await.unwrap();
        let cape = skins.by_nick(Kind::Cape, "Alex").await;
        let missing = skins.by_nick(Kind::Skin, "Nobody").await;
        let lying = skins.by_nick(Kind::Skin, "Liar").await;
        let junk = skins.by_nick(Kind::Skin, "../../v1/me").await;
        let uploaded = skins.upload_with("token", Kind::Skin, true, None).await;
        LOCAL_SERVER.with(|s| s.set(None));

        assert_eq!((view.name.as_str(), view.frames), ("Alex", 2));
        assert_eq!(view.frame_ms, [120, 240]);
        assert_eq!(view.slim, Some(true));
        assert_eq!(view.preview, data_url(&strip));
        assert_eq!(cape, Err(SkinsProblem::NotFound));
        assert_eq!(missing, Err(SkinsProblem::NotFound));
        assert_eq!(lying, Err(SkinsProblem::Server));
        assert_eq!(junk, Err(SkinsProblem::Invalid));
        assert!(uploaded.is_ok());
        let requests = lock(&requests).clone();
        assert!(requests.iter().all(|(h, _)| !h.contains("../")));
        let (head, body) = requests.last().unwrap();
        assert!(
            head.starts_with("PUT /v1/me/skin?model=slim&frames=2&frame_ms=120,240 HTTP/1.1"),
            "{head}"
        );
        assert_eq!(body, &strip);
    }

    #[tokio::test]
    async fn previews_are_kept_by_count_and_size() {
        let textures: Vec<Vec<u8>> = (0..TEXTURES_KEPT as u32 + 2)
            .map(|i| {
                png_of(64, 64 * (1 + i % 2))
                    .into_iter()
                    .chain([i as u8])
                    .collect()
            })
            .collect();
        let routes = textures
            .iter()
            .map(|t| {
                (
                    format!("/v1/textures/{}", sha(t)),
                    200,
                    "image/png",
                    t.clone(),
                )
            })
            .collect();
        let (address, requests) = routed_server(routes).await;
        LOCAL_SERVER.with(|s| s.set(Some(Box::leak(address.into_boxed_str()))));
        let skins = Skins::new("test").unwrap();
        for t in &textures {
            skins.texture(&sha(t)).await.unwrap();
        }
        // The last one again, from what is kept; the first one went.
        skins.texture(&sha(textures.last().unwrap())).await.unwrap();
        skins.texture(&sha(&textures[0])).await.unwrap();
        LOCAL_SERVER.with(|s| s.set(None));
        assert_eq!(lock(&requests).len(), textures.len() + 1);
        assert_eq!(lock(&skins.textures).len(), TEXTURES_KEPT);
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

    #[test]
    fn a_worn_texture_is_its_hash_and_a_size_the_service_takes() {
        let hash_of = |png: &[u8]| hex::encode(Sha256::digest(png));
        for (width, height) in [(64, 64), (64, 32), (512, 256), (64, 128)] {
            let png = png_of(width, height);
            assert_eq!(check_worn(&png, &hash_of(&png)), Ok(()), "{width}×{height}");
        }
        let skin = png_of(64, 64);
        assert_eq!(
            check_worn(&skin, &hash_of(&png_of(64, 32))),
            Err(SkinsProblem::Server)
        );
        for png in [png_of(100, 100), png_of(64, 48)] {
            assert_eq!(check_worn(&png, &hash_of(&png)), Err(SkinsProblem::Server));
        }
        // A header that claims a huge image: refused before anything decodes it.
        let mut huge = skin;
        huge[16..20].copy_from_slice(&23_000u32.to_be_bytes());
        huge[20..24].copy_from_slice(&23_000u32.to_be_bytes());
        assert_eq!(
            check_worn(&huge, &hash_of(&huge)),
            Err(SkinsProblem::Server)
        );
        let text = b"<script>alert(1)</script>".to_vec();
        assert_eq!(
            check_worn(&text, &hash_of(&text)),
            Err(SkinsProblem::Server)
        );
    }
}
