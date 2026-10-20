//! Tunnel endpoints (ADAPTATION A5): iroh with our own relay and no public
//! discovery. Endpoint addresses travel only through the worlds API, so no
//! address lookup service is configured, and every room session gets a fresh
//! key, so an endpoint is never shared between rooms.

use std::time::Duration;

use iroh::endpoint::{IdleTimeout, QuicTransportConfig, VarInt, presets};
use iroh::{Endpoint, RelayMode, SecretKey};

use super::frame::ALPN;
use crate::error::{ErrorCode, LauncherError, LauncherResult};

/// QUIC buffers sized to the forwarding bounds (spec 07): the stream window
/// matches the per-direction cap, the connection window a few streams.
fn transport() -> LauncherResult<QuicTransportConfig> {
    let idle = IdleTimeout::try_from(Duration::from_secs(30)).map_err(|_| internal())?;
    Ok(QuicTransportConfig::builder()
        .max_concurrent_bidi_streams(VarInt::from_u32(16))
        .max_concurrent_uni_streams(VarInt::from_u32(0))
        .stream_receive_window(VarInt::from_u32(256 * 1024))
        .receive_window(VarInt::from_u32(4 * 1024 * 1024))
        .send_window(4 * 1024 * 1024)
        .keep_alive_interval(Duration::from_secs(5))
        .max_idle_timeout(Some(idle))
        .build())
}

fn internal() -> LauncherError {
    LauncherError::internal("The tunnel could not be configured.")
}

/// Where an endpoint may reach peers.
#[derive(Debug, Clone)]
pub enum Reach {
    /// Loopback only, no relay: tests and the host's own game.
    Loopback,
    /// Direct paths plus the given relays.
    Network(RelayMode),
    /// The given relays only: no IP transport is bound, so no direct
    /// candidate is ever offered or probed and peers see only the relay.
    RelayOnly(RelayMode),
}

/// Binds an endpoint with a fresh key. `accept` is true on the host, which
/// is the only side that accepts tunnel connections.
pub async fn bind(reach: Reach, accept: bool) -> LauncherResult<Endpoint> {
    let mut builder = Endpoint::builder(presets::Minimal)
        .secret_key(SecretKey::generate())
        .transport_config(transport()?);
    if accept {
        builder = builder.alpns(vec![ALPN.to_vec()]);
    }
    builder = match reach {
        Reach::Loopback => builder
            .relay_mode(RelayMode::Disabled)
            .bind_addr("127.0.0.1:0")
            .map_err(|_| internal())?,
        Reach::Network(relays) => builder.relay_mode(relays),
        Reach::RelayOnly(relays) => builder.relay_mode(relays).clear_ip_transports(),
    };
    builder.bind().await.map_err(|_| {
        LauncherError::new(
            ErrorCode::NetworkError,
            "The world connection could not be opened on this computer.",
        )
    })
}
