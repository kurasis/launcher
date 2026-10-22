//! `lapi-stats`: anonymous statistics of the launcher and counts of the
//! server ads, at stats.lapi.sh (D-071).
//!
//! `lapi-stats serve` runs the HTTP API, which only takes counts. The other
//! commands read them on the server, for the owner (docs/STATS.md); nothing
//! reads them from outside, and their numbers never go to a public log:
//!
//! ```text
//! lapi-stats overview [--days 30]       players, new, retention, countries
//! lapi-stats players [--days 30]        per day
//! lapi-stats cut <cut> [--days 30] [--period day|week|month|new] [--limit 30]
//! lapi-stats retention [--weeks 12] [--country RU]
//! lapi-stats ads [--days 30]            every ad: shown, reach, Play, joined
//! lapi-stats ad <id> [--days 30]        one ad: from Play to the server, online
//! lapi-stats ord <YYYY-MM>              impressions per erid for the ad register
//! lapi-stats backup                     a copy of the database in data/backups
//! lapi-stats prune                      what is kept no longer, removed
//! ```
//!
//! Tables take `--csv`. Everything printed is cleaned of control and
//! direction characters first.
//!
//! Settings come from the environment: `LAPI_STATS_DATA` (default
//! `/home/www/lapi/stats/data`), `LAPI_STATS_ADDR` (`127.0.0.1:8791`),
//! `LAPI_STATS_SERVERS` (the ads service on this server,
//! `http://127.0.0.1:8789`) and `LAPI_STATS_WORK_BITS` (20).

mod ads;
mod api;
mod model;
mod pow;
mod report;
mod store;
mod tally;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use store::Store;

/// Where the server keeps the counts (D-071); the commands run through
/// `sudo`, which clears the environment, so these are the defaults.
const DATA: &str = "/home/www/lapi/stats/data";
const SERVERS: &str = "http://127.0.0.1:8789";

fn env(name: &str, default: &str) -> String {
    std::env::var(name)
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| default.to_owned())
}

fn flag<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .map(String::as_str)
}

fn number(args: &[String], name: &str, default: i64) -> Result<i64, String> {
    match flag(args, name) {
        None => Ok(default),
        Some(value) => value
            .parse()
            .ok()
            .filter(|n| (1..=3660).contains(n))
            .ok_or_else(|| format!("{name} takes a number from 1 to 3660")),
    }
}

/// Characters that change how text around them is laid out or hide it.
pub fn invisible(c: char) -> bool {
    matches!(c,
        '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2060}'..='\u{2069}'
        | '\u{FEFF}' | '\u{FFF9}'..='\u{FFFB}' | '\u{061C}' | '\u{180E}')
}

/// Text as it is printed: control characters (but newlines and tabs) and the
/// characters that reorder text become U+FFFD.
fn plain(text: &str) -> String {
    text.chars()
        .map(|c| {
            if (c.is_control() && c != '\n' && c != '\t') || invisible(c) {
                '\u{FFFD}'
            } else {
                c
            }
        })
        .collect()
}

fn open() -> Result<Store, String> {
    Store::open(&PathBuf::from(env("LAPI_STATS_DATA", DATA)))
}

fn bits() -> u32 {
    env("LAPI_STATS_WORK_BITS", &pow::DEFAULT_BITS.to_string())
        .parse()
        .unwrap_or(pow::DEFAULT_BITS)
        .clamp(pow::MIN_BITS, pow::MAX_BITS)
}

fn today() -> chrono::NaiveDate {
    chrono::Utc::now().date_naive()
}

const USAGE: &str = "usage: lapi-stats serve|overview|players|cut <cut>|retention|ads|ad <id>|ord <YYYY-MM>|backup|prune (docs/STATS.md)";

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let csv = args.iter().any(|a| a == "--csv");
    let printed = match args.first().map(String::as_str) {
        Some("serve") => serve().map(|()| String::new()),
        Some("--version") => Ok(format!("lapi-stats {}\n", env!("CARGO_PKG_VERSION"))),
        Some(command) => run(command, &args, csv),
        None => Err(USAGE.to_owned()),
    };
    match printed {
        Ok(text) => {
            print!("{}", plain(&text));
            std::process::ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("lapi-stats: {}", plain(&error));
            std::process::ExitCode::FAILURE
        }
    }
}

fn run(command: &str, args: &[String], csv: bool) -> Result<String, String> {
    let store = open()?;
    let today = today();
    let days = number(args, "--days", 30)?;
    match command {
        "overview" => report::overview(&store, today, days),
        "players" => report::players(&store, today, days, csv),
        "cut" => {
            let names = model::CUTS.join(", ");
            let cut = args
                .get(1)
                .and_then(|name| report::cut_name(name))
                .ok_or_else(|| format!("give a cut: {names}"))?;
            let period = match flag(args, "--period") {
                None => model::Period::Day,
                Some(name) => {
                    report::period_name(name).ok_or("--period is day, week, month or new")?
                }
            };
            let limit = number(args, "--limit", 30)? as usize;
            report::cut(&store, today, (cut, period), days, limit, csv)
        }
        "retention" => {
            let country = flag(args, "--country").map(model::country);
            report::retention(
                &store,
                today,
                number(args, "--weeks", 12)?,
                country.as_deref(),
                csv,
            )
        }
        "ads" => report::ads(&store, today, days, csv),
        "ad" => {
            let ad = args
                .get(1)
                .and_then(|id| id.parse::<i64>().ok())
                .filter(|id| *id > 0)
                .ok_or("give the ad's number (lapi-stats ads shows them)")?;
            let online = if csv { Err(String::new()) } else { online(ad) };
            report::ad(&store, today, (ad, days), online, csv)
        }
        "ord" => {
            let month = args
                .get(1)
                .ok_or("give the month: lapi-stats ord 2026-10")?;
            report::ord(&store, month, csv)
        }
        "backup" => {
            let data = PathBuf::from(env("LAPI_STATS_DATA", DATA));
            store
                .backup(&data.join("backups"), 14)
                .map(|path| format!("{}\n", path.display()))
        }
        "prune" => store.prune(today).map(|()| "pruned\n".to_owned()),
        _ => Err(USAGE.to_owned()),
    }
}

/// The ad's online players by hour, from the ads service on this server.
fn online(ad: i64) -> Result<ads::Online, String> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;
    runtime.block_on(async {
        let http = ads::client()?;
        ads::online(&http, &env("LAPI_STATS_SERVERS", SERVERS), ad).await
    })
}

/// Runs `job` every `every`, from now on; a failure is logged (our words
/// only) and tried again next time.
fn every<F>(name: &'static str, every: Duration, app: Arc<api::App>, job: F)
where
    F: Fn(&api::App) -> Result<(), String> + Send + Sync + 'static,
{
    let job = Arc::new(job);
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(every).await;
            let (app, job) = (Arc::clone(&app), Arc::clone(&job));
            match tokio::task::spawn_blocking(move || job(&app)).await {
                Ok(Err(e)) => eprintln!("{name}: {e}"),
                Err(_) => eprintln!("{name}: stopped"),
                Ok(Ok(())) => {}
            }
        }
    });
}

fn serve() -> Result<(), String> {
    let store = open()?;
    let address = env("LAPI_STATS_ADDR", "127.0.0.1:8791");
    let servers = env("LAPI_STATS_SERVERS", SERVERS);
    let app = Arc::new(api::App::new(store, bits(), api::now)?);
    let runtime = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
    let result = runtime.block_on(async {
        every("write", tally::FLUSH_EVERY, Arc::clone(&app), |app| {
            match app.tally.take_lost() {
                0 => {}
                n => eprintln!("write: {n} rows did not fit in memory and are lost"),
            }
            match app.flush()? {
                0 => {}
                n => eprintln!("write: {n} new rows went to other (a limit is reached)"),
            }
            Ok(())
        });
        every(
            "prune",
            Duration::from_secs(6 * 3600),
            Arc::clone(&app),
            |app| app.store.prune(app.today()),
        );
        let polling = Arc::clone(&app);
        tokio::spawn(async move {
            let http = match ads::client() {
                Ok(http) => http,
                Err(e) => {
                    eprintln!("ads: {e}");
                    return;
                }
            };
            loop {
                match ads::fetch(&http, &servers).await {
                    Ok(shown) => {
                        let day = polling.today();
                        let app = Arc::clone(&polling);
                        let saved = tokio::task::spawn_blocking(move || {
                            app.store.save_ads(day, &shown).map(|()| {
                                app.shown.note(day, &shown);
                                shown.len()
                            })
                        })
                        .await;
                        if let Ok(Err(e)) = saved {
                            eprintln!("ads: {e}");
                        }
                    }
                    Err(e) => eprintln!("ads: {e}"),
                }
                tokio::time::sleep(ads::POLL_EVERY).await;
            }
        });
        let listener = tokio::net::TcpListener::bind(&address)
            .await
            .map_err(|e| format!("binding {address}: {e}"))?;
        eprintln!(
            "lapi-stats {} listening on {address}",
            env!("CARGO_PKG_VERSION")
        );
        axum::serve(listener, api::router(Arc::clone(&app)))
            .with_graceful_shutdown(stopped())
            .await
            .map_err(|e| e.to_string())
    });
    // What was counted since the last write is kept before the process ends.
    if let Err(e) = app.flush() {
        eprintln!("write at exit: {e}");
    }
    result
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

#[cfg(test)]
#[test]
fn printed_text_cannot_drive_the_terminal() {
    assert_eq!(
        plain("a\u{1b}]52;c;cm0gLXJm\u{7}b\u{202E}c\u{9b}d\n\te\u{2066}"),
        "a\u{FFFD}]52;c;cm0gLXJm\u{FFFD}b\u{FFFD}c\u{FFFD}d\n\te\u{FFFD}"
    );
}
