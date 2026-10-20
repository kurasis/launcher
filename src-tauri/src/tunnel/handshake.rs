//! Classification of the first bytes a Minecraft client sends (ADAPTATION A3,
//! spec 07 "Local handshake ownership and bounds").
//!
//! The parser is incremental and bounded: it is called with everything read so
//! far and either asks for more, or classifies the connection as a status ping
//! or a login with the player's name, or rejects it. It never consumes bytes on
//! its own: the caller keeps the buffer and replays it unchanged, exactly once,
//! to the server after admission. A login is read in the Login Start layout of
//! the protocol its handshake names, for every release from 1.7.6 on, and in
//! whichever layout fits for a snapshot; the host also requires that protocol
//! to be its server's.

use crate::worlds::identity::Nick;
use crate::worlds::protocol::{decode_varint, read_string};

/// The handshake and Login Start together never exceed this.
pub const MAX_PREFIX: usize = 8 * 1024;
/// The game limits the server address in the handshake to 255 characters.
const MAX_ADDRESS_BYTES: usize = 255 * 4;
/// The protocol of 1.7.6, the oldest world version.
const OLDEST_PROTOCOL: i32 = 5;
/// Snapshot protocols have this bit set (from 1.16.4's snapshots); their
/// numbers say nothing about the layout.
const SNAPSHOT_BIT: i32 = 0x4000_0000;
/// 1.19's chat signing key in Login Start: an X.509 public key and Mojang's
/// signature of it (294 and 512 bytes in practice).
const MAX_PUBLIC_KEY: usize = 512;
const MAX_KEY_SIGNATURE: usize = 4096;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Classified {
    /// Read more bytes and call again.
    NeedMore,
    /// A server-list ping from a client of `protocol`; `consumed` covers the
    /// handshake packet only.
    Status { protocol: i32, consumed: usize },
    /// A login of `nick`; `consumed` covers the handshake and Login Start.
    Login { nick: Nick, consumed: usize },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reject {
    TooLarge,
    Malformed,
    /// The pre-1.7 ping starting with 0xFE.
    Legacy,
    /// A login from a version older than 1.7.6, or from another version than
    /// the server's.
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

/// Classifies the bytes read so far from a client connection. A login must
/// come from a client of 1.7.6 or later, of `expected` when it is given.
pub fn classify(bytes: &[u8], expected: Option<i32>) -> Result<Classified, Reject> {
    if bytes.first() == Some(&0xFE) {
        return Err(Reject::Legacy);
    }
    let result = classify_inner(bytes, expected)?;
    if result == Classified::NeedMore && bytes.len() >= MAX_PREFIX {
        return Err(Reject::TooLarge);
    }
    Ok(result)
}

fn classify_inner(bytes: &[u8], expected: Option<i32>) -> Result<Classified, Reject> {
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
            protocol: client_protocol,
            consumed: handshake_len,
        }),
        2 => {
            if client_protocol < OLDEST_PROTOCOL || expected.is_some_and(|p| p != client_protocol) {
                return Err(Reject::WrongVersion);
            }
            login_start(&bytes[handshake_len..], client_protocol).map(|found| match found {
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

/// Login Start: the name (at most 16 bytes), then what `protocol` adds:
/// - 5–758 (1.7.6–1.18.2): nothing;
/// - 759 (1.19): an optional signing key;
/// - 760 (1.19.1–1.19.2): an optional signing key and an optional profile UUID;
/// - 761–763 (1.19.3–1.20.1): an optional profile UUID;
/// - 764 and later (1.20.2 on): the profile UUID;
/// - a snapshot's (bit 30): whichever of these fits; the server, which reads
///   its own, gets the bytes unchanged.
fn login_start(bytes: &[u8], protocol: i32) -> Result<Option<(Nick, usize)>, Reject> {
    let Some((id, body, length)) = packet(bytes)? else {
        return Ok(None);
    };
    if id != 0x00 {
        return Err(Reject::Malformed);
    }
    let (name, used) = read_string(body, 16).map_err(|_| Reject::InvalidName)?;
    let rest = &body[used..];
    let fits = |layout: fn(&[u8]) -> Option<&[u8]>| layout(rest).is_some_and(<[u8]>::is_empty);
    let fitting = match protocol {
        ..=758 => fits(nothing),
        759 => fits(signing_key),
        760 => fits(|r| signing_key(r).and_then(optional_uuid)),
        761..=763 => fits(optional_uuid),
        p if p & SNAPSHOT_BIT != 0 => [
            nothing as fn(&[u8]) -> Option<&[u8]>,
            signing_key,
            |r| signing_key(r).and_then(optional_uuid),
            optional_uuid,
            uuid,
        ]
        .into_iter()
        .any(fits),
        _ => fits(uuid),
    };
    if !fitting {
        return Err(Reject::Malformed);
    }
    let nick = Nick::parse(&name).map_err(|_| Reject::InvalidName)?;
    Ok(Some((nick, length)))
}

/// A boolean byte: 0 or 1, nothing else.
fn flag(bytes: &[u8]) -> Option<(bool, &[u8])> {
    match bytes.split_first()? {
        (0, rest) => Some((false, rest)),
        (1, rest) => Some((true, rest)),
        _ => None,
    }
}

/// The name alone: everything follows it.
fn nothing(bytes: &[u8]) -> Option<&[u8]> {
    Some(bytes)
}

/// What follows the profile UUID.
fn uuid(bytes: &[u8]) -> Option<&[u8]> {
    bytes.get(16..)
}

/// What follows a present-or-absent profile UUID.
fn optional_uuid(bytes: &[u8]) -> Option<&[u8]> {
    let (present, rest) = flag(bytes)?;
    if present { rest.get(16..) } else { Some(rest) }
}

/// What follows a length-prefixed byte array of at most `max` bytes.
fn byte_array(bytes: &[u8], max: usize) -> Option<&[u8]> {
    let (length, used) = decode_varint(bytes).ok()??;
    let length = usize::try_from(length).ok().filter(|l| *l <= max)?;
    bytes.get(used + length..)
}

/// What follows 1.19's optional signing key: expiry, public key, signature.
fn signing_key(bytes: &[u8]) -> Option<&[u8]> {
    let (present, rest) = flag(bytes)?;
    if !present {
        return Some(rest);
    }
    let rest = byte_array(rest.get(8..)?, MAX_PUBLIC_KEY)?;
    byte_array(rest, MAX_KEY_SIGNATURE)
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
        login_start_with(name, &[0x11; 16])
    }

    /// Login Start with `after` following the name.
    fn login_start_with(name: &str, after: &[u8]) -> Vec<u8> {
        let mut body = Vec::new();
        write_string(&mut body, name);
        body.extend_from_slice(after);
        let mut wire = Vec::new();
        write_packet(&mut wire, 0, &body).unwrap();
        wire
    }

    fn login(protocol: i32, after: &[u8]) -> Result<Classified, Reject> {
        let mut wire = handshake(protocol, 2);
        wire.extend(login_start_with("Notch", after));
        classify(&wire, None)
    }

    /// 1.19's signing key: expiry, then key and signature with their lengths.
    fn signing_key(key: usize, signature: usize) -> Vec<u8> {
        let mut out = vec![1];
        out.extend_from_slice(&1_900_000_000_000i64.to_be_bytes());
        for len in [key, signature] {
            write_varint(&mut out, i32::try_from(len).unwrap());
            out.extend(std::iter::repeat_n(0x5a, len));
        }
        out
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
                classify(&wire[..split], Some(767)),
                Ok(Classified::NeedMore),
                "{split}"
            );
        }
        for end in login_len..=wire.len() {
            assert_eq!(
                classify(&wire[..end], Some(767)),
                Ok(Classified::Login {
                    nick: nick("Notch"),
                    consumed: login_len
                })
            );
        }
        assert!(handshake_len < login_len);
    }

    #[test]
    fn every_login_start_layout_is_read() {
        let uuid = [0x11u8; 16];
        let with_uuid = [&[1u8][..], &uuid].concat();
        let key = signing_key(294, 512);
        let cases: Vec<(i32, Vec<u8>)> = vec![
            // 1.7.6, 1.8.9, 1.12.2, 1.16.5, 1.18.2: the name alone.
            (5, vec![]),
            (47, vec![]),
            (340, vec![]),
            (754, vec![]),
            (758, vec![]),
            // 1.19: with or without a signing key.
            (759, vec![0]),
            (759, key.clone()),
            // 1.19.2: signing key and UUID, each optional.
            (760, vec![0, 0]),
            (760, [&[0u8][..], &with_uuid].concat()),
            (760, [&key[..], &[0]].concat()),
            (760, [&key[..], &with_uuid].concat()),
            // 1.19.3 to 1.20.1: an optional UUID.
            (761, vec![0]),
            (763, with_uuid.clone()),
            // 1.20.2 on: the UUID.
            (764, uuid.to_vec()),
            (767, uuid.to_vec()),
            (775, uuid.to_vec()),
        ];
        for (protocol, after) in cases {
            assert!(
                matches!(login(protocol, &after), Ok(Classified::Login { nick, .. }) if nick.as_str() == "Notch"),
                "{protocol} {after:?}"
            );
        }
    }

    #[test]
    fn a_login_start_of_another_layout_is_refused() {
        let uuid = [0x11u8; 16];
        let cases: Vec<(i32, Vec<u8>)> = vec![
            // Bytes the layout does not have.
            (5, vec![0]),
            (758, uuid.to_vec()),
            (759, vec![]),
            (759, vec![0, 0]),
            (760, vec![0]),
            (761, vec![]),
            (761, uuid.to_vec()),
            (763, [&[1u8][..], &uuid[..15]].concat()),
            (764, vec![]),
            (764, [&[0u8][..], &uuid].concat()),
            // A flag must be 0 or 1.
            (759, vec![2]),
            (761, [&[7u8][..], &uuid].concat()),
            // A key or signature over its bound, or longer than the packet.
            (759, signing_key(513, 512)),
            (759, signing_key(294, 4097)),
            (759, signing_key(294, 512)[..300].to_vec()),
            (759, {
                let mut key = vec![1];
                key.extend_from_slice(&[0; 8]);
                write_varint(&mut key, -1);
                key
            }),
        ];
        for (protocol, after) in cases {
            assert_eq!(
                login(protocol, &after),
                Err(Reject::Malformed),
                "{protocol} {after:?}"
            );
        }
    }

    #[test]
    fn only_clients_from_1_7_6_may_log_in() {
        for protocol in [0, 4, -1, i32::MIN] {
            assert_eq!(
                login(protocol, &[]),
                Err(Reject::WrongVersion),
                "{protocol}"
            );
        }
        // The host names its server's protocol.
        let mut wire = handshake(5, 2);
        wire.extend(login_start_with("Notch", &[]));
        assert!(classify(&wire, Some(5)).is_ok());
        assert_eq!(classify(&wire, Some(47)), Err(Reject::WrongVersion));
    }

    #[test]
    fn a_snapshot_login_is_read_in_whichever_layout_fits() {
        let uuid = [0x11u8; 16];
        let with_uuid = [&[1u8][..], &uuid].concat();
        let key = signing_key(294, 512);
        for protocol in [SNAPSHOT_BIT + 1, SNAPSHOT_BIT + 250, i32::MAX] {
            for after in [
                vec![],
                vec![0],
                key.clone(),
                vec![0, 0],
                [&key[..], &with_uuid].concat(),
                with_uuid.clone(),
                uuid.to_vec(),
            ] {
                assert!(
                    matches!(login(protocol, &after), Ok(Classified::Login { .. })),
                    "{protocol} {after:?}"
                );
            }
            for after in [
                vec![2],
                vec![0, 0, 0],
                uuid[..15].to_vec(),
                [&uuid[..], &[0]].concat(),
                signing_key(513, 512),
            ] {
                assert_eq!(
                    login(protocol, &after),
                    Err(Reject::Malformed),
                    "{protocol} {after:?}"
                );
            }
        }
    }

    #[test]
    fn a_status_ping_is_classified_by_its_handshake() {
        let mut wire = handshake(5, 1);
        let len = wire.len();
        wire.extend_from_slice(&[0x01, 0x00]);
        assert_eq!(
            classify(&wire, Some(767)),
            Ok(Classified::Status {
                protocol: 5,
                consumed: len
            }),
            "any client version may ask for status"
        );
    }

    #[test]
    fn junk_and_tricks_are_rejected() {
        assert_eq!(classify(&[0xFE, 0x01], Some(767)), Err(Reject::Legacy));
        assert_eq!(
            classify(&handshake(767, 3), Some(767)),
            Err(Reject::Transfer)
        );
        assert_eq!(
            classify(&handshake(767, 7), Some(767)),
            Err(Reject::Malformed)
        );
        let mut other_version = handshake(766, 2);
        other_version.extend(login_start("Notch"));
        assert_eq!(
            classify(&other_version, Some(767)),
            Err(Reject::WrongVersion)
        );
        for name in ["ab", "with space", "seventeen_chars__"] {
            let mut wire = handshake(767, 2);
            wire.extend(login_start(name));
            assert!(classify(&wire, Some(767)).is_err(), "{name}");
        }
        // A declared length beyond the bound is refused before it arrives.
        let mut huge = Vec::new();
        write_varint(&mut huge, 9000);
        assert_eq!(classify(&huge, Some(767)), Err(Reject::TooLarge));
        assert_eq!(classify(&[0x00], Some(767)), Err(Reject::Malformed));
        assert_eq!(classify(&[0x80; 6], Some(767)), Err(Reject::Malformed));
        // A handshake with trailing bytes inside its packet.
        let mut body = Vec::new();
        write_varint(&mut body, 767);
        write_string(&mut body, "x");
        body.extend_from_slice(&[0, 0]);
        write_varint(&mut body, 2);
        body.push(0);
        let mut wire = Vec::new();
        write_packet(&mut wire, 0, &body).unwrap();
        assert_eq!(classify(&wire, Some(767)), Err(Reject::Malformed));
        // The wrong packet id.
        let mut wire = Vec::new();
        write_packet(&mut wire, 1, &[0; 4]).unwrap();
        assert_eq!(classify(&wire, Some(767)), Err(Reject::Malformed));
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
        assert!(classify(&wire, Some(767)).is_err());
    }
}
