//! A picked GIF or animated PNG made into what skins.lapi.sh takes (D-065):
//! one PNG, its frames stacked top to bottom, and a duration per frame.
//!
//! Every frame is drawn onto the whole picture the way browsers draw it.
//! Then frames that look the same in a row become one, more than 32 are
//! thinned out and durations are brought within 50–10 000 ms; each change is
//! noted for the interface to say. A plain PNG is checked as it is.
//!
//! The format is read from the content, never from the name. The work is
//! bounded before it starts: the file's weight, then the picture's size from
//! the header, then the frames, counted as they are read.

use std::io::Cursor;
use std::num::NonZeroU64;

use serde::Serialize;

use crate::skins::{self, Kind, Shape, SkinsProblem};

/// Largest file to import, whatever its format.
pub const IMPORT_MAX_BYTES: usize = 8 * 1024 * 1024;
/// Frames in the file, before any are merged or left out.
pub const SOURCE_FRAMES_MAX: u32 = 256;
/// The picture's pixels times the file's frames.
pub const SOURCE_PIXELS_MAX: u64 = 16_777_216;
/// What a GIF's metadata (XMP, ICC) may take, whatever the picture's size.
const GIF_METADATA_BYTES: u64 = 1024 * 1024;
/// A delay this short or shorter is shown as 100 ms, as browsers do.
const SHORTEST_DELAY_MS: u32 = 10;
const DEFAULT_DELAY_MS: u32 = 100;

const GIF_SIGNATURES: [&[u8; 6]; 2] = [b"GIF87a", b"GIF89a"];

/// The format the file had.
#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    Png,
    Apng,
    Gif,
}

/// What the import changed, for the interface to say.
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Note {
    /// Frames that looked the same in a row became one, their durations
    /// added.
    Merged { from: u32, to: u32 },
    /// More than 32 frames: every `step`-th was kept, lasting as long as
    /// the frames left out after it.
    EveryNth { from: u32, to: u32, step: u32 },
    /// A duration outside 50–10 000 ms (`frame` counts from 0).
    Clamped { frame: u32, was: u32, now: u32 },
}

/// A file ready to upload.
#[derive(Debug)]
pub struct Imported {
    pub png: Vec<u8>,
    pub shape: Shape,
    /// One per frame; empty for a single frame.
    pub frame_ms: Vec<u32>,
    pub source: Source,
    pub notes: Vec<Note>,
}

/// Reads a PNG, an animated PNG or a GIF for `kind`.
pub fn import(kind: Kind, bytes: &[u8]) -> Result<Imported, SkinsProblem> {
    if bytes.len() > IMPORT_MAX_BYTES {
        return Err(SkinsProblem::TooLarge);
    }
    if bytes.starts_with(skins::PNG_SIGNATURE) {
        match announced_frames(bytes) {
            Some(count) if count > 1 => apng(kind, bytes, count),
            _ => plain(kind, bytes),
        }
    } else if GIF_SIGNATURES.iter().any(|s| bytes.starts_with(*s)) {
        gif(kind, bytes)
    } else {
        Err(SkinsProblem::BadPng)
    }
}

/// A PNG of one picture, checked and sent as it is.
fn plain(kind: Kind, bytes: &[u8]) -> Result<Imported, SkinsProblem> {
    let shape = skins::check(kind, bytes)?;
    Ok(Imported {
        png: bytes.to_vec(),
        shape,
        frame_ms: Vec::new(),
        source: Source::Png,
        notes: Vec::new(),
    })
}

fn apng(kind: Kind, bytes: &[u8], count: u32) -> Result<Imported, SkinsProblem> {
    let (width, height) = skins::header_size(bytes)?;
    picture_fits(kind, width, height)?;
    let mut frames = Frames::new(width, height);
    frames.within(count)?;
    read_apng(bytes, count, &mut frames)?;
    finish(kind, Source::Apng, frames)
}

fn gif(kind: Kind, bytes: &[u8]) -> Result<Imported, SkinsProblem> {
    let (width, height) = gif_size(bytes)?;
    picture_fits(kind, width, height)?;
    let mut frames = Frames::new(width, height);
    read_gif(bytes, &mut frames)?;
    finish(kind, Source::Gif, frames)
}

/// The frame count an `acTL` before the image data announces; `None` when
/// the PNG is not animated (or its chunks end first).
fn announced_frames(bytes: &[u8]) -> Option<u32> {
    let mut at = skins::PNG_SIGNATURE.len();
    loop {
        // Length (4), type (4), data, CRC (4).
        let head = bytes.get(at..at.checked_add(8)?)?;
        let length =
            usize::try_from(u32::from_be_bytes([head[0], head[1], head[2], head[3]])).ok()?;
        match &head[4..8] {
            b"IDAT" | b"IEND" => return None,
            b"acTL" if length == 8 => {
                let data = bytes.get(at + 8..at + 12)?;
                return Some(u32::from_be_bytes([data[0], data[1], data[2], data[3]]));
            }
            _ => {}
        }
        at = at.checked_add(12)?.checked_add(length)?;
    }
}

/// A GIF's size from its header: the logical screen after the signature.
fn gif_size(bytes: &[u8]) -> Result<(u32, u32), SkinsProblem> {
    // Width and height (little-endian), flags, background, aspect.
    let screen = bytes.get(6..13).ok_or(SkinsProblem::BadPng)?;
    Ok((
        u32::from(u16::from_le_bytes([screen[0], screen[1]])),
        u32::from(u16::from_le_bytes([screen[2], screen[3]])),
    ))
}

/// The picture's size, before any frame is read: a frame the service takes
/// in an animation, or a skin in the layout before 1.8 (which only passes if
/// it ends up as one frame).
fn picture_fits(kind: Kind, width: u32, height: u32) -> Result<(), SkinsProblem> {
    let (wide, high) = (u64::from(width), u64::from(height));
    let fits = skins::WIDTHS.contains(&width)
        && match kind {
            Kind::Skin => high == wide || high * 2 == wide,
            Kind::Cape => high * 2 == wide,
        };
    if fits {
        Ok(())
    } else {
        Err(SkinsProblem::BadSize)
    }
}

fn count(len: usize) -> u32 {
    u32::try_from(len).unwrap_or(u32::MAX)
}

/// One frame drawn on the whole picture, RGBA.
struct Frame {
    rgba: Vec<u8>,
    ms: u32,
}

/// The frames read so far. One that looks like the frame before it only
/// adds its duration to it.
struct Frames {
    width: u32,
    height: u32,
    /// Frames in the file so far.
    read: u32,
    kept: Vec<Frame>,
}

impl Frames {
    fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            read: 0,
            kept: Vec::new(),
        }
    }

    fn pixels(&self) -> u64 {
        u64::from(self.width) * u64::from(self.height)
    }

    /// A file of `frames` frames is within the limits.
    fn within(&self, frames: u32) -> Result<(), SkinsProblem> {
        if frames > SOURCE_FRAMES_MAX || self.pixels() * u64::from(frames) > SOURCE_PIXELS_MAX {
            Err(SkinsProblem::TooManyFrames)
        } else {
            Ok(())
        }
    }

    /// One more frame is about to be read.
    fn admit(&mut self) -> Result<(), SkinsProblem> {
        let next = self.read.saturating_add(1);
        self.within(next)?;
        self.read = next;
        Ok(())
    }

    fn push(&mut self, canvas: &Canvas, ms: u32) {
        if let Some(last) = self.kept.last_mut()
            && last.rgba == canvas.rgba
        {
            last.ms = last.ms.saturating_add(ms);
            return;
        }
        self.kept.push(Frame {
            rgba: canvas.rgba.clone(),
            ms,
        });
    }
}

/// A frame's rectangle on the picture.
#[derive(Clone, Copy)]
struct Area {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}

/// The picture frames are drawn on, RGBA, transparent at first.
struct Canvas {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

impl Canvas {
    /// Sizes come from `picture_fits`: at most 512×512.
    fn new(width: u32, height: u32) -> Result<Self, SkinsProblem> {
        let len = usize::try_from(u64::from(width) * u64::from(height) * 4)
            .map_err(|_| SkinsProblem::BadSize)?;
        Ok(Self {
            width,
            height,
            rgba: vec![0; len],
        })
    }

    /// The columns and rows of `area` that are on the picture.
    fn visible(&self, area: Area) -> (u32, u32) {
        (
            area.width.min(self.width.saturating_sub(area.x)),
            area.height.min(self.height.saturating_sub(area.y)),
        )
    }

    /// The pixel at `x`, `y`, if it is on the picture.
    fn at(&mut self, x: u64, y: u64) -> Option<&mut [u8; 4]> {
        if x >= u64::from(self.width) || y >= u64::from(self.height) {
            return None;
        }
        let start = usize::try_from((y * u64::from(self.width) + x) * 4).ok()?;
        self.rgba
            .get_mut(start..start.checked_add(4)?)?
            .try_into()
            .ok()
    }

    /// Makes `area` transparent.
    fn clear(&mut self, area: Area) {
        let (columns, rows) = self.visible(area);
        for row in 0..rows {
            for column in 0..columns {
                if let Some(pixel) = self.at(
                    u64::from(area.x) + u64::from(column),
                    u64::from(area.y) + u64::from(row),
                ) {
                    pixel.fill(0);
                }
            }
        }
    }
}

/// `source` over `target` with its alpha, neither premultiplied.
pub(crate) fn over(target: &mut [u8; 4], source: [u8; 4]) {
    let alpha = u32::from(source[3]);
    if alpha == 0 {
        return;
    }
    if alpha == 255 {
        *target = source;
        return;
    }
    // Weights scaled by 255²: the source's, and what shows of the target.
    let under = u32::from(target[3]) * (255 - alpha);
    let total = alpha * 255 + under;
    for channel in 0..3 {
        let mixed = (u32::from(source[channel]) * alpha * 255
            + u32::from(target[channel]) * under
            + total / 2)
            / total;
        target[channel] = u8::try_from(mixed).unwrap_or(u8::MAX);
    }
    target[3] = u8::try_from((total + 127) / 255).unwrap_or(u8::MAX);
}

/// A GIF delay (hundredths of a second) in milliseconds.
fn gif_ms(delay: u16) -> u32 {
    let ms = u32::from(delay) * 10;
    if ms <= SHORTEST_DELAY_MS {
        DEFAULT_DELAY_MS
    } else {
        ms
    }
}

/// An APNG delay (a fraction of a second) in milliseconds.
fn apng_ms(numerator: u16, denominator: u16) -> u32 {
    let denominator = match denominator {
        0 => 100,
        d => u32::from(d),
    };
    let ms = (u32::from(numerator) * 1000 + denominator / 2) / denominator;
    if ms <= SHORTEST_DELAY_MS {
        DEFAULT_DELAY_MS
    } else {
        ms
    }
}

fn read_gif(bytes: &[u8], frames: &mut Frames) -> Result<(), SkinsProblem> {
    let bad = |_| SkinsProblem::BadPng;
    let mut options = gif::DecodeOptions::new();
    options.set_color_output(gif::ColorOutput::RGBA);
    // A frame outside the picture is refused, not cut.
    options.check_frame_consistency(true);
    let limit = NonZeroU64::new((frames.pixels() * 4).max(GIF_METADATA_BYTES))
        .ok_or(SkinsProblem::BadSize)?;
    options.set_memory_limit(gif::MemoryLimit::Bytes(limit));
    let mut decoder = options.read_info(bytes).map_err(bad)?;
    if (u32::from(decoder.width()), u32::from(decoder.height())) != (frames.width, frames.height) {
        return Err(SkinsProblem::BadPng);
    }
    let mut canvas = Canvas::new(frames.width, frames.height)?;
    let mut before = Vec::new();
    let mut pixels = Vec::new();
    while let Some(frame) = decoder.next_frame_info().map_err(bad)? {
        let area = Area {
            x: u32::from(frame.left),
            y: u32::from(frame.top),
            width: u32::from(frame.width),
            height: u32::from(frame.height),
        };
        let (delay, dispose) = (frame.delay, frame.dispose);
        frames.admit()?;
        let size = usize::from(frame.width) * usize::from(frame.height) * 4;
        if size > canvas.rgba.len() {
            return Err(SkinsProblem::BadPng);
        }
        pixels.clear();
        pixels.resize(size, 0);
        decoder.read_into_buffer(&mut pixels).map_err(bad)?;
        if dispose == gif::DisposalMethod::Previous {
            before.clone_from(&canvas.rgba);
        }
        let (columns, rows) = canvas.visible(area);
        for row in 0..rows {
            for column in 0..columns {
                let from = (row as usize * area.width as usize + column as usize) * 4;
                let Some(&[r, g, b, a]) = pixels.get(from..from + 4) else {
                    return Err(SkinsProblem::BadPng);
                };
                // GIF pixels are opaque or not drawn at all.
                if a == 0 {
                    continue;
                }
                if let Some(target) = canvas.at(u64::from(area.x + column), u64::from(area.y + row))
                {
                    *target = [r, g, b, a];
                }
            }
        }
        frames.push(&canvas, gif_ms(delay));
        match dispose {
            gif::DisposalMethod::Background => canvas.clear(area),
            gif::DisposalMethod::Previous => canvas.rgba.clone_from(&before),
            gif::DisposalMethod::Keep | gif::DisposalMethod::Any => {}
        }
    }
    Ok(())
}

fn read_apng(bytes: &[u8], count: u32, frames: &mut Frames) -> Result<(), SkinsProblem> {
    use png::{BlendOp, DisposeOp};
    let bad = |_| SkinsProblem::BadPng;
    let mut decoder = png::Decoder::new_with_limits(
        Cursor::new(bytes),
        png::Limits {
            bytes: skins::DECODER_BYTES,
        },
    );
    decoder.set_transformations(
        png::Transformations::EXPAND | png::Transformations::ALPHA | png::Transformations::STRIP_16,
    );
    decoder.set_ignore_text_chunk(true);
    decoder.set_ignore_iccp_chunk(true);
    let mut reader = decoder.read_info().map_err(bad)?;
    let info = reader.info();
    if (info.width, info.height) != (frames.width, frames.height)
        || info.animation_control.map(|a| a.num_frames) != Some(count)
    {
        return Err(SkinsProblem::BadPng);
    }
    // Without an fcTL before it, the first image is for viewers that do not
    // animate: it is not a frame.
    let first_is_frame = info.frame_control.is_some();
    let interlaced = info.interlaced;
    let mut canvas = Canvas::new(frames.width, frames.height)?;
    let size = reader.output_buffer_size().ok_or(SkinsProblem::BadPng)?;
    // Four bytes a pixel at most.
    if size > canvas.rgba.len() {
        return Err(SkinsProblem::BadPng);
    }
    // Interlaced frames are written with the whole picture's rows.
    let picture_line = reader
        .output_line_size(frames.width)
        .ok_or(SkinsProblem::BadPng)?;
    let mut buffer = vec![0u8; size];
    let mut before = Vec::new();
    for index in 0..count {
        frames.admit()?;
        let control = if index == 0 && first_is_frame {
            reader.info().frame_control.ok_or(SkinsProblem::BadPng)?
        } else {
            *reader.next_frame_info().map_err(bad)?
        };
        buffer.fill(0);
        let output = reader.next_frame(&mut buffer).map_err(bad)?;
        if (output.width, output.height) != (control.width, control.height)
            || output.bit_depth != png::BitDepth::Eight
        {
            return Err(SkinsProblem::BadPng);
        }
        let channels = match output.color_type {
            png::ColorType::Grayscale => 1,
            png::ColorType::GrayscaleAlpha => 2,
            png::ColorType::Rgb => 3,
            png::ColorType::Rgba => 4,
            png::ColorType::Indexed => return Err(SkinsProblem::BadPng),
        };
        let line = if interlaced {
            picture_line
        } else {
            output.line_size
        };
        let area = Area {
            x: control.x_offset,
            y: control.y_offset,
            width: control.width,
            height: control.height,
        };
        // Before the first frame there is only the empty picture.
        let dispose = match control.dispose_op {
            DisposeOp::Previous if index == 0 => DisposeOp::Background,
            op => op,
        };
        if dispose == DisposeOp::Previous {
            before.clone_from(&canvas.rgba);
        }
        let (columns, rows) = canvas.visible(area);
        for row in 0..rows {
            for column in 0..columns {
                let from = row as usize * line + column as usize * channels;
                let Some(pixel) = buffer.get(from..from + channels) else {
                    return Err(SkinsProblem::BadPng);
                };
                let source = match *pixel {
                    [grey] => [grey, grey, grey, 255],
                    [grey, alpha] => [grey, grey, grey, alpha],
                    [r, g, b] => [r, g, b, 255],
                    [r, g, b, a] => [r, g, b, a],
                    _ => return Err(SkinsProblem::BadPng),
                };
                let Some(target) = canvas.at(
                    u64::from(area.x) + u64::from(column),
                    u64::from(area.y) + u64::from(row),
                ) else {
                    continue;
                };
                match control.blend_op {
                    BlendOp::Source => *target = source,
                    BlendOp::Over => over(target, source),
                }
            }
        }
        frames.push(&canvas, apng_ms(control.delay_num, control.delay_den));
        match dispose {
            DisposeOp::None => {}
            DisposeOp::Background => canvas.clear(area),
            DisposeOp::Previous => canvas.rgba.clone_from(&before),
        }
    }
    Ok(())
}

/// Every `step`-th frame, each lasting as long as its group.
fn thin(frames: Vec<Frame>, step: usize) -> Vec<Frame> {
    let mut kept: Vec<Frame> = Vec::with_capacity(frames.len().div_ceil(step.max(1)));
    for (index, frame) in frames.into_iter().enumerate() {
        if index.is_multiple_of(step.max(1)) {
            kept.push(frame);
        } else if let Some(last) = kept.last_mut() {
            last.ms = last.ms.saturating_add(frame.ms);
        }
    }
    kept
}

/// The frames read, reduced to what the service takes, as one PNG strip.
fn finish(kind: Kind, source: Source, frames: Frames) -> Result<Imported, SkinsProblem> {
    let Frames {
        width,
        height,
        read,
        mut kept,
    } = frames;
    if kept.is_empty() {
        return Err(SkinsProblem::BadPng);
    }
    let mut notes = Vec::new();
    if count(kept.len()) != read {
        notes.push(Note::Merged {
            from: read,
            to: count(kept.len()),
        });
    }
    let max = skins::MAX_FRAMES as usize;
    if kept.len() > max {
        let from = count(kept.len());
        let step = kept.len().div_ceil(max);
        kept = thin(kept, step);
        notes.push(Note::EveryNth {
            from,
            to: count(kept.len()),
            step: count(step),
        });
    }
    let total = count(kept.len());
    // An animated skin's frames are square; the layout before 1.8 is one
    // frame alone.
    if total > 1 && kind == Kind::Skin && height != width {
        return Err(SkinsProblem::BadSize);
    }
    let mut frame_ms = Vec::new();
    if total > 1 {
        for (index, frame) in kept.iter().enumerate() {
            let now = frame.ms.clamp(skins::FRAME_MS_MIN, skins::FRAME_MS_MAX);
            if now != frame.ms {
                notes.push(Note::Clamped {
                    frame: count(index),
                    was: frame.ms,
                    now,
                });
            }
            frame_ms.push(now);
        }
    }
    let strip_height = height.checked_mul(total).ok_or(SkinsProblem::BadSize)?;
    let mut strip = Vec::with_capacity(kept.iter().map(|f| f.rgba.len()).sum());
    for frame in kept {
        strip.extend_from_slice(&frame.rgba);
    }
    let png = encode(width, strip_height, &strip)?;
    drop(strip);
    if png.len() > kind.max_bytes() {
        return Err(SkinsProblem::TooLarge);
    }
    let shape = skins::check(kind, &png)?;
    if shape.frames != total {
        return Err(SkinsProblem::BadSize);
    }
    Ok(Imported {
        png,
        shape,
        frame_ms,
        source,
        notes,
    })
}

pub(crate) fn encode(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>, SkinsProblem> {
    let failed = |_| SkinsProblem::BadPng;
    let mut png = Vec::new();
    let mut encoder = png::Encoder::new(&mut png, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_compression(png::Compression::High);
    let mut writer = encoder.write_header().map_err(failed)?;
    writer.write_image_data(rgba).map_err(failed)?;
    writer.finish().map_err(failed)?;
    Ok(png)
}

#[cfg(test)]
mod tests {
    use super::*;
    use png::{BlendOp, DisposeOp};
    use std::borrow::Cow;

    /// Black, red, green, blue, white, grey.
    const PALETTE: [u8; 18] = [
        0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255, 128, 128, 128,
    ];
    const BLACK: u8 = 0;
    const RED: u8 = 1;
    const GREEN: u8 = 2;
    const BLUE: u8 = 3;
    const WHITE: u8 = 4;
    const GREY: u8 = 5;

    const CLEAR_RGBA: [u8; 4] = [0, 0, 0, 0];
    const RED_RGBA: [u8; 4] = [255, 0, 0, 255];
    const GREEN_RGBA: [u8; 4] = [0, 255, 0, 255];
    const BLUE_RGBA: [u8; 4] = [0, 0, 255, 255];
    const WHITE_RGBA: [u8; 4] = [255, 255, 255, 255];

    struct GifFrame {
        left: u16,
        top: u16,
        width: u16,
        height: u16,
        indices: Vec<u8>,
        delay: u16,
        dispose: gif::DisposalMethod,
        transparent: Option<u8>,
    }

    /// A rectangle of one colour, kept after it is shown.
    fn rect(left: u16, top: u16, width: u16, height: u16, index: u8, delay: u16) -> GifFrame {
        GifFrame {
            left,
            top,
            width,
            height,
            indices: vec![index; usize::from(width) * usize::from(height)],
            delay,
            dispose: gif::DisposalMethod::Keep,
            transparent: None,
        }
    }

    fn gif_file(width: u16, height: u16, frames: &[GifFrame]) -> Vec<u8> {
        let mut encoder = gif::Encoder::new(Vec::new(), width, height, &PALETTE).unwrap();
        for f in frames {
            encoder
                .write_frame(&gif::Frame {
                    delay: f.delay,
                    dispose: f.dispose,
                    transparent: f.transparent,
                    left: f.left,
                    top: f.top,
                    width: f.width,
                    height: f.height,
                    buffer: Cow::Borrowed(&f.indices),
                    ..gif::Frame::default()
                })
                .unwrap();
        }
        encoder.into_inner().unwrap()
    }

    struct ApngFrame {
        x: u32,
        y: u32,
        width: u32,
        height: u32,
        pixels: Vec<u8>,
        delay: (u16, u16),
        dispose: DisposeOp,
        blend: BlendOp,
    }

    /// A rectangle of one RGBA colour, replacing what is under it.
    fn area(x: u32, y: u32, width: u32, height: u32, rgba: [u8; 4], delay_cs: u16) -> ApngFrame {
        ApngFrame {
            x,
            y,
            width,
            height,
            pixels: rgba.repeat((width * height) as usize),
            delay: (delay_cs, 100),
            dispose: DisposeOp::None,
            blend: BlendOp::Source,
        }
    }

    /// An animated PNG; `default` is an image before the animation, not
    /// part of it.
    fn apng_with(
        width: u32,
        height: u32,
        color: png::ColorType,
        default: Option<Vec<u8>>,
        frames: &[ApngFrame],
    ) -> Vec<u8> {
        let mut out = Vec::new();
        let mut encoder = png::Encoder::new(&mut out, width, height);
        encoder.set_color(color);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_animated(frames.len() as u32, 0).unwrap();
        if default.is_some() {
            encoder.set_sep_def_img(true).unwrap();
        }
        let mut writer = encoder.write_header().unwrap();
        if let Some(pixels) = &default {
            writer.write_image_data(pixels).unwrap();
        }
        for f in frames {
            writer.reset_frame_position().unwrap();
            writer.set_frame_dimension(f.width, f.height).unwrap();
            writer.set_frame_position(f.x, f.y).unwrap();
            writer.set_frame_delay(f.delay.0, f.delay.1).unwrap();
            writer.set_dispose_op(f.dispose).unwrap();
            writer.set_blend_op(f.blend).unwrap();
            writer.write_image_data(&f.pixels).unwrap();
        }
        writer.finish().unwrap();
        out
    }

    fn apng_file(width: u32, height: u32, frames: &[ApngFrame]) -> Vec<u8> {
        apng_with(width, height, png::ColorType::Rgba, None, frames)
    }

    /// The RGBA pixel at `x`, `y` of a frame of the imported strip.
    fn pixel(imported: &Imported, frame: u32, x: u32, y: u32) -> [u8; 4] {
        let mut decoder = png::Decoder::new(Cursor::new(&imported.png));
        decoder.set_transformations(png::Transformations::EXPAND);
        let mut reader = decoder.read_info().unwrap();
        let mut buffer = vec![0; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut buffer).unwrap();
        assert_eq!(info.color_type, png::ColorType::Rgba);
        let frame_height = imported.shape.height / imported.shape.frames;
        let at = (((frame * frame_height + y) * info.width + x) * 4) as usize;
        buffer[at..at + 4].try_into().unwrap()
    }

    /// Only the header of a GIF of this size: no palette, no frame.
    fn gif_header(width: u16, height: u16) -> Vec<u8> {
        let mut out = b"GIF89a".to_vec();
        out.extend_from_slice(&width.to_le_bytes());
        out.extend_from_slice(&height.to_le_bytes());
        out.extend_from_slice(&[0, 0, 0]);
        out
    }

    /// Only the header of an animated PNG of this size: IHDR and acTL.
    fn apng_header(width: u32, height: u32, frames: u32) -> Vec<u8> {
        let mut out = skins::PNG_SIGNATURE.to_vec();
        out.extend_from_slice(&[0, 0, 0, 13]);
        out.extend_from_slice(b"IHDR");
        out.extend_from_slice(&width.to_be_bytes());
        out.extend_from_slice(&height.to_be_bytes());
        out.extend_from_slice(&[8, 6, 0, 0, 0, 0, 0, 0, 0]);
        out.extend_from_slice(&[0, 0, 0, 8]);
        out.extend_from_slice(b"acTL");
        out.extend_from_slice(&frames.to_be_bytes());
        out.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 0]);
        out
    }

    #[test]
    fn gifs_become_strips_the_service_takes() {
        let skin = gif_file(
            128,
            128,
            &[
                rect(0, 0, 128, 128, RED, 10),
                rect(0, 0, 128, 128, GREEN, 20),
                rect(0, 0, 128, 128, BLUE, 30),
            ],
        );
        let imported = import(Kind::Skin, &skin).unwrap();
        assert_eq!(
            imported.shape,
            Shape {
                width: 128,
                height: 384,
                frames: 3
            }
        );
        assert_eq!(imported.frame_ms, [100, 200, 300]);
        assert_eq!(imported.source, Source::Gif);
        assert!(imported.notes.is_empty());
        assert_eq!(skins::check(Kind::Skin, &imported.png), Ok(imported.shape));
        assert_eq!(pixel(&imported, 0, 5, 5), RED_RGBA);
        assert_eq!(pixel(&imported, 1, 127, 127), GREEN_RGBA);
        assert_eq!(pixel(&imported, 2, 0, 0), BLUE_RGBA);

        let cape = gif_file(
            256,
            128,
            &[
                rect(0, 0, 256, 128, WHITE, 50),
                rect(0, 0, 256, 128, GREY, 50),
            ],
        );
        let imported = import(Kind::Cape, &cape).unwrap();
        assert_eq!((imported.shape.width, imported.shape.height), (256, 256));
        assert_eq!(imported.shape.frames, 2);
        assert_eq!(imported.frame_ms, [500, 500]);
        assert_eq!(skins::check(Kind::Cape, &imported.png), Ok(imported.shape));
        // A cape's picture is not a skin's.
        assert_eq!(import(Kind::Skin, &cape).err(), Some(SkinsProblem::BadSize));
    }

    #[test]
    fn animated_pngs_become_strips_the_service_takes() {
        let mut first = area(0, 0, 256, 256, RED_RGBA, 0);
        first.delay = (1, 4);
        let mut second = area(0, 0, 256, 256, BLUE_RGBA, 0);
        second.delay = (3, 10);
        let imported = import(Kind::Skin, &apng_file(256, 256, &[first, second])).unwrap();
        assert_eq!(
            imported.shape,
            Shape {
                width: 256,
                height: 512,
                frames: 2
            }
        );
        assert_eq!(imported.frame_ms, [250, 300]);
        assert_eq!(imported.source, Source::Apng);
        assert!(imported.notes.is_empty());
        assert_eq!(skins::check(Kind::Skin, &imported.png), Ok(imported.shape));
        assert_eq!(pixel(&imported, 1, 200, 200), BLUE_RGBA);

        let cape = apng_file(
            512,
            256,
            &[
                area(0, 0, 512, 256, GREEN_RGBA, 10),
                area(0, 0, 512, 256, WHITE_RGBA, 10),
                area(0, 0, 512, 256, RED_RGBA, 10),
            ],
        );
        let imported = import(Kind::Cape, &cape).unwrap();
        assert_eq!((imported.shape.width, imported.shape.height), (512, 768));
        assert_eq!(imported.frame_ms, [100, 100, 100]);
        assert_eq!(pixel(&imported, 2, 511, 255), RED_RGBA);

        // Grey pictures come out as RGBA.
        let grey = |level: u8, x: u32, y: u32, side: u32| ApngFrame {
            pixels: vec![level; (side * side) as usize],
            ..area(x, y, side, side, CLEAR_RGBA, 10)
        };
        let imported = import(
            Kind::Skin,
            &apng_with(
                64,
                64,
                png::ColorType::Grayscale,
                None,
                &[grey(100, 0, 0, 64), grey(200, 1, 1, 1)],
            ),
        )
        .unwrap();
        assert_eq!(pixel(&imported, 0, 0, 0), [100, 100, 100, 255]);
        assert_eq!(pixel(&imported, 1, 1, 1), [200, 200, 200, 255]);
        assert_eq!(pixel(&imported, 1, 2, 2), [100, 100, 100, 255]);
    }

    #[test]
    fn plain_pngs_are_checked_as_they_are() {
        let mut png = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut png, 64, 64);
            encoder.set_color(png::ColorType::Rgba);
            let mut writer = encoder.write_header().unwrap();
            writer.write_image_data(&[90; 64 * 64 * 4]).unwrap();
            writer.finish().unwrap();
        }
        let imported = import(Kind::Skin, &png).unwrap();
        assert_eq!(imported.png, png);
        assert_eq!(imported.source, Source::Png);
        assert!(imported.frame_ms.is_empty() && imported.notes.is_empty());
        // As a cape, a strip of two frames; durations come from the interface.
        let imported = import(Kind::Cape, &png).unwrap();
        assert_eq!((imported.shape.frames, imported.frame_ms.len()), (2, 0));
        png[16..24].copy_from_slice(&[0, 0, 0, 64, 0, 0, 0, 48]);
        assert_eq!(import(Kind::Skin, &png).err(), Some(SkinsProblem::BadSize));
        // An animation of one frame is a plain PNG.
        let one = apng_file(64, 32, &[area(0, 0, 64, 32, RED_RGBA, 10)]);
        let imported = import(Kind::Cape, &one).unwrap();
        assert_eq!((imported.source, imported.shape.frames), (Source::Png, 1));
        assert_eq!(imported.png, one);
    }

    #[test]
    fn gif_disposal_and_transparency_draw_as_browsers_do() {
        let mut gone = rect(0, 0, 8, 8, RED, 10);
        gone.dispose = gif::DisposalMethod::Background;
        let mut undone = rect(30, 30, 8, 8, WHITE, 10);
        undone.dispose = gif::DisposalMethod::Previous;
        let mut holes = rect(0, 10, 4, 1, RED, 10);
        holes.indices = vec![RED, GREY, RED, GREY];
        holes.transparent = Some(GREY);
        let file = gif_file(
            64,
            64,
            &[
                rect(0, 0, 64, 64, GREEN, 10),
                gone,
                rect(20, 20, 1, 1, BLUE, 10),
                undone,
                rect(40, 40, 1, 1, RED, 10),
                holes,
            ],
        );
        let imported = import(Kind::Skin, &file).unwrap();
        assert_eq!(imported.shape.frames, 6);
        // Shown, then cleared to transparent.
        assert_eq!(pixel(&imported, 1, 0, 0), RED_RGBA);
        assert_eq!(pixel(&imported, 1, 10, 10), GREEN_RGBA);
        assert_eq!(pixel(&imported, 2, 0, 0), CLEAR_RGBA);
        assert_eq!(pixel(&imported, 2, 7, 7), CLEAR_RGBA);
        assert_eq!(pixel(&imported, 2, 8, 8), GREEN_RGBA);
        assert_eq!(pixel(&imported, 2, 20, 20), BLUE_RGBA);
        // Shown, then what was there before comes back.
        assert_eq!(pixel(&imported, 3, 30, 30), WHITE_RGBA);
        assert_eq!(pixel(&imported, 4, 30, 30), GREEN_RGBA);
        assert_eq!(pixel(&imported, 4, 40, 40), RED_RGBA);
        assert_eq!(pixel(&imported, 4, 0, 0), CLEAR_RGBA);
        assert_eq!(pixel(&imported, 4, 20, 20), BLUE_RGBA);
        // Transparent pixels leave what is under them.
        assert_eq!(pixel(&imported, 5, 0, 10), RED_RGBA);
        assert_eq!(pixel(&imported, 5, 1, 10), GREEN_RGBA);
        assert_eq!(pixel(&imported, 5, 2, 10), RED_RGBA);
        assert_eq!(pixel(&imported, 5, 3, 10), GREEN_RGBA);
    }

    #[test]
    fn apng_offsets_blending_and_disposal_draw_as_browsers_do() {
        let half_blue = [0, 0, 255, 128];
        let mut over = area(10, 5, 4, 4, half_blue, 10);
        over.blend = BlendOp::Over;
        over.dispose = DisposeOp::Background;
        let mut source = area(30, 30, 2, 2, half_blue, 10);
        source.dispose = DisposeOp::Previous;
        let mut last = area(50, 50, 1, 1, GREEN_RGBA, 10);
        last.blend = BlendOp::Over;
        let file = apng_file(
            64,
            64,
            &[area(0, 0, 64, 64, RED_RGBA, 10), over, source, last],
        );
        let imported = import(Kind::Skin, &file).unwrap();
        assert_eq!(imported.shape.frames, 4);
        // Half blue over red, inside the frame's rectangle only.
        let mixed = pixel(&imported, 1, 10, 5);
        assert!(mixed[0].abs_diff(127) <= 1 && mixed[1] == 0, "{mixed:?}");
        assert!(mixed[2].abs_diff(128) <= 1 && mixed[3] == 255, "{mixed:?}");
        assert_eq!(pixel(&imported, 1, 13, 8), mixed);
        assert_eq!(pixel(&imported, 1, 9, 5), RED_RGBA);
        assert_eq!(pixel(&imported, 1, 14, 5), RED_RGBA);
        assert_eq!(pixel(&imported, 1, 10, 9), RED_RGBA);
        // Source replaces, alpha too; the rectangle before was cleared.
        assert_eq!(pixel(&imported, 2, 30, 30), half_blue);
        assert_eq!(pixel(&imported, 2, 31, 31), half_blue);
        assert_eq!(pixel(&imported, 2, 10, 5), CLEAR_RGBA);
        assert_eq!(pixel(&imported, 2, 9, 5), RED_RGBA);
        // What was under the last frame comes back.
        assert_eq!(pixel(&imported, 3, 30, 30), RED_RGBA);
        assert_eq!(pixel(&imported, 3, 50, 50), GREEN_RGBA);
        assert_eq!(pixel(&imported, 3, 10, 5), CLEAR_RGBA);

        // A first frame's "previous" is the empty picture.
        let mut first = area(0, 0, 64, 64, BLUE_RGBA, 10);
        first.dispose = DisposeOp::Previous;
        let imported = import(
            Kind::Skin,
            &apng_file(64, 64, &[first, area(1, 1, 1, 1, RED_RGBA, 10)]),
        )
        .unwrap();
        assert_eq!(pixel(&imported, 1, 0, 0), CLEAR_RGBA);
        assert_eq!(pixel(&imported, 1, 1, 1), RED_RGBA);
    }

    #[test]
    fn an_image_before_the_animation_is_not_a_frame() {
        let file = apng_with(
            64,
            64,
            png::ColorType::Rgba,
            Some(WHITE_RGBA.repeat(64 * 64)),
            &[
                area(0, 0, 64, 64, RED_RGBA, 10),
                area(0, 0, 64, 64, GREEN_RGBA, 20),
            ],
        );
        let imported = import(Kind::Skin, &file).unwrap();
        assert_eq!(imported.shape.frames, 2);
        assert_eq!(imported.frame_ms, [100, 200]);
        assert_eq!(pixel(&imported, 0, 3, 3), RED_RGBA);
        assert_eq!(pixel(&imported, 1, 3, 3), GREEN_RGBA);
    }

    #[test]
    fn short_delays_are_100_ms_and_durations_are_clamped() {
        let colours = [BLACK, RED, GREEN, BLUE, WHITE, GREY];
        let delays = [0, 1, 2, 3, 2000, 5];
        let frames: Vec<GifFrame> = colours
            .iter()
            .zip(delays)
            .map(|(&c, d)| rect(0, 0, 64, 64, c, d))
            .collect();
        let imported = import(Kind::Skin, &gif_file(64, 64, &frames)).unwrap();
        assert_eq!(imported.frame_ms, [100, 100, 50, 50, 10_000, 50]);
        assert_eq!(
            imported.notes,
            [
                Note::Clamped {
                    frame: 2,
                    was: 20,
                    now: 50
                },
                Note::Clamped {
                    frame: 3,
                    was: 30,
                    now: 50
                },
                Note::Clamped {
                    frame: 4,
                    was: 20_000,
                    now: 10_000
                },
            ]
        );

        let colours = [RED_RGBA, GREEN_RGBA, BLUE_RGBA, WHITE_RGBA, CLEAR_RGBA];
        let delays = [(1, 100), (0, 0), (3, 100), (20, 1), (1, 0)];
        let frames: Vec<ApngFrame> = colours
            .iter()
            .zip(delays)
            .map(|(&c, delay)| ApngFrame {
                delay,
                ..area(0, 0, 64, 64, c, 0)
            })
            .collect();
        let imported = import(Kind::Skin, &apng_file(64, 64, &frames)).unwrap();
        assert_eq!(imported.frame_ms, [100, 100, 50, 10_000, 100]);
        assert_eq!(imported.notes.len(), 2);
        assert_eq!(
            (apng_ms(1, 3), apng_ms(2, 3), apng_ms(11, 1000)),
            (333, 667, 11)
        );
        assert_eq!(
            (gif_ms(0), gif_ms(1), gif_ms(2), gif_ms(u16::MAX)),
            (100, 100, 20, 655_350)
        );
    }

    #[test]
    fn identical_frames_in_a_row_become_one() {
        let file = gif_file(
            64,
            64,
            &[
                rect(0, 0, 64, 64, RED, 10),
                rect(0, 0, 64, 64, RED, 20),
                // Draws what is already there.
                rect(5, 5, 1, 1, RED, 30),
                rect(0, 0, 64, 64, GREEN, 10),
                rect(0, 0, 64, 64, RED, 10),
            ],
        );
        let imported = import(Kind::Skin, &file).unwrap();
        assert_eq!(imported.shape.frames, 3);
        assert_eq!(imported.frame_ms, [600, 100, 100]);
        assert_eq!(imported.notes, [Note::Merged { from: 5, to: 3 }]);

        // All the same: one frame, no duration.
        let same = gif_file(
            64,
            32,
            &[rect(0, 0, 64, 32, RED, 10), rect(0, 0, 64, 32, RED, 10)],
        );
        let imported = import(Kind::Cape, &same).unwrap();
        assert_eq!(imported.shape.frames, 1);
        assert!(imported.frame_ms.is_empty());
        assert_eq!(imported.notes, [Note::Merged { from: 2, to: 1 }]);
    }

    /// `count` frames, each one more red dot on a 64×64 picture.
    fn dots(count: u16, delay: u16) -> Vec<u8> {
        let frames: Vec<GifFrame> = (0..count)
            .map(|i| rect(i % 64, i / 64, 1, 1, RED, delay))
            .collect();
        gif_file(64, 64, &frames)
    }

    #[test]
    fn long_animations_are_thinned() {
        let imported = import(Kind::Skin, &dots(48, 5)).unwrap();
        assert_eq!(imported.shape.frames, 24);
        assert_eq!(imported.frame_ms, [100; 24]);
        assert_eq!(
            imported.notes,
            [Note::EveryNth {
                from: 48,
                to: 24,
                step: 2
            }]
        );
        // Frame k is the file's frame 2k: dots 0..=2k.
        for k in [0, 1, 11, 23] {
            assert_eq!(pixel(&imported, k, 2 * k, 0), RED_RGBA, "{k}");
            assert_eq!(pixel(&imported, k, 2 * k + 1, 0), CLEAR_RGBA, "{k}");
        }

        let imported = import(Kind::Skin, &dots(100, 5)).unwrap();
        assert!(imported.shape.frames <= skins::MAX_FRAMES);
        assert_eq!(imported.shape.frames, 25);
        assert_eq!(imported.frame_ms, [200; 25]);
        assert_eq!(
            imported.notes,
            [Note::EveryNth {
                from: 100,
                to: 25,
                step: 4
            }]
        );
        // A last group shorter than the others lasts as long as it is.
        let imported = import(Kind::Skin, &dots(33, 10)).unwrap();
        assert_eq!(imported.shape.frames, 17);
        assert_eq!(imported.frame_ms[..16], [200; 16]);
        assert_eq!(imported.frame_ms[16], 100);
    }

    #[test]
    fn the_old_skin_layout_is_one_frame_only() {
        let one = gif_file(64, 32, &[rect(0, 0, 64, 32, RED, 10)]);
        let imported = import(Kind::Skin, &one).unwrap();
        assert_eq!(
            imported.shape,
            Shape {
                width: 64,
                height: 32,
                frames: 1
            }
        );
        assert!(imported.frame_ms.is_empty());
        assert_eq!(imported.source, Source::Gif);
        // Two of them would read as one square frame.
        let two = gif_file(
            64,
            32,
            &[rect(0, 0, 64, 32, RED, 10), rect(0, 0, 64, 32, BLUE, 10)],
        );
        assert_eq!(import(Kind::Skin, &two).err(), Some(SkinsProblem::BadSize));
        assert_eq!(
            import(Kind::Cape, &two).map(|i| i.shape.frames).ok(),
            Some(2)
        );
    }

    #[test]
    fn bad_sizes_are_refused_before_decoding() {
        for (w, h) in [
            (65, 65),
            (64, 48),
            (1024, 1024),
            (65_535, 65_535),
            (0, 0),
            (32, 32),
            (128, 32),
        ] {
            assert_eq!(
                import(Kind::Skin, &gif_header(w, h)).err(),
                Some(SkinsProblem::BadSize),
                "{w}×{h}"
            );
            assert_eq!(
                import(Kind::Skin, &apng_header(u32::from(w), u32::from(h), 2)).err(),
                Some(SkinsProblem::BadSize),
                "{w}×{h}"
            );
        }
        for (w, h) in [(64, 64), (64, 48), (1024, 512)] {
            assert_eq!(
                import(Kind::Cape, &gif_header(w, h)).err(),
                Some(SkinsProblem::BadSize),
                "{w}×{h}"
            );
        }
        assert_eq!(
            import(Kind::Skin, &apng_header(u32::MAX, u32::MAX, 2)).err(),
            Some(SkinsProblem::BadSize)
        );
        // A size the rules take, and nothing after the header.
        assert_eq!(
            import(Kind::Skin, &gif_header(64, 64)).err(),
            Some(SkinsProblem::BadPng)
        );
        assert_eq!(
            import(Kind::Skin, &apng_header(64, 64, 2)).err(),
            Some(SkinsProblem::BadPng)
        );
    }

    #[test]
    fn too_many_frames_are_refused() {
        assert_eq!(
            import(Kind::Skin, &dots(257, 10)).err(),
            Some(SkinsProblem::TooManyFrames)
        );
        assert_eq!(
            import(Kind::Skin, &dots(256, 10))
                .map(|i| i.shape.frames)
                .ok(),
            Some(32)
        );
        // 512×512: 64 frames are as many pixels as a file may have.
        let big = |count: u16| -> Vec<u8> {
            let frames: Vec<GifFrame> = (0..count).map(|_| rect(0, 0, 1, 1, RED, 10)).collect();
            gif_file(512, 512, &frames)
        };
        assert_eq!(
            import(Kind::Skin, &big(65)).err(),
            Some(SkinsProblem::TooManyFrames)
        );
        assert_eq!(
            import(Kind::Skin, &big(64)).map(|i| i.shape.frames).ok(),
            Some(1)
        );
        // An animated PNG says how many frames it has: refused before any is read.
        let mut frames = vec![area(0, 0, 512, 512, RED_RGBA, 10)];
        frames.extend((1..65).map(|_| area(0, 0, 1, 1, BLUE_RGBA, 10)));
        assert_eq!(
            import(Kind::Skin, &apng_file(512, 512, &frames)).err(),
            Some(SkinsProblem::TooManyFrames)
        );
        assert_eq!(
            import(Kind::Skin, &apng_header(64, 64, 257)).err(),
            Some(SkinsProblem::TooManyFrames)
        );
        assert_eq!(
            import(Kind::Skin, &apng_header(64, 64, u32::MAX)).err(),
            Some(SkinsProblem::TooManyFrames)
        );
    }

    #[test]
    fn a_gif_frame_outside_the_picture_is_refused() {
        for frame in [
            rect(0, 0, 65, 1, RED, 10),
            rect(60, 60, 8, 8, RED, 10),
            rect(64, 0, 1, 1, RED, 10),
        ] {
            let file = gif_file(64, 64, &[rect(0, 0, 64, 64, GREEN, 10), frame]);
            assert_eq!(import(Kind::Skin, &file).err(), Some(SkinsProblem::BadPng));
        }
    }

    #[test]
    fn heavy_files_are_refused() {
        let mut heavy = dots(2, 10);
        heavy.resize(IMPORT_MAX_BYTES + 1, 0);
        assert_eq!(
            import(Kind::Skin, &heavy).err(),
            Some(SkinsProblem::TooLarge)
        );
        // A strip heavier than a cape may be.
        let mut seed = 1u32;
        let mut noise = || {
            seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
            (seed >> 16) as u8
        };
        let frames: Vec<ApngFrame> = (0..12)
            .map(|_| ApngFrame {
                pixels: (0..512 * 256 * 4).map(|_| noise()).collect(),
                ..area(0, 0, 512, 256, CLEAR_RGBA, 10)
            })
            .collect();
        let file = apng_file(512, 256, &frames);
        assert!(file.len() <= IMPORT_MAX_BYTES);
        assert_eq!(
            import(Kind::Cape, &file).err(),
            Some(SkinsProblem::TooLarge)
        );
    }

    #[test]
    fn junk_and_cut_files_never_panic() {
        for junk in [
            Vec::new(),
            b"hello".to_vec(),
            b"GIF89a".to_vec(),
            b"GIF87a\x40\x00".to_vec(),
            skins::PNG_SIGNATURE.to_vec(),
            b"<svg onload=alert(1)>".to_vec(),
            b"\xff\xd8\xff\xe0JFIF".to_vec(),
            vec![0u8; 4096],
            vec![0xff; 100],
        ] {
            assert!(import(Kind::Skin, &junk).is_err());
        }
        let gif = gif_file(
            64,
            64,
            &[rect(0, 0, 64, 64, RED, 10), {
                let mut f = rect(3, 4, 5, 6, BLUE, 10);
                f.dispose = gif::DisposalMethod::Previous;
                f
            }],
        );
        let mut over = area(3, 4, 5, 6, [1, 2, 3, 100], 10);
        over.blend = BlendOp::Over;
        over.dispose = DisposeOp::Background;
        let apng = apng_with(
            64,
            64,
            png::ColorType::Rgba,
            Some(WHITE_RGBA.repeat(64 * 64)),
            &[
                area(0, 0, 64, 64, RED_RGBA, 10),
                over,
                area(1, 1, 2, 2, GREEN_RGBA, 3),
            ],
        );
        for (kind, file) in [(Kind::Skin, &gif), (Kind::Skin, &apng)] {
            assert!(import(kind, file).is_ok());
            // Every cut, and every byte changed.
            for len in 0..file.len() {
                if let Ok(imported) = import(kind, &file[..len]) {
                    assert_eq!(skins::check(kind, &imported.png), Ok(imported.shape));
                }
            }
            for at in 0..file.len() {
                let mut broken = file.clone();
                broken[at] ^= 0xa5;
                if let Ok(imported) = import(kind, &broken) {
                    assert_eq!(skins::check(kind, &imported.png), Ok(imported.shape));
                }
            }
        }
    }

    #[test]
    fn notes_and_sources_read_as_the_interface_expects() {
        use serde_json::json;
        assert_eq!(
            serde_json::to_value(Note::Merged { from: 40, to: 30 }).unwrap(),
            json!({ "kind": "merged", "from": 40, "to": 30 })
        );
        assert_eq!(
            serde_json::to_value(Note::EveryNth {
                from: 48,
                to: 24,
                step: 2
            })
            .unwrap(),
            json!({ "kind": "everyNth", "from": 48, "to": 24, "step": 2 })
        );
        assert_eq!(
            serde_json::to_value(Note::Clamped {
                frame: 4,
                was: 30,
                now: 50
            })
            .unwrap(),
            json!({ "kind": "clamped", "frame": 4, "was": 30, "now": 50 })
        );
        assert_eq!(
            serde_json::to_value([Source::Png, Source::Apng, Source::Gif]).unwrap(),
            json!(["png", "apng", "gif"])
        );
    }
}
