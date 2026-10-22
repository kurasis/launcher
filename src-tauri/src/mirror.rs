//! Where downloads come from: the origin or our mirror at mirror.lapi.sh
//! (D-062).
//!
//! Every download first asks [`Routing::first`]. In the automatic mode that is
//! the origin unless the host failed twice within [`FAILURE_WINDOW`]; then the
//! host is "down" for [`DOWN_FOR`] and its downloads go to the mirror, after
//! which one download tries the origin again. Callers report what happened
//! with [`Routing::origin_failed`] and [`Routing::origin_worked`]. Files keep
//! their checksums wherever they came from.
//!
//! A host can also be slower than the mirror without failing, small files
//! included. Downloads of files report their size and time
//! ([`Routing::origin_served`], [`Routing::mirror_served`]). When the origin's
//! files of the last [`SPEED_WINDOW`] came in under [`FAST_ORIGIN`] together,
//! a trial starts: the host's next files take turns between the mirror and
//! the origin ([`Routing::first_for_file`]) until each side has
//! [`TRIAL_FILES`], so both are measured at the same time on the same kind of
//! files. If the mirror was at least [`FASTER_BY`] times faster, the host is
//! served by the mirror for [`DOWN_FOR`], then measured again, unless the
//! mirror fails first; otherwise no trial for [`TRIAL_COOLDOWN`].

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
/// Origin files within this window tell how fast a host is.
pub const SPEED_WINDOW: Duration = Duration::from_secs(60);
/// Enough to judge: this many files that took this long together.
pub const SPEED_MIN_FILES: usize = 4;
pub const SPEED_MIN_TIME: Duration = Duration::from_secs(1);
/// Only the latest files of the window count.
const SPEED_MAX_FILES: usize = 256;
/// A host whose files come at least this fast is not compared.
pub const FAST_ORIGIN: u64 = 2 * 1024 * 1024;
/// Files each side brings in a trial.
pub const TRIAL_FILES: usize = 3;
/// How much faster the mirror must be to take a host over.
pub const FASTER_BY: u64 = 2;
/// A trial that has not finished by then counts as not faster.
pub const TRIAL_LIMIT: Duration = Duration::from_secs(120);
pub const TRIAL_COOLDOWN: Duration = Duration::from_secs(300);

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

/// One finished download: when, how many bytes, how long it took.
#[derive(Clone, Copy)]
struct Sample {
    at: Instant,
    bytes: u64,
    took: Duration,
}

/// Bytes per second over samples, if they are enough to judge. Time per file
/// counts, so many small files that each wait long come out slow.
fn rate(samples: &[Sample], min_files: usize) -> Option<u64> {
    let took: Duration = samples.iter().map(|s| s.took).sum();
    let bytes: u64 = samples.iter().map(|s| s.bytes).sum();
    if samples.len() < min_files || took.is_zero() || bytes == 0 {
        return None;
    }
    Some((u128::from(bytes) * 1000 / took.as_millis().max(1)) as u64)
}

/// The mirror is compared with a host that looks slow.
struct Trial {
    started: Instant,
    /// Files sent to the mirror so far.
    sent: usize,
    /// The next file goes to the mirror.
    mirror_turn: bool,
    origin: Vec<Sample>,
    mirror: Vec<Sample>,
}

#[derive(Default)]
struct Health {
    failures: Vec<Instant>,
    down_until: Option<Instant>,
    /// Served by the mirror because it was faster; only time ends it.
    slow_until: Option<Instant>,
    origin: Vec<Sample>,
    trial: Option<Trial>,
    no_trial_until: Option<Instant>,
}

impl Health {
    fn mirrored(&self, now: Instant) -> bool {
        self.down_until.is_some_and(|u| u > now) || self.slow_until.is_some_and(|u| u > now)
    }

    /// Ends a trial that did not show a faster mirror.
    fn trial_lost(&mut self, now: Instant) {
        self.trial = None;
        self.no_trial_until = Some(now + TRIAL_COOLDOWN);
    }

    /// The running trial; one past [`TRIAL_LIMIT`] is lost.
    fn trial(&mut self, now: Instant) -> Option<&mut Trial> {
        if self
            .trial
            .as_ref()
            .is_some_and(|t| now.duration_since(t.started) > TRIAL_LIMIT)
        {
            self.trial_lost(now);
        }
        self.trial.as_mut()
    }

    /// Ends the trial once both sides have their files. `true` when the
    /// mirror won and the host newly goes to it.
    fn judge(&mut self, now: Instant) -> bool {
        let Some(trial) = &self.trial else {
            return false;
        };
        let (Some(origin), Some(mirror)) = (
            rate(&trial.origin, TRIAL_FILES),
            rate(&trial.mirror, TRIAL_FILES),
        ) else {
            return false;
        };
        if mirror < origin.saturating_mul(FASTER_BY) {
            self.trial_lost(now);
            return false;
        }
        let was = self.mirrored(now);
        self.trial = None;
        self.origin.clear();
        self.slow_until = Some(now + DOWN_FOR);
        !was
    }
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

/// Routing that never uses the mirror, for files the mirror does not carry
/// (the skins mod's GitHub release, D-065). Its failures never send another
/// download of the same host to the mirror.
pub fn origin_only() -> Arc<Routing> {
    let routing = Routing::new(MIRROR_BASE);
    routing.set_mode(DownloadSource::OriginOnly);
    Arc::new(routing)
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
        self.hosts().get(host).is_some_and(|h| h.mirrored(now))
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

    /// Where a file of `host` goes first: as [`Self::first`], and every other
    /// file to the mirror while a trial wants files.
    pub fn first_for_file(&self, host: &str) -> Route {
        self.first_for_file_at(host, Instant::now())
    }

    fn first_for_file_at(&self, host: &str, now: Instant) -> Route {
        let route = self.first(host);
        if route == Route::Mirror || self.mode() != DownloadSource::Automatic {
            return route;
        }
        let mut hosts = self.hosts();
        let Some(trial) = hosts
            .get_mut(host)
            .and_then(|h| h.trial(now))
            .filter(|t| t.sent < TRIAL_FILES)
        else {
            return route;
        };
        trial.mirror_turn = !trial.mirror_turn;
        if trial.mirror_turn {
            trial.sent += 1;
            Route::Mirror
        } else {
            route
        }
    }

    /// A file of `host` came from the origin: `bytes` in `took`. It works; a
    /// host that looks slow gets a trial, and a running trial gets the file.
    pub fn origin_served(&self, host: &str, bytes: u64, took: Duration) {
        self.origin_served_at(host, bytes, took, Instant::now());
    }

    fn origin_served_at(&self, host: &str, bytes: u64, took: Duration, now: Instant) {
        self.origin_worked(host);
        if self.mode() != DownloadSource::Automatic {
            return;
        }
        let sample = Sample {
            at: now,
            bytes,
            took,
        };
        let newly_slow = {
            let mut hosts = self.hosts();
            let health = hosts.entry(host.to_owned()).or_default();
            if let Some(trial) = health.trial(now) {
                // Only files asked for during the trial compare fairly.
                if now
                    .checked_sub(took)
                    .is_some_and(|asked| asked >= trial.started)
                {
                    trial.origin.push(sample);
                }
                health.judge(now)
            } else {
                health
                    .origin
                    .retain(|s| now.duration_since(s.at) < SPEED_WINDOW);
                health.origin.push(sample);
                let over = health.origin.len().saturating_sub(SPEED_MAX_FILES);
                health.origin.drain(..over);
                let took: Duration = health.origin.iter().map(|s| s.took).sum();
                let slow = took >= SPEED_MIN_TIME
                    && rate(&health.origin, SPEED_MIN_FILES).is_some_and(|r| r < FAST_ORIGIN);
                if slow && !health.mirrored(now) && !health.no_trial_until.is_some_and(|u| u > now)
                {
                    health.trial = Some(Trial {
                        started: now,
                        sent: 0,
                        mirror_turn: false,
                        origin: Vec::new(),
                        mirror: Vec::new(),
                    });
                }
                false
            }
        };
        if newly_slow {
            self.changed();
        }
    }

    /// A file of `host` came from the mirror: `bytes` in `took`. A running
    /// trial gets the file.
    pub fn mirror_served(&self, host: &str, bytes: u64, took: Duration) {
        self.mirror_served_at(host, bytes, took, Instant::now());
    }

    fn mirror_served_at(&self, host: &str, bytes: u64, took: Duration, now: Instant) {
        let newly_slow = {
            let mut hosts = self.hosts();
            let Some(health) = hosts.get_mut(host) else {
                return;
            };
            let Some(trial) = health.trial(now) else {
                return;
            };
            trial.mirror.push(Sample {
                at: now,
                bytes,
                took,
            });
            health.judge(now)
        };
        if newly_slow {
            self.changed();
        }
    }

    /// A file of `host` failed at the mirror: a running trial is lost, and a
    /// host the mirror took over for speed goes back to its origin.
    pub fn mirror_failed(&self, host: &str) {
        let now = Instant::now();
        let back = {
            let mut hosts = self.hosts();
            let Some(health) = hosts.get_mut(host) else {
                return;
            };
            let slow = health.slow_until.is_some_and(|u| u > now);
            if slow || health.trial.is_some() {
                health.trial_lost(now);
            }
            let before = health.mirrored(now);
            health.slow_until = None;
            before && !health.mirrored(now)
        };
        if back {
            self.changed();
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
            let was_down = health.mirrored(now);
            if health.failures.len() >= FAILURES_TO_DOWN {
                health.down_until = Some(now + DOWN_FOR);
                health.failures.clear();
                health.trial = None;
            }
            !was_down && health.mirrored(now)
        };
        if newly_down {
            self.changed();
        }
    }

    /// The origin served a request: it is not down any more. A host the
    /// mirror took over for speed stays with it until its time ends.
    pub fn origin_worked(&self, host: &str) {
        let now = Instant::now();
        let was_down = {
            let mut hosts = self.hosts();
            let Some(health) = hosts.get_mut(host) else {
                return;
            };
            health.failures.clear();
            let before = health.mirrored(now);
            health.down_until = None;
            before && !health.mirrored(now)
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
            .filter(|(_, h)| h.mirrored(now))
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

    /// Routing that counts its changes.
    fn counted() -> (Routing, Arc<Mutex<usize>>) {
        let routing = Routing::new(MIRROR_BASE);
        let changes = Arc::new(Mutex::new(0));
        let counted = Arc::clone(&changes);
        routing.on_change(move || *counted.lock().unwrap() += 1);
        (routing, changes)
    }

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    /// Four origin files of `bytes` that took `took` each, one after another
    /// from `at`; returns when the last one ended.
    fn origin_files(
        routing: &Routing,
        host: &str,
        at: Instant,
        bytes: u64,
        took: Duration,
    ) -> Instant {
        let mut now = at;
        for _ in 0..SPEED_MIN_FILES {
            now += took;
            routing.origin_served_at(host, bytes, took, now);
        }
        now
    }

    #[test]
    fn small_files_of_a_slow_origin_are_compared_and_a_faster_mirror_takes_over() {
        let (routing, changes) = counted();
        let host = "resources.download.minecraft.net";
        // Small files that each wait long: 4 KB in 300 ms.
        let mut now = origin_files(&routing, host, Instant::now(), 4096, ms(300));
        // Files take turns: mirror, origin, ... until each side has three.
        let routes: Vec<Route> = (0..8)
            .map(|_| routing.first_for_file_at(host, now))
            .collect();
        use Route::{Mirror, Origin};
        assert_eq!(
            routes,
            [
                Mirror, Origin, Mirror, Origin, Mirror, Origin, Origin, Origin
            ]
        );
        for _ in 0..TRIAL_FILES {
            now += ms(300);
            routing.mirror_served_at(host, 4096, ms(40), now);
            routing.origin_served_at(host, 4096, ms(300), now);
        }
        assert_eq!(routing.first(host), Route::Mirror);
        assert_eq!(routing.mirrored(), [host]);
        assert_eq!(*changes.lock().unwrap(), 1);
        // A file that still came from the origin does not end it; time does.
        routing.origin_worked(host);
        assert_eq!(routing.first(host), Route::Mirror);
        assert_eq!(*changes.lock().unwrap(), 1);
    }

    #[test]
    fn a_mirror_not_twice_as_fast_leaves_the_host_alone_for_a_while() {
        let (routing, changes) = counted();
        let host = "libraries.minecraft.net";
        let mut now = origin_files(&routing, host, Instant::now(), 500_000, ms(500));
        for _ in 0..TRIAL_FILES {
            now += ms(500);
            routing.mirror_served_at(host, 500_000, ms(300), now);
            routing.origin_served_at(host, 500_000, ms(500), now);
        }
        assert_eq!(routing.first(host), Route::Origin);
        assert!(routing.mirrored().is_empty());
        assert_eq!(*changes.lock().unwrap(), 0);
        // No new trial until the cooldown has passed.
        now = origin_files(&routing, host, now, 500_000, ms(500));
        assert_eq!(routing.first_for_file_at(host, now), Route::Origin);
        now = origin_files(&routing, host, now + TRIAL_COOLDOWN, 500_000, ms(500));
        assert_eq!(routing.first_for_file_at(host, now), Route::Mirror);
    }

    #[test]
    fn a_fast_origin_or_too_few_files_start_no_trial() {
        let routing = Routing::new(MIRROR_BASE);
        let now = Instant::now();
        // 1 MiB in 100 ms: fast.
        let end = origin_files(&routing, "fast.example", now, 1 << 20, ms(100));
        let end = origin_files(&routing, "fast.example", end, 1 << 20, ms(100));
        assert_eq!(
            routing.first_for_file_at("fast.example", end),
            Route::Origin
        );
        // Slow, but four files of 100 ms are not enough time to judge.
        let end = origin_files(&routing, "brief.example", now, 1000, ms(100));
        assert_eq!(
            routing.first_for_file_at("brief.example", end),
            Route::Origin
        );
        // Slow, but only three files.
        let mut at = now;
        for _ in 0..SPEED_MIN_FILES - 1 {
            at += ms(900);
            routing.origin_served_at("few.example", 1000, ms(900), at);
        }
        assert_eq!(routing.first_for_file_at("few.example", at), Route::Origin);
    }

    #[test]
    fn trials_end_on_a_mirror_failure_or_after_their_limit() {
        let routing = Routing::new(MIRROR_BASE);
        let now = origin_files(&routing, "a.example", Instant::now(), 1000, ms(400));
        assert_eq!(routing.first_for_file_at("a.example", now), Route::Mirror);
        routing.mirror_failed("a.example");
        assert_eq!(routing.first_for_file_at("a.example", now), Route::Origin);
        assert_eq!(routing.first_for_file_at("a.example", now), Route::Origin);

        let now = origin_files(&routing, "b.example", Instant::now(), 1000, ms(400));
        assert_eq!(routing.first_for_file_at("b.example", now), Route::Mirror);
        let late = now + TRIAL_LIMIT + ms(1);
        assert_eq!(routing.first_for_file_at("b.example", late), Route::Origin);
        routing.mirror_served_at("b.example", 1000, ms(1), late);
        assert!(routing.mirrored().is_empty());
    }

    #[test]
    fn a_mirror_failure_gives_a_slow_host_back_to_its_origin() {
        let (routing, changes) = counted();
        let host = "e.example";
        let mut now = origin_files(&routing, host, Instant::now(), 1000, ms(400));
        for _ in 0..TRIAL_FILES {
            now += ms(400);
            routing.mirror_served_at(host, 1000, ms(20), now);
            routing.origin_served_at(host, 1000, ms(400), now);
        }
        assert_eq!(routing.mirrored(), [host]);
        routing.mirror_failed(host);
        assert!(routing.mirrored().is_empty());
        assert_eq!(*changes.lock().unwrap(), 2, "to the mirror, then back");
        // A host down at the origin stays with the mirror.
        routing.origin_failed(host);
        routing.origin_failed(host);
        routing.mirror_failed(host);
        assert_eq!(routing.mirrored(), [host]);
    }

    #[test]
    fn only_origin_files_asked_during_a_trial_are_compared() {
        let routing = Routing::new(MIRROR_BASE);
        let host = "c.example";
        let mut now = origin_files(&routing, host, Instant::now(), 1000, ms(400));
        for _ in 0..TRIAL_FILES {
            now += ms(10);
            routing.mirror_served_at(host, 1000, ms(10), now);
            // Asked before the trial began: fast, but not counted.
            routing.origin_served_at(host, 1000, ms(5000), now);
        }
        assert!(
            routing.mirrored().is_empty(),
            "no origin files of the trial yet"
        );
        for _ in 0..TRIAL_FILES {
            now += ms(400);
            routing.origin_served_at(host, 1000, ms(400), now);
        }
        assert_eq!(routing.mirrored(), [host]);
    }

    #[test]
    fn only_the_automatic_mode_compares() {
        let routing = Routing::new(MIRROR_BASE);
        routing.set_mode(DownloadSource::OriginOnly);
        let now = origin_files(&routing, "d.example", Instant::now(), 1000, ms(400));
        assert_eq!(routing.first_for_file_at("d.example", now), Route::Origin);
        routing.set_mode(DownloadSource::MirrorFirst);
        assert_eq!(routing.first_for_file_at("d.example", now), Route::Mirror);
        routing.set_mode(DownloadSource::Automatic);
        assert_eq!(routing.first_for_file_at("d.example", now), Route::Origin);
    }
}
