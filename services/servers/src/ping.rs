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

    async fn ping_inner(&self, address: &str) -> Result<Status, String> {
        let (addresses, host, port) = self.resolve(address).await?;
        let allowed: Vec<SocketAddr> = addresses
            .into_iter()
            .filter(|a| self.allow_local || public(a.ip()))
            .collect();
        let target = allowed
            .first()
            .ok_or_else(|| "the address is not on the public internet".to_owned())?;
        let mut stream = TcpStream::connect(target)
            .await
            .map_err(|_| "the server does not accept connections".to_owned())?;
        let mut handshake = Vec::with_capacity(300);
        write_varint(&mut handshake, 0x00);
        write_varint(&mut handshake, PROTOCOL);
        write_string(&mut handshake, &host);
        handshake.extend_from_slice(&port.to_be_bytes());
        write_varint(&mut handshake, 1);
        let mut wire = Vec::with_capacity(320);
        write_varint(&mut wire, handshake.len() as i32);
        wire.extend_from_slice(&handshake);
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

/// The body of the status packet (id 0), at most `MAX_PACKET` bytes.
pub async fn read_packet(stream: &mut (impl AsyncReadExt + Unpin)) -> Result<Vec<u8>, String> {
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
    match decode_varint(&payload) {
        Some((0, used)) => Ok(payload.split_off(used)),
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
