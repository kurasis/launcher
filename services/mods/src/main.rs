//! `lapi-mods`: the catalog of mods, modpacks, resource packs, shaders and
//! worlds at mods.lapi.sh, its files at files-mods.lapi.sh (D-066).
//!
//! Players add projects of Modrinth (CurseForge comes next) by an address or
//! a name; the service fetches their pages, pictures and files through
//! curl-impersonate and keeps them for good, so the launcher no longer
//! depends on those sites being up. nginx serves the kept files from disk.
//!
//! ```text
//! lapi-mods serve                 the API and the workers
//! lapi-mods counts                numbers only (fit for a public log)
//! lapi-mods backup                a copy of the database in data/backups
//!                                 (daily, by lapi-mods-backup.timer)
//! lapi-mods thumb <in> <out> <size> <webp|jpeg>
//!                                 a small picture; the service runs it as a
//!                                 process of its own for each picture
//! ```
//!
//! Settings come from the environment: `LAPI_MODS_DATA` (default
//! `/home/www/lapi/mods/data`), `LAPI_MODS_ADDR` (`127.0.0.1:8786`),
//! `LAPI_MODS_SETTINGS` (the deploy's settings file with the quota,
//! `/home/www/lapi/mods/settings.json`), `LAPI_MODS_FILES_URL`
//! (`https://files-mods.lapi.sh`), `LAPI_MODS_CURL` (curl-impersonate,
//! `/home/www/lapi/mods/current/curl/curl_chrome150`), `LAPI_MODS_CURL_ARGS`
//! (arguments before ours, space-separated), `LAPI_MODS_PROXIES` (a secret:
//! proxies to the sources), `LAPI_MODS_DIRECT` (`1`: also connect directly
//! when there are proxies), `LAPI_MODS_WORKERS` (`4`), `LAPI_MODS_ISSUER`
//! (`https://oauth.lapi.sh`) and `LAPI_MODS_JWKS`
//! (`http://127.0.0.1:8783/.well-known/jwks.json`).

mod api;
mod auth;
mod blobs;
mod fetch;
mod import;
mod input;
mod model;
mod modrinth;
mod store;
mod thumb;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

fn env(name: &str, default: &str) -> String {
    std::env::var(name)
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| default.to_owned())
}

const DATA: &str = "/home/www/lapi/mods/data";
/// Until the settings file says otherwise.
const DEFAULT_QUOTA_GB: u64 = 50;
/// Left free on the disk whatever the quota.
const MIN_FREE: u64 = 5 * blobs::GB;

/// Where players download what is kept: files-mods.lapi.sh, behind
/// Cloudflare's cache.
#[derive(Debug, Clone)]
pub struct Urls {
    base: String,
}

impl Urls {
    pub fn new(base: &str) -> Self {
        Self {
            base: base.trim_end_matches('/').to_owned(),
        }
    }

    /// A kept file under its own name.
    pub fn file(&self, sha256: &str, name: &str) -> String {
        format!(
            "{}/f/{sha256}/{}",
            self.base,
            percent_encoding::utf8_percent_encode(name, percent_encoding::NON_ALPHANUMERIC)
        )
    }

    /// A kept picture; nginx gives it the type its extension names.
    pub fn image(&self, sha256: &str, media_type: &str) -> String {
        format!("{}/i/{sha256}.{}", self.base, thumb::extension(media_type))
    }
}

fn data() -> PathBuf {
    PathBuf::from(env("LAPI_MODS_DATA", DATA))
}

fn settings() -> PathBuf {
    PathBuf::from(env(
        "LAPI_MODS_SETTINGS",
        "/home/www/lapi/mods/settings.json",
    ))
}

fn open() -> Result<store::Store, String> {
    store::Store::open(&data())
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
        Some("backup") => open().and_then(|store| {
            let path = store.backup(&data().join("backups"), 14)?;
            println!("{}", path.display());
            Ok(())
        }),
        Some("thumb") => match &args[1..] {
            [input, output, size, format] => size
                .parse::<u32>()
                .ok()
                .filter(|s| (16..=2048).contains(s))
                .ok_or_else(|| "bad size".to_owned())
                .and_then(|size| thumb::make(input.as_ref(), output.as_ref(), size, format)),
            _ => Err("usage: lapi-mods thumb <in> <out> <size> <webp|jpeg>".to_owned()),
        },
        Some("--version") => {
            println!("lapi-mods {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        _ => Err("usage: lapi-mods serve|counts|backup|thumb".to_owned()),
    };
    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("lapi-mods: {}", model::clean(&error, 2000, false));
            std::process::ExitCode::FAILURE
        }
    }
}

fn serve() -> Result<(), String> {
    let data = data();
    let store = open()?;
    let blobs = blobs::Blobs::open(&data, DEFAULT_QUOTA_GB * blobs::GB, MIN_FREE)?;
    blobs.reload_quota(&settings());
    // Parts a stop cut short, and jobs it left running.
    blobs.clear_parts();
    store.requeue_running()?;
    let proxies = fetch::parse_proxies(&std::env::var("LAPI_MODS_PROXIES").unwrap_or_default());
    let curl = fetch::Curl {
        program: PathBuf::from(env(
            "LAPI_MODS_CURL",
            "/home/www/lapi/mods/current/curl/curl_chrome150",
        )),
        leading: env("LAPI_MODS_CURL_ARGS", "")
            .split_whitespace()
            .map(str::to_owned)
            .collect(),
    };
    let direct = env("LAPI_MODS_DIRECT", "1") == "1";
    let client = fetch::Client::new(Box::new(curl), proxies, direct, blobs.tmp());
    let workers: usize = env("LAPI_MODS_WORKERS", "4")
        .parse()
        .unwrap_or(4)
        .clamp(1, 32);
    let address = env("LAPI_MODS_ADDR", "127.0.0.1:8786");
    let jwks_url = env(
        "LAPI_MODS_JWKS",
        "http://127.0.0.1:8783/.well-known/jwks.json",
    );
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| e.to_string())?;
    // The count of exits is all a log says of the proxies.
    eprintln!("lapi-mods: {} exits to the sources", client.exits());
    let app = Arc::new(App {
        store,
        blobs,
        client,
        tokens: auth::Tokens::new(&env("LAPI_MODS_ISSUER", "https://oauth.lapi.sh")),
        limiter: api::Limiter::default(),
        urls: Urls::new(&env("LAPI_MODS_FILES_URL", "https://files-mods.lapi.sh")),
        thumbs: import::Thumbs::Process(std::env::current_exe().map_err(|e| e.to_string())?),
    });
    let runtime = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
    runtime.block_on(async move {
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
        // Counting the kept files can take a while on a large disk.
        let counting = Arc::clone(&app);
        tokio::task::spawn_blocking(move || {
            let (bytes, files) = counting.blobs.count();
            eprintln!(
                "lapi-mods: {files} files kept, {:.1} GB",
                bytes as f64 / blobs::GB as f64
            );
        });
        let upkeep = Arc::clone(&app);
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(5)).await;
                import::check_requests(&upkeep);
                upkeep.blobs.reload_quota(&settings());
            }
        });
        let (stop, stopping) = tokio::sync::watch::channel(false);
        for _ in 0..workers {
            tokio::spawn(import::work(Arc::clone(&app), stopping.clone()));
        }
        let listener = tokio::net::TcpListener::bind(&address)
            .await
            .map_err(|e| format!("binding {address}: {e}"))?;
        eprintln!(
            "lapi-mods {} listening on {address}",
            env!("CARGO_PKG_VERSION")
        );
        let served = axum::serve(listener, api::router(Arc::clone(&app)))
            .with_graceful_shutdown(stopped())
            .await
            .map_err(|e| e.to_string());
        let _ = stop.send(true);
        served
    })
}

/// What the API and the workers share.
pub struct App {
    pub store: store::Store,
    pub blobs: blobs::Blobs,
    pub client: fetch::Client,
    pub tokens: auth::Tokens,
    pub limiter: api::Limiter,
    pub urls: Urls,
    pub thumbs: import::Thumbs,
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
