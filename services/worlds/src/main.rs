//! `lapi-worlds`: rooms, invitations, host leases and join tickets for worlds
//! with friends at worlds.lapi.sh (D-068, ADAPTATION A4).
//!
//! `lapi-worlds serve` runs the API. On the server, for the owner:
//!
//! ```text
//! lapi-worlds counts     numbers only (fit for a public log)
//! lapi-worlds backup     a copy of the database in data/backups
//!                        (daily, by lapi-worlds-backup.timer)
//! ```
//!
//! Settings come from the environment: `LAPI_WORLDS_DATA` (default
//! `/home/www/lapi/worlds/data`), `LAPI_WORLDS_ADDR` (`127.0.0.1:8788`),
//! `LAPI_WORLDS_BASE` (`https://worlds.lapi.sh`), `LAPI_WORLDS_ISSUER`
//! (`https://oauth.lapi.sh`), `LAPI_WORLDS_JWKS` (the account service's keys,
//! `http://127.0.0.1:8783/.well-known/jwks.json`) and
//! `LAPI_WORLDS_AUTHORITY_EPOCH` (required: a random value kept outside the
//! database and changed after a restore, so restored leases never come back).
//! The ticket signing key is made on first start in `data/ticket-key`.

mod api;
mod auth;
mod hub;
mod store;
mod text;
mod tickets;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

fn env(name: &str, default: &str) -> String {
    std::env::var(name)
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| default.to_owned())
}

const DATA: &str = "/home/www/lapi/worlds/data";

fn data() -> PathBuf {
    PathBuf::from(env("LAPI_WORLDS_DATA", DATA))
}

/// The authority epoch: 16–128 characters of `A-Z a-z 0-9 - _`.
fn authority() -> Result<String, String> {
    let value = env("LAPI_WORLDS_AUTHORITY_EPOCH", "");
    let fits = (16..=128).contains(&value.len())
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
    fits.then_some(value).ok_or_else(|| {
        "LAPI_WORLDS_AUTHORITY_EPOCH must be 16-128 letters, digits, - or _".to_owned()
    })
}

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // Counts and backups need no authority: they read rows only.
    let read_only = || store::Store::open(&data(), "");
    let result = match args.first().map(String::as_str) {
        Some("serve") => serve(),
        Some("backup") => read_only().and_then(|store| {
            let path = store.backup(&data().join("backups"), 14)?;
            println!("{}", path.display());
            Ok(())
        }),
        Some("counts") => read_only().and_then(|store| {
            let counts = store
                .counts(chrono::Utc::now().timestamp())
                .map_err(|e| format!("{e:?}"))?;
            println!(
                "{}",
                serde_json::to_string_pretty(&counts).unwrap_or_default()
            );
            Ok(())
        }),
        Some("--version") => {
            println!("lapi-worlds {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        _ => Err("usage: lapi-worlds serve|counts|backup".to_owned()),
    };
    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("lapi-worlds: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn serve() -> Result<(), String> {
    let authority = authority()?;
    let data = data();
    let store = store::Store::open(&data, &authority)?;
    let tickets = tickets::TicketKey::load_or_create(&data.join("ticket-key"))?;
    let address = env("LAPI_WORLDS_ADDR", "127.0.0.1:8788");
    let jwks_url = env(
        "LAPI_WORLDS_JWKS",
        "http://127.0.0.1:8783/.well-known/jwks.json",
    );
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| e.to_string())?;
    let app = Arc::new(api::App {
        store,
        tokens: auth::Tokens::new(&env("LAPI_WORLDS_ISSUER", "https://oauth.lapi.sh")),
        tickets,
        replays: auth::Replays::default(),
        hub: hub::Hub::default(),
        limiter: api::Limiter::default(),
        clock: api::Clock::default(),
        base: env("LAPI_WORLDS_BASE", "https://worlds.lapi.sh")
            .trim_end_matches('/')
            .to_owned(),
    });
    let runtime = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
    runtime.block_on(async move {
        // The account service's keys: at start, then every 10 minutes.
        let keys = Arc::clone(&app);
        tokio::spawn(async move {
            loop {
                match auth::fetch_jwks(&http, &jwks_url).await {
                    Ok(jwks) => {
                        keys.tokens.set_jwks(&jwks);
                    }
                    Err(e) => eprintln!("reading the account service's keys failed: {e}"),
                }
                let wait = if keys.tokens.has_keys() { 600 } else { 10 };
                tokio::time::sleep(Duration::from_secs(wait)).await;
            }
        });
        // Leases that ran out end even when nobody asks about their room.
        let sweeping = Arc::clone(&app);
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(15)).await;
                if let Err(e) = sweeping.store.sweep(sweeping.clock.now()) {
                    eprintln!("sweeping failed: {e:?}");
                }
            }
        });
        let listener = tokio::net::TcpListener::bind(&address)
            .await
            .map_err(|e| format!("binding {address}: {e}"))?;
        eprintln!(
            "lapi-worlds {} listening on {address}",
            env!("CARGO_PKG_VERSION")
        );
        axum::serve(listener, api::router(app))
            .with_graceful_shutdown(stopped())
            .await
            .map_err(|e| e.to_string())
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
