//! `lapi-servers`: ads for Minecraft servers in the launcher, at
//! servers.lapi.sh (D-069). The service only answers reads; the owner
//! writes ads with these commands on the server (docs/SERVER-ADS.md):
//!
//! ```text
//! lapi-servers serve                 the API, an hourly ping of each server
//! lapi-servers add --name … --address … --version … [--from … --to …]
//!              --text … | --text-file … [--text-ru … --text-en-file …]
//!              [--start "2026-10-21 14:00"] --for 7d | --until "…"
//!              [--weight 10] [--place feed] [--erid …] [--if-new] [--quiet]
//! lapi-servers list [--all]          ads now and to come (all: ended too)
//! lapi-servers show <id>             one ad and its last ping
//! lapi-servers edit <id> [flags of add] [--clear-erid]
//! lapi-servers extend <id> <30d>     shows longer
//! lapi-servers pause|resume <id>
//! lapi-servers stop <id>             ends now, kept in the list
//! lapi-servers remove <id>           gone for good
//! lapi-servers ping <id>             asks the server now
//! lapi-servers icon <id> <file.png> | --auto
//! lapi-servers counts                numbers only (fit for a public log)
//! lapi-servers backup                a copy of the database in data/backups
//! ```
//!
//! Settings come from the environment: `LAPI_SERVERS_DATA` (default
//! `/home/www/lapi/servers/data`), `LAPI_SERVERS_ADDR` (`127.0.0.1:8789`) and
//! `LAPI_SERVERS_TZ` (the offset times are typed in, `+03:00`).

mod ads;
mod api;
mod icons;
mod ping;
mod releases;
mod store;
mod text;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

const DATA: &str = "/home/www/lapi/servers/data";
/// Each server is asked this often.
const PING_EVERY: Duration = Duration::from_secs(3600);

fn env(name: &str, default: &str) -> String {
    std::env::var(name)
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| default.to_owned())
}

fn data() -> PathBuf {
    PathBuf::from(env("LAPI_SERVERS_DATA", DATA))
}

fn zone() -> Result<chrono::FixedOffset, String> {
    text::offset(&env("LAPI_SERVERS_TZ", "+03:00"))
}

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("serve") => serve(),
        Some("--version") => {
            println!("lapi-servers {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        Some(command) => run(command, &args[1..]),
        None => Err(USAGE.to_owned()),
    };
    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            let error: String = error
                .chars()
                .map(|c| if c.is_control() { '\u{FFFD}' } else { c })
                .collect();
            eprintln!("lapi-servers: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}

const USAGE: &str = "usage: lapi-servers serve|add|list|show|edit|extend|pause|resume|stop|remove|ping|icon|counts|backup (docs/SERVER-ADS.md)";

fn runtime() -> Result<tokio::runtime::Runtime, String> {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())
}

fn id_of(plain: &[String]) -> Result<i64, String> {
    plain
        .first()
        .and_then(|id| id.parse().ok())
        .filter(|id| *id > 0)
        .ok_or_else(|| "give the ad's number (lapi-servers list shows them)".to_owned())
}

fn run(command: &str, args: &[String]) -> Result<(), String> {
    let data = data();
    let store = store::Store::open(&data)?;
    let now = api::now();
    let (plain, flags) = ads::Flags::parse(args)?;
    match command {
        "counts" => {
            let counts = store.counts(now)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&counts).unwrap_or_default()
            );
            Ok(())
        }
        "backup" => {
            let path = store.backup(&data.join("backups"), 14)?;
            println!("{}", path.display());
            Ok(())
        }
        "list" => {
            let zone = zone()?;
            for ad in store.all()? {
                if flags.has("--all") || store::state(&ad, now) != "ended" {
                    println!("{}", ads::line(&ad, now, zone));
                }
            }
            Ok(())
        }
        "show" => {
            let ad = store.ad(id_of(&plain)?)?.ok_or("no such ad")?;
            show(&store, &ad, now)
        }
        "add" | "edit" => runtime()?.block_on(async {
            let zone = zone()?;
            let http = releases::client()?;
            if let Err(e) = releases::refresh(&store, &http, now, false).await {
                // An old list still checks versions; no list does not.
                if store.releases_at()?.is_none() {
                    return Err(e);
                }
                eprintln!("the release list was not refreshed: {e}");
            }
            let (ad, id) = if command == "add" {
                let ad = ads::build(&store, None, &flags, zone, now)?;
                if flags.has("--if-new")
                    && let Some(same) = store
                        .all()?
                        .into_iter()
                        .find(|a| a.address == ad.address && a.ends > now)
                {
                    println!("ad {} shows this address already; nothing added", same.id);
                    return Ok(());
                }
                let id = store.insert(&ad, now)?;
                (store::Ad { id, ..ad }, id)
            } else {
                let id = id_of(&plain)?;
                let base = store.ad(id)?.ok_or("no such ad")?;
                let ad = ads::build(&store, Some(&base), &flags, zone, now)?;
                store.update(&ad, now)?;
                (ad, id)
            };
            let ping = if flags.has("--no-ping") {
                None
            } else {
                let icons = icons::Icons::new(&data)?;
                Some(ads::ping(&store, &icons, &ping::Pinger::new(), &ad, now).await?)
            };
            let verb = if command == "add" { "added" } else { "changed" };
            if flags.has("--quiet") {
                // Fixed words and numbers only: fit for a public log.
                let answer = match ping {
                    Some(p) if p.up => "the server answers",
                    Some(_) => "the server does not answer",
                    None => "not pinged",
                };
                println!("{verb} ad {id}; {answer}");
                Ok(())
            } else {
                println!("{verb} ad {id}");
                show(&store, &ad, now)
            }
        }),
        "extend" => {
            let id = id_of(&plain)?;
            let mut ad = store.ad(id)?.ok_or("no such ad")?;
            let run = plain.get(1).ok_or("give how much longer: 30d")?;
            ad.ends = ad.ends.max(now) + text::duration(run)?;
            ads::check_run(&ad, now)?;
            store.update(&ad, now)?;
            println!("{}", ads::line(&ad, now, zone()?));
            Ok(())
        }
        "pause" | "resume" | "stop" => {
            let id = id_of(&plain)?;
            let mut ad = store.ad(id)?.ok_or("no such ad")?;
            match command {
                "pause" => ad.paused = true,
                "resume" => ad.paused = false,
                _ => ad.ends = ad.ends.min(now),
            }
            store.update(&ad, now)?;
            println!("{}", ads::line(&ad, now, zone()?));
            Ok(())
        }
        "remove" => {
            let id = id_of(&plain)?;
            store.remove(id)?;
            icons::Icons::new(&data)?.clean(&store.used_icons()?);
            println!("removed ad {id}");
            Ok(())
        }
        "ping" => runtime()?.block_on(async {
            let ad = store.ad(id_of(&plain)?)?.ok_or("no such ad")?;
            let icons = icons::Icons::new(&data)?;
            ads::ping(&store, &icons, &ping::Pinger::new(), &ad, now).await?;
            show(&store, &ad, now)
        }),
        "icon" => {
            let id = id_of(&plain)?;
            let mut ad = store.ad(id)?.ok_or("no such ad")?;
            let icons = icons::Icons::new(&data)?;
            if flags.has("--auto") {
                ad.icon = None;
            } else {
                let file = plain.get(1).ok_or("give a PNG file, or --auto")?;
                let meta = std::fs::metadata(file).map_err(|e| format!("{file}: {e}"))?;
                if meta.len() > icons::MAX_BYTES as u64 {
                    return Err(format!("{file} is larger than 1 MB"));
                }
                let bytes = std::fs::read(file).map_err(|e| format!("{file}: {e}"))?;
                ad.icon = Some(icons.keep(&icons::clean(&bytes)?)?);
            }
            store.update(&ad, now)?;
            icons.clean(&store.used_icons()?);
            println!(
                "ad {id}: {}",
                if ad.icon.is_some() {
                    "own icon"
                } else {
                    "the server's icon"
                }
            );
            Ok(())
        }
        _ => Err(USAGE.to_owned()),
    }
}

/// Everything about an ad, for the owner's terminal.
fn show(store: &store::Store, ad: &store::Ad, now: i64) -> Result<(), String> {
    let zone = zone()?;
    println!("{}", ads::line(ad, now, zone));
    println!(
        "  version {} (from {} to {})",
        ad.version, ad.oldest, ad.newest
    );
    for (lang, text) in &ad.texts {
        let lang = if lang == "*" { "text" } else { lang.as_str() };
        println!("  {lang}: {}", text.replace('\n', " / "));
    }
    if let Some(erid) = &ad.erid {
        println!("  erid {erid}");
    }
    match store.ping(ad.id)? {
        Some(p) if p.up => println!(
            "  pinged {}: answers, {}/{} online, version {:?}, {}",
            ads::local(p.at, zone),
            p.online.unwrap_or(0),
            p.max.unwrap_or(0),
            p.server_version.unwrap_or_default(),
            if ad.icon.is_some() {
                "own icon"
            } else if p.icon.is_some() {
                "the server's icon"
            } else {
                "no icon"
            }
        ),
        Some(p) => println!("  pinged {}: does not answer", ads::local(p.at, zone)),
        None => println!("  not pinged yet"),
    }
    let versions = store.range(&ad.oldest, &ad.newest)?;
    println!("  players choose from {} releases", versions.len());
    Ok(())
}

/// Runs `job` every `every`, from now on; a failure is logged (our words
/// only) and tried again next time.
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
    let data = data();
    let app = Arc::new(api::App {
        store: store::Store::open(&data)?,
        icons: icons::Icons::new(&data)?,
        clock: api::now,
    });
    let address = env("LAPI_SERVERS_ADDR", "127.0.0.1:8789");
    let http = releases::client()?;
    runtime()?.block_on(async move {
        every(
            "releases",
            Duration::from_secs(3600),
            Arc::clone(&app),
            move |app| {
                let http = http.clone();
                async move { releases::refresh(&app.store, &http, api::now(), false).await }
            },
        );
        every("ping", PING_EVERY, Arc::clone(&app), |app| async move {
            let pinger = ping::Pinger::new();
            let now = api::now();
            let due = app.store.to_ping(now, PING_EVERY.as_secs() as i64)?;
            let mut answering = 0;
            for ad in &due {
                if ads::ping(&app.store, &app.icons, &pinger, ad, now)
                    .await?
                    .up
                {
                    answering += 1;
                }
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
            app.icons.clean(&app.store.used_icons()?);
            eprintln!("ping: {answering} of {} servers answer", due.len());
            Ok(())
        });
        let listener = tokio::net::TcpListener::bind(&address)
            .await
            .map_err(|e| format!("binding {address}: {e}"))?;
        eprintln!(
            "lapi-servers {} listening on {address}",
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
