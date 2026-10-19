//! `lapi-relay`: the relay of worlds with friends at relay.lapi.sh, on a
//! server of its own (D-068, ADAPTATION A5, plan W6). It is iroh-relay 1.3.0,
//! pinned with the launcher's iroh, admitting only the tunnel keys
//! worlds.lapi.sh lists (see `gate`). It carries end-to-end encrypted QUIC
//! packets between a host and its guests when they cannot reach each other
//! directly, and sees nothing inside them.
//!
//! `lapi-relay serve` runs it. Settings come from the environment:
//! `RELAY_TOKEN` (required: the bearer token for the allow list, the same
//! value as the worlds service's `LAPI_WORLDS_RELAY_TOKEN`), `RELAY_ALLOW_URL`
//! (`https://worlds.lapi.sh/v1/relay/allow`), `RELAY_DOMAIN` (`relay.lapi.sh`:
//! its Let's Encrypt certificate is fetched and renewed by the relay itself,
//! over TLS-ALPN on port 443), `RELAY_DATA` (`/home/www/lapi/relay/data`, for
//! the certificate cache), `RELAY_ACME_CONTACT` (optional email for Let's
//! Encrypt), `RELAY_HTTP_ADDR` (`[::]:80`), `RELAY_HTTPS_ADDR` (`[::]:443`),
//! `RELAY_QUIC_ADDR` (`[::]:7842`, UDP, for address discovery),
//! `RELAY_METRICS_ADDR` (`127.0.0.1:9090`), `RELAY_RATE` (bytes per second a
//! connection may send, 4 MiB) and `RELAY_PER_ACCOUNT` (connections at once
//! per account, 8).

mod gate;

use std::net::SocketAddr;
use std::num::NonZeroU32;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use iroh_relay::server::clients::Clients;
use iroh_relay::server::{
    AcmeConfig, CertConfig, ClientRateLimit, QuicConfig, RelayConfig, Server, ServerConfig,
    TlsConfig,
};

use crate::gate::{Gate, Guard};

/// The allow list is asked for this often; records last 30 seconds.
const REFRESH: Duration = Duration::from_secs(10);
/// Keys whose record ran out are closed within this.
const SWEEP: Duration = Duration::from_secs(2);

fn env(name: &str, default: &str) -> String {
    std::env::var(name)
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| default.to_owned())
}

fn address(name: &str, default: &str) -> Result<SocketAddr, String> {
    env(name, default)
        .parse()
        .map_err(|_| format!("{name} is not an address"))
}

/// What the relay serves.
struct Settings {
    http: SocketAddr,
    tls: Option<Tls>,
    metrics: Option<SocketAddr>,
    rate: Option<ClientRateLimit>,
}

struct Tls {
    https: SocketAddr,
    quic: SocketAddr,
    domain: String,
    cache: PathBuf,
    contact: Option<String>,
}

fn rate_limit(bytes_per_second: u32) -> Option<ClientRateLimit> {
    let mut limit = ClientRateLimit::new(NonZeroU32::new(bytes_per_second)?);
    // A burst of four seconds' worth, for chunks loading on join.
    limit.max_burst_bytes = NonZeroU32::new(bytes_per_second.saturating_mul(4));
    Some(limit)
}

fn server_config(settings: Settings, gate: Arc<Gate>) -> ServerConfig {
    let mut relay = RelayConfig::new(settings.http);
    relay.access = Arc::new(Guard(gate));
    relay.key_cache_capacity = Some(4096);
    relay.limits.client_rx = settings.rate;
    let mut quic = None;
    if let Some(tls) = settings.tls {
        let builder =
            rustls::ServerConfig::builder_with_provider(iroh_relay::tls::default_provider())
                .with_safe_default_protocol_versions()
                .expect("ring supports the default protocol versions")
                .with_no_client_auth();
        let acme = AcmeConfig::letsencrypt(true)
            .domains(vec![tls.domain])
            .contact(
                tls.contact
                    .into_iter()
                    .map(|c| format!("mailto:{c}"))
                    .collect(),
            )
            .cache_path(tls.cache);
        relay.tls = Some(TlsConfig::new(
            tls.https,
            CertConfig::LetsEncrypt {
                acme_config: acme,
                server_config_builder: builder,
            },
        ));
        quic = Some(QuicConfig::new(tls.quic));
    }
    let mut config = ServerConfig::default();
    config.relay = Some(relay);
    config.quic = quic;
    config.metrics_addr = settings.metrics;
    config
}

/// Reads the allow list into the gate. Failures leave the records as they
/// were: they run out by themselves, so a lost list closes everything within
/// 30 seconds.
async fn refresh(
    http: &reqwest::Client,
    url: &str,
    token: &str,
    gate: &Gate,
) -> Result<usize, String> {
    let response = http
        .get(url)
        .bearer_auth(token)
        .send()
        .await
        .map_err(|_| "the allow list did not answer".to_owned())?;
    if !response.status().is_success() {
        return Err(format!(
            "the allow list answered {}",
            response.status().as_u16()
        ));
    }
    let body = response
        .bytes()
        .await
        .map_err(|_| "the allow list was cut off".to_owned())?;
    // Two megabytes hold the largest list (100 000 entries) many times over.
    if body.len() > 16 * 1024 * 1024 {
        return Err("the allow list is too large".to_owned());
    }
    let list = gate::parse(&body)?;
    let count = list.len();
    gate.replace(list, Instant::now());
    Ok(count)
}

/// Closes every connection whose key is no longer allowed.
fn close_outlawed(clients: &Clients, gate: &Gate) -> usize {
    let outlawed = gate.outlawed(Instant::now());
    for key in &outlawed {
        clients.disconnect(*key, None);
    }
    outlawed.len()
}

fn http_client() -> Result<reqwest::Client, String> {
    let tls = iroh_relay::tls::CaTlsConfig::default()
        .client_config(iroh_relay::tls::default_provider())
        .map_err(|e| e.to_string())?;
    reqwest::Client::builder()
        .tls_backend_preconfigured(tls)
        .timeout(Duration::from_secs(5))
        .build()
        .map_err(|e| e.to_string())
}

fn main() -> std::process::ExitCode {
    let result = match std::env::args().nth(1).as_deref() {
        Some("serve") => serve(),
        Some("--version") => {
            println!("lapi-relay {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        _ => Err("usage: lapi-relay serve".to_owned()),
    };
    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("lapi-relay: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn serve() -> Result<(), String> {
    let token = env("RELAY_TOKEN", "");
    if !(32..=128).contains(&token.len()) {
        return Err("RELAY_TOKEN must be 32-128 characters".to_owned());
    }
    let url = env("RELAY_ALLOW_URL", "https://worlds.lapi.sh/v1/relay/allow");
    let rate: u32 = env("RELAY_RATE", "4194304")
        .parse()
        .map_err(|_| "RELAY_RATE is not a number")?;
    let per_account: usize = env("RELAY_PER_ACCOUNT", "8")
        .parse()
        .map_err(|_| "RELAY_PER_ACCOUNT is not a number")?;
    let settings = Settings {
        http: address("RELAY_HTTP_ADDR", "[::]:80")?,
        tls: Some(Tls {
            https: address("RELAY_HTTPS_ADDR", "[::]:443")?,
            quic: address("RELAY_QUIC_ADDR", "[::]:7842")?,
            domain: env("RELAY_DOMAIN", "relay.lapi.sh"),
            cache: PathBuf::from(env("RELAY_DATA", "/home/www/lapi/relay/data")).join("acme"),
            contact: std::env::var("RELAY_ACME_CONTACT")
                .ok()
                .filter(|c| !c.trim().is_empty()),
        }),
        metrics: Some(address("RELAY_METRICS_ADDR", "127.0.0.1:9090")?),
        rate: rate_limit(rate),
    };
    let http = http_client()?;
    let gate = Arc::new(Gate::new(per_account.clamp(1, 64)));
    let runtime = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
    runtime.block_on(async move {
        // The list first, so the keys of games already running get back in.
        if let Err(e) = refresh(&http, &url, &token, &gate).await {
            eprintln!("{e}");
        }
        let mut server = Server::spawn(server_config(settings, Arc::clone(&gate)))
            .await
            .map_err(|e| e.to_string())?;
        let clients = server
            .relay_service()
            .ok_or("the relay did not start")?
            .clients()
            .clone();
        eprintln!("lapi-relay {} running", env!("CARGO_PKG_VERSION"));
        let keeper = tokio::spawn(async move {
            let mut asked = Instant::now();
            let mut counted = Instant::now();
            let mut sweep = tokio::time::interval(SWEEP);
            loop {
                sweep.tick().await;
                // A number for the journal, nothing about who.
                if counted.elapsed() >= Duration::from_secs(600) {
                    counted = Instant::now();
                    eprintln!("connections {}", gate.connections());
                }
                if asked.elapsed() >= REFRESH {
                    asked = Instant::now();
                    if let Err(e) = refresh(&http, &url, &token, &gate).await {
                        eprintln!("{e}");
                    }
                }
                close_outlawed(&clients, &gate);
            }
        });
        tokio::select! {
            () = stopped() => {}
            _ = server.join() => eprintln!("the relay stopped"),
        }
        keeper.abort();
        server.shutdown().await.map_err(|e| e.to_string())
    })
}

async fn stopped() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        if let Ok(mut term) = signal(SignalKind::terminate()) {
            tokio::select! {
                _ = term.recv() => {}
                _ = tokio::signal::ctrl_c() => {}
            }
            return;
        }
    }
    let _ = tokio::signal::ctrl_c().await;
}

#[cfg(test)]
mod tests;
