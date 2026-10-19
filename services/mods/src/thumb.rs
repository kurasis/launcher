//! Pictures: what a downloaded one is (by its first bytes, never by what the
//! site said), and small copies for lists. Decoding runs in a process of its
//! own (`lapi-mods thumb`), with bounds on size and memory, so a picture made
//! to hurt a decoder cannot take the service down.

use std::path::Path;

/// A picture's type by its first bytes.
pub fn sniff(head: &[u8]) -> Option<&'static str> {
    if head.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if head.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("image/jpeg")
    } else if head.starts_with(b"GIF87a") || head.starts_with(b"GIF89a") {
        Some("image/gif")
    } else if head.len() >= 12 && &head[0..4] == b"RIFF" && &head[8..12] == b"WEBP" {
        Some("image/webp")
    } else {
        None
    }
}

/// The extension a picture's address ends with; nginx gives the type by it.
pub fn extension(media_type: &str) -> &'static str {
    match media_type {
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "image/gif" => "gif",
        "image/webp" => "webp",
        _ => "bin",
    }
}

/// The biggest picture we decode.
const MAX_SIDE: u32 = 8192;
const MAX_INPUT: u64 = 20 * 1024 * 1024;

/// A copy of `input` that fits in `size`×`size`, as lossless WebP (icons)
/// or JPEG (gallery), written to `output`. The first frame of an animation.
pub fn make(input: &Path, output: &Path, size: u32, format: &str) -> Result<(), String> {
    let length = std::fs::metadata(input).map_err(|e| e.to_string())?.len();
    if length > MAX_INPUT {
        return Err("picture too large".to_owned());
    }
    let mut reader = image::ImageReader::open(input)
        .map_err(|e| e.to_string())?
        .with_guessed_format()
        .map_err(|e| e.to_string())?;
    if !matches!(
        reader.format(),
        Some(
            image::ImageFormat::Png
                | image::ImageFormat::Jpeg
                | image::ImageFormat::Gif
                | image::ImageFormat::WebP
        )
    ) {
        return Err("not a picture we read".to_owned());
    }
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_SIDE);
    limits.max_image_height = Some(MAX_SIDE);
    limits.max_alloc = Some(512 * 1024 * 1024);
    reader.limits(limits);
    let picture = reader.decode().map_err(|e| e.to_string())?;
    let picture = if picture.width() > size || picture.height() > size {
        picture.resize(size, size, image::imageops::FilterType::Triangle)
    } else {
        picture
    };
    let mut out =
        std::io::BufWriter::new(std::fs::File::create(output).map_err(|e| e.to_string())?);
    match format {
        "webp" => picture
            .to_rgba8()
            .write_with_encoder(image::codecs::webp::WebPEncoder::new_lossless(&mut out))
            .map_err(|e| e.to_string())?,
        "jpeg" => picture
            .to_rgb8()
            .write_with_encoder(image::codecs::jpeg::JpegEncoder::new_with_quality(
                &mut out, 82,
            ))
            .map_err(|e| e.to_string())?,
        _ => return Err("format: webp or jpeg".to_owned()),
    }
    use std::io::Write as _;
    out.flush().map_err(|e| e.to_string())
}
