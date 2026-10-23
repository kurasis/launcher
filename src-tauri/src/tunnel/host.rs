//! The host's side of the tunnel (ADAPTATION A3, spec 07).
//!
//! Every stream starts unadmitted: it gets an admission frame within ten
//! seconds or is reset, and only four per connection and 64 in total may wait
//! at once. The frame's ticket is checked by [`Admit`] (the worlds service
//! consumes it and names the member). Then the stream must carry a login of
//! exactly that member's nick, also within ten seconds; a status ping or any
//! other name closes it. Only after that does the host open its fixed local
//! connection to the world server, replay the bytes it read, and bridge.
//! A guest never chooses a destination.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use iroh::endpoint::{Connection, RecvStream, SendStream, VarInt};
use iroh::{Endpoint, EndpointId};
use tokio::net::TcpStream;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tokio::task::JoinHandle;
use tokio::time::timeout;

use super::bridge;
use super::frame::{
    AdmissionRequest, AdmissionResponse, PROTOCOL_VERSION, Refusal, read_frame, write_frame,
};
use super::handshake::{self, Classified, MAX_PREFIX};
use crate::install::service::BoxFuture;
use crate::sync::lock;
use crate::worlds::identity::Nick;
use crate::worlds::joining::{PathKind, path_of};

pub const ADMISSION_DEADLINE: Duration = Duration::from_secs(10);
pub const LOGIN_DEADLINE: Duration = Duration::from_secs(10);
/// A ticket check that takes longer is refused as unavailable.
pub const CHECK_DEADLINE: Duration = Duration::from_secs(15);
pub const UNADMITTED_PER_CONNECTION: usize = 4;
pub const UNADMITTED_TOTAL: usize = 64;
/// Connections being set up or holding no admitted stream yet are bounded
/// too, since anyone can make new endpoint keys.
pub const CONNECTIONS_TOTAL: usize = 128;

/// Stream reset code for a refused or failed admission.
pub const RESET_REFUSED: u32 = 2;

/// The member a ticket admits, with the host's own id for this connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Admitted {
    pub connection_id: String,
    pub nick: Nick,
}

/// Ticket checking and slot accounting, provided by the room (W5, W7).
pub trait Admit: Send + Sync + 'static {
    fn admit<'a>(
        &'a self,
        peer: EndpointId,
        request: &'a AdmissionRequest,
    ) -> BoxFuture<'a, Result<Admitted, Refusal>>;

    /// The admitted stream ended, or its login was refused.
    fn released(&self, connection_id: &str);
}

/// A running tunnel host; dropping it stops accepting.
pub struct TunnelHost {
    accept: JoinHandle<()>,
    shared: Arc<Shared>,
}

impl TunnelHost {
    /// How the guest computer `peer` reaches this one now, while connected.
    pub fn path(&self, peer: &EndpointId) -> Option<PathKind> {
        let connection = lock(&self.shared.connections)
            .get(peer)
            .map(|(_, c)| c.clone())?;
        path_of(&connection)
    }
}

impl Drop for TunnelHost {
    fn drop(&mut self) {
        self.accept.abort();
    }
}

struct Shared {
    admit: Arc<dyn Admit>,
    server_port: u16,
    protocol: i32,
    unadmitted: Arc<Semaphore>,
    /// The newest connection of each computer, with its number; as many as
    /// [`CONNECTIONS_TOTAL`].
    connections: Mutex<HashMap<EndpointId, (u64, Connection)>>,
}

/// Accepts tunnel connections on `endpoint` and forwards admitted logins to
/// the world server on 127.0.0.1:`server_port`.
pub fn serve(
    endpoint: Endpoint,
    admit: Arc<dyn Admit>,
    server_port: u16,
    protocol: i32,
) -> TunnelHost {
    let shared = Arc::new(Shared {
        admit,
        server_port,
        protocol,
        unadmitted: Arc::new(Semaphore::new(UNADMITTED_TOTAL)),
        connections: Mutex::new(HashMap::new()),
    });
    let connections = Arc::new(Semaphore::new(CONNECTIONS_TOTAL));
    let accepting = Arc::clone(&shared);
    let accept = tokio::spawn(async move {
        while let Some(incoming) = endpoint.accept().await {
            let Ok(permit) = Arc::clone(&connections).try_acquire_owned() else {
                incoming.refuse();
                continue;
            };
            let shared = Arc::clone(&accepting);
            tokio::spawn(async move {
                let Ok(Ok(connection)) = timeout(ADMISSION_DEADLINE, incoming).await else {
                    return;
                };
                serve_connection(connection, shared, permit).await;
            });
        }
    });
    TunnelHost { accept, shared }
}

async fn serve_connection(
    connection: Connection,
    shared: Arc<Shared>,
    _slot: OwnedSemaphorePermit,
) {
    static NUMBERS: AtomicU64 = AtomicU64::new(0);
    let (peer, number) = (
        connection.remote_id(),
        NUMBERS.fetch_add(1, Ordering::Relaxed),
    );
    lock(&shared.connections).insert(peer, (number, connection.clone()));
    let per_connection = Arc::new(Semaphore::new(UNADMITTED_PER_CONNECTION));
    while let Ok((mut send, mut recv)) = connection.accept_bi().await {
        let permits = (
            Arc::clone(&per_connection).try_acquire_owned(),
            Arc::clone(&shared.unadmitted).try_acquire_owned(),
        );
        let (Ok(local), Ok(global)) = permits else {
            refuse_stream(&mut send, &mut recv);
            continue;
        };
        let shared = Arc::clone(&shared);
        tokio::spawn(async move {
            serve_stream(send, recv, peer, &shared, (local, global)).await;
        });
    }
    // A newer connection of the same computer stays.
    let mut connections = lock(&shared.connections);
    if connections.get(&peer).is_some_and(|(n, _)| *n == number) {
        connections.remove(&peer);
    }
}

fn refuse_stream(send: &mut SendStream, recv: &mut RecvStream) {
    let _ = send.reset(VarInt::from_u32(RESET_REFUSED));
    let _ = recv.stop(VarInt::from_u32(RESET_REFUSED));
}

async fn answer(send: &mut SendStream, request_id: &str, refusal: Option<Refusal>) -> bool {
    let response = AdmissionResponse {
        protocol_version: PROTOCOL_VERSION,
        request_id: request_id.to_owned(),
        accepted: refusal.is_none(),
        refusal,
    };
    write_frame(send, &response).await.is_ok()
}

/// Reads the login from the admitted stream: the bytes read so far, or `None`
/// when it is not a login of `expected` from a client of the server's
/// `protocol`.
async fn read_login(recv: &mut RecvStream, protocol: i32, expected: &Nick) -> Option<Vec<u8>> {
    let mut buffer = Vec::with_capacity(512);
    let mut chunk = [0u8; 1024];
    loop {
        match handshake::classify(&buffer, Some(protocol)) {
            Ok(Classified::NeedMore) => {}
            Ok(Classified::Login { nick, .. }) if nick == *expected => return Some(buffer),
            // A status ping, another name or junk never reaches the server.
            _ => return None,
        }
        let room = MAX_PREFIX.saturating_sub(buffer.len()).min(chunk.len());
        let read = recv.read(&mut chunk[..room]).await.ok()??;
        buffer.extend_from_slice(&chunk[..read]);
    }
}

async fn serve_stream(
    mut send: SendStream,
    mut recv: RecvStream,
    peer: EndpointId,
    shared: &Shared,
    unadmitted: (OwnedSemaphorePermit, OwnedSemaphorePermit),
) {
    let request = match timeout(
        ADMISSION_DEADLINE,
        read_frame::<AdmissionRequest>(&mut recv),
    )
    .await
    {
        Ok(Ok(request)) if request.validate().is_ok() => request,
        _ => {
            // Finish rather than reset, so the refusal is delivered.
            let _ = answer(&mut send, "unknown", Some(Refusal::BadFrame)).await;
            let _ = send.finish();
            let _ = recv.stop(VarInt::from_u32(RESET_REFUSED));
            return;
        }
    };
    let admitted = match timeout(CHECK_DEADLINE, shared.admit.admit(peer, &request)).await {
        Ok(Ok(admitted)) => admitted,
        Ok(Err(refusal)) => {
            let _ = answer(&mut send, &request.request_id, Some(refusal)).await;
            let _ = send.finish();
            return;
        }
        Err(_) => {
            let _ = answer(&mut send, &request.request_id, Some(Refusal::Unavailable)).await;
            let _ = send.finish();
            return;
        }
    };
    let release = || shared.admit.released(&admitted.connection_id);
    let Some(budget) = bridge::reserve() else {
        let _ = answer(&mut send, &request.request_id, Some(Refusal::HostBusy)).await;
        let _ = send.finish();
        release();
        return;
    };
    if !answer(&mut send, &request.request_id, None).await {
        release();
        return;
    }
    let login = timeout(
        LOGIN_DEADLINE,
        read_login(&mut recv, shared.protocol, &admitted.nick),
    )
    .await;
    let Ok(Some(prefix)) = login else {
        refuse_stream(&mut send, &mut recv);
        release();
        return;
    };
    drop(unadmitted);
    let Ok(server) = TcpStream::connect(("127.0.0.1", shared.server_port)).await else {
        refuse_stream(&mut send, &mut recv);
        release();
        return;
    };
    bridge::run(server, send, recv, &prefix, false, budget).await;
    release();
}
