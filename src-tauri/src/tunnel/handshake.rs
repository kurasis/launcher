//! Classification of the first bytes a Minecraft client sends (ADAPTATION A3,
//! spec 07 "Local handshake ownership and bounds").
//!
//! The parser is incremental and bounded: it is called with everything read so
//! far and either asks for more, or classifies the connection as a status ping
//! or a login with the player's name, or rejects it. It never consumes bytes on
//! its own: the caller keeps the buffer and replays it unchanged, exactly once,
//! to the server after admission. Only the version of the world tuple is
//! accepted for a login, so the Login Start layout is known.

use crate::worlds::identity::Nick;
use crate::worlds::protocol::{decode_varint, read_string};

/// The handshake and Login Start together never exceed this.
pub const MAX_PREFIX: usize = 8 * 1024;
/// The game limits the server address in the handshake to 255 characters.
const MAX_ADDRESS_BYTES: usize = 255 * 4;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Classified {
    /// Read more bytes and call again.
    NeedMore,
    /// A server-list ping; `consumed` covers the handshake packet only.
    Status { consumed: usize },
    /// A login of `nick`; `consumed` covers the handshake and Login Start.
    Login { nick: Nick, consumed: usize },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reject {
    TooLarge,
    Malformed,
    /// The pre-1.7 ping starting with 0xFE.
    Legacy,
    /// A login from another game version than the world's.
    WrongVersion,
    /// Next state 3: a transfer from another server.
    Transfer,
    InvalidName,
}

/// A packet's id, body and length on the wire.
type Packet<'a> = (i32, &'a [u8], usize);

/// One length-prefixed packet at `bytes[..]`: `Ok(None)` if incomplete.
fn packet(bytes: &[u8]) -> Result<Option<Packet<'_>>, Reject> {
    let Some((length, used)) = decode_varint(bytes).map_err(|_| Reject::Malformed)? else {
        return Ok(None);
    };
    let length = usize::try_from(length).map_err(|_| Reject::Malformed)?;
    if length == 0 {
        return Err(Reject::Malformed);
    }
    if used + length > MAX_PREFIX {
        return Err(Reject::TooLarge);
    }
    let Some(payload) = bytes.get(used..used + length) else {
        return Ok(None);
    };
    let Some((id, id_len)) = decode_varint(payload).map_err(|_| Reject::Malformed)? else {
        return Err(Reject::Malformed);
    };
    Ok(Some((id, &payload[id_len..], used + length)))
}

/// Classifies the bytes read so far from a client connection.
pub fn classify(bytes: &[u8], protocol: i32) -> Result<Classified, Reject> {
    if bytes.first() == Some(&0xFE) {
        return Err(Reject::Legacy);
    }
    let result = classify_inner(bytes, protocol)?;
    if result == Classified::NeedMore && bytes.len() >= MAX_PREFIX {
        return Err(Reject::TooLarge);
    }
    Ok(result)
}

fn classify_inner(bytes: &[u8], protocol: i32) -> Result<Classified, Reject> {
    let Some((id, body, handshake_len)) = packet(bytes)? else {
        return Ok(Classified::NeedMore);
    };
    if id != 0x00 {
        return Err(Reject::Malformed);
    }
    let (client_protocol, mut at) = decode_varint(body)
        .map_err(|_| Reject::Malformed)?
        .ok_or(Reject::Malformed)?;
    let (_address, used) =
        read_string(&body[at..], MAX_ADDRESS_BYTES).map_err(|_| Reject::Malformed)?;
    at += used;
    if body.len() < at + 2 {
        return Err(Reject::Malformed);
    }
    at += 2;
    let (next, used) = decode_varint(&body[at..])
        .map_err(|_| Reject::Malformed)?
        .ok_or(Reject::Malformed)?;
    if at + used != body.len() {
        return Err(Reject::Malformed);
    }
    match next {
        1 => Ok(Classified::Status {
            consumed: handshake_len,
        }),
        2 => {
            if client_protocol != protocol {
                return Err(Reject::WrongVersion);
            }
            login_start(&bytes[handshake_len..]).map(|found| match found {
                None => Classified::NeedMore,
                Some((nick, len)) => Classified::Login {
                    nick,
                    consumed: handshake_len + len,
                },
            })
        }
        3 => Err(Reject::Transfer),
        _ => Err(Reject::Malformed),
    }
}

/// Login Start of protocol 767: name (at most 16 bytes) and the profile UUID.
fn login_start(bytes: &[u8]) -> Result<Option<(Nick, usize)>, Reject> {
    let Some((id, body, length)) = packet(bytes)? else {
        return Ok(None);
    };
    if id != 0x00 {
        return Err(Reject::Malformed);
    }
    let (name, used) = read_string(body, 16).map_err(|_| Reject::InvalidName)?;
    if body.len() != used + 16 {
        return Err(Reject::Malformed);
    }
    let nick = Nick::parse(&name).map_err(|_| Reject::InvalidName)?;
    Ok(Some((nick, length)))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::worlds::protocol::{write_packet, write_string, write_varint};

    pub fn handshake(protocol: i32, next: i32) -> Vec<u8> {
        let mut body = Vec::new();
        write_varint(&mut body, protocol);
        write_string(&mut body, "127.0.0.1");
        body.extend_from_slice(&25565u16.to_be_bytes());
        write_varint(&mut body, next);
        let mut wire = Vec::new();
        write_packet(&mut wire, 0, &body).unwrap();
        wire
    }

    pub fn login_start(name: &str) -> Vec<u8> {
        let mut body = Vec::new();
        write_string(&mut body, name);
        body.extend_from_slice(&[0x11; 16]);
        let mut wire = Vec::new();
        write_packet(&mut wire, 0, &body).unwrap();
        wire
    }

    fn nick(name: &str) -> Nick {
        Nick::parse(name).unwrap()
    }

    #[test]
    fn a_login_is_classified_at_every_split() {
        let mut wire = handshake(767, 2);
        let handshake_len = wire.len();
        wire.extend(login_start("Notch"));
        let login_len = wire.len();
        // Bytes the client sends right after Login Start stay with the caller.
        wire.extend_from_slice(&[0x01, 0x03]);
        for split in 0..login_len {
            assert_eq!(
                classify(&wire[..split], 767),
                Ok(Classified::NeedMore),
                "{split}"
            );
        }
        for end in login_len..=wire.len() {
            assert_eq!(
                classify(&wire[..end], 767),
                Ok(Classified::Login {
                    nick: nick("Notch"),
                    consumed: login_len
                })
            );
        }
        assert!(handshake_len < login_len);
    }

    #[test]
    fn a_status_ping_is_classified_by_its_handshake() {
        let mut wire = handshake(5, 1);
        let len = wire.len();
        wire.extend_from_slice(&[0x01, 0x00]);
        assert_eq!(
            classify(&wire, 767),
            Ok(Classified::Status { consumed: len }),
            "any client version may ask for status"
        );
    }

    #[test]
    fn junk_and_tricks_are_rejected() {
        assert_eq!(classify(&[0xFE, 0x01], 767), Err(Reject::Legacy));
        assert_eq!(classify(&handshake(767, 3), 767), Err(Reject::Transfer));
        assert_eq!(classify(&handshake(767, 7), 767), Err(Reject::Malformed));
        let mut other_version = handshake(766, 2);
        other_version.extend(login_start("Notch"));
        assert_eq!(classify(&other_version, 767), Err(Reject::WrongVersion));
        for name in ["ab", "with space", "seventeen_chars__"] {
            let mut wire = handshake(767, 2);
            wire.extend(login_start(name));
            assert!(classify(&wire, 767).is_err(), "{name}");
        }
        // A declared length beyond the bound is refused before it arrives.
        let mut huge = Vec::new();
        write_varint(&mut huge, 9000);
        assert_eq!(classify(&huge, 767), Err(Reject::TooLarge));
        assert_eq!(classify(&[0x00], 767), Err(Reject::Malformed));
        assert_eq!(classify(&[0x80; 6], 767), Err(Reject::Malformed));
        // A handshake with trailing bytes inside its packet.
        let mut body = Vec::new();
        write_varint(&mut body, 767);
        write_string(&mut body, "x");
        body.extend_from_slice(&[0, 0]);
        write_varint(&mut body, 2);
        body.push(0);
        let mut wire = Vec::new();
        write_packet(&mut wire, 0, &body).unwrap();
        assert_eq!(classify(&wire, 767), Err(Reject::Malformed));
        // The wrong packet id.
        let mut wire = Vec::new();
        write_packet(&mut wire, 1, &[0; 4]).unwrap();
        assert_eq!(classify(&wire, 767), Err(Reject::Malformed));
    }

    #[test]
    fn never_needs_more_than_the_bound() {
        // A login split into many packets cannot grow past MAX_PREFIX unseen.
        let mut wire = handshake(767, 2);
        let mut body = vec![0u8; 0];
        write_string(&mut body, "Notch");
        let mut partial = Vec::new();
        write_varint(&mut partial, 8000);
        partial.push(0);
        partial.extend(body);
        wire.extend(partial);
        wire.resize(MAX_PREFIX, 0);
        assert!(classify(&wire, 767).is_err());
    }
}
