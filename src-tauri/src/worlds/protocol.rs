//! The few pieces of the Minecraft Java protocol the launcher speaks itself:
//! VarInts, strings and length-prefixed packets, all bounded.

use std::io::{self, Read, Write};

/// A VarInt is at most five bytes.
pub const MAX_VARINT_BYTES: usize = 5;

pub fn write_varint(out: &mut Vec<u8>, value: i32) {
    #[allow(clippy::cast_sign_loss)] // VarInts encode the two's complement bits
    let mut value = value as u32;
    loop {
        let byte = (value & 0x7f) as u8;
        value >>= 7;
        if value == 0 {
            out.push(byte);
            return;
        }
        out.push(byte | 0x80);
    }
}

/// Decodes a VarInt from the start of `bytes`: `Ok(None)` when more bytes are
/// needed, an error when it is longer than five bytes.
pub fn decode_varint(bytes: &[u8]) -> io::Result<Option<(i32, usize)>> {
    let mut value: u32 = 0;
    for (index, byte) in bytes.iter().take(MAX_VARINT_BYTES).enumerate() {
        value |= u32::from(byte & 0x7f) << (7 * index);
        if byte & 0x80 == 0 {
            #[allow(clippy::cast_possible_wrap)]
            return Ok(Some((value as i32, index + 1)));
        }
    }
    if bytes.len() >= MAX_VARINT_BYTES {
        Err(invalid("VarInt too long"))
    } else {
        Ok(None)
    }
}

pub fn read_varint(reader: &mut impl Read) -> io::Result<i32> {
    let mut bytes = Vec::with_capacity(MAX_VARINT_BYTES);
    loop {
        let mut byte = [0u8];
        reader.read_exact(&mut byte)?;
        bytes.push(byte[0]);
        if let Some((value, _)) = decode_varint(&bytes)? {
            return Ok(value);
        }
    }
}

pub fn write_string(out: &mut Vec<u8>, text: &str) {
    write_varint(out, i32::try_from(text.len()).unwrap_or(i32::MAX));
    out.extend_from_slice(text.as_bytes());
}

/// Writes one uncompressed packet: length, id, body.
pub fn write_packet(writer: &mut impl Write, id: i32, body: &[u8]) -> io::Result<()> {
    let mut payload = Vec::with_capacity(body.len() + MAX_VARINT_BYTES);
    write_varint(&mut payload, id);
    payload.extend_from_slice(body);
    let mut frame = Vec::with_capacity(payload.len() + MAX_VARINT_BYTES);
    write_varint(
        &mut frame,
        i32::try_from(payload.len()).map_err(|_| invalid("packet too large"))?,
    );
    frame.extend_from_slice(&payload);
    writer.write_all(&frame)
}

/// Reads one uncompressed packet of at most `max` bytes: `(id, body)`.
pub fn read_packet(reader: &mut impl Read, max: usize) -> io::Result<(i32, Vec<u8>)> {
    let length = usize::try_from(read_varint(reader)?).map_err(|_| invalid("negative length"))?;
    if length == 0 || length > max {
        return Err(invalid("packet length out of bounds"));
    }
    let mut payload = vec![0u8; length];
    reader.read_exact(&mut payload)?;
    let (id, used) = decode_varint(&payload)?.ok_or_else(|| invalid("truncated packet id"))?;
    Ok((id, payload.split_off(used)))
}

/// Reads a protocol string of at most `max` bytes from the start of `bytes`.
pub fn read_string(bytes: &[u8], max: usize) -> io::Result<(String, usize)> {
    let (length, used) = decode_varint(bytes)?.ok_or_else(|| invalid("truncated string"))?;
    let length = usize::try_from(length).map_err(|_| invalid("negative length"))?;
    if length > max {
        return Err(invalid("string too long"));
    }
    let text = bytes
        .get(used..used + length)
        .ok_or_else(|| invalid("truncated string"))?;
    let text = std::str::from_utf8(text).map_err(|_| invalid("string not UTF-8"))?;
    Ok((text.to_owned(), used + length))
}

pub fn invalid(what: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, what)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn varints_round_trip() {
        for value in [
            0,
            1,
            127,
            128,
            255,
            25_565,
            2_097_151,
            i32::MAX,
            -1,
            i32::MIN,
        ] {
            let mut bytes = Vec::new();
            write_varint(&mut bytes, value);
            assert!(bytes.len() <= MAX_VARINT_BYTES);
            assert_eq!(decode_varint(&bytes).unwrap(), Some((value, bytes.len())));
            assert_eq!(read_varint(&mut &bytes[..]).unwrap(), value);
            if bytes.len() > 1 {
                assert_eq!(decode_varint(&bytes[..bytes.len() - 1]).unwrap(), None);
            }
        }
        let mut bytes = Vec::new();
        write_varint(&mut bytes, 767);
        assert_eq!(bytes, [0xff, 0x05]);
    }

    #[test]
    fn overlong_and_oversized_input_is_refused() {
        assert!(decode_varint(&[0x80; 6]).is_err());
        assert!(read_varint(&mut &[0x80u8; 10][..]).is_err());
        let mut frame = Vec::new();
        write_varint(&mut frame, 1_000_000);
        assert!(read_packet(&mut &frame[..], 64 * 1024).is_err());
        assert!(read_packet(&mut &[0u8][..], 16).is_err(), "empty packet");
        let mut long = Vec::new();
        write_string(&mut long, &"x".repeat(100));
        assert!(read_string(&long, 99).is_err());
        assert_eq!(read_string(&long, 100).unwrap().0.len(), 100);
        assert!(read_string(&[5, b'a'], 10).is_err(), "truncated");
        assert!(read_string(&[2, 0xff, 0xfe], 10).is_err(), "not UTF-8");
    }

    #[test]
    fn packets_round_trip() {
        let mut wire = Vec::new();
        write_packet(&mut wire, 0x00, b"hello").unwrap();
        let (id, body) = read_packet(&mut &wire[..], 64).unwrap();
        assert_eq!((id, body.as_slice()), (0, &b"hello"[..]));
    }
}
