//! `lapi-friends`: friends and chats of launcher accounts at friends.lapi.sh
//! (D-064).
//!
//! `lapi-friends serve` runs the API. On the server, for the owner:
//!
//! ```text
//! lapi-friends counts                 numbers only (fit for a public log)
//! lapi-friends reports [--all] [--limit N]
//!                                     open (or all) reports, newest first,
//!                                     each with a copy of the message
//! lapi-friends resolve <report> [note]  marks a report handled
//! lapi-friends resolve-request <id> [note]  the same for a reported request
//! lapi-friends remove-message <id>    removes a message for everyone
//! lapi-friends mute <account> <hours> [reason]
//!                                     no messages, requests or groups
//! lapi-friends ban <account> [days] [reason]
//!                                     no friends at all (for good without days)
//! lapi-friends lift <account>         ends a mute or ban
//! lapi-friends sanctions              mutes and bans in force
//! lapi-friends backup                 a copy of the database in data/backups
//!                                     (daily, by lapi-friends-backup.timer)
//! ```
//!
//! Settings come from the environment: `LAPI_FRIENDS_DATA` (default
//! `/home/www/lapi/friends/data`), `LAPI_FRIENDS_ADDR` (`127.0.0.1:8784`),
//! `LAPI_FRIENDS_ISSUER` (`https://oauth.lapi.sh`), `LAPI_FRIENDS_JWKS` (the
//! account service's keys, `http://127.0.0.1:8783/.well-known/jwks.json`),
//! `LAPI_FRIENDS_ACCOUNTS` (its internal API, `http://127.0.0.1:8793`) and
//! `LAPI_FRIENDS_REQUIRE_EMAIL` (`auto`, the default: only confirmed
//! addresses once the account service sends letters; `1` always; `0` never).

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

/// The owner's moderation commands. The running service tells the people
/// concerned within seconds.
fn moderate(store: &store::Store, command: &str, args: &[String]) -> Result<(), String> {
    let now = chrono::Utc::now().timestamp();
    let account = |i: usize| -> Result<&str, String> {
        args.get(i)
            .map(String::as_str)
            .filter(|id| text::account_id(id))
            .ok_or_else(|| "give an account ID (lapi-oauth find <nick> shows it)".to_owned())
    };
    let words = |from: usize| -> String {
        text::line(
            &args.get(from..).unwrap_or_default().join(" "),
            text::REASON_MAX,
        )
        .unwrap_or_default()
    };
    let number = |i: usize, what: &str| -> Result<i64, String> {
        args.get(i)
            .and_then(|n| n.parse::<i64>().ok())
            .filter(|n| *n > 0)
            .ok_or_else(|| format!("give {what} as a positive number"))
    };
    match command {
        "resolve" => {
            let report = number(0, "the report")?;
            if !store.resolve(report, &words(1), now)? {
                return Err("no such report".to_owned());
            }
            println!("report {report} resolved");
        }
        "resolve-request" => {
            let report = number(0, "the reported request")?;
            if !store.resolve_request_report(report, &words(1))? {
                return Err("no such reported request".to_owned());
            }
            println!("reported request {report} resolved");
        }
        "remove-message" => {
            let message = number(0, "the message")?;
            if !store.remove_message(message)? {
                return Err("no such message".to_owned());
            }
            println!("message {message} removed");
        }
        "mute" => {
            let who = account(0)?;
            let hours = number(1, "the hours")?.min(24 * 365);
            store.sanction(who, "mute", now + hours * 3600, &words(2), now)?;
            println!("{who} muted for {hours} h");
        }
        "ban" => {
            let who = account(0)?;
            // Days are optional; without them the ban is for good.
            let (until, reason_from) = match args.get(1).and_then(|n| n.parse::<i64>().ok()) {
                Some(days) if days > 0 => (now + days.min(3650) * 86_400, 2),
                _ => (0, 1),
            };
            store.sanction(who, "ban", until, &words(reason_from), now)?;
            println!("{who} banned{}", if until == 0 { " for good" } else { "" });
        }
        "lift" => {
            let who = account(0)?;
            if !store.lift(who)? {
                return Err("no mute or ban on that account".to_owned());
            }
            println!("{who}: lifted");
        }
        _ => {
            for (who, sanction) in store.sanctions(now)? {
                println!(
                    "{who}  {}  until {}  {}",
                    sanction.kind,
                    if sanction.until == 0 {
                        "for good".to_owned()
                    } else {
                        sanction.until.to_string()
                    },
                    plain(&sanction.reason)
                );
            }
        }
    }
    Ok(())
}

fn open() -> Result<store::Store, String> {
    store::Store::open(&PathBuf::from(env("LAPI_FRIENDS_DATA", DATA)))
}

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("serve") => serve(),
        Some("backup") => open().and_then(|store| {
            let data = PathBuf::from(env("LAPI_FRIENDS_DATA", DATA));
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
        Some("reports") => open().and_then(|store| {
            let limit = args
                .iter()
                .position(|a| a == "--limit")
                .and_then(|i| args.get(i + 1))
                .and_then(|n| n.parse().ok())
                .unwrap_or(30);
            let all = args.iter().any(|a| a == "--all");
            for report in store.reports(all, limit)? {
                println!("{}", plain(&report.to_string()));
            }
            for report in store.request_reports(all, limit)? {
                println!("{}", plain(&report.to_string()));
            }
            Ok(())
        }),
        Some(
            command @ ("resolve" | "resolve-request" | "remove-message" | "mute" | "ban" | "lift"
            | "sanctions"),
        ) => {
            open().and_then(|store| moderate(&store, command, &args[1..]))
        }
        Some("--version") => {
            println!("lapi-friends {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        _ => Err(
            "usage: lapi-friends serve|counts|reports|resolve|resolve-request|remove-message|mute|ban|lift|sanctions|backup"
                .to_owned(),
        ),
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
        deleted_since: std::sync::Mutex::new(0),
        limiter: api::Limiter::default(),
        email: api::EmailRule::new(match env("LAPI_FRIENDS_REQUIRE_EMAIL", "auto").as_str() {
            "0" => api::EmailMode::Off,
            "1" => api::EmailMode::On,
            _ => api::EmailMode::Auto,
        }),
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
        // Sign-outs, switched-off and deleted accounts, and what the owner
        // did from the command line, reach friends within seconds.
        let checking = Arc::clone(&app);
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(15)).await;
                api::check_mail(&checking).await;
                api::check_sessions(&checking).await;
                api::tell_moderation(&checking);
                api::purge_deleted(&checking).await;
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
