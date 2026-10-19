//! `lapi-friends`: friends and chats of launcher accounts at friends.lapi.sh
//! (D-064).
//!
//! `lapi-friends serve` runs the API. On the server, for the owner:
//!
//! ```text
//! lapi-friends counts                 numbers only (fit for a public log)
//! lapi-friends reports [--limit N]    reported messages, newest first
//! ```
//!
//! Settings come from the environment: `LAPI_FRIENDS_DATA` (default
//! `/home/www/lapi/friends/data`), `LAPI_FRIENDS_ADDR` (`127.0.0.1:8784`),
//! `LAPI_FRIENDS_ISSUER` (`https://oauth.lapi.sh`), `LAPI_FRIENDS_JWKS` (the
//! account service's keys, `http://127.0.0.1:8783/.well-known/jwks.json`),
//! `LAPI_FRIENDS_ACCOUNTS` (its internal API, `http://127.0.0.1:8793`) and
//! `LAPI_FRIENDS_REQUIRE_EMAIL` (`1`: only confirmed addresses; off until
//! letters are sent).

mod api;
mod auth;
mod directory;
mod hub;
mod store;
mod text;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

fn env(name: &str, default: &str) -> String {
    std::env::var(name)
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| default.to_owned())
}

const DATA: &str = "/home/www/lapi/friends/data";

/// What players wrote, as it is printed: control and direction characters
/// become U+FFFD, so it cannot drive the owner's terminal.
fn plain(text: &str) -> String {
    text.chars()
        .map(|c| {
            let reorders = matches!(c, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}');
            if (c.is_control() && c != '\n') || reorders {
                '\u{FFFD}'
            } else {
                c
            }
        })
        .collect()
}

fn open() -> Result<store::Store, String> {
    store::Store::open(&PathBuf::from(env("LAPI_FRIENDS_DATA", DATA)))
}

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("serve") => serve(),
        Some("counts") => open().and_then(|store| {
            println!(
                "{}",
                serde_json::to_string_pretty(&store.counts()?).unwrap_or_default()
            );
            Ok(())
        }),
        Some("reports") => open().and_then(|store| {
            let limit = args
                .iter()
                .position(|a| a == "--limit")
                .and_then(|i| args.get(i + 1))
                .and_then(|n| n.parse().ok())
                .unwrap_or(30);
            for report in store.reports(limit)? {
                println!("{}", plain(&report.to_string()));
            }
            Ok(())
        }),
        Some("--version") => {
            println!("lapi-friends {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        _ => Err("usage: lapi-friends serve|counts|reports [--limit N]".to_owned()),
    };
    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("lapi-friends: {}", plain(&error));
            std::process::ExitCode::FAILURE
        }
    }
}

fn serve() -> Result<(), String> {
    let store = open()?;
    let address = env("LAPI_FRIENDS_ADDR", "127.0.0.1:8784");
    let jwks_url = env(
        "LAPI_FRIENDS_JWKS",
        "http://127.0.0.1:8783/.well-known/jwks.json",
    );
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| e.to_string())?;
    let app = Arc::new(api::App {
        store,
        tokens: auth::Tokens::new(&env("LAPI_FRIENDS_ISSUER", "https://oauth.lapi.sh")),
        directory: directory::Directory::new(Box::new(directory::Internal {
            base: env("LAPI_FRIENDS_ACCOUNTS", "http://127.0.0.1:8793"),
            http: http.clone(),
        })),
        hub: hub::Hub::default(),
        sessions: auth::Sessions::default(),
        limiter: api::Limiter::default(),
        require_email: env("LAPI_FRIENDS_REQUIRE_EMAIL", "0") == "1",
    });
    let runtime = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
    runtime.block_on(async move {
        // The account service's keys: at start, then every 10 minutes (a new
        // key is picked up without a restart).
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
        // Sign-outs and switched-off accounts reach friends within seconds.
        let checking = Arc::clone(&app);
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(15)).await;
                api::check_sessions(&checking).await;
            }
        });
        let listener = tokio::net::TcpListener::bind(&address)
            .await
            .map_err(|e| format!("binding {address}: {e}"))?;
        eprintln!(
            "lapi-friends {} listening on {address}",
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
