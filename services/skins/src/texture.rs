//! What a skin or cape may be (D-065), and how an upload becomes the file we
//! keep. Every rule is here, in one place.
//!
//! An upload is decoded with limits set before decoding: the header alone
//! decides whether the size is one we take, so a small file that claims a
//! huge image is refused before anything is allocated for it. What is kept is
//! our own PNG, re-encoded from the pixels as RGBA with no other chunks (no
//! text, no colour profile, nothing hidden), named by its SHA-256.

use std::io::Cursor;

use sha2::{Digest, Sha256};

/// Largest upload, and largest file kept, per kind.
pub const SKIN_MAX_BYTES: usize = 2 * 1024 * 1024;
pub const CAPE_MAX_BYTES: usize = 1024 * 1024;
/// Widths of a frame: standard and HD (×2, ×4, ×8).
pub const WIDTHS: [u32; 4] = [64, 128, 256, 512];
/// Frames of an animation, a vertical strip.
pub const MAX_FRAMES: u32 = 32;
/// No image larger than this is decoded, whatever its shape.
pub const MAX_PIXELS: u64 = 16_777_216;
/// Duration of a frame, in milliseconds.
pub const FRAME_MS_MIN: u32 = 50;
pub const FRAME_MS_MAX: u32 = 10_000;
pub const FRAME_MS_DEFAULT: u32 = 100;
/// What the decoder may allocate for itself (rows, chunks); the image buffer
/// is ours and bounded by the size rules.
const DECODER_BYTES: usize = 4 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Skin,
    Cape,
}

impl Kind {
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "skin" => Some(Self::Skin),
            "cape" => Some(Self::Cape),
            _ => None,
        }
    }

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
}

/// Why an upload is refused: a short code for the launcher.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    BadPng,
    BadSize,
    TooLarge,
    BadModel,
    BadFrames,
    BadFrameMs,
}

impl Refusal {
    pub fn code(self) -> &'static str {
        match self {
            Self::BadPng => "bad_png",
            Self::BadSize => "bad_size",
            Self::TooLarge => "too_large",
            Self::BadModel => "bad_model",
            Self::BadFrames => "bad_frames",
            Self::BadFrameMs => "bad_frame_ms",
        }
    }
}

/// What the uploader says about the file (`?model=&frames=&frame_ms=`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Options {
    pub slim: bool,
    pub frames: Option<u32>,
    pub frame_ms: Option<Vec<u32>>,
}

/// Reads the query of an upload. Unknown keys are ignored; a known key twice,
/// or with a value outside its rule, is refused.
pub fn options(pairs: &[(String, String)]) -> Result<Options, Refusal> {
    let mut options = Options::default();
    let mut seen = [false; 3];
    for (key, value) in pairs {
        match key.as_str() {
            "model" => {
                if std::mem::replace(&mut seen[0], true) {
                    return Err(Refusal::BadModel);
                }
                options.slim = match value.as_str() {
                    "default" => false,
                    "slim" => true,
                    _ => return Err(Refusal::BadModel),
                };
            }
            "frames" => {
                if std::mem::replace(&mut seen[1], true) {
                    return Err(Refusal::BadFrames);
                }
                options.frames = Some(number(value, 2).ok_or(Refusal::BadFrames)?);
            }
            "frame_ms" => {
                if std::mem::replace(&mut seen[2], true) {
                    return Err(Refusal::BadFrameMs);
                }
                options.frame_ms = Some(frame_ms(value)?);
            }
            _ => {}
        }
    }
    if options
        .frames
        .is_some_and(|n| !(1..=MAX_FRAMES).contains(&n))
    {
        return Err(Refusal::BadFrames);
    }
    Ok(options)
}

/// Digits only (no sign, no spaces), at most `digits` of them.
fn number(text: &str, digits: usize) -> Option<u32> {
    if text.is_empty() || text.len() > digits || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

/// `100` or `100,200,…`: each 50–10 000 ms, at most one per frame.
fn frame_ms(text: &str) -> Result<Vec<u32>, Refusal> {
    let mut list = Vec::new();
    for part in text.split(',') {
        if list.len() == MAX_FRAMES as usize {
            return Err(Refusal::BadFrameMs);
        }
        let ms = number(part, 5).ok_or(Refusal::BadFrameMs)?;
        if !(FRAME_MS_MIN..=FRAME_MS_MAX).contains(&ms) {
            return Err(Refusal::BadFrameMs);
        }
        list.push(ms);
    }
    Ok(list)
}

/// The frames of an image of this size, if its shape is one we take: a frame
/// is `w`×`w` for a skin (or 64×32, the old layout, alone) and `w`×`w/2`
/// for a cape, stacked into a vertical strip of 1–32.
pub fn frames_of(kind: Kind, width: u32, height: u32) -> Result<u32, Refusal> {
    if u64::from(width) * u64::from(height) > MAX_PIXELS || !WIDTHS.contains(&width) {
        return Err(Refusal::BadSize);
    }
    if kind == Kind::Skin && width == 64 && height == 32 {
        return Ok(1);
    }
    let frame = match kind {
        Kind::Skin => width,
        Kind::Cape => width / 2,
    };
    if height == 0 || !height.is_multiple_of(frame) {
        return Err(Refusal::BadSize);
    }
    let frames = height / frame;
    if frames > MAX_FRAMES {
        return Err(Refusal::BadFrames);
    }
    Ok(frames)
}

/// A texture ready to keep.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Texture {
    pub kind: Kind,
    /// Our own PNG.
    pub png: Vec<u8>,
    /// SHA-256 of `png`, lowercase hex.
    pub hash: String,
    pub width: u32,
    pub height: u32,
    pub frames: u32,
    /// One per frame; empty for a single frame.
    pub frame_ms: Vec<u32>,
    /// Skins only.
    pub slim: bool,
}

/// Checks an upload against the rules and re-encodes it.
pub fn process(kind: Kind, body: &[u8], options: &Options) -> Result<Texture, Refusal> {
    if body.len() > kind.max_bytes() {
        return Err(Refusal::TooLarge);
    }
    if !body.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Err(Refusal::BadPng);
    }
    let mut decoder = png::Decoder::new_with_limits(
        Cursor::new(body),
        png::Limits {
            bytes: DECODER_BYTES,
        },
    );
    decoder.set_transformations(
        png::Transformations::EXPAND | png::Transformations::ALPHA | png::Transformations::STRIP_16,
    );
    decoder.set_ignore_text_chunk(true);
    decoder.set_ignore_iccp_chunk(true);
    // The size, from the header alone, before anything else is read.
    let (width, height) = {
        let info = decoder.read_header_info().map_err(|_| Refusal::BadPng)?;
        (info.width, info.height)
    };
    let frames = frames_of(kind, width, height)?;
    if options.frames.is_some_and(|n| n != frames) {
        return Err(Refusal::BadFrames);
    }
    let frame_ms = match (&options.frame_ms, frames) {
        (_, 1) => Vec::new(),
        (None, n) => vec![FRAME_MS_DEFAULT; n as usize],
        (Some(list), n) if list.len() == 1 => vec![list[0]; n as usize],
        (Some(list), n) if list.len() == n as usize => list.clone(),
        _ => return Err(Refusal::BadFrameMs),
    };
    let rgba = decode(decoder, width, height).ok_or(Refusal::BadPng)?;
    let png = encode(width, height, &rgba).ok_or(Refusal::BadPng)?;
    if png.len() > kind.max_bytes() {
        return Err(Refusal::TooLarge);
    }
    let hash = hex(&Sha256::digest(&png));
    Ok(Texture {
        kind,
        png,
        hash,
        width,
        height,
        frames,
        frame_ms,
        slim: kind == Kind::Skin && options.slim,
    })
}

/// The pixels as RGBA, 8 bits each; the whole file must be sound, up to its
/// end.
fn decode(decoder: png::Decoder<Cursor<&[u8]>>, width: u32, height: u32) -> Option<Vec<u8>> {
    let mut reader = decoder.read_info().ok()?;
    if reader.info().width != width || reader.info().height != height {
        return None;
    }
    let mut buffer = vec![0; reader.output_buffer_size()?];
    reader.next_frame(&mut buffer).ok()?;
    let (color, depth) = reader.output_color_type();
    reader.finish().ok()?;
    if depth != png::BitDepth::Eight {
        return None;
    }
    let pixels = width as usize * height as usize;
    let rgba: Vec<u8> = match color {
        png::ColorType::Rgba => buffer,
        png::ColorType::Rgb => buffer
            .chunks_exact(3)
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect(),
        png::ColorType::GrayscaleAlpha => buffer
            .chunks_exact(2)
            .flat_map(|p| [p[0], p[0], p[0], p[1]])
            .collect(),
        png::ColorType::Grayscale => buffer.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::Indexed => return None,
    };
    (rgba.len() == pixels * 4).then_some(rgba)
}

/// A fresh PNG of these pixels: IHDR, IDAT and IEND, nothing else.
pub fn encode(width: u32, height: u32, rgba: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_compression(png::Compression::High);
    let mut writer = encoder.write_header().ok()?;
    writer.write_image_data(rgba).ok()?;
    writer.finish().ok()?;
    Some(out)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// A texture's name: 64 lowercase hex digits.
pub fn is_hash(text: &str) -> bool {
    text.len() == 64 && text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shapes_we_take() {
        for (w, h, frames) in [
            (64, 64, 1),
            (64, 32, 1),
            (128, 128, 1),
            (256, 256, 1),
            (512, 512, 1),
            (64, 64 * 8, 8),
            (64, 64 * 32, 32),
            (512, 512 * 32, 32),
        ] {
            assert_eq!(frames_of(Kind::Skin, w, h), Ok(frames), "{w}×{h}");
        }
        for (w, h, frames) in [(64, 32, 1), (64, 64, 2), (512, 256 * 32, 32)] {
            assert_eq!(frames_of(Kind::Cape, w, h), Ok(frames), "{w}×{h}");
        }
        for (w, h) in [
            (65, 65),
            (64, 48),
            (1024, 1024),
            (64, 100),
            (64, 96),
            (0, 0),
            (64, 0),
            (128, 64),
            (30_000, 30_000),
            (u32::MAX, u32::MAX),
        ] {
            assert_eq!(
                frames_of(Kind::Skin, w, h),
                Err(Refusal::BadSize),
                "{w}×{h}"
            );
        }
        assert_eq!(frames_of(Kind::Skin, 64, 64 * 33), Err(Refusal::BadFrames));
        assert_eq!(frames_of(Kind::Cape, 64, 32 * 33), Err(Refusal::BadFrames));
        assert_eq!(frames_of(Kind::Cape, 64, 48), Err(Refusal::BadSize));
    }

    #[test]
    fn options_are_strict() {
        let q = |s: &[(&str, &str)]| {
            options(
                &s.iter()
                    .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                    .collect::<Vec<_>>(),
            )
        };
        assert_eq!(q(&[]), Ok(Options::default()));
        assert!(q(&[("model", "slim")]).unwrap().slim);
        assert_eq!(
            q(&[("frame_ms", "50,10000"), ("frames", "2"), ("other", "x")])
                .unwrap()
                .frame_ms,
            Some(vec![50, 10_000])
        );
        for model in ["", "Slim", "alex", "slim ", "default\0"] {
            assert_eq!(q(&[("model", model)]), Err(Refusal::BadModel), "{model}");
        }
        assert_eq!(
            q(&[("model", "slim"), ("model", "default")]),
            Err(Refusal::BadModel)
        );
        for frames in ["0", "33", "-1", "+2", "1.0", "", "999999999999", "２"] {
            assert_eq!(
                q(&[("frames", frames)]),
                Err(Refusal::BadFrames),
                "{frames}"
            );
        }
        for ms in [
            "49", "10001", "", ",", "100,", "1;DROP", "100 ", "-100", "1e3", "0x64", "100000",
        ] {
            assert_eq!(q(&[("frame_ms", ms)]), Err(Refusal::BadFrameMs), "{ms}");
        }
        let many = vec!["100"; 33].join(",");
        assert_eq!(q(&[("frame_ms", &many)]), Err(Refusal::BadFrameMs));
    }

    #[test]
    fn hashes() {
        assert!(is_hash(&"a".repeat(64)));
        assert!(!is_hash(&"A".repeat(64)));
        assert!(!is_hash(&"a".repeat(63)));
        assert!(!is_hash(&format!("{}g", "a".repeat(63))));
        assert!(!is_hash("../../../../etc/passwd"));
    }
}
