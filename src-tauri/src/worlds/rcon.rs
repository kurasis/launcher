//! RCON client for a server on 127.0.0.1 with a per-run password
//! (ADAPTATION A1): the trusted channel for a controlled `stop` of a server
//! that outlived the launcher core, whose stdin is gone.

use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::time::Duration;

use crate::secrets::SecretString;

const LOGIN: i32 = 3;
const COMMAND: i32 = 2;
/// The server sends at most 4096 bytes of body per packet.
const MAX_PACKET: usize = 4096 + 10;
/// Commands the launcher sends are short console commands.
pub const MAX_COMMAND_BYTES: usize = 256;
/// Servers before 1.16 start a client's thread before marking it running, so a
/// thread that runs first closes the connection unanswered (seen on 1.12.2).
/// The login is then tried again on a fresh connection, up to this many times.
const LOGIN_TRIES: usize = 3;

#[derive(Debug, PartialEq, Eq)]
pub enum RconError {
    Unreachable,
    Refused,
    Protocol,
    InvalidCommand,
}

pub struct Rcon {
    stream: TcpStream,
    next_id: i32,
}

impl Rcon {
    pub fn connect(
        port: u16,
        password: &SecretString,
        timeout: Duration,
    ) -> Result<Self, RconError> {
        let mut tries = 1;
        loop {
            match Self::log_in(port, password, timeout) {
                Err(Login::ClosedUnanswered) if tries < LOGIN_TRIES => tries += 1,
                Err(Login::ClosedUnanswered) => return Err(RconError::Unreachable),
                Err(Login::Failed(error)) => return Err(error),
                Ok(rcon) => return Ok(rcon),
            }
        }
    }

    fn log_in(port: u16, password: &SecretString, timeout: Duration) -> Result<Self, Login> {
        let address = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
        let stream =
            TcpStream::connect_timeout(&address, timeout).map_err(|_| RconError::Unreachable)?;
        stream
            .set_read_timeout(Some(timeout))
            .and_then(|()| stream.set_write_timeout(Some(timeout)))
            .map_err(|_| RconError::Unreachable)?;
        let mut rcon = Self { stream, next_id: 1 };
        let (id, packet) = rcon.packet(LOGIN, password.expose().as_bytes())?;
        rcon.stream.write_all(&packet).map_err(unanswered)?;
        let mut header = [0u8; 4];
        rcon.stream.read_exact(&mut header).map_err(unanswered)?;
        let (answer_id, kind, _) = rcon.receive_after(header)?;
        if answer_id == -1 {
            return Err(RconError::Refused.into());
        }
        if answer_id != id || kind != COMMAND {
            return Err(RconError::Protocol.into());
        }
        Ok(rcon)
    }

    /// Runs one console command: a single line of printable ASCII.
    pub fn command(&mut self, command: &str) -> Result<String, RconError> {
        if !valid_command(command) {
            return Err(RconError::InvalidCommand);
        }
        let id = self.send(COMMAND, command.as_bytes())?;
        let (answer_id, _, body) = self.receive()?;
        if answer_id != id {
            return Err(RconError::Protocol);
        }
        Ok(String::from_utf8_lossy(&body).into_owned())
    }

    fn send(&mut self, kind: i32, body: &[u8]) -> Result<i32, RconError> {
        let (id, packet) = self.packet(kind, body)?;
        self.stream
            .write_all(&packet)
            .map_err(|_| RconError::Unreachable)?;
        Ok(id)
    }

    /// The next request ID and its packet.
    fn packet(&mut self, kind: i32, body: &[u8]) -> Result<(i32, Vec<u8>), RconError> {
        let id = self.next_id;
        self.next_id += 1;
        let length = i32::try_from(body.len() + 10).map_err(|_| RconError::Protocol)?;
        let mut packet = Vec::with_capacity(body.len() + 14);
        packet.extend_from_slice(&length.to_le_bytes());
        packet.extend_from_slice(&id.to_le_bytes());
        packet.extend_from_slice(&kind.to_le_bytes());
        packet.extend_from_slice(body);
        packet.extend_from_slice(&[0, 0]);
        Ok((id, packet))
    }

    fn receive(&mut self) -> Result<(i32, i32, Vec<u8>), RconError> {
        let mut header = [0u8; 4];
        self.stream
            .read_exact(&mut header)
            .map_err(|_| RconError::Unreachable)?;
        self.receive_after(header)
    }

    /// The rest of a packet whose length is `header`.
    fn receive_after(&mut self, header: [u8; 4]) -> Result<(i32, i32, Vec<u8>), RconError> {
        let length =
            usize::try_from(i32::from_le_bytes(header)).map_err(|_| RconError::Protocol)?;
        if !(10..=MAX_PACKET).contains(&length) {
            return Err(RconError::Protocol);
        }
        let mut packet = vec![0u8; length];
        self.stream
            .read_exact(&mut packet)
            .map_err(|_| RconError::Unreachable)?;
        let id = i32::from_le_bytes([packet[0], packet[1], packet[2], packet[3]]);
        let kind = i32::from_le_bytes([packet[4], packet[5], packet[6], packet[7]]);
        let body = packet[8..length - 2].to_vec();
        Ok((id, kind, body))
    }
}

enum Login {
    /// The server closed the connection before answering the login.
    ClosedUnanswered,
    Failed(RconError),
}

impl From<RconError> for Login {
    fn from(error: RconError) -> Self {
        Self::Failed(error)
    }
}

/// A login cut off by the server closing or resetting the connection, or any
/// other failure to reach it.
fn unanswered(error: std::io::Error) -> Login {
    use std::io::ErrorKind::{BrokenPipe, ConnectionAborted, ConnectionReset, UnexpectedEof};
    match error.kind() {
        UnexpectedEof | ConnectionReset | ConnectionAborted | BrokenPipe => Login::ClosedUnanswered,
        _ => Login::Failed(RconError::Unreachable),
    }
}

/// A console command the launcher may send: one line of printable ASCII.
pub fn valid_command(command: &str) -> bool {
    !command.is_empty()
        && command.len() <= MAX_COMMAND_BYTES
        && command.bytes().all(|b| (b' '..=b'~').contains(&b))
}

/// A random per-run password: 32 hex characters from the OS generator.
pub fn generate_password() -> Option<SecretString> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).ok()?;
    Some(SecretString::new(hex::encode(bytes)))
}

#[cfg(test)]
mod tests {
    use std::net::TcpListener;

    use super::*;

    /// A fake server that checks the password and answers one command.
    fn serve(password: &'static str) -> u16 {
        serve_after_closing(0, password)
    }

    /// The same, after closing the first `unanswered` connections unread, as a
    /// server before 1.13 does when a client's thread runs too early.
    fn serve_after_closing(unanswered: usize, password: &'static str) -> u16 {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for _ in 0..unanswered {
                drop(listener.accept().unwrap());
            }
            let (mut stream, _) = listener.accept().unwrap();
            let read = |stream: &mut TcpStream| {
                let mut header = [0u8; 4];
                stream.read_exact(&mut header).unwrap();
                let mut packet = vec![0u8; i32::from_le_bytes(header) as usize];
                stream.read_exact(&mut packet).unwrap();
                let id = i32::from_le_bytes(packet[0..4].try_into().unwrap());
                (id, packet[8..packet.len() - 2].to_vec())
            };
            let write = |stream: &mut TcpStream, id: i32, kind: i32, body: &[u8]| {
                let mut out = Vec::new();
                out.extend_from_slice(&((body.len() + 10) as i32).to_le_bytes());
                out.extend_from_slice(&id.to_le_bytes());
                out.extend_from_slice(&kind.to_le_bytes());
                out.extend_from_slice(body);
                out.extend_from_slice(&[0, 0]);
                stream.write_all(&out).unwrap();
            };
            let (id, body) = read(&mut stream);
            if body != password.as_bytes() {
                write(&mut stream, -1, 2, b"");
                return;
            }
            write(&mut stream, id, 2, b"");
            let (id, body) = read(&mut stream);
            let answer = format!("ran {}", String::from_utf8(body).unwrap());
            write(&mut stream, id, 0, answer.as_bytes());
        });
        port
    }

    #[test]
    fn logs_in_and_runs_a_command() {
        let port = serve("secret");
        let mut rcon =
            Rcon::connect(port, &SecretString::new("secret"), Duration::from_secs(5)).unwrap();
        assert_eq!(rcon.command("list").unwrap(), "ran list");
    }

    #[test]
    fn a_wrong_password_is_refused() {
        let port = serve("secret");
        let result = Rcon::connect(port, &SecretString::new("guess"), Duration::from_secs(5));
        assert_eq!(result.err(), Some(RconError::Refused));
    }

    #[test]
    fn a_connection_closed_unanswered_is_tried_again() {
        let port = serve_after_closing(LOGIN_TRIES - 1, "secret");
        let mut rcon =
            Rcon::connect(port, &SecretString::new("secret"), Duration::from_secs(5)).unwrap();
        assert_eq!(rcon.command("list").unwrap(), "ran list");
        let port = serve_after_closing(LOGIN_TRIES - 1, "secret");
        let result = Rcon::connect(port, &SecretString::new("guess"), Duration::from_secs(5));
        assert_eq!(result.err(), Some(RconError::Refused));
    }

    #[test]
    fn a_server_that_always_closes_is_unreachable() {
        let port = serve_after_closing(LOGIN_TRIES, "secret");
        let result = Rcon::connect(port, &SecretString::new("secret"), Duration::from_secs(5));
        assert_eq!(result.err(), Some(RconError::Unreachable));
    }

    #[test]
    fn commands_are_single_ascii_lines() {
        for bad in ["", "stop\nop Notch", "say ü", &"x".repeat(257), "a\0"] {
            assert!(!valid_command(bad), "{bad:?}");
        }
        assert!(valid_command("whitelist add Notch"));
        let password = generate_password().unwrap();
        assert_eq!(password.expose().len(), 32);
        assert_ne!(password, generate_password().unwrap());
    }
}
