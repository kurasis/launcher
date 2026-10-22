//! `lapi-mods`: the catalog of mods, modpacks, resource packs, shaders and
//! worlds at mods.lapi.sh, its files at files-mods.lapi.sh (D-066).
//!
//! Players add projects of Modrinth and CurseForge by an address or a name;
//! the service fetches their pages, pictures and files through
//! curl-impersonate and keeps them for good, so the launcher no longer
//! depends on those sites being up. nginx serves the kept files from disk.
//!
//! ```text
//! lapi-mods serve                 the API and the workers
//! lapi-mods counts                numbers only (fit for a public log)
//! lapi-mods backup                a copy of the database in data/backups
//!                                 (daily, by lapi-mods-backup.timer)
//! lapi-mods probe                 how CurseForge answers each exit (our
//!                                 words and numbers only, for a public log)
//! lapi-mods scan <file>          the scanner's verdict on a file
//! lapi-mods find|review|block|unblock|remove|forget|virustotal …
//!                                 the owner's commands (owner.rs)
//! lapi-mods restore               brings back to the disk what only R2 has
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
//! (`http://127.0.0.1:8783/.well-known/jwks.json`). The owner's commands read
//! `LAPI_MODS_CF_ZONE` and `LAPI_MODS_CF_TOKEN` (secrets: Cloudflare's zone
//! and a token that may only purge its cache), `LAPI_MODS_R2_ACCOUNT`,
//! `LAPI_MODS_R2_KEY`, `LAPI_MODS_R2_SECRET` (secrets: R2's account and an
//! access key to its two buckets), `LAPI_MODS_R2_PUBLIC` (`mods-files`),
//! `LAPI_MODS_R2_PRIVATE` (`mods-private`), `LAPI_MODS_VIRUSTOTAL_KEY` (a
//! secret: VirusTotal's API key), `LAPI_MODS_CDN_URL` (the public
//! bucket's address, `https://cdn-mods.lapi.sh`, once it serves; empty:
//! files are copied but served from our disk) and `LAPI_MODS_PLAIN_CURL`
//! (the plain curl, `curl`).

mod api;
mod auth;
mod blobs;
mod curseforge;
mod external;
mod fetch;
mod import;
mod input;
mod model;
mod modrinth;
mod own;
mod owner;
mod pack;
mod packwiz;
mod probe;
mod r2;
mod scan;
mod store;
mod thumb;
mod upkeep;
mod virustotal;

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
    /// The public bucket's address (R2, D-066 stage 6), once it serves.
    cdn: Option<String>,
}

impl Urls {
    pub fn new(base: &str) -> Self {
        Self {
            base: base.trim_end_matches('/').to_owned(),
            cdn: None,
        }
    }

    /// Files copied to R2 are served from `cdn` (none: from our disk).
    pub fn with_cdn(mut self, cdn: &str) -> Self {
        let cdn = cdn.trim().trim_end_matches('/');
        self.cdn = cdn
            .strip_prefix("https://")
            .filter(|host| {
                !host.is_empty()
                    && host.len() <= 100
                    && host.bytes().all(|b| {
                        b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'.' || b == b'-'
                    })
            })
            .map(|_| cdn.to_owned());
        self
    }

    /// Where players download a kept file: from R2 once it is copied there
    /// (by its hash alone), else from our disk. Our disk stays the reserve
    /// at the same path on files-mods.lapi.sh.
    pub fn served(&self, sha256: &str, name: &str, public: bool) -> String {
        match &self.cdn {
            Some(cdn) if public => format!("{cdn}/f/{sha256}"),
            _ => self.file(sha256, name),
        }
    }

    /// The address of a kept file in R2, for clearing caches.
    pub fn cdn_file(&self, sha256: &str) -> Option<String> {
        self.cdn.as_ref().map(|cdn| format!("{cdn}/f/{sha256}"))
    }

    /// A kept file under its own name, on our disk.
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
        Some("probe") => probe(),
        // What the scanner makes of a file: its verdict and signs, fixed words.
        Some("scan") => match &args[1..] {
            [file] => scan::scan_file(file.as_ref())
                .map(|v| println!("{} {}", v.level.as_str(), v.signs_text()))
                .map_err(|e| e.to_string()),
            _ => Err("usage: lapi-mods scan <file>".to_owned()),
        },
        Some(
            command @ ("find" | "review" | "block" | "unblock" | "remove" | "forget" | "restore"),
        ) => owner_command(command, &args[1..]).map(|lines| {
            for line in lines {
                println!("{line}");
            }
        }),
        // VirusTotal's word on files (virustotal.rs), and the owner's
        // additions to the catalog; `--stdin` as below.
        Some(command @ ("virustotal" | "add" | "slug")) => {
            external_args(&args[1..]).and_then(|given| {
                for line in owner_command(command, &given)? {
                    println!("{line}");
                }
                Ok(())
            })
        }
        // The service's own checks (owner.rs `health`): a problem fails the
        // command, and so the daily workflow that runs it.
        Some("health") => owner_command("health", &args[1..]).and_then(|lines| {
            for line in &lines {
                println!("{line}");
            }
            match lines.last() {
                Some(last) if last == "all ok" => Ok(()),
                _ => Err("a check found a problem".to_owned()),
            }
        }),
        // Projects from sites that are not catalogs (external.rs).
        // With `--stdin`, its arguments come as a JSON array on the standard
        // input: the workflow passes them so, never through a shell.
        Some("external") => external_args(&args[1..]).and_then(|given| {
            let store = open()?;
            let blobs = blobs::Blobs::open(&data(), DEFAULT_QUOTA_GB * blobs::GB, MIN_FREE)?;
            for line in external::command(&store, &blobs, &given, import::now())? {
                println!("{line}");
            }
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
        _ => Err(
            "usage: lapi-mods serve|counts|backup|probe|scan|thumb|find|review|block|unblock|remove|forget|restore|external|virustotal|add|slug|health"
                .to_owned(),
        ),
    };
    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("lapi-mods: {}", model::clean(&error, 2000, false));
            std::process::ExitCode::FAILURE
        }
    }
}

fn curl() -> fetch::Curl {
    fetch::Curl {
        program: PathBuf::from(env(
            "LAPI_MODS_CURL",
            "/home/www/lapi/mods/current/curl/curl_chrome150",
        )),
        leading: env("LAPI_MODS_CURL_ARGS", "")
            .split_whitespace()
            .map(str::to_owned)
            .collect(),
    }
}

/// A secret for a command run by hand (through sudo, without the service's
/// environment): from the deploy's secrets file when the environment has none.
fn secret(key: &str) -> String {
    let text = std::env::var(key).unwrap_or_default();
    if !text.trim().is_empty() {
        return text;
    }
    let secrets = env("LAPI_MODS_SECRETS", "/home/www/lapi/mods/secrets.env");
    std::fs::read_to_string(secrets)
        .ok()
        .and_then(|file| env_file_value(&file, key))
        .unwrap_or_default()
}

/// Most downloaded projects of each kind the catalog keeps (0: none).
fn popular() -> usize {
    env("LAPI_MODS_POPULAR", &import::POPULAR.to_string())
        .trim()
        .parse()
        .unwrap_or(import::POPULAR)
        .min(100)
}

/// Where players download: our disk, and R2 once it serves.
fn urls() -> Urls {
    Urls::new(&env("LAPI_MODS_FILES_URL", "https://files-mods.lapi.sh"))
        .with_cdn(&secret("LAPI_MODS_CDN_URL"))
}

/// The plain curl (not curl-impersonate), for Cloudflare's APIs.
fn plain_curl() -> PathBuf {
    PathBuf::from(env("LAPI_MODS_PLAIN_CURL", "curl"))
}

fn cloudflare() -> Option<owner::Cloudflare> {
    owner::Cloudflare::new(
        &secret("LAPI_MODS_CF_ZONE"),
        &secret("LAPI_MODS_CF_TOKEN"),
        plain_curl(),
    )
}

fn virustotal_api() -> Option<virustotal::Curl> {
    virustotal::Curl::new(&secret("LAPI_MODS_VIRUSTOTAL_KEY"), plain_curl())
}

fn bucket() -> Option<r2::R2> {
    r2::R2::new(
        &secret("LAPI_MODS_R2_ACCOUNT"),
        &secret("LAPI_MODS_R2_KEY"),
        &secret("LAPI_MODS_R2_SECRET"),
        &env("LAPI_MODS_R2_PUBLIC", "mods-files"),
        &env("LAPI_MODS_R2_PRIVATE", "mods-private"),
        plain_curl(),
    )
}

/// The owner's commands (owner.rs); what they print is the result.
fn owner_command(command: &str, args: &[String]) -> Result<Vec<String>, String> {
    let data = data();
    let store = open()?;
    let blobs = blobs::Blobs::open(&data, DEFAULT_QUOTA_GB * blobs::GB, MIN_FREE)?;
    blobs.reload_quota(&settings());
    let urls = urls();
    let cloudflare = cloudflare();
    let bucket = bucket();
    let virustotal = virustotal_api();
    let owner = owner::Owner {
        store: &store,
        blobs: &blobs,
        urls: &urls,
        purge: cloudflare.as_ref().map(|c| c as &dyn owner::Purge),
        bucket: bucket.as_ref().map(|b| b as &dyn r2::Bucket),
        virustotal: virustotal.as_ref().map(|v| v as &dyn virustotal::Api),
        now: import::now(),
    };
    let hash =
        |text: &str| owner::Hash::parse(text).ok_or_else(|| "not a SHA-256 or SHA-1".to_owned());
    match (command, args) {
        ("find", [query]) => owner.find(query),
        ("review", []) => owner.review(),
        ("review", [count]) if count == "--count" => owner.review_count(),
        ("review", [id, decision, reason @ ..]) => {
            let id = id
                .trim_start_matches('#')
                .parse()
                .map_err(|_| "not a number of the queue".to_owned())?;
            owner.decide(id, decision, &owner::reason(reason))
        }
        ("block", [h, reason @ ..]) => owner.block(&hash(h)?, &owner::reason(reason)),
        ("unblock", [h]) => owner.unblock(&hash(h)?),
        ("virustotal", [status]) if status == "status" => owner.virustotal_status(),
        ("virustotal", [check, h]) if check == "check" => owner.virustotal_check(&hash(h)?),
        ("add", [status]) if status == "--status" => owner.add_status(),
        ("add", inputs) => owner.add(inputs),
        ("slug", [project, slug]) => owner.slug(project, slug),
        ("health", []) => owner.health(popular(), &data.join("backups")),
        ("remove", [project, reason @ ..]) => owner.remove(project, &owner::reason(reason)),
        ("forget", [h, rest @ ..]) => match (hash(h)?, rest) {
            (owner::Hash::Sha256(sha256), []) => owner.forget(&sha256, false),
            (owner::Hash::Sha256(sha256), [yes]) if yes == "--yes" => owner.forget(&sha256, true),
            _ => Err("usage: lapi-mods forget <sha256> [--yes]".to_owned()),
        },
        ("restore", []) => match &bucket {
            Some(bucket) => upkeep::restore(&store, &blobs, bucket),
            None => Err("R2 is not set up (LAPI_MODS_R2_*)".to_owned()),
        },
        _ => Err(format!(
            "usage: see lapi-mods {command} in docs/MODS-CATALOG.md"
        )),
    }
}

fn external_args(args: &[String]) -> Result<Vec<String>, String> {
    if args != ["--stdin"] {
        return Ok(args.to_vec());
    }
    use std::io::Read;
    let mut text = String::new();
    std::io::stdin()
        .take(64 * 1024 + 1)
        .read_to_string(&mut text)
        .map_err(|_| "the standard input is not text".to_owned())?;
    external::stdin_args(&text)
}

/// Run by hand, so the proxies may come from the deploy's secrets file.
fn probe() -> Result<(), String> {
    let text = secret("LAPI_MODS_PROXIES");
    let tmp = std::env::temp_dir().join(format!("lapi-mods-probe-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&tmp).map_err(|e| e.to_string())?;
    let client = fetch::Client::new(Box::new(curl()), fetch::parse_proxies(&text), true, &tmp);
    let runtime = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
    let lines = runtime.block_on(probe::run(&client, &tmp));
    let _ = std::fs::remove_dir_all(&tmp);
    for line in lines {
        println!("{line}");
    }
    Ok(())
}

/// A `KEY="value"` line as deploy.sh writes it for systemd, unescaped.
fn env_file_value(file: &str, key: &str) -> Option<String> {
    let line = file
        .lines()
        .find_map(|l| l.strip_prefix(key)?.strip_prefix('='))?;
    let quoted = line.strip_prefix('"')?.strip_suffix('"')?;
    let mut value = String::with_capacity(quoted.len());
    let mut chars = quoted.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            value.push(chars.next()?);
        } else {
            value.push(c);
        }
    }
    Some(value)
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
    let direct = env("LAPI_MODS_DIRECT", "1") == "1";
    let client = fetch::Client::new(Box::new(curl()), proxies, direct, blobs.tmp());
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
        sessions: auth::Sessions::default(),
        email: auth::EmailRule::named(&env("LAPI_MODS_REQUIRE_EMAIL", "auto")),
        limiter: api::Limiter::default(),
        uploads: own::Uploads::default(),
        urls: urls(),
        thumbs: import::Thumbs::Process(std::env::current_exe().map_err(|e| e.to_string())?),
        r2: bucket().map(|b| Arc::new(b) as Arc<dyn r2::Bucket>),
        purge: cloudflare().map(|c| Arc::new(c) as Arc<dyn owner::Purge>),
        virustotal: virustotal_api().map(|v| Arc::new(v) as Arc<dyn virustotal::Api>),
    });
    eprintln!(
        "lapi-mods: R2 {}, cache clearing {}, VirusTotal {}",
        if app.r2.is_some() { "on" } else { "off" },
        if app.purge.is_some() { "on" } else { "off" },
        if app.virustotal.is_some() {
            "on"
        } else {
            "off"
        }
    );
    let accounts = env("LAPI_MODS_ACCOUNTS", "http://127.0.0.1:8793")
        .trim_end_matches('/')
        .to_owned();
    let runtime = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
    runtime.block_on(async move {
        // Sign-outs, switched-off and deleted accounts reach the catalog
        // within seconds.
        let checking = Arc::clone(&app);
        let asking = http.clone();
        tokio::spawn(async move {
            let mut since = 0;
            loop {
                tokio::time::sleep(Duration::from_secs(15)).await;
                api::check_mail(&checking, &asking, &accounts).await;
                api::check_sessions(&checking, &asking, &accounts).await;
                since = api::purge_deleted(&checking, &asking, &accounts, since).await;
            }
        });
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
        // New versions: projects are checked at their source again in the
        // background (D-066).
        let refreshing = Arc::clone(&app);
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(600)).await;
                match import::refresh(&refreshing, None) {
                    Ok(0) => {}
                    Ok(n) => eprintln!("lapi-mods: {n} projects queued for a check"),
                    Err(e) => eprintln!("lapi-mods: queueing checks failed: {e}"),
                }
            }
        });
        // Modrinth's most downloaded projects of each kind, looked for once a
        // day (a failed look is tried again the next hour) and queued a few
        // at a time while the queue is nearly empty.
        let seeding = Arc::clone(&app);
        let popular = popular();
        tokio::spawn(async move {
            if popular == 0 {
                return;
            }
            // What is still missing lives in memory: looked for again at
            // each start, so a restart loses none of it.
            let mut missing = Vec::new();
            let mut looked = false;
            let mut tried = 0;
            loop {
                let last = seeding
                    .store
                    .upkeep("popular_seeded")
                    .ok()
                    .flatten()
                    .unwrap_or(0);
                let now = import::now();
                if (!looked || now - last >= import::POPULAR_EVERY) && now - tried >= 3600 {
                    tried = now;
                    match import::popular(&seeding, popular).await {
                        Ok(found) => {
                            looked = true;
                            eprintln!("lapi-mods: {} popular projects to add", found.len());
                            missing = found;
                            // Taken from the end: the most downloaded first.
                            missing.reverse();
                        }
                        Err(e) => eprintln!("lapi-mods: looking for popular projects failed: {e}"),
                    }
                }
                if let Err(e) = import::seed(&seeding, &mut missing) {
                    eprintln!("lapi-mods: queueing popular projects failed: {e}");
                }
                tokio::time::sleep(Duration::from_secs(60)).await;
            }
        });
        // Kept archives scanned under older rules, a few at a time.
        let scanning = Arc::clone(&app);
        tokio::spawn(async move {
            loop {
                let wait = match import::rescan(&scanning, 20).await {
                    Ok(0) => 3600,
                    Ok(_) => 30,
                    Err(e) => {
                        eprintln!("lapi-mods: scanning kept files failed: {e}");
                        600
                    }
                };
                tokio::time::sleep(Duration::from_secs(wait)).await;
            }
        });
        // A first copy of the database, beside the requests rather than
        // before them: copying takes a while.
        let backing = Arc::clone(&app);
        let backups = data.join("backups");
        tokio::task::spawn_blocking(
            move || match upkeep::first_backup(&backing.store, &backups) {
                Ok(true) => eprintln!("lapi-mods: made a first copy of the database"),
                Ok(false) => {}
                Err(e) => eprintln!("lapi-mods: copying the database failed: {e}"),
            },
        );
        // R2 kept in step with the disk, the database copied there daily,
        // a sample checked weekly.
        let copying = Arc::clone(&app);
        tokio::spawn(async move {
            if copying.r2.is_none() {
                return;
            }
            let backups = data.join("backups");
            loop {
                if let Err(e) = upkeep::copy_database(&copying, backups.clone()).await {
                    eprintln!("lapi-mods: copying the database to R2 failed: {e}");
                }
                match upkeep::check(&copying).await {
                    Ok(Some(n)) => eprintln!("lapi-mods: R2 checked, {n} copies to make again"),
                    Ok(None) => {}
                    Err(e) => eprintln!("lapi-mods: checking R2 failed: {e}"),
                }
                let wait = match upkeep::copy(&copying, 20).await {
                    Ok(0) => 60,
                    Ok(_) => 1,
                    Err(e) => {
                        eprintln!("lapi-mods: copying to R2 failed: {e}");
                        600
                    }
                };
                tokio::time::sleep(Duration::from_secs(wait)).await;
            }
        });
        // VirusTotal's word on suspected files, one after another while any
        // waits and the day's quota lasts.
        let asking = Arc::clone(&app);
        tokio::spawn(async move {
            if asking.virustotal.is_none() {
                return;
            }
            loop {
                let app = Arc::clone(&asking);
                let stepped = tokio::task::spawn_blocking(move || {
                    let api = app.virustotal.as_deref().ok_or("off")?;
                    virustotal::step(&app.store, &app.blobs, api, import::now())
                })
                .await;
                let pace = match stepped {
                    Ok(Ok(true)) => virustotal::PACE,
                    Ok(Err(e)) => {
                        eprintln!("lapi-mods: asking VirusTotal failed: {e}");
                        virustotal::IDLE
                    }
                    _ => virustotal::IDLE,
                };
                tokio::time::sleep(Duration::from_secs(pace)).await;
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
    /// Sign-outs and deleted accounts, from the account service.
    pub sessions: auth::Sessions,
    /// Whether publishing a pack needs a confirmed email.
    pub email: auth::EmailRule,
    pub limiter: api::Limiter,
    /// Own packs' archives being sent in parts.
    pub uploads: own::Uploads,
    pub urls: Urls,
    pub thumbs: import::Thumbs,
    /// R2, when its settings are there (D-066, stage 6).
    pub r2: Option<Arc<dyn r2::Bucket>>,
    /// Cloudflare's cache, when a token is there.
    pub purge: Option<Arc<dyn owner::Purge>>,
    /// VirusTotal, when its key is there.
    pub virustotal: Option<Arc<dyn virustotal::Api>>,
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
