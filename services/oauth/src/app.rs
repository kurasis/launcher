//! What every request shares: settings, the store, the signing key, rate
//! limits, the captcha check and the password-hashing slots.

use std::collections::HashMap;
use std::future::Future;
use std::net::IpAddr;
use std::pin::Pin;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use axum::http::HeaderMap;
use tokio::sync::Semaphore;

use crate::crypto::Keys;
use crate::store::Store;

/// The only client: the launcher.
pub const CLIENT_ID: &str = "launcher";
/// Access tokens live 10 minutes; refresh tokens 60 days from their last use.
pub const ACCESS_SECONDS: i64 = 600;
pub const SESSION_SECONDS: i64 = 60 * 86_400;
/// A sign-in page is good for 30 minutes, a code for one.
pub const REQUEST_SECONDS: i64 = 30 * 60;
pub const CODE_SECONDS: i64 = 60;
/// Passwords hashed at the same time (19 MiB each).
pub const HASHING_AT_ONCE: usize = 8;

pub struct Config {
    /// `https://oauth.lapi.sh`: the issuer of tokens and the base of links.
    pub base: String,
    pub turnstile_site_key: String,
}

impl Config {
    /// Where a finished sign-in goes; the launcher's window catches it.
    pub fn redirect_uri(&self) -> String {
        format!("{}/app/done", self.base)
    }

    pub fn host(&self) -> &str {
        self.base
            .split("://")
            .nth(1)
            .unwrap_or(&self.base)
            .split('/')
            .next()
            .unwrap_or_default()
    }
}

/// Checks a Turnstile answer.
pub trait Captcha: Send + Sync {
    fn verify<'a>(
        &'a self,
        response: &'a str,
        ip: IpAddr,
        action: &'a str,
    ) -> Pin<Box<dyn Future<Output = bool> + Send + 'a>>;
}

/// Cloudflare's `siteverify`; the answer must be for our host and the action
/// of the form it came with.
pub struct Turnstile {
    pub secret: String,
    pub host: String,
    pub http: reqwest::Client,
}

impl Captcha for Turnstile {
    fn verify<'a>(
        &'a self,
        response: &'a str,
        ip: IpAddr,
        action: &'a str,
    ) -> Pin<Box<dyn Future<Output = bool> + Send + 'a>> {
        Box::pin(async move {
            if self.secret.is_empty() || response.is_empty() || response.len() > 2048 {
                return false;
            }
            let answer = self
                .http
                .post("https://challenges.cloudflare.com/turnstile/v0/siteverify")
                .form(&[
                    ("secret", self.secret.as_str()),
                    ("response", response),
                    ("remoteip", &ip.to_string()),
                ])
                .send()
                .await;
            let Ok(answer) = answer else {
                return false;
            };
            let Ok(body) = answer.json::<serde_json::Value>().await else {
                return false;
            };
            body["success"] == true && body["hostname"] == self.host && body["action"] == action
        })
    }
}

/// Requests counted per address (an IPv6 /64 counts as one) and kind within
/// a window, and over all addresses.
/// Requests counted since the start of their window, per address and kind.
type Windows = HashMap<(IpAddr, &'static str), (Instant, u32)>;

#[derive(Default)]
pub struct Limiter {
    windows: Mutex<Windows>,
}

pub const ALL: IpAddr = IpAddr::V6(std::net::Ipv6Addr::UNSPECIFIED);

fn subject(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V4(_) => ip,
        IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => IpAddr::V4(v4),
            None => {
                let mut segments = v6.segments();
                segments[4..].fill(0);
                IpAddr::from(segments)
            }
        },
    }
}

impl Limiter {
    fn bump(&self, key: IpAddr, bucket: &'static str, window: Duration, add: bool) -> u32 {
        let mut windows = self
            .windows
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let now = Instant::now();
        if windows.len() > 200_000 {
            windows.retain(|_, (start, _)| now.duration_since(*start) < Duration::from_secs(3600));
        }
        let entry = windows.entry((key, bucket)).or_insert((now, 0));
        if now.duration_since(entry.0) >= window {
            *entry = (now, 0);
        }
        if add {
            entry.1 = entry.1.saturating_add(1);
        }
        entry.1
    }

    /// Counts one request; false once the address's or everyone's allowance
    /// for the hour is used.
    pub fn allow(&self, ip: IpAddr, bucket: &'static str, per_hour: u32, overall: u32) -> bool {
        let hour = Duration::from_secs(3600);
        self.bump(subject(ip), bucket, hour, true) <= per_hour
            && self.bump(ALL, bucket, hour, true) <= overall
    }

    /// Counts an event for the address (failed sign-ins) in a window.
    pub fn note(&self, ip: IpAddr, bucket: &'static str, window: Duration) {
        self.bump(subject(ip), bucket, window, true);
    }

    pub fn count(&self, ip: IpAddr, bucket: &'static str, window: Duration) -> u32 {
        self.bump(subject(ip), bucket, window, false)
    }
}

pub struct App {
    pub config: Config,
    pub store: Store,
    pub keys: Keys,
    pub limiter: Limiter,
    pub captcha: Box<dyn Captcha>,
    pub hashing: Semaphore,
}

impl App {
    pub fn new(config: Config, store: Store, keys: Keys, captcha: Box<dyn Captcha>) -> Self {
        Self {
            config,
            store,
            keys,
            limiter: Limiter::default(),
            captcha,
            hashing: Semaphore::new(HASHING_AT_ONCE),
        }
    }

    /// Hashes a password in a hashing slot, off the request threads.
    pub async fn hash(&self, password: String) -> Option<String> {
        let _slot = self.hashing.acquire().await.ok()?;
        tokio::task::spawn_blocking(move || crate::crypto::hash_password(&password))
            .await
            .ok()
    }

    /// Checks a password the same way, also when there is no account.
    pub async fn check(&self, password: String, stored: Option<String>) -> bool {
        let Ok(_slot) = self.hashing.acquire().await else {
            return false;
        };
        let real = stored.is_some();
        let stored = stored.unwrap_or_else(|| crate::crypto::dummy_hash().to_owned());
        tokio::task::spawn_blocking(move || crate::crypto::password_matches(&password, &stored))
            .await
            .unwrap_or(false)
            && real
    }
}

/// The client's address as nginx passes it (`X-Real-IP`, from Cloudflare's
/// `CF-Connecting-IP`).
pub fn client_ip(headers: &HeaderMap) -> IpAddr {
    headers
        .get("x-real-ip")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(IpAddr::from([0, 0, 0, 0]))
}

/// The client's country as nginx passes it (`X-Country`, from Cloudflare's
/// `CF-IPCountry`).
pub fn client_country(headers: &HeaderMap) -> String {
    crate::rules::country(
        headers
            .get("x-country")
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default(),
    )
}

pub fn now() -> i64 {
    chrono::Utc::now().timestamp()
}
