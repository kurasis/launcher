//! Skin heads: the face of a skin with its hat over it, as friends lists,
//! rooms and chats show a player. Cut here from the skin the service keeps,
//! so the page gets a small square PNG and never the whole texture.

use std::io::Cursor;

use super::{DECODER_BYTES, Kind, SkinsProblem, frames_of, header_size};
use crate::skins_import::{encode, over};

/// A head's side in a 64-wide skin; HD skins scale it.
const SIDE: u32 = 8;
/// Where the face and the hat start in a 64-wide skin (the hat is a second
/// layer over the face, 32 pixels to its right).
const FACE: (u32, u32) = (8, 8);
const HAT_X: u32 = 40;

/// The head of a skin (its first frame, for an animation) as a PNG of
/// 8×8 pixels per 64 of the skin's width: 8×8 for a standard skin, 64×64
/// for a 512-wide one. Only rows down to the head are decoded.
pub fn head(png: &[u8]) -> Result<Vec<u8>, SkinsProblem> {
    let (width, height) = header_size(png)?;
    frames_of(Kind::Skin, width, height)?;
    let scale = width / 64;
    let side = SIDE * scale;
    let mut decoder = png::Decoder::new_with_limits(
        Cursor::new(png),
        png::Limits {
            bytes: DECODER_BYTES,
        },
    );
    decoder.set_transformations(
        png::Transformations::EXPAND | png::Transformations::ALPHA | png::Transformations::STRIP_16,
    );
    decoder.set_ignore_text_chunk(true);
    decoder.set_ignore_iccp_chunk(true);
    let mut reader = decoder.read_info().map_err(|_| SkinsProblem::BadPng)?;
    // The service writes its PNGs row by row; an interlaced one would need
    // the whole image decoded for a few rows of it.
    if reader.info().interlaced || (reader.info().width, reader.info().height) != (width, height) {
        return Err(SkinsProblem::BadPng);
    }
    let channels = match reader.output_color_type() {
        (png::ColorType::Rgba, png::BitDepth::Eight) => 4,
        (png::ColorType::GrayscaleAlpha, png::BitDepth::Eight) => 2,
        _ => return Err(SkinsProblem::BadPng),
    };
    let pixel = |row: &[u8], x: u32| -> [u8; 4] {
        let at = x as usize * channels;
        if channels == 4 {
            [row[at], row[at + 1], row[at + 2], row[at + 3]]
        } else {
            [row[at], row[at], row[at], row[at + 1]]
        }
    };
    let cells = (side * side) as usize;
    let mut face = vec![[0u8; 4]; cells];
    let mut hat = vec![[0u8; 4]; cells];
    for y in 0..FACE.1 * scale + side {
        let row = reader
            .next_row()
            .map_err(|_| SkinsProblem::BadPng)?
            .ok_or(SkinsProblem::BadPng)?;
        let row = row.data();
        if row.len() < width as usize * channels {
            return Err(SkinsProblem::BadPng);
        }
        let Some(at) = y.checked_sub(FACE.1 * scale) else {
            continue;
        };
        for x in 0..side {
            let cell = (at * side + x) as usize;
            face[cell] = pixel(row, FACE.0 * scale + x);
            hat[cell] = pixel(row, HAT_X * scale + x);
        }
    }
    // The game draws the face's own layer without transparency.
    for p in &mut face {
        p[3] = 255;
    }
    // A skin of the layout before 1.8 has no transparent hat when its maker
    // filled that area: the game then leaves the hat out, and so does this.
    let legacy = u64::from(height) * 2 == u64::from(width);
    if !(legacy && hat.iter().all(|p| p[3] >= 128)) {
        for (under, above) in face.iter_mut().zip(&hat) {
            over(under, *above);
        }
    }
    encode(side, side, &face.concat())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A skin `width` wide and `height` high: every pixel `base`, the face
    /// `face`, the hat area `hat`, in RGBA.
    fn skin(width: u32, height: u32, base: [u8; 4], face: [u8; 4], hat: [u8; 4]) -> Vec<u8> {
        let scale = width / 64;
        let mut rgba = Vec::with_capacity((width * height * 4) as usize);
        for y in 0..height {
            for x in 0..width {
                let in_head = (8 * scale..16 * scale).contains(&y);
                let pixel = if in_head && (8 * scale..16 * scale).contains(&x) {
                    face
                } else if in_head && (40 * scale..48 * scale).contains(&x) {
                    hat
                } else {
                    base
                };
                rgba.extend_from_slice(&pixel);
            }
        }
        encode(width, height, &rgba).unwrap()
    }

    fn pixels(png: &[u8]) -> (u32, u32, Vec<u8>) {
        let mut reader = png::Decoder::new(Cursor::new(png)).read_info().unwrap();
        let mut buffer = vec![0; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut buffer).unwrap();
        buffer.truncate(info.buffer_size());
        (info.width, info.height, buffer)
    }

    const RED: [u8; 4] = [255, 0, 0, 255];
    const BLUE: [u8; 4] = [0, 0, 255, 255];
    const CLEAR: [u8; 4] = [0, 0, 0, 0];
    const GREY: [u8; 4] = [9, 9, 9, 255];

    #[test]
    fn the_hat_is_drawn_over_the_face_at_the_skins_scale() {
        for (width, height, side) in [(64, 64, 8), (128, 128, 16), (512, 512, 64), (64, 256, 8)] {
            let (w, h, rgba) = pixels(&head(&skin(width, height, GREY, RED, BLUE)).unwrap());
            assert_eq!((w, h), (side, side), "{width}×{height}");
            assert!(rgba.chunks(4).all(|p| p == BLUE), "{width}×{height}");
        }
        // A transparent hat shows the face; the face is never transparent.
        let (_, _, rgba) = pixels(&head(&skin(64, 64, GREY, [255, 0, 0, 0], CLEAR)).unwrap());
        assert!(rgba.chunks(4).all(|p| p == RED));
    }

    #[test]
    fn an_old_skin_with_a_filled_hat_shows_its_face() {
        let (w, _, rgba) = pixels(&head(&skin(64, 32, GREY, RED, BLUE)).unwrap());
        assert_eq!(w, 8);
        assert!(rgba.chunks(4).all(|p| p == RED));
        // Nearly opaque is opaque.
        let png = skin(64, 32, GREY, RED, [0, 0, 255, 200]);
        assert!(pixels(&head(&png).unwrap()).2.chunks(4).all(|p| p == RED));
        // With any transparency in it, the hat is the maker's own.
        let mut rgba = pixels(&skin(64, 32, GREY, RED, BLUE)).2;
        let corner = (8 * 64 + 40) * 4;
        rgba[corner..corner + 4].copy_from_slice(&CLEAR);
        let (_, _, head) = pixels(&head(&encode(64, 32, &rgba).unwrap()).unwrap());
        assert_eq!(&head[..4], RED);
        assert!(head[4..].chunks(4).all(|p| p == BLUE));
    }

    #[test]
    fn junk_and_capes_have_no_head() {
        for junk in [
            Vec::new(),
            b"<svg onload=alert(1)>".to_vec(),
            skin(64, 64, GREY, RED, BLUE)[..60].to_vec(),
            // A cape's shape is not a skin's.
            encode(64, 96, &vec![0; 64 * 96 * 4]).unwrap(),
            encode(32, 32, &vec![0; 32 * 32 * 4]).unwrap(),
        ] {
            assert!(head(&junk).is_err());
        }
    }
}
