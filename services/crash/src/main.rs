//! `lapi-crash`: the crash report service at crash.lapi.sh (D-061).
//!
//! `lapi-crash serve` runs the HTTP API. The other commands read the stored
//! reports on the server, for the developers:
//!
//! ```text
//! lapi-crash groups [--days N] [--source game|launcher] [--limit N] [--json]
//! lapi-crash list [--kind K] [--signature S] [--limit N]
//! lapi-crash show <id> [--file NAME]
//! lapi-crash stats [--days N]
//! lapi-crash overview [--days N]
//! lapi-crash forget <install-id>
//! lapi-crash prune
//! ```
//!
//! `overview` prints counts per cause only, nothing a player wrote or a log
//! held: it is the one output fit for a public place such as the workflow
//! summary of a public repository. Everything printed is cleaned of control
//! and direction characters first, so a report cannot drive the terminal.
//!
//! Settings come from the environment: `LAPI_CRASH_DATA` (data folder, default
//! `/home/www/lapi/crash/data`),
//! `LAPI_CRASH_ADDR` (default `127.0.0.1:8781`), `LAPI_CRASH_KEEP_DAYS`
//! (reports, default 90), `LAPI_CRASH_KEEP_STATS_DAYS` (default 400) and
//! `LAPI_CRASH_MAX_STORED_MB` (stored reports, default 5120).

mod api;
mod model;
mod store;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use store::Store;

fn env(name: &str, default: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| default.to_owned())
}

/// A whole number from the environment (days, megabytes), or `default`.
fn env_number(name: &str, default: i64) -> i64 {
    env(name, &default.to_string()).parse().unwrap_or(default)
}

fn flag<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .map(String::as_str)
}

fn number(args: &[String], name: &str, default: i64) -> i64 {
    flag(args, name)
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

/// Where the server keeps reports (docs/DECISIONS.md D-061); the commands run
/// through `sudo`, which clears the environment, so this is the default.
const DATA: &str = "/home/www/lapi/crash/data";

/// Text as it is printed: control characters (but newlines and tabs) and the
/// characters that reorder text become U+FFFD.
fn plain(text: &str) -> String {
    text.chars()
        .map(|c| {
            let reorders = matches!(c, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}');
            if (c.is_control() && c != '\n' && c != '\t') || reorders {
                '\u{FFFD}'
            } else {
                c
            }
        })
        .collect()
}

macro_rules! out {
    ($($arg:tt)*) => { println!("{}", plain(&format!($($arg)*))) };
}

fn open() -> Result<Store, String> {
    Store::open(&PathBuf::from(env("LAPI_CRASH_DATA", DATA)))
}

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("serve") => serve(),
        Some("groups") => open().map(|store| {
            let groups = store.groups(
                number(&args, "--days", 30),
                flag(&args, "--source"),
                number(&args, "--limit", 50),
            );
            if args.iter().any(|a| a == "--json") {
                out!(
                    "{}",
                    serde_json::to_string_pretty(&groups).unwrap_or_default()
                );
            } else {
                out!("reports installs  last             source   kind / signature");
                for g in groups {
                    out!(
                        "{:>7} {:>8}  {}  {:<8} {} / {} (up to {})",
                        g.reports,
                        g.installs,
                        g.last,
                        g.source,
                        g.kind,
                        g.signature,
                        g.latest_version
                    );
                }
            }
        }),
        Some("list") => open().map(|store| {
            let listed = store.list(
                flag(&args, "--kind"),
                flag(&args, "--signature"),
                number(&args, "--limit", 30),
            );
            for r in listed {
                out!(
                    "{}  {}  {} {} {}  {} {}  {}  {} B",
                    r.id,
                    r.received,
                    r.source,
                    r.kind,
                    r.signature,
                    r.launcher_version,
                    r.os,
                    r.game,
                    r.bytes
                );
            }
        }),
        Some("show") => match args.get(1) {
            Some(id) => open().and_then(|store| match store.get(id)? {
                None => Err(format!("no report {id}")),
                Some(report) => {
                    match flag(&args, "--file") {
                        Some(name) => match report.files.iter().find(|f| f.name == name) {
                            Some(file) => print!("{}", plain(&file.text)),
                            None => return Err(format!("no file {name} in {id}")),
                        },
                        None => {
                            let mut head = report.clone();
                            for file in &mut head.files {
                                file.text =
                                    format!("<{} bytes; --file {}>", file.text.len(), file.name);
                            }
                            out!(
                                "{}",
                                serde_json::to_string_pretty(&head).unwrap_or_default()
                            );
                        }
                    }
                    Ok(())
                }
            }),
            None => Err("show needs a report ID".to_owned()),
        },
        Some("stats") => open().map(|store| {
            let summary = store.summary(number(&args, "--days", 30));
            out!(
                "{}",
                serde_json::to_string_pretty(&summary).unwrap_or_default()
            );
        }),
        Some("overview") => open().map(|store| {
            out!(
                "{}",
                serde_json::to_string_pretty(&store.overview(number(&args, "--days", 30)))
                    .unwrap_or_default()
            );
        }),
        Some("forget") => match args.get(1) {
            Some(install) if uuid::Uuid::parse_str(install).is_ok() => open().and_then(|store| {
                out!("removed {} reports", store.forget(install)?);
                Ok(())
            }),
            _ => Err("forget needs an install ID".to_owned()),
        },
        Some("prune") => open().and_then(|store| {
            let removed = store.prune(
                env_number("LAPI_CRASH_KEEP_DAYS", 90),
                env_number("LAPI_CRASH_KEEP_STATS_DAYS", 400),
            )?;
            println!("removed {removed} reports");
            Ok(())
        }),
        Some("--version") => {
            println!("lapi-crash {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        _ => Err("usage: lapi-crash serve|groups|list|show <id>|stats|overview|forget <install-id>|prune".to_owned()),
    };
    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("lapi-crash: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn serve() -> Result<(), String> {
    let store = open()?;
    let address = env("LAPI_CRASH_ADDR", "127.0.0.1:8781");
    let keep = (
        env_number("LAPI_CRASH_KEEP_DAYS", 90),
        env_number("LAPI_CRASH_KEEP_STATS_DAYS", 400),
    );
    let runtime = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
    runtime.block_on(async move {
        let max_stored = env_number("LAPI_CRASH_MAX_STORED_MB", 5120).saturating_mul(1024 * 1024);
        let app = Arc::new(api::App::new(store, max_stored));
        let pruning = Arc::clone(&app);
        tokio::spawn(async move {
            loop {
                let app = Arc::clone(&pruning);
                match tokio::task::spawn_blocking(move || app.store.prune(keep.0, keep.1)).await {
                    Ok(Ok(n)) if n > 0 => eprintln!("pruned {n} old reports"),
                    Ok(Err(e)) => eprintln!("pruning failed: {e}"),
                    _ => {}
                }
                tokio::time::sleep(Duration::from_secs(6 * 3600)).await;
            }
        });
        let listener = tokio::net::TcpListener::bind(&address)
            .await
            .map_err(|e| format!("binding {address}: {e}"))?;
        eprintln!(
            "lapi-crash {} listening on {address}",
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

#[cfg(test)]
#[test]
fn printed_text_cannot_drive_the_terminal() {
    assert_eq!(
        plain("a\u{1b}]52;c;cm0gLXJm\u{7}b\u{202E}c\u{9b}d\n\te"),
        "a\u{FFFD}]52;c;cm0gLXJm\u{FFFD}b\u{FFFD}c\u{FFFD}d\n\te"
    );
}
