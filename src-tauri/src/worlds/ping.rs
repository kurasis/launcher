//! Local status ping (Server List Ping) to confirm a started server answers on
//! its loopback port (PLAN W3 readiness).

use std::io::Write;
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::time::Duration;

use super::protocol::{read_packet, read_string, write_packet, write_string, write_varint};

/// A status response is one JSON string; the game caps it at 32767 characters.
const MAX_STATUS_BYTES: usize = 4 * 32_767 + 8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    pub protocol: i64,
    pub online: i64,
    pub max: i64,
}

/// Asks the server on 127.0.0.1:`port` for its status. Any failure, including
/// a malformed answer, is `None`.
pub fn status(port: u16, protocol: i32, timeout: Duration) -> Option<Status> {
    let address = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let mut stream = TcpStream::connect_timeout(&address, timeout).ok()?;
    stream.set_read_timeout(Some(timeout)).ok()?;
    stream.set_write_timeout(Some(timeout)).ok()?;
    let mut handshake = Vec::with_capacity(32);
    write_varint(&mut handshake, protocol);
    write_string(&mut handshake, "127.0.0.1");
    handshake.extend_from_slice(&port.to_be_bytes());
    write_varint(&mut handshake, 1);
    write_packet(&mut stream, 0x00, &handshake).ok()?;
    write_packet(&mut stream, 0x00, &[]).ok()?;
    stream.flush().ok()?;
    let (id, body) = read_packet(&mut stream, MAX_STATUS_BYTES).ok()?;
    if id != 0x00 {
        return None;
    }
    let (json, _) = read_string(&body, MAX_STATUS_BYTES).ok()?;
    parse(&json)
}

fn parse(json: &str) -> Option<Status> {
    let value: serde_json::Value = serde_json::from_str(json).ok()?;
    Some(Status {
        protocol: value.pointer("/version/protocol")?.as_i64()?,
        online: value.pointer("/players/online")?.as_i64()?,
        max: value.pointer("/players/max")?.as_i64()?,
    })
}

#[cfg(test)]
mod tests {
    use std::net::TcpListener;

    use super::*;
    use crate::worlds::protocol::read_varint;

    fn serve(answer: Vec<u8>) -> u16 {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let (id, body) = read_packet(&mut stream, 1024).unwrap();
            assert_eq!(id, 0);
            assert_eq!(read_varint(&mut &body[..]).unwrap(), 767);
            assert_eq!(*body.last().unwrap(), 1, "next state: status");
            let (id, body) = read_packet(&mut stream, 1024).unwrap();
            assert_eq!((id, body.len()), (0, 0));
            stream.write_all(&answer).unwrap();
        });
        port
    }

    fn response(json: &str) -> Vec<u8> {
        let mut body = Vec::new();
        write_string(&mut body, json);
        let mut wire = Vec::new();
        write_packet(&mut wire, 0, &body).unwrap();
        wire
    }

    #[test]
    fn reads_a_status() {
        let json = r#"{"version":{"name":"1.21.1","protocol":767},"players":{"max":8,"online":0},"description":"x"}"#;
        let port = serve(response(json));
        assert_eq!(
            status(port, 767, Duration::from_secs(5)),
            Some(Status {
                protocol: 767,
                online: 0,
                max: 8
            })
        );
    }

    #[test]
    fn junk_is_not_a_status() {
        let port = serve(response("not json"));
        assert_eq!(status(port, 767, Duration::from_secs(5)), None);
        let port = serve(vec![0xff, 0xff, 0xff, 0xff, 0x0f]);
        assert_eq!(status(port, 767, Duration::from_secs(5)), None);
        let free = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let closed = free.local_addr().unwrap().port();
        drop(free);
        assert_eq!(status(closed, 767, Duration::from_millis(500)), None);
    }
}
