//! Server List Ping, the status the game's server list shows: version,
//! players and icon. Every length is bounded here (no ping library we found
//! bounded what a server may send), and only public addresses are asked:
//! never this machine, a private network or anything special, before or
//! after DNS.

use std::net::{IpAddr, SocketAddr};
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use crate::text;

/// The whole ping, DNS included, takes at most this long.
pub const TIMEOUT: Duration = Duration::from_secs(8);
/// A status answer with a large icon fits; more is refused.
pub const MAX_PACKET: usize = 256 * 1024;
/// The protocol we say we speak (1.21.1); servers answer status anyway.
const PROTOCOL: i32 = 767;
/// The player name the login check gives: a valid one, and it says what it is.
pub const CHECK_NAME: &str = "ServerListCheck";
/// The UUID it gives where the protocol wants one (1.20.2 and later).
const CHECK_UUID: [u8; 16] = *b"ServerListCheck!";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    pub version: Option<String>,
    pub protocol: Option<i64>,
    pub online: Option<i64>,
    pub max: Option<i64>,
    pub favicon: Option<String>,
}

/// Whether an address is on the public internet.
pub fn public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let [a, b, c, _] = v4.octets();
            !(v4.is_unspecified()
                || v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_broadcast()
                || v4.is_multicast()
                || v4.is_documentation()
                || a == 0
                || a >= 240
                // Shared address space (carrier NAT), benchmarking, IETF.
                || (a == 100 && (64..128).contains(&b))
                || (a == 198 && (b == 18 || b == 19))
                || (a == 192 && b == 0 && c == 0))
        }
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return public(IpAddr::V4(v4));
            }
            let first = v6.segments()[0];
            !(v6.is_unspecified()
                || v6.is_loopback()
                || v6.is_multicast()
                // Unique local, link local, documentation, NAT64, 6to4.
                || (first & 0xfe00) == 0xfc00
                || (first & 0xffc0) == 0xfe80
                || (first == 0x2001 && v6.segments()[1] == 0x0db8)
                || first == 0x0064
                || first == 0x2002)
        }
    }
}

pub struct Pinger {
    resolver: Option<hickory_resolver::TokioResolver>,
    /// Tests ping a server on this machine.
    pub allow_local: bool,
}

impl Pinger {
    pub fn new() -> Self {
        let resolver = hickory_resolver::TokioResolver::builder_tokio()
            .map(|b| b.build())
            .map_err(|e| eprintln!("ping: no DNS resolver for SRV records: {e}"))
            .ok()
            .and_then(Result::ok);
        Self {
            resolver,
            allow_local: false,
        }
    }

    /// The server's own name and port from its SRV record, if it has one.
    async fn srv(&self, host: &str) -> Option<(String, u16)> {
        use hickory_resolver::proto::rr::RData;
        let resolver = self.resolver.as_ref()?;
        let lookup = resolver
            .srv_lookup(format!("_minecraft._tcp.{host}."))
            .await
            .ok()?;
        let mut records: Vec<_> = lookup
            .answers()
            .iter()
            .filter_map(|record| match &record.data {
                RData::SRV(srv) => Some(srv.clone()),
                _ => None,
            })
            .collect();
        records.sort_by_key(|srv| (srv.priority, std::cmp::Reverse(srv.weight)));
        let srv = records.into_iter().next()?;
        let target = srv.target.to_ascii();
        let target = target.trim_end_matches('.').to_ascii_lowercase();
        (text::valid_host(&target) && srv.port != 0).then_some((target, srv.port))
    }

    /// The addresses to connect to, and the name and port to say in the
    /// handshake.
    async fn resolve(&self, address: &str) -> Result<(Vec<SocketAddr>, String, u16), String> {
        let (host, port) =
            text::parse_address(address).ok_or_else(|| "not a server address".to_owned())?;
        let bare = host.trim_start_matches('[').trim_end_matches(']');
        if let Ok(ip) = bare.parse::<IpAddr>() {
            return Ok((vec![SocketAddr::new(ip, port)], bare.to_owned(), port));
        }
        // The game looks for an SRV record when no port is given.
        let (target, port) = if port == 25565 {
            self.srv(&host).await.unwrap_or((host.clone(), port))
        } else {
            (host.clone(), port)
        };
        let found: Vec<SocketAddr> = tokio::net::lookup_host((target.as_str(), port))
            .await
            .map_err(|_| "the name does not resolve".to_owned())?
            .take(8)
            .collect();
        if found.is_empty() {
            return Err("the name does not resolve".to_owned());
        }
        Ok((found, host, port))
    }

    pub async fn ping(&self, address: &str) -> Result<Status, String> {
        tokio::time::timeout(TIMEOUT, self.ping_inner(address))
            .await
            .map_err(|_| "the server did not answer in time".to_owned())?
    }

    /// A connection to the server's first public address, and the name and
    /// port to say in the handshake.
    async fn connect(&self, address: &str) -> Result<(TcpStream, String, u16), String> {
        let (addresses, host, port) = self.resolve(address).await?;
        let allowed: Vec<SocketAddr> = addresses
            .into_iter()
            .filter(|a| self.allow_local || public(a.ip()))
            .collect();
        let target = allowed
            .first()
            .ok_or_else(|| "the address is not on the public internet".to_owned())?;
        let stream = TcpStream::connect(target)
            .await
            .map_err(|_| "the server does not accept connections".to_owned())?;
        Ok((stream, host, port))
    }

    async fn ping_inner(&self, address: &str) -> Result<Status, String> {
        let (mut stream, host, port) = self.connect(address).await?;
        let mut wire = handshake(PROTOCOL, &host, port, 1);
        // The status request: length 1, id 0.
        wire.extend_from_slice(&[1, 0]);
        stream
            .write_all(&wire)
            .await
            .map_err(|_| "the server closed the connection".to_owned())?;
        let body = read_packet(&mut stream).await?;
        let json = read_string(&body)?;
        parse(&json)
    }

    /// Whether the server lets in only players signed in with Microsoft
    /// (online mode): it then asks for encryption before anything else.
    /// `Some(false)` when it lets the check's name in without, `None` when
    /// it answers neither (a kick: a whitelist, a version it refuses).
    /// The check speaks the protocol the server's status named and leaves
    /// as soon as the server answers; an offline-mode server may show the
    /// name joining for that moment.
    pub async fn licensed(
        &self,
        address: &str,
        protocol: Option<i64>,
    ) -> Result<Option<bool>, String> {
        tokio::time::timeout(TIMEOUT, self.licensed_inner(address, protocol))
            .await
            .map_err(|_| "the server did not answer in time".to_owned())?
    }

    async fn licensed_inner(
        &self,
        address: &str,
        protocol: Option<i64>,
    ) -> Result<Option<bool>, String> {
        let protocol = protocol
            .and_then(|p| i32::try_from(p).ok())
            .filter(|p| *p > 0)
            .unwrap_or(PROTOCOL);
        let (mut stream, host, port) = self.connect(address).await?;
        let mut wire = handshake(protocol, &host, port, 2);
        wire.extend_from_slice(&login_start(protocol));
        stream
            .write_all(&wire)
            .await
            .map_err(|_| "the server closed the connection".to_owned())?;
        let (id, _) = read_frame(&mut stream).await?;
        Ok(match id {
            // Encryption request: a Microsoft sign-in is needed.
            0x01 => Some(true),
            // Login success, or compression before it: in without one.
            0x02 | 0x03 => Some(false),
            _ => None,
        })
    }
}

/// A handshake packet, framed; `next` is 1 for status, 2 for login.
fn handshake(protocol: i32, host: &str, port: u16, next: i32) -> Vec<u8> {
    let mut body = Vec::with_capacity(300);
    write_varint(&mut body, 0x00);
    write_varint(&mut body, protocol);
    write_string(&mut body, host);
    body.extend_from_slice(&port.to_be_bytes());
    write_varint(&mut body, next);
    frame(body)
}

/// Login Start with the check's name, in the shape `protocol` wants.
pub fn login_start(protocol: i32) -> Vec<u8> {
    let mut body = Vec::with_capacity(40);
    write_varint(&mut body, 0x00);
    write_string(&mut body, CHECK_NAME);
    match protocol {
        // 1.19: no signature data.
        759 => body.push(0),
        // 1.19.1 and 1.19.2: no signature data, no UUID.
        760 => body.extend_from_slice(&[0, 0]),
        // 1.19.3 to 1.20.1: no UUID.
        761..=763 => body.push(0),
        // 1.20.2 and later: a UUID, always.
        764.. => body.extend_from_slice(&CHECK_UUID),
        // Older: the name alone.
        _ => {}
    }
    frame(body)
}

fn frame(body: Vec<u8>) -> Vec<u8> {
    let mut wire = Vec::with_capacity(body.len() + 5);
    write_varint(&mut wire, body.len() as i32);
    wire.extend_from_slice(&body);
    wire
}

pub fn write_varint(out: &mut Vec<u8>, value: i32) {
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

pub fn write_string(out: &mut Vec<u8>, text: &str) {
    write_varint(out, text.len() as i32);
    out.extend_from_slice(text.as_bytes());
}

async fn read_varint(stream: &mut (impl AsyncReadExt + Unpin)) -> Result<i32, String> {
    let mut value: u32 = 0;
    for index in 0..5 {
        let byte = stream
            .read_u8()
            .await
            .map_err(|_| "the server's answer ended early".to_owned())?;
        value |= u32::from(byte & 0x7f) << (7 * index);
        if byte & 0x80 == 0 {
            return Ok(value as i32);
        }
    }
    Err("the server's answer is not the game's protocol".to_owned())
}

fn decode_varint(bytes: &[u8]) -> Option<(i32, usize)> {
    let mut value: u32 = 0;
    for (index, byte) in bytes.iter().take(5).enumerate() {
        value |= u32::from(byte & 0x7f) << (7 * index);
        if byte & 0x80 == 0 {
            return Some((value as i32, index + 1));
        }
    }
    None
}

/// A packet's id and the rest of it, at most `MAX_PACKET` bytes.
pub async fn read_frame(
    stream: &mut (impl AsyncReadExt + Unpin),
) -> Result<(i32, Vec<u8>), String> {
    let length = read_varint(stream).await?;
    let length = usize::try_from(length).map_err(|_| "a negative length".to_owned())?;
    if length == 0 || length > MAX_PACKET {
        return Err("the server's answer is too large or empty".to_owned());
    }
    let mut payload = vec![0u8; length];
    stream
        .read_exact(&mut payload)
        .await
        .map_err(|_| "the server's answer ended early".to_owned())?;
    let (id, used) = decode_varint(&payload)
        .ok_or_else(|| "the server's answer is not the game's protocol".to_owned())?;
    Ok((id, payload.split_off(used)))
}

/// The body of the status packet (id 0), at most `MAX_PACKET` bytes.
pub async fn read_packet(stream: &mut (impl AsyncReadExt + Unpin)) -> Result<Vec<u8>, String> {
    match read_frame(stream).await? {
        (0, body) => Ok(body),
        _ => Err("the server answered something else than its status".to_owned()),
    }
}

pub fn read_string(bytes: &[u8]) -> Result<String, String> {
    let (length, used) = decode_varint(bytes).ok_or_else(|| "a truncated status".to_owned())?;
    let length = usize::try_from(length).map_err(|_| "a negative length".to_owned())?;
    let text = bytes
        .get(used..used.saturating_add(length))
        .ok_or_else(|| "a truncated status".to_owned())?;
    String::from_utf8(text.to_vec()).map_err(|_| "the status is not UTF-8".to_owned())
}

/// The status JSON. Numbers out of reason are left out.
pub fn parse(json: &str) -> Result<Status, String> {
    let value: serde_json::Value =
        serde_json::from_str(json).map_err(|_| "the status is not JSON".to_owned())?;
    let count = |pointer: &str| {
        value
            .pointer(pointer)
            .and_then(serde_json::Value::as_i64)
            .filter(|n| (0..=10_000_000).contains(n))
    };
    let version = value
        .pointer("/version/name")
        .and_then(serde_json::Value::as_str)
        .map(|v| {
            v.chars()
                .filter(|c| !c.is_control())
                .take(80)
                .collect::<String>()
        });
    Ok(Status {
        version,
        protocol: value
            .pointer("/version/protocol")
            .and_then(serde_json::Value::as_i64),
        online: count("/players/online"),
        max: count("/players/max"),
        favicon: value
            .get("favicon")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned),
    })
}
