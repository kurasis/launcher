//! Traffic the launcher downloads, by month and source (D-051).
//!
//! Every response body the launcher reads from the network is counted where it
//! is read: game files, Java and loader files in the download executor, version
//! lists and other metadata in [`crate::net::PublicClient::get_bounded`]. The
//! counts collect in memory and are added to `DATA_ROOT/traffic.json` under the
//! current month by [`flush`]. The game's own traffic is not the launcher's and
//! is not counted.

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

use crate::error::{ErrorCode, LauncherError, LauncherResult};
use crate::net::SourcePolicy;
use crate::storage::document::{self, Loaded, SCHEMA_VERSION};
use crate::storage::paths::DataRoot;

/// Months kept in `traffic.json`, the current one included.
const MONTHS_KEPT: usize = 12;
const MIB: u64 = 1024 * 1024;
/// Downloads of at least this many MiB ask first unless the user changes it.
pub const DEFAULT_WARN_MIB: u32 = 100;
/// Largest warning size: 100 GiB. 0 asks before every download.
const MAX_WARN_MIB: u32 = 100 * 1024;
/// Largest monthly limit: 10 TiB.
const MAX_LIMIT_MIB: u32 = 10 * 1024 * 1024;

pub fn validate_warn(mib: u32) -> LauncherResult<u32> {
    if mib <= MAX_WARN_MIB {
        Ok(mib)
    } else {
        Err(LauncherError::templated(
            ErrorCode::InvalidInput,
            "Ask before downloads of at most {max} MiB.",
            crate::error::params([("max", u64::from(MAX_WARN_MIB).into())]),
        ))
    }
}

/// A monthly limit in MiB; 0 is no limit.
pub fn validate_limit(mib: u32) -> LauncherResult<u32> {
    if mib <= MAX_LIMIT_MIB {
        Ok(mib)
    } else {
        Err(LauncherError::templated(
            ErrorCode::InvalidInput,
            "The monthly limit can be at most {max} MiB.",
            crate::error::params([("max", u64::from(MAX_LIMIT_MIB).into())]),
        ))
    }
}

/// Bytes downloaded, by source.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    /// Game files and Mojang's Java (Mojang's file servers).
    #[serde(default)]
    pub game: u64,
    /// Java from Adoptium.
    #[serde(default)]
    pub java: u64,
    /// Mod loader files and metadata.
    #[serde(default)]
    pub loaders: u64,
    /// Version lists and other Mojang metadata.
    #[serde(default)]
    pub metadata: u64,
}

impl Usage {
    pub fn total(&self) -> u64 {
        self.game + self.java + self.loaders + self.metadata
    }

    fn add(&mut self, other: Self) {
        self.game += other.game;
        self.java += other.java;
        self.loaders += other.loaders;
        self.metadata += other.metadata;
    }
}

/// Bytes counted but not yet written, by source.
pub struct Meter([AtomicU64; 4]);

/// The application's meter. Unit tests never count into it, so tests that
/// download in parallel cannot disturb each other's numbers.
static GLOBAL: Meter = Meter::new();
/// Serializes changes to `traffic.json`.
static FILE: Mutex<()> = Mutex::new(());

fn slot(source: SourcePolicy) -> usize {
    match source {
        SourcePolicy::MojangArtifacts => 0,
        SourcePolicy::Adoptium => 1,
        SourcePolicy::ModLoaders => 2,
        // Crash reports are small and count with the metadata (D-061).
        SourcePolicy::MojangMetadata | SourcePolicy::CrashReports => 3,
    }
}

fn usage_of(values: [u64; 4]) -> Usage {
    Usage {
        game: values[0],
        java: values[1],
        loaders: values[2],
        metadata: values[3],
    }
}

impl Meter {
    pub const fn new() -> Self {
        Self([const { AtomicU64::new(0) }; 4])
    }

    pub fn record(&self, source: SourcePolicy, bytes: u64) {
        self.0[slot(source)].fetch_add(bytes, Ordering::Relaxed);
    }

    fn pending(&self) -> Usage {
        usage_of(self.0.each_ref().map(|c| c.load(Ordering::Relaxed)))
    }

    fn take(&self) -> Usage {
        usage_of(self.0.each_ref().map(|c| c.swap(0, Ordering::Relaxed)))
    }

    fn put_back(&self, usage: Usage) {
        for (counter, value) in
            self.0
                .iter()
                .zip([usage.game, usage.java, usage.loaders, usage.metadata])
        {
            counter.fetch_add(value, Ordering::Relaxed);
        }
    }
}

impl Default for Meter {
    fn default() -> Self {
        Self::new()
    }
}

/// Counts bytes read from the network from `source`.
pub fn record(source: SourcePolicy, bytes: u64) {
    #[cfg(not(test))]
    GLOBAL.record(source, bytes);
    #[cfg(test)]
    let _ = (source, bytes);
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TrafficFile {
    schema_version: u64,
    /// `YYYY-MM` in local time.
    #[serde(default)]
    months: BTreeMap<String, Usage>,
}

/// The current month as `traffic.json` names it.
pub fn this_month() -> String {
    chrono::Local::now().format("%Y-%m").to_string()
}

fn read(root: &DataRoot) -> TrafficFile {
    match document::load::<TrafficFile>(&root.traffic(), None) {
        Ok(Loaded::Valid(file) | Loaded::RecoveredFromBackup { value: file, .. }) => file,
        _ => TrafficFile {
            schema_version: SCHEMA_VERSION,
            months: BTreeMap::new(),
        },
    }
}

/// Adds what was counted since the last flush to the current month. Counts
/// stay in memory if the file cannot be written.
pub fn flush(root: &DataRoot) {
    flush_from(&GLOBAL, root);
}

fn flush_from(meter: &Meter, root: &DataRoot) {
    let _guard = FILE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let counted = meter.take();
    if counted.total() == 0 {
        return;
    }
    let mut file = read(root);
    file.schema_version = SCHEMA_VERSION;
    file.months.entry(this_month()).or_default().add(counted);
    while file.months.len() > MONTHS_KEPT {
        file.months.pop_first();
    }
    if document::save(&root.traffic(), &file, None).is_err() {
        meter.put_back(counted);
    }
}

/// One month's total.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MonthTotal {
    pub month: String,
    pub bytes: u64,
}

/// What the Settings screen shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrafficSummary {
    pub month: String,
    pub this_month: Usage,
    /// Earlier months, newest first.
    pub earlier: Vec<MonthTotal>,
}

/// Traffic of this month (counts not yet written included) and earlier months.
pub fn summary(root: &DataRoot) -> TrafficSummary {
    summary_from(&GLOBAL, root)
}

fn summary_from(meter: &Meter, root: &DataRoot) -> TrafficSummary {
    let file = {
        let _guard = FILE
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        read(root)
    };
    let month = this_month();
    let mut this_month = file.months.get(&month).copied().unwrap_or_default();
    this_month.add(meter.pending());
    let earlier = file
        .months
        .iter()
        .rev()
        .filter(|(m, _)| **m != month)
        .map(|(m, usage)| MonthTotal {
            month: m.clone(),
            bytes: usage.total(),
        })
        .collect();
    TrafficSummary {
        month,
        this_month,
        earlier,
    }
}

/// Bytes downloaded this month.
pub fn month_total(root: &DataRoot) -> u64 {
    summary(root).this_month.total()
}

/// Forgets all counted traffic.
pub fn reset(root: &DataRoot) -> std::io::Result<()> {
    reset_from(&GLOBAL, root)
}

fn reset_from(meter: &Meter, root: &DataRoot) -> std::io::Result<()> {
    let _guard = FILE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    meter.take();
    match std::fs::remove_file(root.traffic()) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}

/// Whether a download of `bytes` must be confirmed first, and why.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Caution {
    /// At least the size the user asked to be warned about.
    Large,
    /// It would take this month's traffic past the user's limit.
    OverLimit,
}

/// The metered-connection rule: nothing to confirm unless the connection is
/// metered; then a download confirms first when it reaches the warning size or
/// would pass the monthly limit (`0` means no limit).
pub fn caution(
    metered: bool,
    warn_mib: u32,
    limit_mib: u32,
    used: u64,
    bytes: u64,
) -> Option<Caution> {
    if !metered || bytes == 0 {
        return None;
    }
    if limit_mib > 0 && used.saturating_add(bytes) > u64::from(limit_mib) * MIB {
        return Some(Caution::OverLimit);
    }
    (bytes >= u64::from(warn_mib) * MIB).then_some(Caution::Large)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_are_kept_by_month_and_source() {
        let dir = tempfile::tempdir().unwrap();
        let root = DataRoot::new(dir.path().to_path_buf()).unwrap();
        let meter = Meter::new();
        meter.record(SourcePolicy::MojangArtifacts, 3 * MIB);
        meter.record(SourcePolicy::Adoptium, 2 * MIB);
        meter.record(SourcePolicy::ModLoaders, 1000);
        meter.record(SourcePolicy::MojangMetadata, 10);
        // Not written yet, but counted.
        let before = summary_from(&meter, &root);
        assert_eq!(before.this_month.game, 3 * MIB);
        assert!(!root.traffic().exists());

        flush_from(&meter, &root);
        assert!(root.traffic().exists());
        let after = summary_from(&meter, &root);
        assert_eq!(
            after.this_month,
            Usage {
                game: 3 * MIB,
                java: 2 * MIB,
                loaders: 1000,
                metadata: 10,
            }
        );
        meter.record(SourcePolicy::MojangArtifacts, 5);
        flush_from(&meter, &root);
        assert_eq!(
            summary_from(&meter, &root).this_month.total(),
            5 * MIB + 1015
        );

        // Earlier months are listed newest first; only twelve are kept.
        let mut file = read(&root);
        for month in 1..=14 {
            file.months.insert(
                format!("2000-{month:02}"),
                Usage {
                    game: month,
                    ..Usage::default()
                },
            );
        }
        document::save(&root.traffic(), &file, None).unwrap();
        meter.record(SourcePolicy::MojangMetadata, 1);
        flush_from(&meter, &root);
        let kept = summary_from(&meter, &root);
        assert_eq!(kept.earlier.len(), MONTHS_KEPT - 1);
        assert_eq!(kept.earlier[0].month, "2000-14");

        meter.record(SourcePolicy::MojangMetadata, 1);
        reset_from(&meter, &root).unwrap();
        assert_eq!(summary_from(&meter, &root).this_month.total(), 0);
        assert!(summary_from(&meter, &root).earlier.is_empty());
    }

    #[test]
    fn metered_downloads_confirm_when_large_or_over_the_limit() {
        // Not metered: never.
        assert_eq!(caution(false, 1, 1, 0, 10 * MIB), None);
        // Small downloads go ahead.
        assert_eq!(caution(true, 100, 0, 0, 99 * MIB), None);
        assert_eq!(caution(true, 100, 0, 0, 100 * MIB), Some(Caution::Large));
        // The limit counts what was already used this month.
        assert_eq!(caution(true, 100, 1024, 1000 * MIB, 10 * MIB), None);
        assert_eq!(
            caution(true, 100, 1024, 1020 * MIB, 10 * MIB),
            Some(Caution::OverLimit)
        );
        // 0 asks before every download.
        assert_eq!(caution(true, 0, 0, 0, 1), Some(Caution::Large));
        // Nothing to download: nothing to confirm.
        assert_eq!(caution(true, 1, 1, 10 * MIB, 0), None);
        assert!(validate_warn(MAX_WARN_MIB + 1).is_err());
        assert!(validate_limit(MAX_LIMIT_MIB + 1).is_err());
        assert_eq!(validate_limit(0).unwrap(), 0);
    }
}
