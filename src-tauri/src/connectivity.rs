//! Whether the internet can be reached (D-052).
//!
//! The launcher works offline with what is installed; it only needs to know
//! when it is offline to keep downloads waiting and to say so, and when the
//! connection returns to check accounts and resume the queue. A probe asks a
//! Mojang metadata host for headers only (no body): every 30 seconds while
//! offline, every 5 minutes while online, and at once when something asks
//! (a download that failed for lack of network, or the user).

use std::time::Duration;

use tokio::sync::Notify;

use crate::install::service::BoxFuture;

use crate::state::AppState;
use crate::state::dto::NetworkStatus;

/// Any answer from this host means the internet can be reached.
pub const PROBE_URL: &str = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";
const WHILE_ONLINE: Duration = Duration::from_secs(5 * 60);
const WHILE_OFFLINE: Duration = Duration::from_secs(30);
const PROBE_TIMEOUT: Duration = Duration::from_secs(10);

/// One reachability check: `true` when the host answered at all.
pub type Probe = Box<dyn Fn() -> BoxFuture<'static, bool> + Send + Sync>;

pub struct Monitor {
    probe: Probe,
    wake: Notify,
}

/// The production probe: a HEAD request through the metadata client, so the
/// usual host allowlist, public-address resolver and proxy settings apply.
pub fn https_probe(client: crate::net::PublicClient) -> Probe {
    Box::new(move || {
        let client = client.clone();
        Box::pin(async move {
            let Ok(url) = url::Url::parse(PROBE_URL) else {
                return false;
            };
            let Ok(request) = client.head(&url) else {
                return false;
            };
            // Any HTTP status proves the network path works.
            matches!(
                tokio::time::timeout(PROBE_TIMEOUT, request.send()).await,
                Ok(Ok(_))
            )
        })
    })
}

impl Monitor {
    pub fn new(probe: Probe) -> Self {
        Self {
            probe,
            wake: Notify::new(),
        }
    }

    /// Checks again now instead of at the next interval.
    pub fn check_soon(&self) {
        self.wake.notify_one();
    }

    /// Probes once, records the result and reports whether the connection has
    /// just returned (it was offline before).
    pub async fn check(&self, state: &AppState) -> bool {
        let status = if (self.probe)().await {
            NetworkStatus::Online
        } else {
            NetworkStatus::Offline
        };
        let before = state.network();
        if before == status {
            return false;
        }
        state.set_network(status);
        let console = state.console();
        match status {
            NetworkStatus::Offline => console.warn(
                "No internet connection: installed versions still start; downloads wait in \
                 the queue",
            ),
            _ if before == NetworkStatus::Offline => {
                console.info("The internet connection is back");
            }
            _ => {}
        }
        before == NetworkStatus::Offline && status == NetworkStatus::Online
    }

    /// Waits until the next check is due for `status`, or until asked.
    pub async fn next(&self, status: NetworkStatus) {
        let wait = if status == NetworkStatus::Offline {
            WHILE_OFFLINE
        } else {
            WHILE_ONLINE
        };
        tokio::select! {
            () = tokio::time::sleep(wait) => {}
            () = self.wake.notified() => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    use super::*;
    use crate::storage::paths::DataRoot;
    use crate::validation::MemoryPolicy;

    #[tokio::test]
    async fn status_follows_the_probe_and_reports_reconnection_once() {
        let dir = tempfile::tempdir().unwrap();
        let state = AppState::initialize(
            DataRoot::new(dir.path().to_path_buf()),
            "test",
            MemoryPolicy::from_physical_bytes(16 << 30),
            Box::new(crate::state::NoopSink),
        );
        let up = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&up);
        let monitor = Monitor::new(Box::new(move || {
            let reachable = flag.load(Ordering::SeqCst);
            Box::pin(async move { reachable })
        }));
        assert_eq!(state.network(), NetworkStatus::Unknown);
        assert!(!monitor.check(&state).await);
        assert_eq!(state.network(), NetworkStatus::Offline);
        assert!(!monitor.check(&state).await, "still offline");
        up.store(true, Ordering::SeqCst);
        assert!(monitor.check(&state).await, "back online");
        assert_eq!(state.network(), NetworkStatus::Online);
        assert!(!monitor.check(&state).await, "no second reconnection");
        // Starting online is not a reconnection.
        let fresh = Monitor::new(Box::new(|| Box::pin(async { true })));
        let dir = tempfile::tempdir().unwrap();
        let other = AppState::initialize(
            DataRoot::new(dir.path().to_path_buf()),
            "test",
            MemoryPolicy::from_physical_bytes(16 << 30),
            Box::new(crate::state::NoopSink),
        );
        assert!(!fresh.check(&other).await);
        assert_eq!(other.network(), NetworkStatus::Online);
    }
}
