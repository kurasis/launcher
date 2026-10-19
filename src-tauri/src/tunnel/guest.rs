//! The guest's side of the tunnel (spec 07 "Local handshake ownership and
//! bounds").
//!
//! The game connects to a listener on 127.0.0.1. Each local connection is
//! classified within five seconds from at most 8 KiB: a server-list ping is
//! answered here from the room's status (no stream, no ticket); a login of this
//! player gets a fresh ticket at that moment, one tunnel stream and its
//! admission, and then the bytes read so far go first into the stream, exactly
//! once. At most eight local connections per listener and sixteen in total may
//! wait for classification.

use std::net::Ipv4Addr;
use std::sync::{Arc, LazyLock};
use std::time::{Duration, Instant};

use iroh::endpoint::Connection;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Semaphore;
use tokio::task::JoinHandle;
use tokio::time::timeout;

use super::bridge;
use super::frame::{
    AdmissionRequest, AdmissionResponse, PROTOCOL_VERSION, Refusal, Service, read_frame,
    write_frame,
};
use super::handshake::{self, Classified, MAX_PREFIX};
use crate::error::LauncherError;
use crate::install::service::BoxFuture;
use crate::worlds::identity::Nick;
use crate::worlds::protocol::{decode_varint, write_packet, write_string};

pub const CLASSIFY_DEADLINE: Duration = Duration::from_secs(5);
pub const ADMISSION_DEADLINE: Duration = Duration::from_secs(10);
pub const PENDING_PER_LISTENER: usize = 8;
pub const PENDING_TOTAL: usize = 16;
/// A status older than this is not shown to the game.
pub const STATUS_MAX_AGE: Duration = Duration::from_secs(30);

static PENDING: LazyLock<Arc<Semaphore>> =
    LazyLock::new(|| Arc::new(Semaphore::new(PENDING_TOTAL)));

/// A ticket for one game connection, fetched when the game connects.
#[derive(Debug, Clone)]
pub struct Grant {
    pub ticket: String,
    pub session_id: String,
    pub host_epoch: String,
}

pub trait Tickets: Send + Sync + 'static {
    fn ticket(&self) -> BoxFuture<'_, Result<Grant, LauncherError>>;
}

/// The room status the worlds service last reported.
#[derive(Debug, Clone)]
pub struct StatusSnapshot {
    pub observed_at: Instant,
    pub name: String,
    pub online: u32,
    pub max: u32,
}

pub trait RoomStatus: Send + Sync + 'static {
    fn snapshot(&self) -> Option<StatusSnapshot>;

    /// The host refused a login; the room page shows why.
    fn refused(&self, _refusal: Option<Refusal>) {}
}

/// What one listener serves.
pub struct Target {
    pub connection: Connection,
    pub nick: Nick,
    pub protocol: i32,
    pub version_name: &'static str,
    pub tickets: Arc<dyn Tickets>,
    pub status: Arc<dyn RoomStatus>,
}

/// A listening guest; dropping it closes the listener (running bridges end
/// with the connection).
pub struct GuestListener {
    port: u16,
    accept: JoinHandle<()>,
}

impl GuestListener {
    pub fn port(&self) -> u16 {
        self.port
    }
}

impl Drop for GuestListener {
    fn drop(&mut self) {
        self.accept.abort();
    }
}

pub async fn listen(target: Target) -> std::io::Result<GuestListener> {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
    let port = listener.local_addr()?.port();
    let target = Arc::new(target);
    let local = Arc::new(Semaphore::new(PENDING_PER_LISTENER));
    let accept = tokio::spawn(async move {
        while let Ok((socket, _)) = listener.accept().await {
            // Limits apply before any per-connection work.
            let permits = (
                Arc::clone(&local).try_acquire_owned(),
                Arc::clone(&PENDING).try_acquire_owned(),
            );
            let (Ok(local_permit), Ok(global_permit)) = permits else {
                drop(socket);
                continue;
            };
            let target = Arc::clone(&target);
            tokio::spawn(async move {
                let classified =
                    timeout(CLASSIFY_DEADLINE, classify(socket, target.protocol)).await;
                drop((local_permit, global_permit));
                match classified {
                    Ok(Some((socket, buffer, Classified::Status { consumed }))) => {
                        let _ = timeout(
                            CLASSIFY_DEADLINE,
                            answer_status(socket, buffer, consumed, &target),
                        )
                        .await;
                    }
                    Ok(Some((socket, buffer, Classified::Login { nick, .. })))
                        if nick == target.nick =>
                    {
                        join(socket, buffer, &target).await;
                    }
                    _ => {}
                }
            });
        }
    });
    Ok(GuestListener { port, accept })
}

async fn classify(
    mut socket: TcpStream,
    protocol: i32,
) -> Option<(TcpStream, Vec<u8>, Classified)> {
    let mut buffer = Vec::with_capacity(512);
    let mut chunk = [0u8; 1024];
    loop {
        match handshake::classify(&buffer, protocol) {
            Ok(Classified::NeedMore) => {}
            Ok(found) => return Some((socket, buffer, found)),
            Err(_) => return None,
        }
        let room = MAX_PREFIX.saturating_sub(buffer.len()).min(chunk.len());
        let read = socket.read(&mut chunk[..room]).await.ok()?;
        if read == 0 {
            return None;
        }
        buffer.extend_from_slice(&chunk[..read]);
    }
}

fn status_json(target: &Target, snapshot: &StatusSnapshot) -> String {
    serde_json::json!({
        "version": {"name": target.version_name, "protocol": target.protocol},
        "players": {"max": snapshot.max, "online": snapshot.online},
        "description": {"text": snapshot.name},
        "enforcesSecureChat": false,
    })
    .to_string()
}

/// Answers the status request and ping that follow a status handshake, then
/// closes. Nothing is forwarded.
async fn answer_status(
    mut socket: TcpStream,
    mut buffer: Vec<u8>,
    consumed: usize,
    target: &Target,
) {
    let Some(snapshot) = target
        .status
        .snapshot()
        .filter(|s| s.observed_at.elapsed() <= STATUS_MAX_AGE)
    else {
        return;
    };
    buffer.drain(..consumed);
    let mut chunk = [0u8; 256];
    let mut answered = false;
    loop {
        // Small packets only: status request (empty) and ping (8 bytes).
        while let Some((length, used)) = decode_varint(&buffer).ok().flatten() {
            let Ok(length) = usize::try_from(length) else {
                return;
            };
            if length == 0 || length > 16 {
                return;
            }
            if buffer.len() < used + length {
                break;
            }
            let packet: Vec<u8> = buffer.drain(..used + length).skip(used).collect();
            match (packet.first(), packet.len(), answered) {
                (Some(0x00), 1, false) => {
                    let mut body = Vec::new();
                    write_string(&mut body, &status_json(target, &snapshot));
                    if write_packet_async(&mut socket, 0x00, &body).await.is_err() {
                        return;
                    }
                    answered = true;
                }
                (Some(0x01), 9, true) => {
                    let _ = write_packet_async(&mut socket, 0x01, &packet[1..]).await;
                    return;
                }
                _ => return,
            }
        }
        if buffer.len() > 64 {
            return;
        }
        match socket.read(&mut chunk).await {
            Ok(0) | Err(_) => return,
            Ok(read) => buffer.extend_from_slice(&chunk[..read]),
        }
    }
}

async fn write_packet_async(socket: &mut TcpStream, id: i32, body: &[u8]) -> std::io::Result<()> {
    let mut wire = Vec::with_capacity(body.len() + 8);
    write_packet(&mut wire, id, body)?;
    socket.write_all(&wire).await
}

/// One login: ticket, stream, admission, then the bridge.
async fn join(socket: TcpStream, prefix: Vec<u8>, target: &Target) {
    let Some(budget) = bridge::reserve() else {
        return;
    };
    let Ok(grant) = target.tickets.ticket().await else {
        target.status.refused(Some(Refusal::Unavailable));
        return;
    };
    let Ok((mut send, mut recv)) = target.connection.open_bi().await else {
        target.status.refused(None);
        return;
    };
    let request = AdmissionRequest {
        protocol_version: PROTOCOL_VERSION,
        request_id: uuid::Uuid::new_v4().to_string(),
        ticket: grant.ticket,
        session_id: grant.session_id,
        expected_host_epoch: grant.host_epoch,
        service: Service::Gameplay,
    };
    let admitted = timeout(ADMISSION_DEADLINE, async {
        write_frame(&mut send, &request).await.ok()?;
        read_frame::<AdmissionResponse>(&mut recv).await.ok()
    })
    .await;
    match admitted {
        Ok(Some(response)) if response.accepted && response.request_id == request.request_id => {}
        Ok(Some(response)) => {
            target.status.refused(response.refusal);
            return;
        }
        _ => {
            target.status.refused(None);
            return;
        }
    }
    bridge::run(socket, send, recv, &prefix, true, budget).await;
}
