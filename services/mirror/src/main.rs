//! `lapi-mirror`: the download mirror at mirror.lapi.sh (D-062).
//!
//! ```text
//! lapi-mirror serve       the HTTP service
//! lapi-mirror overview    kept files per host, quota, free disk (JSON)
//! lapi-mirror warm        fetches what players will need (a timer runs it)
//! ```
//!
//! Settings come from the environment: `LAPI_MIRROR_DATA` (data folder,
//! default `/home/www/lapi/mirror/data`), `LAPI_MIRROR_ADDR` (default
//! `127.0.0.1:8782`), `LAPI_MIRROR_SETTINGS` (the deploy's settings file with
//! the quota, default `/home/www/lapi/mirror/settings.json`) and
//! `LAPI_MIRROR_MIN_FREE_GB` (free disk always kept, default 5).

mod api;
mod origin;
mod store;
mod warm;

use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use store::{GB, Store};

/// Until the settings file says otherwise.
const DEFAULT_QUOTA_GB: u64 = 1000;

fn env(name: &str, default: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| default.to_owned())
}

fn open() -> Result<Store, String> {
    let store = Store::open(
        &PathBuf::from(env("LAPI_MIRROR_DATA", "/home/www/lapi/mirror/data")),
        DEFAULT_QUOTA_GB * GB,
    )?
    .with_min_free(
        env("LAPI_MIRROR_MIN_FREE_GB", "5")
            .parse::<u64>()
            .unwrap_or(5)
            .saturating_mul(GB),
    );
    store.reload_quota(&settings());
    Ok(store)
}

fn settings() -> PathBuf {
    PathBuf::from(env(
        "LAPI_MIRROR_SETTINGS",
        "/home/www/lapi/mirror/settings.json",
    ))
}

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("serve") => serve(),
        Some("overview") => open().map(|store| {
            store.count();
            println!(
                "{}",
                serde_json::to_string_pretty(&store.overview()).unwrap_or_default()
            );
        }),
        Some("warm") => open().and_then(|store| {
            let runtime = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
            let service = format!("http://{}", env("LAPI_MIRROR_ADDR", "127.0.0.1:8782"));
            let summary = runtime.block_on(warm::Warm::new(&store, &service).run());
            println!("warm: {summary}");
            Ok(())
        }),
        Some("--version") => {
            println!("lapi-mirror {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        _ => Err("usage: lapi-mirror serve|overview|warm".to_owned()),
    };
    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("lapi-mirror: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}

/// True for globally routable unicast addresses: an origin's name must not
/// lead the mirror into this server's own network.
fn is_public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let [a, b, ..] = v4.octets();
            !(v4.is_private()
                || v4.is_loopback()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_broadcast()
                || v4.is_multicast()
                || v4.is_documentation()
                || a == 0
                || (a == 100 && (64..128).contains(&b))
                || a >= 240)
        }
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_public(IpAddr::V4(v4));
            }
            let s = v6.segments();
            !(v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                || (s[0] & 0xfe00) == 0xfc00
                || (s[0] & 0xffc0) == 0xfe80
                || (s[0] == 0x2001 && s[1] == 0x0db8)
                || (s[0] == 0x0064 && s[1] == 0xff9b))
        }
    }
}

struct PublicOnly;

impl reqwest::dns::Resolve for PublicOnly {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        let host = name.as_str().to_owned();
        Box::pin(async move {
            let addrs: Vec<SocketAddr> = tokio::net::lookup_host((host.as_str(), 0))
                .await?
                .filter(|a| is_public(a.ip()))
                .collect();
            if addrs.is_empty() {
                return Err(Box::new(std::io::Error::new(
                    std::io::ErrorKind::AddrNotAvailable,
                    "no public address",
                ))
                    as Box<dyn std::error::Error + Send + Sync>);
            }
            Ok(Box::new(addrs.into_iter()) as reqwest::dns::Addrs)
        })
    }
}

/// HTTPS to the listed hosts only, redirects included.
fn origin_client() -> Result<reqwest::Client, String> {
    let allowed = |url: &reqwest::Url| {
        url.scheme() == "https"
            && url.port().is_none()
            && url
                .host_str()
                .is_some_and(|h| origin::HOSTS.contains(&h) || origin::REDIRECT_HOSTS.contains(&h))
    };
    reqwest::Client::builder()
        .https_only(true)
        .redirect(reqwest::redirect::Policy::custom(move |attempt| {
            if attempt.previous().len() > 5 {
                attempt.error("too many redirects")
            } else if allowed(attempt.url()) {
                attempt.follow()
            } else {
                attempt.error("redirect to an unlisted host")
            }
        }))
        .dns_resolver(Arc::new(PublicOnly))
        .connect_timeout(Duration::from_secs(15))
        .read_timeout(Duration::from_secs(60))
        .user_agent(concat!(
            "lapi-mirror/",
            env!("CARGO_PKG_VERSION"),
            " (+https://mirror.lapi.sh)"
        ))
        .build()
        .map_err(|e| e.to_string())
}

fn serve() -> Result<(), String> {
    let store = open()?;
    store.clear_parts();
    let address = env("LAPI_MIRROR_ADDR", "127.0.0.1:8782");
    let runtime = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
    runtime.block_on(async move {
        let app = Arc::new(api::App::new(
            store,
            origin_client()?,
            Box::new(|o: &origin::Origin| o.url.clone()),
        ));
        let counting = Arc::clone(&app);
        tokio::task::spawn_blocking(move || {
            let (bytes, files) = counting.store.count();
            eprintln!("{files} kept files, {} GB", bytes / GB);
        });
        let settings_app = Arc::clone(&app);
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(60)).await;
                settings_app.store.reload_quota(&settings());
            }
        });
        let listener = tokio::net::TcpListener::bind(&address)
            .await
            .map_err(|e| format!("binding {address}: {e}"))?;
        eprintln!(
            "lapi-mirror {} listening on {address}",
            env!("CARGO_PKG_VERSION")
        );
        axum::serve(listener, api::router(app))
            .with_graceful_shutdown(stopped())
            .await
            .map_err(|e| e.to_string())
    })
}

/// systemd stops the service with SIGTERM; a terminal with Ctrl+C.
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
