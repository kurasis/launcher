//! Where downloads come from: the origin or our mirror at mirror.lapi.sh
//! (D-062).
//!
//! Every download first asks [`Routing::first`]. In the automatic mode that is
//! the origin unless the host failed twice within [`FAILURE_WINDOW`]; then the
//! host is "down" for [`DOWN_FOR`] and its downloads go to the mirror, after
//! which one download tries the origin again. Callers report what happened
//! with [`Routing::origin_failed`] and [`Routing::origin_worked`]. Files keep
//! their checksums wherever they came from.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use url::Url;

pub const MIRROR_HOST: &str = "mirror.lapi.sh";
const MIRROR_BASE: &str = "https://mirror.lapi.sh/";

/// Failures of one host within this window mark it as down.
pub const FAILURE_WINDOW: Duration = Duration::from_secs(120);
pub const FAILURES_TO_DOWN: usize = 2;
pub const DOWN_FOR: Duration = Duration::from_secs(600);
/// Attempts at the origin for one file or document before the mirror.
pub const ORIGIN_ATTEMPTS: u32 = 2;
/// An origin is "very slow" when, after [`SLOW_AFTER`], a file still arrives
/// at under [`SLOW_BYTES_PER_SECOND`] with more than [`SLOW_MIN_LEFT`] to go.
pub const SLOW_AFTER: Duration = Duration::from_secs(10);
pub const SLOW_BYTES_PER_SECOND: u64 = 64 * 1024;
pub const SLOW_MIN_LEFT: u64 = 1024 * 1024;

/// The player's choice in Settings → Network.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DownloadSource {
    /// The origin, the mirror when the origin fails or is very slow.
    #[default]
    Automatic,
    /// The mirror, the origin when the mirror fails (blocked or slow origins).
    MirrorFirst,
    /// Never the mirror.
    OriginOnly,
}

impl DownloadSource {
    fn code(self) -> u8 {
        match self {
            Self::Automatic => 0,
            Self::MirrorFirst => 1,
            Self::OriginOnly => 2,
        }
    }

    fn from_code(code: u8) -> Self {
        match code {
            1 => Self::MirrorFirst,
            2 => Self::OriginOnly,
            _ => Self::Automatic,
        }
    }
}

/// Where one request goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Route {
    Origin,
    Mirror,
}

#[derive(Default)]
struct Health {
    failures: Vec<Instant>,
    down_until: Option<Instant>,
}

type Listener = Box<dyn Fn() + Send + Sync>;

pub struct Routing {
    mode: AtomicU8,
    base: Url,
    hosts: Mutex<HashMap<String, Health>>,
    listener: Mutex<Option<Listener>>,
}

static SHARED: LazyLock<Arc<Routing>> = LazyLock::new(|| Arc::new(Routing::new(MIRROR_BASE)));

/// The launcher's routing; tests make their own.
pub fn shared() -> Arc<Routing> {
    Arc::clone(&SHARED)
}

impl Routing {
    /// `base` is the mirror's root: `https://mirror.lapi.sh/` (a local server
    /// in tests).
    pub fn new(base: &str) -> Self {
        Self {
            mode: AtomicU8::new(DownloadSource::Automatic.code()),
            base: Url::parse(base).expect("mirror base URL"),
            hosts: Mutex::new(HashMap::new()),
            listener: Mutex::new(None),
        }
    }

    fn hosts(&self) -> std::sync::MutexGuard<'_, HashMap<String, Health>> {
        self.hosts
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Called when the set of hosts served by the mirror changes.
    pub fn on_change(&self, listener: impl Fn() + Send + Sync + 'static) {
        *self
            .listener
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Box::new(listener));
    }

    fn changed(&self) {
        if let Some(listener) = &*self
            .listener
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
        {
            listener();
        }
    }

    pub fn set_mode(&self, mode: DownloadSource) {
        let before = self.mode.swap(mode.code(), Ordering::Relaxed);
        if before != mode.code() {
            self.changed();
        }
    }

    pub fn mode(&self) -> DownloadSource {
        DownloadSource::from_code(self.mode.load(Ordering::Relaxed))
    }

    pub fn mirror_allowed(&self) -> bool {
        self.mode() != DownloadSource::OriginOnly
    }

    fn down(&self, host: &str, now: Instant) -> bool {
        self.hosts()
            .get(host)
            .and_then(|h| h.down_until)
            .is_some_and(|until| until > now)
    }

    /// Where a request to `host` goes first.
    pub fn first(&self, host: &str) -> Route {
        match self.mode() {
            DownloadSource::OriginOnly => Route::Origin,
            DownloadSource::MirrorFirst => Route::Mirror,
            DownloadSource::Automatic if self.down(host, Instant::now()) => Route::Mirror,
            DownloadSource::Automatic => Route::Origin,
        }
    }

    /// The origin failed (after its attempts), was very slow, or did not have
    /// the file. Two of these within [`FAILURE_WINDOW`] mark the host down.
    pub fn origin_failed(&self, host: &str) {
        let now = Instant::now();
        let newly_down = {
            let mut hosts = self.hosts();
            let health = hosts.entry(host.to_owned()).or_default();
            health
                .failures
                .retain(|t| now.duration_since(*t) < FAILURE_WINDOW);
            health.failures.push(now);
            let was_down = health.down_until.is_some_and(|u| u > now);
            if health.failures.len() >= FAILURES_TO_DOWN {
                health.down_until = Some(now + DOWN_FOR);
                health.failures.clear();
            }
            !was_down && health.down_until.is_some_and(|u| u > now)
        };
        if newly_down {
            self.changed();
        }
    }

    /// The origin served a request: it is not down any more.
    pub fn origin_worked(&self, host: &str) {
        let was_down = {
            let mut hosts = self.hosts();
            hosts
                .remove(host)
                .and_then(|h| h.down_until)
                .is_some_and(|u| u > Instant::now())
        };
        if was_down {
            self.changed();
        }
    }

    /// Hosts whose downloads go to the mirror now, sorted.
    pub fn mirrored(&self) -> Vec<String> {
        if self.mode() == DownloadSource::OriginOnly {
            return Vec::new();
        }
        let now = Instant::now();
        let mut hosts: Vec<String> = self
            .hosts()
            .iter()
            .filter(|(_, h)| h.down_until.is_some_and(|u| u > now))
            .map(|(host, _)| host.clone())
            .collect();
        hosts.sort();
        hosts
    }

    /// `https://mirror.lapi.sh/<host>/<path>?<query>` for an origin URL.
    pub fn mirror_url(&self, url: &Url) -> Option<Url> {
        let host = url.host_str()?;
        let mut path = format!("{host}{}", url.path());
        if let Some(query) = url.query() {
            path.push('?');
            path.push_str(query);
        }
        self.base.join(&path).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_failures_send_a_host_to_the_mirror_until_it_works() {
        let routing = Routing::new(MIRROR_BASE);
        let changes = Arc::new(Mutex::new(0));
        let counted = Arc::clone(&changes);
        routing.on_change(move || *counted.lock().unwrap() += 1);
        let host = "maven.minecraftforge.net";
        assert_eq!(routing.first(host), Route::Origin);
        routing.origin_failed(host);
        assert_eq!(
            routing.first(host),
            Route::Origin,
            "one failure is not enough"
        );
        routing.origin_failed(host);
        assert_eq!(routing.first(host), Route::Mirror);
        assert_eq!(
            routing.first("maven.fabricmc.net"),
            Route::Origin,
            "per host"
        );
        assert_eq!(routing.mirrored(), [host]);
        routing.origin_worked(host);
        assert_eq!(routing.first(host), Route::Origin);
        assert!(routing.mirrored().is_empty());
        assert_eq!(*changes.lock().unwrap(), 2, "down, then up");
    }

    #[test]
    fn the_modes_decide_first() {
        let routing = Routing::new(MIRROR_BASE);
        routing.set_mode(DownloadSource::MirrorFirst);
        assert_eq!(routing.first("libraries.minecraft.net"), Route::Mirror);
        routing.set_mode(DownloadSource::OriginOnly);
        routing.origin_failed("libraries.minecraft.net");
        routing.origin_failed("libraries.minecraft.net");
        assert_eq!(routing.first("libraries.minecraft.net"), Route::Origin);
        assert!(!routing.mirror_allowed());
        assert!(routing.mirrored().is_empty());
    }

    #[test]
    fn mirror_urls_keep_host_path_and_query() {
        let routing = Routing::new(MIRROR_BASE);
        let url = |s: &str| {
            routing
                .mirror_url(&Url::parse(s).unwrap())
                .unwrap()
                .to_string()
        };
        assert_eq!(
            url("https://piston-data.mojang.com/v1/objects/ab12/client.jar"),
            "https://mirror.lapi.sh/piston-data.mojang.com/v1/objects/ab12/client.jar"
        );
        assert_eq!(
            url("https://api.adoptium.net/v3/assets/latest/21/hotspot?os=linux&image_type=jre"),
            "https://mirror.lapi.sh/api.adoptium.net/v3/assets/latest/21/hotspot?os=linux&image_type=jre"
        );
        assert_eq!(
            url("https://github.com/adoptium/t/releases/download/jdk-21%2B11/a.tar.gz"),
            "https://mirror.lapi.sh/github.com/adoptium/t/releases/download/jdk-21%2B11/a.tar.gz"
        );
    }
}
