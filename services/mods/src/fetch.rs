//! Everything the catalog downloads from Modrinth, CurseForge and image hosts
//! goes through curl-impersonate with Chrome's fingerprint (the owner's
//! choice: those sites treat it as a browser), through one of the exits: a
//! direct connection or a proxy from `LAPI_MODS_PROXIES`.
//!
//! Only HTTPS to the listed hosts, redirects included and followed here one by
//! one; names must resolve to public addresses. curl runs without a shell, its
//! arguments one by one, and a proxy's address (a secret) goes to it in the
//! environment, never on a command line or in a log.

use std::collections::HashMap;
use std::future::Future;
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Who a host belongs to; each has its own pace per exit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Site {
    Modrinth,
    CurseForge,
    Images,
    /// Where a modpack's files may come from besides Modrinth.
    Code,
    /// Sites the owner added projects from (external.rs).
    External,
}

impl Site {
    /// Requests a minute through one exit (Modrinth allows 300 per address).
    fn per_minute(self) -> u32 {
        match self {
            Site::Modrinth => 200,
            Site::CurseForge => 60,
            Site::Images => 120,
            Site::Code => 60,
            Site::External => 20,
        }
    }
}

pub const HOSTS: &[(&str, Site)] = &[
    ("api.modrinth.com", Site::Modrinth),
    ("cdn.modrinth.com", Site::Modrinth),
    ("www.curseforge.com", Site::CurseForge),
    ("edge.forgecdn.net", Site::CurseForge),
    ("mediafilez.forgecdn.net", Site::CurseForge),
    ("media.forgecdn.net", Site::CurseForge),
    ("i.imgur.com", Site::Images),
    ("raw.githubusercontent.com", Site::Images),
    ("user-images.githubusercontent.com", Site::Images),
    ("github.com", Site::Code),
    ("objects.githubusercontent.com", Site::Code),
    ("release-assets.githubusercontent.com", Site::Code),
    ("gitlab.com", Site::Code),
    ("optifine.net", Site::External),
];

/// An HTTPS URL with no user or port, of a sane length.
fn plain_https(url: &str) -> Option<reqwest::Url> {
    if url.len() > 2048 || url.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return None;
    }
    let parsed = reqwest::Url::parse(url).ok()?;
    (parsed.scheme() == "https"
        && parsed.port().is_none()
        && parsed.username().is_empty()
        && parsed.password().is_none())
    .then_some(parsed)
}

/// The site of a URL we may fetch: HTTPS, a listed host, no user or port.
pub fn site(url: &str) -> Option<Site> {
    let parsed = plain_https(url)?;
    let host = parsed.host_str()?;
    HOSTS.iter().find(|(h, _)| *h == host).map(|(_, s)| *s)
}

/// Names that never lead to the open internet.
const INNER_NAMES: &[&str] = &[
    ".localhost",
    ".local",
    ".internal",
    ".intranet",
    ".lan",
    ".home",
    ".corp",
    ".private",
    ".arpa",
    ".onion",
    ".test",
    ".invalid",
    ".example",
];

/// Whether a URL names a public host: never an address, never a name of an
/// inner network.
fn public_name(parsed: &reqwest::Url) -> bool {
    // `domain` is `None` for an address.
    let Some(host) = parsed.domain() else {
        return false;
    };
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    host.len() <= 253
        && host.contains('.')
        && host
            .split('.')
            .all(|l| !l.is_empty() && l.len() <= 63 && l.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-'))
        // A last label of digits only is an address written oddly.
        && !host.rsplit('.').next().is_some_and(|l| l.bytes().all(|b| b.is_ascii_digit()))
        && !INNER_NAMES.iter().any(|n| host.ends_with(n) || host == n[1..])
}

/// The site of a picture a description shows: a listed host, or any other
/// public name ([`public_name`]), whose pictures are fetched at the pace of
/// [`Site::Images`]. Whatever comes back is kept only when it is a picture,
/// and players get our copy.
pub fn picture_site(url: &str) -> Option<Site> {
    if let Some(listed) = site(url) {
        return Some(listed);
    }
    let parsed = plain_https(url)?;
    public_name(&parsed).then_some(Site::Images)
}

/// The site of a file of a project the owner added (external.rs): a listed
/// host, or any other public name, at the pace of [`Site::External`].
/// Whatever comes back is kept only when it is an archive, and scanned.
pub fn external_site(url: &str) -> Option<Site> {
    if let Some(listed) = site(url) {
        return Some(listed);
    }
    let parsed = plain_https(url)?;
    public_name(&parsed).then_some(Site::External)
}

/// True for globally routable unicast addresses (as the mirror checks).
pub fn is_public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let [a, b, ..] = v4.octets();
            !(v4.is_private()
                || v4.is_loopback()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_broadcast()
                || v4.is_multicast()
                || v4.is_documentation()
                || a == 0
                || (a == 100 && (64..128).contains(&b))
                || a >= 240)
        }
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_public(IpAddr::V4(v4));
            }
            let s = v6.segments();
            !(v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                || (s[0] & 0xfe00) == 0xfc00
                || (s[0] & 0xffc0) == 0xfe80
                || (s[0] == 0x2001 && s[1] == 0x0db8)
                || (s[0] == 0x0064 && s[1] == 0xff9b))
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FetchError {
    /// Not a URL we fetch (host, scheme, a private address, a redirect away).
    NotAllowed,
    /// 404 or 410: gone at the source.
    NotFound,
    /// Larger than allowed.
    TooBig,
    /// The site refused this exit (403 with a challenge, 429).
    Blocked,
    /// Another HTTP status.
    Status(u16),
    /// Connection, TLS, timeout: curl's exit code.
    Network(i32),
    /// Every exit is resting or at its pace for this site.
    NoExit,
}

impl FetchError {
    /// Worth trying again later.
    pub fn retryable(&self) -> bool {
        match self {
            FetchError::Blocked | FetchError::Network(_) | FetchError::NoExit => true,
            FetchError::Status(code) => *code >= 500,
            _ => false,
        }
    }

    /// A short code, fit for a public log and for the player.
    pub fn code(&self) -> String {
        match self {
            FetchError::NotAllowed => "not_allowed".into(),
            FetchError::NotFound => "not_found".into(),
            FetchError::TooBig => "too_big".into(),
            FetchError::Blocked => "blocked".into(),
            FetchError::Status(code) => format!("status_{code}"),
            FetchError::Network(code) => format!("network_{code}"),
            FetchError::NoExit => "no_exit".into(),
        }
    }
}

/// What a browser sends for an `<img>`: asked as a page, some picture hosts
/// (imgur) answer with a redirect to their own page instead of the picture.
pub const PICTURE: &[&str] = &[
    "Accept: image/avif,image/webp,image/apng,image/svg+xml,image/*,*/*;q=0.8",
    "Sec-Fetch-Dest: image",
    "Sec-Fetch-Mode: no-cors",
    "Sec-Fetch-Site: cross-site",
];

/// One request as curl gets it.
#[derive(Debug, Clone)]
pub struct Request {
    pub url: String,
    pub to: PathBuf,
    pub max_bytes: u64,
    pub json: bool,
    /// Headers of our own choosing (never from a player or a source).
    pub headers: &'static [&'static str],
    /// The proxy's address, or None for a direct connection.
    pub proxy: Option<String>,
    /// For a direct connection: the checked address to connect to.
    pub resolve: Option<SocketAddr>,
    pub timeout: Duration,
}

/// What came back: status, bytes written, and the next hop of a redirect.
#[derive(Debug, Clone, Default)]
pub struct Reply {
    pub status: u16,
    pub size: u64,
    pub location: Option<String>,
    /// Cloudflare asked for a browser check (`cf-mitigated: challenge`).
    pub challenge: bool,
}

type Pending<'a> = Pin<Box<dyn Future<Output = Result<Reply, FetchError>> + Send + 'a>>;

/// Runs one request; curl on the server, canned answers in tests.
pub trait Transport: Send + Sync {
    fn run(&self, request: Request) -> Pending<'_>;
}

/// curl-impersonate: `curl_chrome150` (its wrapper script) or the binary
/// with `--impersonate chrome150`.
pub struct Curl {
    pub program: PathBuf,
    /// Extra arguments before ours, such as `--impersonate chrome150`.
    pub leading: Vec<String>,
}

impl Curl {
    /// The arguments for one request, in order; tested on their own.
    pub fn arguments(&self, request: &Request) -> Vec<String> {
        let mut args = self.leading.clone();
        args.extend(
            [
                "--silent",
                "--show-error",
                "--proto",
                "=https",
                "--proto-redir",
                "=https",
                "--max-redirs",
                "0",
                "--connect-timeout",
                "15",
            ]
            .map(String::from),
        );
        args.push("--max-time".into());
        args.push(request.timeout.as_secs().max(1).to_string());
        args.push("--max-filesize".into());
        args.push(request.max_bytes.to_string());
        if request.json {
            args.push("--header".into());
            args.push("Accept: application/json".into());
        }
        for header in request.headers {
            args.push("--header".into());
            args.push((*header).to_owned());
        }
        if let (Some(address), None) = (request.resolve, &request.proxy)
            && let Some(host) = reqwest::Url::parse(&request.url)
                .ok()
                .and_then(|u| u.host_str().map(str::to_owned))
        {
            let ip = match address.ip() {
                IpAddr::V6(v6) => format!("[{v6}]"),
                IpAddr::V4(v4) => v4.to_string(),
            };
            args.push("--resolve".into());
            args.push(format!("{host}:443:{ip}"));
        }
        args.push("--output".into());
        args.push(request.to.to_string_lossy().into_owned());
        args.push("--write-out".into());
        args.push(
            "%{http_code}\\t%{size_download}\\t%{redirect_url}\\t%header{cf-mitigated}".into(),
        );
        args.push("--url".into());
        args.push(request.url.clone());
        args
    }
}

/// Reads curl's `--write-out` line.
pub fn parse_write_out(out: &str) -> Reply {
    let mut fields = out.trim_end_matches(['\n', '\r']).split('\t');
    let status = fields
        .next()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0);
    let size = fields
        .next()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0);
    let location = fields
        .next()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned);
    let challenge = fields
        .next()
        .is_some_and(|s| s.trim().eq_ignore_ascii_case("challenge"));
    Reply {
        status,
        size,
        location,
        challenge,
    }
}

impl Transport for Curl {
    fn run(&self, request: Request) -> Pending<'_> {
        let args = self.arguments(&request);
        Box::pin(async move {
            let mut command = tokio::process::Command::new(&self.program);
            command
                .args(&args)
                .env_clear()
                .env("PATH", "/usr/local/bin:/usr/bin:/bin")
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::null())
                .kill_on_drop(true);
            if let Some(proxy) = &request.proxy {
                command.env("ALL_PROXY", proxy).env("HTTPS_PROXY", proxy);
            }
            let output =
                tokio::time::timeout(request.timeout + Duration::from_secs(10), command.output())
                    .await
                    .map_err(|_| FetchError::Network(28))?
                    .map_err(|_| FetchError::Network(-1))?;
            match output.status.code() {
                Some(0) => Ok(parse_write_out(&String::from_utf8_lossy(&output.stdout))),
                Some(63) => Err(FetchError::TooBig),
                Some(code) => Err(FetchError::Network(code)),
                None => Err(FetchError::Network(-1)),
            }
        })
    }
}

/// One way out: direct, or a proxy, with its pace and rest per site.
struct Exit {
    proxy: Option<String>,
    sites: Mutex<HashMap<Site, Pace>>,
}

#[derive(Default)]
struct Pace {
    window: Option<Instant>,
    used: u32,
    resting_until: Option<Instant>,
}

/// The proxies from `LAPI_MODS_PROXIES` (whitespace- or comma-separated
/// `http`, `https`, `socks5` or `socks5h` URLs); entries that are not are
/// left out. Their count is all a log may say about them.
///
/// `socks5://` is taken as `socks5h://`: the proxy resolves the name. The
/// owner's proxies refuse a connection to an address curl resolved itself
/// (code 97, found by `lapi-mods probe`), and every host we ask is a listed
/// name, so nothing is lost by letting the proxy look it up.
pub fn parse_proxies(text: &str) -> Vec<String> {
    text.split(|c: char| c == ',' || c.is_whitespace())
        .filter(|p| !p.is_empty() && p.len() <= 500 && !p.chars().any(char::is_control))
        .filter(|p| {
            reqwest::Url::parse(p).is_ok_and(|u| {
                matches!(u.scheme(), "http" | "https" | "socks5" | "socks5h")
                    && u.host_str().is_some()
            })
        })
        .map(|p| match p.strip_prefix("socks5://") {
            Some(rest) => format!("socks5h://{rest}"),
            None => p.to_owned(),
        })
        .collect()
}

/// How a name is turned into addresses; tests answer without DNS.
pub type Resolver =
    Box<dyn Fn(String) -> Pin<Box<dyn Future<Output = Vec<IpAddr>> + Send>> + Send + Sync>;

pub fn system_resolver() -> Resolver {
    Box::new(|host: String| {
        Box::pin(async move {
            tokio::net::lookup_host((host.as_str(), 443))
                .await
                .map(|addrs| addrs.map(|a| a.ip()).collect())
                .unwrap_or_default()
        })
    })
}

pub struct Client {
    transport: Box<dyn Transport>,
    exits: Vec<Exit>,
    tmp: PathBuf,
    resolver: Resolver,
}

const JSON_MAX: u64 = 16 * 1024 * 1024;
const REST: Duration = Duration::from_secs(3600);
const DOWN: Duration = Duration::from_secs(600);

impl Client {
    pub fn new(
        transport: Box<dyn Transport>,
        proxies: Vec<String>,
        direct: bool,
        tmp: &Path,
    ) -> Self {
        let mut exits: Vec<Exit> = proxies
            .into_iter()
            .map(|p| Exit {
                proxy: Some(p),
                sites: Mutex::default(),
            })
            .collect();
        if direct || exits.is_empty() {
            exits.push(Exit {
                proxy: None,
                sites: Mutex::default(),
            });
        }
        Self {
            transport,
            exits,
            tmp: tmp.to_owned(),
            resolver: system_resolver(),
        }
    }

    pub fn with_resolver(mut self, resolver: Resolver) -> Self {
        self.resolver = resolver;
        self
    }

    pub fn exits(&self) -> usize {
        self.exits.len()
    }

    /// The least used exit that is not resting and within its pace for the
    /// site, counted as used.
    fn pick(&self, site: Site, skip: &[usize]) -> Option<usize> {
        let now = Instant::now();
        let mut best: Option<(usize, u32)> = None;
        for (i, exit) in self.exits.iter().enumerate() {
            if skip.contains(&i) {
                continue;
            }
            let mut sites = exit
                .sites
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let pace = sites.entry(site).or_default();
            if pace.resting_until.is_some_and(|t| t > now) {
                continue;
            }
            if pace
                .window
                .is_none_or(|w| now.duration_since(w) >= Duration::from_secs(60))
            {
                pace.window = Some(now);
                pace.used = 0;
            }
            if pace.used >= site.per_minute() {
                continue;
            }
            if best.is_none_or(|(_, used)| pace.used < used) {
                best = Some((i, pace.used));
            }
        }
        let (i, _) = best?;
        let mut sites = self.exits[i]
            .sites
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        sites.entry(site).or_default().used += 1;
        Some(i)
    }

    fn rest(&self, exit: usize, site: Site, how_long: Duration) {
        let mut sites = self.exits[exit]
            .sites
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        sites.entry(site).or_default().resting_until = Some(Instant::now() + how_long);
    }

    /// A public address of the URL's host, for a direct connection.
    async fn public_address(&self, url: &str) -> Result<SocketAddr, FetchError> {
        let host = reqwest::Url::parse(url)
            .ok()
            .and_then(|u| u.host_str().map(str::to_owned))
            .ok_or(FetchError::NotAllowed)?;
        let addresses = (self.resolver)(host).await;
        if addresses.is_empty() {
            return Err(FetchError::Network(6));
        }
        addresses
            .into_iter()
            .find(|ip| is_public(*ip))
            .map(|ip| SocketAddr::new(ip, 443))
            .ok_or(FetchError::NotAllowed)
    }

    /// Downloads `url` into `to`, following up to 5 redirects to listed
    /// hosts, trying another exit when one fails or is refused.
    pub async fn get(
        &self,
        url: &str,
        to: &Path,
        max_bytes: u64,
        json: bool,
    ) -> Result<u64, FetchError> {
        self.get_by(site, url, to, max_bytes, json, &[])
            .await
            .map(|(size, _)| size)
    }

    /// A picture a description shows, from any public host
    /// ([`picture_site`]), asked as a browser asks for an `<img>`.
    pub async fn picture(&self, url: &str, to: &Path, max_bytes: u64) -> Result<u64, FetchError> {
        self.get_by(picture_site, url, to, max_bytes, false, PICTURE)
            .await
            .map(|(size, _)| size)
    }

    /// A file of a project the owner added, from any public host
    /// ([`external_site`]).
    pub async fn external(&self, url: &str, to: &Path, max_bytes: u64) -> Result<u64, FetchError> {
        self.get_by(external_site, url, to, max_bytes, false, &[])
            .await
            .map(|(size, _)| size)
    }

    async fn get_by(
        &self,
        rule: fn(&str) -> Option<Site>,
        url: &str,
        to: &Path,
        max_bytes: u64,
        json: bool,
        headers: &'static [&'static str],
    ) -> Result<(u64, String), FetchError> {
        let mut url = url.to_owned();
        for _ in 0..6 {
            let site = rule(&url).ok_or(FetchError::NotAllowed)?;
            // A host nobody listed must have a public name even when a proxy
            // looks it up for us.
            let unlisted = self::site(&url).is_none();
            let mut tried = Vec::new();
            let reply = loop {
                let Some(exit) = self.pick(site, &tried) else {
                    return Err(FetchError::NoExit);
                };
                tried.push(exit);
                let proxy = self.exits[exit].proxy.clone();
                let resolve = if proxy.is_none() {
                    Some(self.public_address(&url).await?)
                } else {
                    if unlisted {
                        self.public_address(&url).await?;
                    }
                    None
                };
                let request = Request {
                    url: url.clone(),
                    to: to.to_owned(),
                    max_bytes,
                    json,
                    headers,
                    proxy,
                    resolve,
                    timeout: if json {
                        Duration::from_secs(60)
                    } else {
                        Duration::from_secs(1800)
                    },
                };
                match self.transport.run(request).await {
                    Ok(reply)
                        if reply.challenge
                            || reply.status == 429
                            || (reply.status == 403 && site != Site::Modrinth) =>
                    {
                        self.rest(exit, site, REST);
                        if tried.len() >= 3 {
                            return Err(FetchError::Blocked);
                        }
                    }
                    Ok(reply) => break reply,
                    Err(FetchError::Network(code)) => {
                        self.rest(exit, site, DOWN);
                        if tried.len() >= 3 {
                            return Err(FetchError::Network(code));
                        }
                    }
                    Err(other) => return Err(other),
                }
            };
            match reply.status {
                200 => {
                    if reply.size > max_bytes {
                        return Err(FetchError::TooBig);
                    }
                    return Ok((reply.size, url));
                }
                301 | 302 | 303 | 307 | 308 => {
                    let next = reply.location.ok_or(FetchError::Status(reply.status))?;
                    // The next hop must pass the same rule, as must every hop after.
                    rule(&next).ok_or(FetchError::NotAllowed)?;
                    url = next;
                }
                404 | 410 => return Err(FetchError::NotFound),
                other => return Err(FetchError::Status(other)),
            }
        }
        Err(FetchError::Status(310))
    }

    /// One request through one exit, with no pace, rest, retry or redirect:
    /// what `lapi-mods probe` asks to see how each exit fares.
    /// With `scheme`, a proxy is asked as if its address had that scheme.
    pub async fn once(
        &self,
        exit: usize,
        scheme: Option<&str>,
        url: &str,
        headers: &'static [&'static str],
        to: &Path,
        max_bytes: u64,
    ) -> Result<Reply, FetchError> {
        site(url).ok_or(FetchError::NotAllowed)?;
        let mut proxy = self
            .exits
            .get(exit)
            .ok_or(FetchError::NoExit)?
            .proxy
            .clone();
        if let (Some(scheme), Some(address)) = (scheme, &proxy) {
            proxy = Some(crate::probe::with_scheme(address, scheme).ok_or(FetchError::NotAllowed)?);
        }
        let resolve = if proxy.is_none() {
            Some(self.public_address(url).await?)
        } else {
            None
        };
        self.transport
            .run(Request {
                url: url.to_owned(),
                to: to.to_owned(),
                max_bytes,
                json: false,
                headers,
                proxy,
                resolve,
                timeout: Duration::from_secs(60),
            })
            .await
    }

    /// Whether the exit is a proxy (they are numbered, never named).
    pub fn is_proxy(&self, exit: usize) -> bool {
        self.exits.get(exit).is_some_and(|e| e.proxy.is_some())
    }

    /// A page's text (at most `max` bytes, invalid UTF-8 replaced); `None`
    /// when the source has no such page.
    pub async fn page(&self, url: &str, max: u64) -> Result<Option<String>, FetchError> {
        Ok(self.page_at(url, max).await?.map(|(_, text)| text))
    }

    /// A page's address after redirects, and its text as [`Client::page`]
    /// gives it.
    pub async fn page_at(
        &self,
        url: &str,
        max: u64,
    ) -> Result<Option<(String, String)>, FetchError> {
        let part = self.tmp.join(format!("{}.html", uuid::Uuid::new_v4()));
        let result = self.get_by(site, url, &part, max, false, &[]).await;
        let body = match std::fs::metadata(&part) {
            Ok(m) if m.len() > max => Err(FetchError::TooBig),
            _ => std::fs::read(&part).map_err(|_| FetchError::Network(-1)),
        };
        let _ = std::fs::remove_file(&part);
        match result {
            Ok((_, at)) => Ok(Some((at, String::from_utf8_lossy(&body?).into_owned()))),
            Err(FetchError::NotFound) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// A JSON answer (at most 16 MB); `None` when the source has no such thing.
    pub async fn json(&self, url: &str) -> Result<Option<serde_json::Value>, FetchError> {
        let part = self.tmp.join(format!("{}.json", uuid::Uuid::new_v4()));
        let result = self.get(url, &part, JSON_MAX, true).await;
        // curl may unpack a compressed answer: the size on disk counts.
        let body = match std::fs::metadata(&part) {
            Ok(m) if m.len() > JSON_MAX => Err(FetchError::TooBig),
            _ => std::fs::read(&part).map_err(|_| FetchError::Network(-1)),
        };
        let _ = std::fs::remove_file(&part);
        match result {
            Ok(_) => {
                let body = body?;
                serde_json::from_slice(&body)
                    .map(Some)
                    .map_err(|_| FetchError::Status(200))
            }
            Err(FetchError::NotFound) => Ok(None),
            Err(e) => Err(e),
        }
    }
}
