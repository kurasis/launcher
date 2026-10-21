//! Pictures are decoded within limits and encoded again: what is stored is
//! pixels we wrote, without the sender's metadata (camera, place, comments).

use std::io::Cursor;

use image::{DynamicImage, ImageFormat, ImageReader, Limits};

/// The longest side kept; bigger pictures are scaled down.
pub const MAX_SIDE: u32 = 2560;
/// Pixels of a picture that is decoded at all.
const MAX_PIXELS: u64 = 40_000_000;

/// A picture as stored: its bytes and extension (`png` or `jpg`).
pub struct Picture {
    pub bytes: Vec<u8>,
    pub extension: &'static str,
}

/// A reader of the sent bytes that stops at 8192 pixels a side and 200 MiB.
fn bounded(sent: &[u8]) -> Result<ImageReader<Cursor<&[u8]>>, &'static str> {
    let mut reader = ImageReader::new(Cursor::new(sent))
        .with_guessed_format()
        .map_err(|_| "image")?;
    let mut limits = Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(200 * 1024 * 1024);
    reader.limits(limits);
    Ok(reader)
}

/// PNG, JPEG or WebP by content, at most 8192 pixels a side and 40 million
/// in all; anything else is refused.
pub fn picture(sent: &[u8]) -> Result<Picture, &'static str> {
    let reader = bounded(sent)?;
    let format = reader.format().ok_or("image type")?;
    if !matches!(
        format,
        ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::WebP
    ) {
        return Err("image type");
    }
    // The size from the header first: a refused picture is never decoded.
    let (width, height) = reader.into_dimensions().map_err(|_| "image")?;
    if u64::from(width) * u64::from(height) > MAX_PIXELS || width == 0 || height == 0 {
        return Err("image size");
    }
    let mut decoded = bounded(sent)?.decode().map_err(|_| "image")?;
    if width.max(height) > MAX_SIDE {
        decoded = decoded.resize(MAX_SIDE, MAX_SIDE, image::imageops::FilterType::Triangle);
    }
    let mut out = Cursor::new(Vec::new());
    // A photo stays a photo; a screenshot (PNG, WebP) keeps sharp text.
    let extension = if format == ImageFormat::Jpeg {
        let rgb = DynamicImage::ImageRgb8(decoded.to_rgb8());
        let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 88);
        rgb.write_with_encoder(encoder).map_err(|_| "image")?;
        "jpg"
    } else {
        let pixels = if decoded.color().has_alpha() {
            DynamicImage::ImageRgba8(decoded.to_rgba8())
        } else {
            DynamicImage::ImageRgb8(decoded.to_rgb8())
        };
        pixels
            .write_to(&mut out, ImageFormat::Png)
            .map_err(|_| "image")?;
        "png"
    };
    Ok(Picture {
        bytes: out.into_inner(),
        extension,
    })
}
