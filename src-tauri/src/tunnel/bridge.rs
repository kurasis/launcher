//! The bounded byte bridge between a local TCP socket and a tunnel stream
//! (spec 07 "Bounds and failure", acceptance A11 and A14).
//!
//! Each direction copies through one buffer of [`CHUNK`] bytes taken from a
//! budget shared by the whole launcher, so a stalled receiver stops reading
//! instead of queueing (backpressure), and memory is bounded by the number of
//! streams the budget admits. End of input is passed on as a half-close
//! (`finish` / TCP write shutdown); an error or reset is passed on as a reset
//! (stream reset / TCP RST). Bytes are never rewritten or replayed.

use std::sync::{Arc, LazyLock};

use iroh::endpoint::{RecvStream, SendStream, VarInt};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

/// One direction's buffer; below the specification's 256 KiB cap.
pub const CHUNK: usize = 64 * 1024;
/// The launcher-wide forwarding budget.
pub const TOTAL_BYTES: usize = 64 * 1024 * 1024;
/// The kernel's own buffers for a bridged TCP socket, each way. Left to the
/// system, Windows grows loopback buffers to many megabytes, which a stalled
/// server would fill before backpressure reaches the game.
pub const SOCKET_BUFFER: usize = 256 * 1024;
/// Stream reset code for a closed local socket.
pub const RESET_LOCAL: u32 = 1;

static BUDGET: LazyLock<Arc<Semaphore>> =
    LazyLock::new(|| Arc::new(Semaphore::new(TOTAL_BYTES / CHUNK)));

/// Buffer permits for both directions of one stream, or `None` when the
/// launcher is at its forwarding budget.
pub fn reserve() -> Option<(OwnedSemaphorePermit, OwnedSemaphorePermit)> {
    let budget = Arc::clone(&BUDGET);
    let first = Arc::clone(&budget).try_acquire_owned().ok()?;
    let second = budget.try_acquire_owned().ok()?;
    Some((first, second))
}

/// How one direction ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum End {
    /// End of input, passed on as a half-close.
    Finished,
    /// An error or reset, passed on as a reset.
    Reset,
}

/// Resets the TCP connection instead of a graceful close: the game sees the
/// same failure the other side had.
fn reset_tcp(writer: &OwnedWriteHalf) {
    let _ = socket2::SockRef::from(writer.as_ref()).set_linger(Some(std::time::Duration::ZERO));
}

async fn tcp_to_stream(mut tcp: OwnedReadHalf, mut send: SendStream) -> End {
    let mut buffer = vec![0u8; CHUNK];
    loop {
        match tcp.read(&mut buffer).await {
            Ok(0) => {
                let _ = send.finish();
                return End::Finished;
            }
            Ok(read) => {
                if send.write_all(&buffer[..read]).await.is_err() {
                    return End::Reset;
                }
            }
            Err(_) => {
                let _ = send.reset(VarInt::from_u32(RESET_LOCAL));
                return End::Reset;
            }
        }
    }
}

async fn stream_to_tcp(mut recv: RecvStream, mut tcp: OwnedWriteHalf) -> End {
    let mut buffer = vec![0u8; CHUNK];
    loop {
        match recv.read(&mut buffer).await {
            Ok(Some(read)) => {
                if tcp.write_all(&buffer[..read]).await.is_err() {
                    let _ = recv.stop(VarInt::from_u32(RESET_LOCAL));
                    return End::Reset;
                }
            }
            Ok(None) => {
                let _ = tcp.shutdown().await;
                return End::Finished;
            }
            Err(_) => {
                reset_tcp(&tcp);
                return End::Reset;
            }
        }
    }
}

/// Forwards both directions until both have ended. `prefix` is written to the
/// TCP socket first (the bytes the admission step consumed from the stream),
/// or to the stream first when `prefix_to_stream` is set (the bytes a local
/// classifier consumed from the game).
pub async fn run(
    tcp: TcpStream,
    send: SendStream,
    recv: RecvStream,
    prefix: &[u8],
    prefix_to_stream: bool,
    permits: (OwnedSemaphorePermit, OwnedSemaphorePermit),
) -> (End, End) {
    let _ = tcp.set_nodelay(true);
    let socket = socket2::SockRef::from(&tcp);
    let _ = socket.set_send_buffer_size(SOCKET_BUFFER);
    let _ = socket.set_recv_buffer_size(SOCKET_BUFFER);
    let (tcp_read, mut tcp_write) = tcp.into_split();
    let mut send = send;
    if !prefix.is_empty() {
        let written = if prefix_to_stream {
            send.write_all(prefix).await.is_ok()
        } else {
            tcp_write.write_all(prefix).await.is_ok()
        };
        if !written {
            let _ = send.reset(VarInt::from_u32(RESET_LOCAL));
            reset_tcp(&tcp_write);
            return (End::Reset, End::Reset);
        }
    }
    // A half-close keeps the other direction running; a reset ends both, and
    // dropping the halves closes the socket.
    let up = tcp_to_stream(tcp_read, send);
    let down = stream_to_tcp(recv, tcp_write);
    tokio::pin!(up, down);
    let (mut up_end, mut down_end) = (None, None);
    while up_end.is_none() || down_end.is_none() {
        tokio::select! {
            end = &mut up, if up_end.is_none() => up_end = Some(end),
            end = &mut down, if down_end.is_none() => down_end = Some(end),
        }
        if up_end == Some(End::Reset) || down_end == Some(End::Reset) {
            break;
        }
    }
    drop(permits);
    (up_end.unwrap_or(End::Reset), down_end.unwrap_or(End::Reset))
}

#[cfg(test)]
pub(crate) fn available() -> usize {
    BUDGET.available_permits()
}
