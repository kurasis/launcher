//! `lapi-feed`: the launcher's start page feed at feed.lapi.sh (D-067).
//! It gathers Minecraft news from Mojang (patch notes, news, versions),
//! YouTube channels, Twitch streams and our own `editorial.toml`, cleans it
//! and serves it as one JSON, with its pictures, so the launcher asks no one
//! else.
//!
//! `lapi-feed serve` runs it; `lapi-feed counts` prints numbers only (fit for
//! a public log).
//!
//! Settings come from the environment: `LAPI_FEED_DATA` (default
//! `/home/www/lapi/feed/data`), `LAPI_FEED_ADDR` (`127.0.0.1:8787`), and the
//! secrets `LAPI_FEED_DEEPL_KEY` (translations), `LAPI_FEED_TWITCH_CLIENT_ID`
//! and `LAPI_FEED_TWITCH_SECRET` (streams). Without a secret its part is
//! simply left out.

mod api;
mod deepl;
mod editorial;
mod feed;
mod html;
mod images;
mod mojang;
mod net;
mod twitch;
mod youtube;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

const DATA: &str = "/home/www/lapi/feed/data";
const USAGE: &str = "usage: lapi-feed serve|counts";
pub const EDITORIAL: &str = include_str!("../editorial.toml");

fn env(name: &str, default: &str) -> String {
    std::env::var(name)
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| default.to_owned())
}

fn open(http: reqwest::Client) -> Result<feed::Feed, String> {
    let editorial = editorial::Editorial::load(EDITORIAL)?;
    feed::Feed::open(
        &PathBuf::from(env("LAPI_FEED_DATA", DATA)),
        editorial,
        http,
        twitch::Twitch::new(
            env("LAPI_FEED_TWITCH_CLIENT_ID", ""),
            env("LAPI_FEED_TWITCH_SECRET", ""),
        ),
        deepl::DeepL::new(env("LAPI_FEED_DEEPL_KEY", "")),
    )
}

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("serve") => serve(),
        Some("counts") => net::client().and_then(open).map(|feed| {
            println!(
                "{}",
                serde_json::to_string_pretty(&feed.counts()).unwrap_or_default()
            );
        }),
        Some("--version") => {
            println!("lapi-feed {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        _ => Err(USAGE.to_owned()),
    };
    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            let error: String = error
                .chars()
                .map(|c| if c.is_control() { '\u{FFFD}' } else { c })
                .collect();
            eprintln!("lapi-feed: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}

/// Runs `job` every `every`, from now on; a failure is logged (our words
/// only, never anything about a player) and tried again next time.
fn every<F, Fut>(name: &'static str, every: Duration, app: Arc<api::App>, job: F)
where
    F: Fn(Arc<api::App>) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = Result<(), String>> + Send,
{
    tokio::spawn(async move {
        loop {
            if let Err(e) = job(Arc::clone(&app)).await {
                eprintln!("{name}: {e}");
            }
            tokio::time::sleep(every).await;
        }
    });
}

fn serve() -> Result<(), String> {
    let http = net::client()?;
    let feed = open(http)?;
    let address = env("LAPI_FEED_ADDR", "127.0.0.1:8787");
    let app = Arc::new(api::App {
        feed,
        limiter: api::Limiter::default(),
    });
    let runtime = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
    runtime.block_on(async move {
        every(
            "mojang",
            Duration::from_secs(5 * 60),
            Arc::clone(&app),
            |app| async move { app.feed.refresh_mojang().await },
        );
        every(
            "youtube",
            Duration::from_secs(15 * 60),
            Arc::clone(&app),
            |app| async move { app.feed.refresh_youtube().await },
        );
        every(
            "twitch",
            Duration::from_secs(5 * 60),
            Arc::clone(&app),
            |app| async move { app.feed.refresh_twitch().await },
        );
        every(
            "translations",
            Duration::from_secs(10 * 60),
            Arc::clone(&app),
            |app| async move {
                // The first notes arrive with Mojang's refresh.
                tokio::time::sleep(Duration::from_secs(60)).await;
                app.feed.translate().await.map(|_| ())
            },
        );
        every(
            "pictures",
            Duration::from_secs(3600),
            Arc::clone(&app),
            |app| async move {
                let used = app.feed.used_images();
                app.feed.images.clean(&used);
                Ok(())
            },
        );
        let listener = tokio::net::TcpListener::bind(&address)
            .await
            .map_err(|e| format!("binding {address}: {e}"))?;
        eprintln!(
            "lapi-feed {} listening on {address}",
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
