//! What the mirror keeps: files forever under `files/<host>/<path>` (nginx
//! serves them from there), metadata by its URL's hash under `meta/`, and the
//! quota that decides whether a new file is kept (D-062).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::Digest as _;

/// Free space a disk keeps whatever the quota says.
pub const MIN_FREE_BYTES: u64 = 5 * 1024 * 1024 * 1024;
pub const GB: u64 = 1024 * 1024 * 1024;

pub struct Store {
    files: PathBuf,
    meta: PathBuf,
    tmp: PathBuf,
    /// Bytes under `files/`; known once the first count finished.
    usage: AtomicU64,
    counted: AtomicBool,
    quota: AtomicU64,
    min_free: u64,
}

/// What is known about a kept metadata response.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MetaInfo {
    pub url: String,
    pub fetched: DateTime<Utc>,
    pub content_type: Option<String>,
}

/// The settings file the deploy writes (repository variable `MIRROR_MAX_GB`).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Settings {
    max_gb: u64,
}

impl Store {
    pub fn open(data: &Path, quota_bytes: u64) -> Result<Self, String> {
        let store = Self {
            files: data.join("files"),
            meta: data.join("meta"),
            tmp: data.join("tmp"),
            usage: AtomicU64::new(0),
            counted: AtomicBool::new(false),
            quota: AtomicU64::new(quota_bytes),
            min_free: MIN_FREE_BYTES,
        };
        for dir in [&store.files, &store.meta, &store.tmp] {
            std::fs::create_dir_all(dir).map_err(|e| format!("creating {}: {e}", dir.display()))?;
        }
        Ok(store)
    }

    /// Removes parts of fetches a stop cut short. Only the service may: the
    /// other commands (`warm`, `overview`) run beside it and would cut its
    /// fetches short.
    pub fn clear_parts(&self) {
        if let Ok(entries) = std::fs::read_dir(&self.tmp) {
            for entry in entries.flatten() {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }

    /// Free space to keep instead of [`MIN_FREE_BYTES`].
    pub fn with_min_free(mut self, bytes: u64) -> Self {
        self.min_free = bytes;
        self
    }

    pub fn file(&self, relative: &Path) -> PathBuf {
        self.files.join(relative)
    }

    /// A new part file; renamed into place when complete.
    pub fn part(&self) -> PathBuf {
        self.tmp.join(uuid::Uuid::new_v4().to_string())
    }

    pub fn quota(&self) -> u64 {
        self.quota.load(Ordering::Relaxed)
    }

    pub fn usage(&self) -> Option<u64> {
        self.counted
            .load(Ordering::Relaxed)
            .then(|| self.usage.load(Ordering::Relaxed))
    }

    /// Reads the quota from the settings file, if there is one.
    pub fn reload_quota(&self, settings: &Path) {
        if let Some(gb) = std::fs::read(settings)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Settings>(&bytes).ok())
            .map(|s| s.max_gb)
        {
            self.quota.store(gb.saturating_mul(GB), Ordering::Relaxed);
        }
    }

    /// Whether a new file of `size` bytes (if known) may be kept: within the
    /// quota, and the disk keeps its reserve free.
    pub fn may_keep(&self, size: Option<u64>) -> bool {
        let size = size.unwrap_or(0);
        let within_quota = self.usage.load(Ordering::Relaxed).saturating_add(size) <= self.quota();
        let free = rustix::fs::statvfs(&self.files)
            .map(|s| s.f_bavail.saturating_mul(s.f_frsize))
            .unwrap_or(0);
        within_quota && free >= self.min_free.saturating_add(size)
    }

    pub fn kept(&self, bytes: u64) {
        self.usage.fetch_add(bytes, Ordering::Relaxed);
    }

    /// Counts the kept files' bytes (at start; files may be many).
    pub fn count(&self) -> (u64, u64) {
        let (mut bytes, mut files) = (0u64, 0u64);
        let mut stack = vec![self.files.clone()];
        while let Some(dir) = stack.pop() {
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries.flatten() {
                match entry.file_type() {
                    Ok(t) if t.is_dir() => stack.push(entry.path()),
                    Ok(t) if t.is_file() => {
                        bytes += entry.metadata().map(|m| m.len()).unwrap_or(0);
                        files += 1;
                    }
                    _ => {}
                }
            }
        }
        self.usage.store(bytes, Ordering::Relaxed);
        self.counted.store(true, Ordering::Relaxed);
        (bytes, files)
    }

    fn meta_paths(&self, url: &str, host: &str) -> (PathBuf, PathBuf) {
        let key = hex::encode(sha2::Sha256::digest(url.as_bytes()));
        let dir = self.meta.join(host);
        (
            dir.join(format!("{key}.body")),
            dir.join(format!("{key}.json")),
        )
    }

    /// The kept copy of a metadata URL.
    pub fn read_meta(&self, url: &str, host: &str) -> Option<(MetaInfo, Vec<u8>)> {
        let (body, info) = self.meta_paths(url, host);
        let info: MetaInfo = serde_json::from_slice(&std::fs::read(info).ok()?).ok()?;
        (info.url == url).then_some(())?;
        Some((info, std::fs::read(body).ok()?))
    }

    /// Keeps a metadata response: body first, then its description, each
    /// written whole and renamed into place.
    pub fn write_meta(&self, host: &str, info: &MetaInfo, body: &[u8]) -> std::io::Result<()> {
        let (body_path, info_path) = self.meta_paths(&info.url, host);
        if let Some(dir) = body_path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let part = self.part();
        std::fs::write(&part, body)?;
        std::fs::rename(&part, &body_path)?;
        let part = self.part();
        std::fs::write(&part, serde_json::to_vec(info).unwrap_or_default())?;
        std::fs::rename(&part, &info_path)
    }

    /// Kept files per host, and metadata copies, for the overview.
    pub fn overview(&self) -> serde_json::Value {
        let mut hosts = serde_json::Map::new();
        for host in crate::origin::HOSTS {
            let mut bytes = 0u64;
            let mut files = 0u64;
            let mut stack = vec![self.files.join(host)];
            while let Some(dir) = stack.pop() {
                let Ok(entries) = std::fs::read_dir(&dir) else {
                    continue;
                };
                for entry in entries.flatten() {
                    match entry.file_type() {
                        Ok(t) if t.is_dir() => stack.push(entry.path()),
                        Ok(t) if t.is_file() => {
                            bytes += entry.metadata().map(|m| m.len()).unwrap_or(0);
                            files += 1;
                        }
                        _ => {}
                    }
                }
            }
            let meta = std::fs::read_dir(self.meta.join(host))
                .map(|e| e.flatten().count() / 2)
                .unwrap_or(0);
            if files > 0 || meta > 0 {
                hosts.insert(
                    (*host).to_owned(),
                    serde_json::json!({ "files": files, "gb": bytes as f64 / GB as f64, "metadata": meta }),
                );
            }
        }
        let free = rustix::fs::statvfs(&self.files)
            .map(|s| s.f_bavail.saturating_mul(s.f_frsize))
            .unwrap_or(0);
        serde_json::json!({
            "keptGb": self.usage().map(|b| b / GB),
            "quotaGb": self.quota() / GB,
            "freeDiskGb": free / GB,
            "hosts": hosts,
        })
    }
}
