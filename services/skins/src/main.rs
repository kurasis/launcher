//! `lapi-skins`: skins and capes of launcher accounts at skins.lapi.sh
//! (D-065), looked up by nick by our Minecraft mod and uploaded by the
//! launcher with the account's access token.
//!
//! `lapi-skins serve` runs the API. On the server, for the owner:
//!
//! ```text
//! lapi-skins counts                   numbers only (fit for a public log)
//! lapi-skins remove <account> [skin|cape]
//!                                     takes a skin or cape (or both) off an
//!                                     account and out of its wardrobe;
//!                                     `lapi-oauth find <nick>` shows the
//!                                     account ID of a nick
//! lapi-skins backup                   a copy of the database in data/backups
//!                                     (daily, by lapi-skins-backup.timer)
//! ```
//!
//! Settings come from the environment: `LAPI_SKINS_DATA` (default
//! `/home/www/lapi/skins/data`), `LAPI_SKINS_ADDR` (`127.0.0.1:8785`),
//! `LAPI_SKINS_ISSUER` (`https://oauth.lapi.sh`), `LAPI_SKINS_JWKS` (the
//! account service's keys, `http://127.0.0.1:8783/.well-known/jwks.json`),
//! `LAPI_SKINS_ACCOUNTS` (its internal API, `http://127.0.0.1:8793`) and
//! `LAPI_SKINS_PUBLIC` (where textures are fetched, `https://skins.lapi.sh`).

mod api;
mod auth;
mod directory;
mod store;
mod texture;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

fn env(name: &str, default: &str) -> String {
    std::env::var(name)
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| default.to_owned())
}

const DATA: &str = "/home/www/lapi/skins/data";
const USAGE: &str = "usage: lapi-skins serve|counts|remove <account> [skin|cape]|backup";

/// The owner's command: takes textures off an account, and out of its
/// wardrobe and sets so they cannot be put on again. The running service
/// shows it at once (profiles are read from the database each time; the
/// mod and Cloudflare keep a profile a minute).
fn remove(store: &store::Store, args: &[String]) -> Result<(), String> {
    let account = args
        .first()
        .map(String::as_str)
        .filter(|id| directory::account_id(id))
        .ok_or("give an account ID (lapi-oauth find <nick> shows it)")?;
    let kind = match args.get(1).map(String::as_str) {
        None => None,
        Some(kind) => Some(texture::Kind::parse(kind).ok_or("give skin or cape, or neither")?),
    };
    if !store.remove(account, kind)? {
        return Err("that account wears nothing of the kind".to_owned());
    }
    println!(
        "{account}: {} removed",
        kind.map_or("skin and cape", texture::Kind::as_str)
    );
    Ok(())
}

fn open() -> Result<store::Store, String> {
    store::Store::open(&PathBuf::from(env("LAPI_SKINS_DATA", DATA)))
}

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("serve") => serve(),
        Some("backup") => open().and_then(|store| {
            let data = PathBuf::from(env("LAPI_SKINS_DATA", DATA));
            let path = store.backup(&data.join("backups"), 14)?;
            println!("{}", path.display());
            Ok(())
        }),
        Some("counts") => open().and_then(|store| {
            println!(
                "{}",
                serde_json::to_string_pretty(&store.counts()?).unwrap_or_default()
            );
            Ok(())
        }),
        Some("remove") => open().and_then(|store| remove(&store, &args[1..])),
        Some("--version") => {
            println!("lapi-skins {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        _ => Err(USAGE.to_owned()),
    };
    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            // Errors are ours (never a nick); control characters go anyway.
            let error: String = error
                .chars()
                .map(|c| if c.is_control() { '\u{FFFD}' } else { c })
                .collect();
            eprintln!("lapi-skins: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn serve() -> Result<(), String> {
    let store = open()?;
    let address = env("LAPI_SKINS_ADDR", "127.0.0.1:8785");
    let jwks_url = env(
        "LAPI_SKINS_JWKS",
        "http://127.0.0.1:8783/.well-known/jwks.json",
    );
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| e.to_string())?;
    let app = Arc::new(api::App::new(
        store,
        auth::Tokens::new(&env("LAPI_SKINS_ISSUER", "https://oauth.lapi.sh")),
        directory::Directory::new(Box::new(directory::Internal {
            base: env("LAPI_SKINS_ACCOUNTS", "http://127.0.0.1:8793"),
            http: http.clone(),
        })),
        &env("LAPI_SKINS_PUBLIC", "https://skins.lapi.sh"),
    ));
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
        // Sign-outs, switched-off and deleted accounts reach skins within
        // seconds.
        let checking = Arc::clone(&app);
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(15)).await;
                api::check_sessions(&checking).await;
                api::purge_deleted(&checking).await;
            }
        });
        let listener = tokio::net::TcpListener::bind(&address)
            .await
            .map_err(|e| format!("binding {address}: {e}"))?;
        eprintln!(
            "lapi-skins {} listening on {address}",
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
