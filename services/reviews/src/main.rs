//! `lapi-reviews`: reviews players send from the launcher, at
//! reviews.lapi.sh (D-070). `serve` runs the API; the owner reads and answers
//! reviews with the other commands on the server (docs/REVIEWS.md):
//!
//! ```text
//! lapi-reviews serve
//! lapi-reviews overview [--days N]            numbers only (fit for a public log)
//! lapi-reviews list [--status S] [--kind K] [--category C] [--days N] [--spam] [--limit N]
//! lapi-reviews show <id> [--file NAME] [--translate ru]
//! lapi-reviews reply <id> <text> | --from <file>
//! lapi-reviews status <id> new|read|answered|closed|spam
//! lapi-reviews note <id> <text>
//! lapi-reviews ban install|account|net <review-id> [days] [--reason TEXT]
//! lapi-reviews bans | lift <ban-id>
//! lapi-reviews pause | resume | pow [bits]
//! lapi-reviews remove <id> | forget <install-id> | prune | backup
//! ```
//!
//! Everything printed is cleaned of control and direction characters first,
//! so a review cannot drive the terminal.
//!
//! Settings come from the environment: `LAPI_REVIEWS_DATA` (default
//! `/home/www/lapi/reviews/data`), `LAPI_REVIEWS_ADDR` (`127.0.0.1:8790`),
//! `LAPI_REVIEWS_MAX_STORED_MB` (10240), `LAPI_REVIEWS_ISSUER`
//! (`https://oauth.lapi.sh`) and `LAPI_REVIEWS_JWKS` (the account service's
//! keys, `http://127.0.0.1:8783/.well-known/jwks.json`). `--translate` reads
//! `LAPI_REVIEWS_DEEPL_KEY`, or the service's `secrets.env`.

mod api;
mod auth;
mod files;
mod model;
mod pow;
mod store;

use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use store::Store;

/// Where the server keeps reviews; the commands run through `sudo`, which
/// clears the environment, so this is the default.
const DATA: &str = "/home/www/lapi/reviews/data";
const SECRETS: &str = "/home/www/lapi/reviews/secrets.env";
const DAY: i64 = 86_400;

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

fn number(args: &[String], name: &str, default: i64) -> i64 {
    flag(args, name)
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

/// Text as it is printed: control characters (but newlines and tabs) and the
/// characters that reorder text become U+FFFD.
fn plain(text: &str) -> String {
    model::clean(text)
}

macro_rules! out {
    ($($arg:tt)*) => { println!("{}", plain(&format!($($arg)*))) };
}

fn open() -> Result<Store, String> {
    Store::open(&PathBuf::from(env("LAPI_REVIEWS_DATA", DATA)))
}

fn id_at(args: &[String], at: usize) -> Result<i64, String> {
    args.get(at)
        .and_then(|id| id.parse().ok())
        .filter(|id| *id > 0)
        .ok_or_else(|| "give the review's number (lapi-reviews list shows them)".to_owned())
}

fn time(at: i64) -> String {
    chrono::DateTime::from_timestamp(at, 0)
        .map(|t| t.format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_default()
}

/// The text after the review's number, or the file of `--from`.
fn text_argument(args: &[String]) -> Result<String, String> {
    let text = match flag(args, "--from") {
        Some(path) => std::fs::read_to_string(path).map_err(|e| format!("reading {path}: {e}"))?,
        None => args.get(2..).unwrap_or_default().join(" "),
    };
    let text = model::clean(text.trim());
    if text.is_empty() || text.chars().count() > 5000 {
        return Err("the text is empty or longer than 5000 characters".to_owned());
    }
    Ok(text)
}

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("serve") => serve(),
        Some("--version") => {
            println!("lapi-reviews {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        Some(command) => run(command, &args),
        None => Err(USAGE.to_owned()),
    };
    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("lapi-reviews: {}", plain(&error).replace('\n', " "));
            std::process::ExitCode::FAILURE
        }
    }
}

const USAGE: &str = "usage: lapi-reviews serve|overview|list|show|reply|status|note|ban|bans|lift|pause|resume|pow|remove|forget|prune|backup (docs/REVIEWS.md)";

fn run(command: &str, args: &[String]) -> Result<(), String> {
    let store = open()?;
    let now = api::now();
    match command {
        "overview" => {
            let overview = store.overview(number(args, "--days", 7), now)?;
            out!(
                "{}",
                serde_json::to_string_pretty(&overview).unwrap_or_default()
            );
        }
        "list" => {
            let days = flag(args, "--days").and_then(|d| d.parse::<i64>().ok());
            let filter = store::Filter {
                status: flag(args, "--status"),
                kind: flag(args, "--kind"),
                category: flag(args, "--category"),
                since: days.map(|d| now - d * DAY),
                suspect: args.iter().any(|a| a == "--spam"),
                limit: number(args, "--limit", 50),
            };
            for line in store.list(&filter)? {
                out!(
                    "{:>6}  {}  {:<10} {:<9} {:<26} {}files:{}  {}",
                    line.id,
                    time(line.received),
                    line.status,
                    line.kind,
                    format!("{}/{}", line.category, line.subcategory),
                    if line.suspect { "spam? " } else { "" },
                    line.files,
                    line.start
                );
            }
        }
        "show" => {
            let id = id_at(args, 1)?;
            let review = store.get(id)?.ok_or("no such review")?;
            if let Some(name) = flag(args, "--file") {
                let (kind, bytes) = store
                    .file(id, name)?
                    .ok_or_else(|| format!("no file {name} in review {id}"))?;
                if kind == "text" {
                    print!("{}", plain(&String::from_utf8_lossy(&bytes)));
                } else {
                    // A picture goes to a file or a pipe, never a terminal.
                    use std::io::IsTerminal;
                    if std::io::stdout().is_terminal() {
                        return Err(format!(
                            "{name} is a picture: send it to a file (… --file {name} > {name})"
                        ));
                    }
                    std::io::stdout()
                        .write_all(&bytes)
                        .map_err(|e| e.to_string())?;
                }
                return Ok(());
            }
            store.seen(id)?;
            out!(
                "{}",
                serde_json::to_string_pretty(&review).unwrap_or_default()
            );
            if let Some(language) = flag(args, "--translate") {
                let translated = translate(&review.message, language)?;
                out!("\n--- {language} (DeepL) ---\n{translated}");
            }
        }
        "reply" => {
            let id = id_at(args, 1)?;
            store.reply(id, &text_argument(args)?, now)?;
            out!("review {id}: answered; the player sees it in the launcher");
        }
        "status" => {
            let id = id_at(args, 1)?;
            store.set_status(id, args.get(2).map(String::as_str).unwrap_or_default())?;
            out!("review {id}: {}", args[2]);
        }
        "note" => {
            let id = id_at(args, 1)?;
            store.note(id, &text_argument(args)?)?;
            out!("review {id}: note kept (the player does not see it)");
        }
        "ban" => {
            let kind = args.get(1).map(String::as_str).unwrap_or_default();
            let review = id_at(args, 2)?;
            let days = args.get(3).and_then(|d| d.parse::<i64>().ok());
            let until = days.map(|d| now + d * DAY);
            let ban = store.ban(kind, review, until, flag(args, "--reason"), now)?;
            store.set_status(review, "spam")?;
            out!(
                "ban {ban}: {kind} of review {review}, {}",
                days.map_or("for good".to_owned(), |d| format!("{d} days"))
            );
        }
        "bans" => {
            for ban in store.bans(now)? {
                out!(
                    "{:>4}  {:<8} since {}  until {}  {}",
                    ban.id,
                    ban.kind,
                    time(ban.made),
                    ban.until.map_or("for good".to_owned(), time),
                    ban.reason.unwrap_or_default()
                );
            }
        }
        "lift" => {
            let ban = id_at(args, 1)?;
            store.lift(ban)?;
            out!("ban {ban} lifted");
        }
        "pause" => {
            store.set_setting("paused", "1")?;
            out!("reviews are not taken; the launcher keeps them until 'resume'");
        }
        "resume" => {
            store.set_setting("paused", "0")?;
            out!("reviews are taken again");
        }
        "pow" => match args.get(1) {
            None => out!(
                "proof of work: {} bits",
                store
                    .setting("pow_bits")?
                    .unwrap_or_else(|| pow::DEFAULT_BITS.to_string())
            ),
            Some(bits) => {
                let bits: u8 = bits
                    .parse()
                    .ok()
                    .filter(|b| *b <= pow::MAX_BITS)
                    .ok_or(format!("bits are 0 to {}", pow::MAX_BITS))?;
                store.set_setting("pow_bits", &bits.to_string())?;
                out!("proof of work: {bits} bits");
            }
        },
        "remove" => {
            let id = id_at(args, 1)?;
            store.get(id)?.ok_or("no such review")?;
            store.remove(id)?;
            out!("review {id} removed");
        }
        "forget" => match args.get(1) {
            Some(install) if model::install_id(install) => {
                out!("removed {} reviews", store.forget(install)?);
            }
            _ => return Err("forget needs an install ID".to_owned()),
        },
        "prune" => {
            let (reviews, files) = store.prune(now)?;
            out!("removed {reviews} reviews and the files of {files}");
        }
        "backup" => {
            let data = PathBuf::from(env("LAPI_REVIEWS_DATA", DATA));
            let path = store.backup(&data.join("backups"), 14)?;
            out!("{}", path.display());
        }
        _ => return Err(USAGE.to_owned()),
    }
    Ok(())
}

/// The DeepL key: from the environment, or the service's secrets file
/// (`sudo` clears the environment).
fn deepl_key() -> Option<String> {
    if let Ok(key) = std::env::var("LAPI_REVIEWS_DEEPL_KEY")
        && !key.trim().is_empty()
    {
        return Some(key.trim().to_owned());
    }
    std::fs::read_to_string(SECRETS)
        .ok()?
        .lines()
        .find_map(|l| l.strip_prefix("LAPI_REVIEWS_DEEPL_KEY="))
        .map(|v| v.trim().trim_matches('"').to_owned())
        .filter(|v| !v.is_empty())
}

/// The message in another language; only when the owner asks for it, since
/// the text then goes to DeepL.
fn translate(text: &str, language: &str) -> Result<String, String> {
    if !(2..=7).contains(&language.len())
        || !language
            .chars()
            .all(|c| c.is_ascii_alphabetic() || c == '-')
    {
        return Err("--translate takes a language code such as ru or en".to_owned());
    }
    let key = deepl_key().ok_or("no DeepL key (secret DEEPL_API_KEY, deployed by the workflow)")?;
    let host = if key.ends_with(":fx") {
        "https://api-free.deepl.com"
    } else {
        "https://api.deepl.com"
    };
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;
    runtime.block_on(async {
        let response = reqwest::Client::new()
            .post(format!("{host}/v2/translate"))
            .header("Authorization", format!("DeepL-Auth-Key {key}"))
            .json(&serde_json::json!({
                "text": [text],
                "target_lang": language.to_uppercase(),
            }))
            .timeout(Duration::from_secs(30))
            .send()
            .await
            .map_err(|e| e.without_url().to_string())?;
        if !response.status().is_success() {
            return Err(format!("DeepL answered {}", response.status()));
        }
        let body: serde_json::Value = response
            .json()
            .await
            .map_err(|e| e.without_url().to_string())?;
        body["translations"][0]["text"]
            .as_str()
            .map(str::to_owned)
            .ok_or_else(|| "DeepL sent no translation".to_owned())
    })
}

fn serve() -> Result<(), String> {
    let store = open()?;
    let address = env("LAPI_REVIEWS_ADDR", "127.0.0.1:8790");
    let max_stored = env("LAPI_REVIEWS_MAX_STORED_MB", "10240")
        .parse::<i64>()
        .unwrap_or(10240)
        .saturating_mul(1024 * 1024);
    let jwks_url = env(
        "LAPI_REVIEWS_JWKS",
        "http://127.0.0.1:8783/.well-known/jwks.json",
    );
    let tokens = auth::Tokens::new(&env("LAPI_REVIEWS_ISSUER", "https://oauth.lapi.sh"));
    let runtime = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
    runtime.block_on(async move {
        let app = Arc::new(api::App::new(store, tokens, max_stored));
        let pruning = Arc::clone(&app);
        tokio::spawn(async move {
            loop {
                let app = Arc::clone(&pruning);
                match tokio::task::spawn_blocking(move || app.store.prune(api::now())).await {
                    Ok(Ok((reviews, files))) if reviews + files > 0 => {
                        eprintln!("pruned {reviews} reviews and the files of {files}");
                    }
                    Ok(Err(e)) => eprintln!("pruning failed: {e}"),
                    _ => {}
                }
                tokio::time::sleep(Duration::from_secs(6 * 3600)).await;
            }
        });
        // The account service's keys: at start, then every ten minutes (a
        // minute while there are none).
        let keys = Arc::clone(&app);
        tokio::spawn(async move {
            let http = reqwest::Client::builder()
                .timeout(Duration::from_secs(10))
                .build()
                .unwrap_or_default();
            loop {
                match auth::fetch_jwks(&http, &jwks_url).await {
                    Ok(jwks) => {
                        keys.tokens.set_jwks(&jwks);
                    }
                    Err(e) => eprintln!("the account keys were not fetched: {e}"),
                }
                let wait = if keys.tokens.has_keys() { 600 } else { 60 };
                tokio::time::sleep(Duration::from_secs(wait)).await;
            }
        });
        let listener = tokio::net::TcpListener::bind(&address)
            .await
            .map_err(|e| format!("binding {address}: {e}"))?;
        eprintln!(
            "lapi-reviews {} listening on {address}",
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
