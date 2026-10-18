//! `lapi-oauth`: launcher accounts at oauth.lapi.sh (D-064).
//!
//! `lapi-oauth serve` runs the pages and the API. The other commands are for
//! the owner on the server, to help a player by hand:
//!
//! ```text
//! lapi-oauth find <nick|email|id>     the account, its sessions and events
//! lapi-oauth disable <id> | enable <id>
//! lapi-oauth sign-out <id>            ends every session of the account
//! lapi-oauth set-email <id> <email>   a new address, to be confirmed
//! lapi-oauth confirm-email <id>       marks the address confirmed
//! lapi-oauth counts                   numbers only (fit for a public log)
//! ```
//!
//! Settings come from the environment: `LAPI_OAUTH_DATA` (default
//! `/home/www/lapi/oauth/data`), `LAPI_OAUTH_ADDR` (`127.0.0.1:8783`),
//! `LAPI_OAUTH_URL` (`https://oauth.lapi.sh`), `LAPI_OAUTH_TURNSTILE_SITE_KEY`
//! and `LAPI_OAUTH_TURNSTILE_SECRET` (without the secret nobody can register),
//! `LAPI_OAUTH_SMTP_URL` and `LAPI_OAUTH_MAIL_FROM` (without them letters wait).

mod api;
mod app;
mod crypto;
mod mail;
mod pages;
mod rules;
mod store;
mod web;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use store::Store;

fn env(name: &str, default: &str) -> String {
    std::env::var(name)
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| default.to_owned())
}

/// Where the server keeps accounts; the commands run through `sudo`, which
/// clears the environment, so this is the default.
const DATA: &str = "/home/www/lapi/oauth/data";

/// Text as it is printed: control and direction characters become U+FFFD, so
/// what a player typed cannot drive the owner's terminal.
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

macro_rules! out {
    ($($arg:tt)*) => { println!("{}", plain(&format!($($arg)*))) };
}

fn data_dir() -> PathBuf {
    PathBuf::from(env("LAPI_OAUTH_DATA", DATA))
}

fn open() -> Result<Store, String> {
    Store::open(&data_dir())
}

fn time(at: i64) -> String {
    chrono::DateTime::from_timestamp(at, 0)
        .map(|t| t.format("%Y-%m-%d %H:%M UTC").to_string())
        .unwrap_or_default()
}

fn find(store: &Store, term: &str) -> Result<store::Account, String> {
    let found = if uuid::Uuid::parse_str(term).is_ok() {
        store.account(term)?
    } else {
        store.find_login(term)?
    };
    found.ok_or_else(|| "no such account".to_owned())
}

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let arg = |i: usize| args.get(i).map(String::as_str);
    let now = app::now();
    let result: Result<(), String> = match arg(0) {
        Some("serve") => serve(),
        Some("find") => match arg(1) {
            Some(term) => open().and_then(|store| {
                let a = find(&store, term)?;
                out!("id:         {}", a.id);
                out!("nick:       {}", a.nick);
                out!(
                    "email:      {} ({})",
                    a.email,
                    if a.email_verified {
                        "confirmed"
                    } else {
                        "not confirmed"
                    }
                );
                out!("created:    {}", time(a.created));
                out!("registered: from {} ({})", a.reg_ip, a.reg_country);
                out!("language:   {}", a.lang);
                out!("disabled:   {}", a.disabled);
                out!("sessions:");
                for s in store.sessions(&a.id, now)? {
                    out!(
                        "  {}  {}  {}  since {}, last {}",
                        s.id,
                        s.device,
                        s.country,
                        time(s.created),
                        time(s.last_used)
                    );
                }
                out!("events:");
                for (at, kind, country) in store.events(&a.id, 30)? {
                    out!("  {}  {kind}  {country}", time(at));
                }
                Ok(())
            }),
            None => Err("find needs a nick, email or ID".to_owned()),
        },
        Some(command @ ("disable" | "enable")) => match arg(1) {
            Some(term) => open().and_then(|store| {
                let a = find(&store, term)?;
                store.set_disabled(&a.id, command == "disable")?;
                if command == "disable" {
                    store.revoke_all(&a.id, None)?;
                }
                store.event(&a.id, command, "XX", now)?;
                out!("{} {command}d", a.nick);
                Ok(())
            }),
            None => Err(format!("{command} needs an account")),
        },
        Some("sign-out") => match arg(1) {
            Some(term) => open().and_then(|store| {
                let a = find(&store, term)?;
                out!("{} sessions ended", store.revoke_all(&a.id, None)?);
                store.event(&a.id, "signed-out-by-owner", "XX", now)
            }),
            None => Err("sign-out needs an account".to_owned()),
        },
        Some("set-email") => match (arg(1), arg(2)) {
            (Some(term), Some(email)) => open().and_then(|store| {
                let a = find(&store, term)?;
                let email = rules::email(email).map_err(|_| "not an email address".to_owned())?;
                match store.set_email(&a.id, &email)? {
                    Ok(()) => {
                        store.event(&a.id, "email-set-by-owner", "XX", now)?;
                        out!("{} now has {email} (not confirmed)", a.nick);
                        Ok(())
                    }
                    Err(_) => Err("another account has this address".to_owned()),
                }
            }),
            _ => Err("set-email needs an account and an address".to_owned()),
        },
        Some("confirm-email") => match arg(1) {
            Some(term) => open().and_then(|store| {
                let a = find(&store, term)?;
                store.verify_email(&a.id, &a.email)?;
                store.event(&a.id, "email-confirmed-by-owner", "XX", now)
            }),
            None => Err("confirm-email needs an account".to_owned()),
        },
        Some("counts") => open().and_then(|store| {
            out!(
                "{}",
                serde_json::to_string_pretty(&store.counts(now)?).unwrap_or_default()
            );
            Ok(())
        }),
        Some("--version") => {
            println!("lapi-oauth {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        _ => Err(
            "usage: lapi-oauth serve|find <who>|disable <who>|enable <who>|sign-out <who>|set-email <who> <email>|confirm-email <who>|counts"
                .to_owned(),
        ),
    };
    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("lapi-oauth: {}", plain(&error));
            std::process::ExitCode::FAILURE
        }
    }
}

fn serve() -> Result<(), String> {
    let data = data_dir();
    let store = open()?;
    let keys = crypto::Keys::load_or_create(&data.join("signing.key"))?;
    let config = app::Config {
        base: env("LAPI_OAUTH_URL", "https://oauth.lapi.sh")
            .trim_end_matches('/')
            .to_owned(),
        turnstile_site_key: env("LAPI_OAUTH_TURNSTILE_SITE_KEY", ""),
    };
    let secret = env("LAPI_OAUTH_TURNSTILE_SECRET", "");
    if secret.is_empty() || config.turnstile_site_key.is_empty() {
        eprintln!("no Turnstile keys: registration and password reset are closed");
    }
    let captcha = app::Turnstile {
        secret,
        host: config.host().to_owned(),
        http: reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .map_err(|e| e.to_string())?,
    };
    let smtp = match (
        std::env::var("LAPI_OAUTH_SMTP_URL")
            .ok()
            .filter(|v| !v.is_empty()),
        std::env::var("LAPI_OAUTH_MAIL_FROM")
            .ok()
            .filter(|v| !v.is_empty()),
    ) {
        (Some(url), Some(from)) => Some(mail::Smtp::new(&url, &from)?),
        _ => {
            eprintln!("no SMTP server: letters wait in the outbox");
            None
        }
    };
    let address = env("LAPI_OAUTH_ADDR", "127.0.0.1:8783");
    let runtime = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
    runtime.block_on(async move {
        let app = Arc::new(app::App::new(config, store, keys, Box::new(captcha)));
        if let Some(smtp) = smtp {
            tokio::spawn(mail::worker(Arc::clone(&app), smtp));
        }
        let pruning = Arc::clone(&app);
        tokio::spawn(async move {
            loop {
                let app = Arc::clone(&pruning);
                if let Ok(Err(e)) =
                    tokio::task::spawn_blocking(move || app.store.prune(app::now())).await
                {
                    eprintln!("pruning failed: {e}");
                }
                tokio::time::sleep(Duration::from_secs(3600)).await;
            }
        });
        let listener = tokio::net::TcpListener::bind(&address)
            .await
            .map_err(|e| format!("binding {address}: {e}"))?;
        eprintln!(
            "lapi-oauth {} listening on {address}",
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
