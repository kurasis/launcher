//! The `wwf-tunnel/1` admission frames (spec 07 "Admission protocol").
//!
//! A 4-byte unsigned big-endian length, then UTF-8 JSON of at most 16 KiB.
//! Unknown fields, duplicate keys, other versions and other services than
//! `gameplay` are refused.

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

pub const ALPN: &[u8] = b"wwf-tunnel/1";
pub const PROTOCOL_VERSION: u32 = 1;
pub const MAX_FRAME: usize = 16 * 1024;
pub const MAX_TICKET: usize = 8 * 1024;
const MAX_ID: usize = 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Service {
    Gameplay,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AdmissionRequest {
    pub protocol_version: u32,
    pub request_id: String,
    pub ticket: String,
    pub session_id: String,
    pub expected_host_epoch: String,
    pub service: Service,
}

/// Why the host refused a stream; the guest shows a message for each.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Refusal {
    BadFrame,
    TicketInvalid,
    NotMember,
    AlreadyConnected,
    ConnectionClosing,
    WrongSession,
    RoomFull,
    HostBusy,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AdmissionResponse {
    pub protocol_version: u32,
    pub request_id: String,
    pub accepted: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refusal: Option<Refusal>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum FrameError {
    Io,
    Length,
    Json,
    Version,
    Field,
}

fn bounded_id(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= MAX_ID
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.')
}

impl AdmissionRequest {
    pub fn validate(&self) -> Result<(), FrameError> {
        if self.protocol_version != PROTOCOL_VERSION {
            return Err(FrameError::Version);
        }
        let ticket_ok = !self.ticket.is_empty()
            && self.ticket.len() <= MAX_TICKET
            && self.ticket.bytes().all(|b| b.is_ascii_graphic());
        if !ticket_ok
            || !bounded_id(&self.request_id)
            || !bounded_id(&self.session_id)
            || !bounded_id(&self.expected_host_epoch)
        {
            return Err(FrameError::Field);
        }
        Ok(())
    }
}

pub async fn write_frame<T: Serialize>(
    writer: &mut (impl AsyncWrite + Unpin),
    value: &T,
) -> Result<(), FrameError> {
    let json = serde_json::to_vec(value).map_err(|_| FrameError::Json)?;
    if json.len() > MAX_FRAME {
        return Err(FrameError::Length);
    }
    let length = u32::try_from(json.len()).map_err(|_| FrameError::Length)?;
    let mut frame = Vec::with_capacity(json.len() + 4);
    frame.extend_from_slice(&length.to_be_bytes());
    frame.extend_from_slice(&json);
    writer.write_all(&frame).await.map_err(|_| FrameError::Io)?;
    writer.flush().await.map_err(|_| FrameError::Io)
}

/// Reads one frame. Serde's derived structs refuse a repeated key, and
/// `deny_unknown_fields` any other.
pub async fn read_frame<T: for<'de> Deserialize<'de>>(
    reader: &mut (impl AsyncRead + Unpin),
) -> Result<T, FrameError> {
    let mut header = [0u8; 4];
    reader
        .read_exact(&mut header)
        .await
        .map_err(|_| FrameError::Io)?;
    let length = usize::try_from(u32::from_be_bytes(header)).map_err(|_| FrameError::Length)?;
    if length == 0 || length > MAX_FRAME {
        return Err(FrameError::Length);
    }
    let mut body = vec![0u8; length];
    reader
        .read_exact(&mut body)
        .await
        .map_err(|_| FrameError::Io)?;
    serde_json::from_slice(&body).map_err(|_| FrameError::Json)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> AdmissionRequest {
        AdmissionRequest {
            protocol_version: 1,
            request_id: "r-1".into(),
            ticket: "eyJhbGciOiJFZERTQSJ9.e30.sig".into(),
            session_id: "s-1".into(),
            expected_host_epoch: "7".into(),
            service: Service::Gameplay,
        }
    }

    fn raw(json: &str) -> Vec<u8> {
        let mut frame = (json.len() as u32).to_be_bytes().to_vec();
        frame.extend_from_slice(json.as_bytes());
        frame
    }

    async fn parse(bytes: &[u8]) -> Result<AdmissionRequest, FrameError> {
        let mut reader = bytes;
        read_frame(&mut reader).await
    }

    #[tokio::test]
    async fn frames_round_trip() {
        let mut wire = Vec::new();
        write_frame(&mut wire, &request()).await.unwrap();
        let back = parse(&wire).await.unwrap();
        assert_eq!(back, request());
        assert!(back.validate().is_ok());
    }

    #[tokio::test]
    async fn hostile_frames_are_refused() {
        let good = serde_json::to_string(&request()).unwrap();
        let status = good.replace("\"gameplay\"", "\"status\"");
        assert_eq!(parse(&raw(&status)).await, Err(FrameError::Json), "A53");
        let extra = good.replace("{", "{\"destination\":\"10.0.0.1:22\",");
        assert_eq!(parse(&raw(&extra)).await, Err(FrameError::Json));
        let duplicate = good.replace("{", "{\"service\":\"gameplay\",");
        assert_eq!(parse(&raw(&duplicate)).await, Err(FrameError::Json));
        assert_eq!(parse(&[0, 0, 0, 0]).await, Err(FrameError::Length));
        assert_eq!(parse(&[0, 1, 0, 1]).await, Err(FrameError::Length));
        assert_eq!(parse(&[0, 0, 0, 10, b'{']).await, Err(FrameError::Io));
        assert_eq!(parse(&raw("not json")).await, Err(FrameError::Json));

        let mut old = request();
        old.protocol_version = 2;
        assert_eq!(old.validate(), Err(FrameError::Version));
        for bad in ["", "a b", "x\n", &"y".repeat(129)] {
            let mut r = request();
            r.session_id = bad.to_owned();
            assert_eq!(r.validate(), Err(FrameError::Field), "{bad:?}");
        }
        let mut r = request();
        r.ticket = "t".repeat(MAX_TICKET + 1);
        assert_eq!(r.validate(), Err(FrameError::Field));
    }
}
