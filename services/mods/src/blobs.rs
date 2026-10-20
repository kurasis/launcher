//! The files of the catalog, kept forever by their SHA-256 under
//! `blobs/ab/cd/<sha256>`; nginx serves them from there at
//! files-mods.lapi.sh. A file is written to `tmp/`, hashed, checked against
//! the source's hashes, synced and renamed into place: a blob that exists is
//! always whole.
//!
//! A file the scanner holds back, or the owner blocks, lives under
//! `quarantine/ab/cd/<sha256>` instead, where nginx does not look.

use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use sha1::Digest as _;

pub const GB: u64 = 1024 * 1024 * 1024;

pub struct Blobs {
    blobs: PathBuf,
    quarantine: PathBuf,
    tmp: PathBuf,
    usage: AtomicU64,
    counted: AtomicBool,
    quota: AtomicU64,
    min_free: u64,
}

/// A file's hashes and size, computed by us.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hashed {
    pub sha256: String,
    pub sha1: String,
    pub sha512: String,
    pub size: u64,
}

pub fn is_sha256(text: &str) -> bool {
    text.len() == 64
        && text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// The settings file the deploy writes (repository variable `MODS_MAX_GB`).
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct Settings {
    max_gb: u64,
}

impl Blobs {
    pub fn open(data: &Path, quota_bytes: u64, min_free: u64) -> Result<Self, String> {
        let blobs = Self {
            blobs: data.join("blobs"),
            quarantine: data.join("quarantine"),
            tmp: data.join("tmp"),
            usage: AtomicU64::new(0),
            counted: AtomicBool::new(false),
            quota: AtomicU64::new(quota_bytes),
            min_free,
        };
        for dir in [&blobs.blobs, &blobs.quarantine, &blobs.tmp] {
            std::fs::create_dir_all(dir).map_err(|e| format!("creating {}: {e}", dir.display()))?;
        }
        // Parts are not served until whole, held files never.
        #[cfg(unix)]
        for dir in [&blobs.tmp, &blobs.quarantine] {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
                .map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        Ok(blobs)
    }

    pub fn tmp(&self) -> &Path {
        &self.tmp
    }

    /// A new part file in `tmp/`, on the same disk as the blobs.
    pub fn part(&self) -> PathBuf {
        self.tmp.join(uuid::Uuid::new_v4().to_string())
    }

    /// Removes what a stop cut short. Only the service does, at its start.
    pub fn clear_parts(&self) {
        if let Ok(entries) = std::fs::read_dir(&self.tmp) {
            for entry in entries.flatten() {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }

    pub fn path(&self, sha256: &str) -> PathBuf {
        self.blobs
            .join(&sha256[0..2])
            .join(&sha256[2..4])
            .join(sha256)
    }

    /// Where a held file lies.
    pub fn held_path(&self, sha256: &str) -> PathBuf {
        self.quarantine
            .join(&sha256[0..2])
            .join(&sha256[2..4])
            .join(sha256)
    }

    /// Whether a file is kept and served.
    pub fn has(&self, sha256: &str) -> bool {
        is_sha256(sha256) && self.path(sha256).is_file()
    }

    /// Whether a file is here at all, served or held: no need to fetch it.
    pub fn holds(&self, sha256: &str) -> bool {
        self.has(sha256) || (is_sha256(sha256) && self.held_path(sha256).is_file())
    }

    /// The file to read, served or held.
    pub fn find(&self, sha256: &str) -> Option<PathBuf> {
        if !is_sha256(sha256) {
            return None;
        }
        [self.path(sha256), self.held_path(sha256)]
            .into_iter()
            .find(|p| p.is_file())
    }

    pub fn quota(&self) -> u64 {
        self.quota.load(Ordering::Relaxed)
    }

    pub fn usage(&self) -> Option<u64> {
        self.counted
            .load(Ordering::Relaxed)
            .then(|| self.usage.load(Ordering::Relaxed))
    }

    pub fn reload_quota(&self, settings: &Path) {
        if let Some(gb) = std::fs::read(settings)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Settings>(&bytes).ok())
            .map(|s| s.max_gb)
        {
            self.quota.store(gb.saturating_mul(GB), Ordering::Relaxed);
        }
    }

    pub fn free(&self) -> u64 {
        rustix::fs::statvfs(&self.blobs)
            .map(|s| s.f_bavail.saturating_mul(s.f_frsize))
            .unwrap_or(0)
    }

    /// Whether `size` more bytes fit the quota and leave the disk its reserve.
    pub fn may_keep(&self, size: u64) -> bool {
        self.usage.load(Ordering::Relaxed).saturating_add(size) <= self.quota()
            && self.free() >= self.min_free.saturating_add(size)
    }

    /// Counts the kept bytes and files, held ones too (at start).
    pub fn count(&self) -> (u64, u64) {
        let (mut bytes, mut files) = (0u64, 0u64);
        let mut stack = vec![self.blobs.clone(), self.quarantine.clone()];
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

    /// Moves a hashed part into place under its SHA-256. A blob that is
    /// there already stays; the part goes. Returns whether it was new.
    pub fn keep(&self, part: &Path, hashed: &Hashed) -> std::io::Result<bool> {
        self.place(part, hashed, self.path(&hashed.sha256), 0o644)
    }

    /// Moves a hashed part into quarantine; nobody downloads it from there.
    pub fn hold(&self, part: &Path, hashed: &Hashed) -> std::io::Result<bool> {
        self.place(part, hashed, self.held_path(&hashed.sha256), 0o600)
    }

    /// Takes a served file out of service (a later scan or the owner).
    pub fn withdraw(&self, sha256: &str) -> std::io::Result<()> {
        let from = self.path(sha256);
        let to = self.held_path(sha256);
        if let Some(dir) = to.parent() {
            std::fs::create_dir_all(dir)?;
        }
        if to.is_file() {
            return std::fs::remove_file(&from);
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&from, std::fs::Permissions::from_mode(0o600))?;
        }
        std::fs::rename(&from, &to)
    }

    fn place(
        &self,
        part: &Path,
        hashed: &Hashed,
        target: PathBuf,
        mode: u32,
    ) -> std::io::Result<bool> {
        if target.is_file() {
            let _ = std::fs::remove_file(part);
            return Ok(false);
        }
        std::fs::File::open(part)?.sync_all()?;
        if let Some(dir) = target.parent() {
            std::fs::create_dir_all(dir)?;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(part, std::fs::Permissions::from_mode(mode))?;
        }
        std::fs::rename(part, &target)?;
        if let Some(dir) = target.parent()
            && let Ok(dir) = std::fs::File::open(dir)
        {
            let _ = dir.sync_all();
        }
        self.usage.fetch_add(hashed.size, Ordering::Relaxed);
        Ok(true)
    }
}

/// Hashes a file in one pass.
pub fn hash_file(path: &Path) -> std::io::Result<Hashed> {
    let mut file = std::fs::File::open(path)?;
    let mut sha256 = sha2::Sha256::new();
    let mut sha1 = sha1::Sha1::new();
    let mut sha512 = sha2::Sha512::new();
    let mut size = 0u64;
    let mut buffer = vec![0u8; 1 << 16];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        sha256.update(&buffer[..n]);
        sha1.update(&buffer[..n]);
        sha512.update(&buffer[..n]);
        size += n as u64;
    }
    Ok(Hashed {
        sha256: hex::encode(sha256.finalize()),
        sha1: hex::encode(sha1.finalize()),
        sha512: hex::encode(sha512.finalize()),
        size,
    })
}
