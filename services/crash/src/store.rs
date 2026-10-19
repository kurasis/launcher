//! Reports on disk (gzip JSON, one file each) and their index in SQLite.

use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Causes the launcher names: its crash analysis (`launch/crash.rs`,
/// `CrashKind`) and its own failures (`journal.rs`, `FailureKind`). Add a
/// new one here too, or the public summary shows it as "other".
pub const KINDS: &[&str] = &[
    "jvm_option",
    "heap_unavailable",
    "java_too_old",
    "java_too_new",
    "missing_dependency",
    "wrong_dependency_version",
    "wrong_minecraft_version",
    "loader_too_old",
    "incompatible_mods",
    "duplicate_mods",
    "mod_for_other_loader",
    "corrupt_mod_file",
    "corrupt_config",
    "mixin_failed",
    "mod_failed_to_load",
    "optifine_conflict",
    "out_of_memory",
    "pack_needs_memory",
    "system_memory",
    "killed",
    "opengl_unsupported",
    "graphics_driver",
    "overlay",
    "native_crash",
    "missing_native_library",
    "broken_installation",
    "disk_full",
    "file_access",
    "world_load_failed",
    "ticking_entity",
    "ticking_block_entity",
    "server_hang",
    "debug_crash",
    "game_crash",
    "terminated",
    "console_question",
    "unknown",
    "panic",
    "frontend",
    "unclean_exit",
];

use chrono::{DateTime, Utc};
use flate2::Compression;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use rusqlite::{Connection, OptionalExtension, params};

use crate::model::{Event, Feedback, Report};

/// Distinct rows a day may add; beyond them a day's counts stop growing, so
/// made-up values cannot fill the disk.
const MAX_STAT_ROWS_PER_DAY: i64 = 100_000;
const MAX_FEEDBACK_ROWS_PER_DAY: i64 = 20_000;

/// What happened to a report.
#[derive(Debug, PartialEq, Eq)]
pub enum Added {
    Stored(String),
    /// The install sent its daily allowance.
    Limited,
    /// The stored reports reached their size limit.
    Full,
}

pub struct Store {
    db: Mutex<Connection>,
    reports: PathBuf,
}

/// One group of reports with the same cause and signature.
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Group {
    pub source: String,
    pub kind: String,
    pub signature: String,
    pub reports: i64,
    pub installs: i64,
    pub first: String,
    pub last: String,
    pub latest_version: String,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Listed {
    pub id: String,
    pub received: String,
    pub source: String,
    pub kind: String,
    pub signature: String,
    pub launcher_version: String,
    pub os: String,
    pub game: String,
    pub bytes: i64,
}

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS reports (
    id TEXT PRIMARY KEY,
    received INTEGER NOT NULL,
    install_id TEXT NOT NULL,
    source TEXT NOT NULL,
    kind TEXT NOT NULL,
    signature TEXT NOT NULL,
    launcher_version TEXT NOT NULL,
    os TEXT NOT NULL,
    arch TEXT NOT NULL,
    game_version TEXT NOT NULL,
    loader TEXT NOT NULL,
    loader_version TEXT NOT NULL,
    analyzer_version INTEGER NOT NULL,
    bytes INTEGER NOT NULL,
    path TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS reports_group ON reports (source, kind, signature);
CREATE INDEX IF NOT EXISTS reports_received ON reports (received);
CREATE INDEX IF NOT EXISTS reports_install ON reports (install_id, received);
CREATE TABLE IF NOT EXISTS stats (
    day TEXT NOT NULL,
    source TEXT NOT NULL,
    kind TEXT NOT NULL,
    signature TEXT NOT NULL,
    launcher_version TEXT NOT NULL,
    os TEXT NOT NULL,
    game_version TEXT NOT NULL,
    loader TEXT NOT NULL,
    loader_version TEXT NOT NULL,
    analyzer_version INTEGER NOT NULL,
    exit_code TEXT NOT NULL,
    count INTEGER NOT NULL,
    UNIQUE (day, source, kind, signature, launcher_version, os, game_version, loader,
            loader_version, analyzer_version, exit_code)
);
CREATE TABLE IF NOT EXISTS feedback (
    day TEXT NOT NULL,
    kind TEXT NOT NULL,
    signature TEXT NOT NULL,
    analyzer_version INTEGER NOT NULL,
    launcher_version TEXT NOT NULL,
    helpful INTEGER NOT NULL,
    count INTEGER NOT NULL,
    UNIQUE (day, kind, signature, analyzer_version, launcher_version, helpful)
);
";

fn day(now: DateTime<Utc>) -> String {
    now.format("%Y-%m-%d").to_string()
}

fn time(seconds: i64) -> String {
    DateTime::from_timestamp(seconds, 0)
        .map(|t| t.format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_default()
}

impl Store {
    pub fn open(data: &Path) -> Result<Self, String> {
        let reports = data.join("reports");
        fs::create_dir_all(&reports).map_err(|e| format!("creating {}: {e}", reports.display()))?;
        let db = Connection::open(data.join("crash.db")).map_err(|e| e.to_string())?;
        db.execute_batch("PRAGMA journal_mode = WAL; PRAGMA busy_timeout = 5000;")
            .map_err(|e| e.to_string())?;
        db.execute_batch(SCHEMA).map_err(|e| e.to_string())?;
        Ok(Self {
            db: Mutex::new(db),
            reports,
        })
    }

    fn db(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.db
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Bytes of the stored reports (gzip).
    pub fn stored_bytes(&self) -> i64 {
        self.db()
            .query_row("SELECT coalesce(sum(bytes), 0) FROM reports", [], |r| {
                r.get(0)
            })
            .unwrap_or(i64::MAX)
    }

    /// Writes the report's file, then its row, unless the stored reports
    /// already take `max_bytes`.
    pub fn add(
        &self,
        report: &Report,
        now: DateTime<Utc>,
        max_bytes: i64,
    ) -> Result<Added, String> {
        if self.stored_bytes() >= max_bytes {
            return Ok(Added::Full);
        }
        let id = uuid::Uuid::new_v4().to_string();
        let relative = format!("{}/{id}.json.gz", now.format("%Y/%m"));
        let path = self.reports.join(&relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let json = serde_json::to_vec(report).map_err(|e| e.to_string())?;
        let mut gz = GzEncoder::new(Vec::new(), Compression::default());
        gz.write_all(&json).map_err(|e| e.to_string())?;
        let bytes = gz.finish().map_err(|e| e.to_string())?;
        let temp = path.with_extension("tmp");
        fs::write(&temp, &bytes).map_err(|e| e.to_string())?;
        fs::rename(&temp, &path).map_err(|e| e.to_string())?;
        let game = report.game.clone().unwrap_or_default();
        let inserted = self.db().execute(
            "INSERT INTO reports VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
            params![
                id,
                now.timestamp(),
                report.install_id,
                report.source,
                report.kind,
                report.signature,
                report.launcher.version,
                report.launcher.os,
                report.launcher.arch,
                game.version,
                game.loader,
                game.loader_version,
                report.analyzer_version,
                i64::try_from(bytes.len()).unwrap_or(i64::MAX),
                relative,
            ],
        );
        if let Err(e) = inserted {
            let _ = fs::remove_file(&path);
            return Err(e.to_string());
        }
        Ok(Added::Stored(id))
    }

    /// Rows a table holds for one day.
    fn rows_on(db: &Connection, table: &str, day: &str) -> i64 {
        db.query_row(
            &format!("SELECT count(*) FROM {table} WHERE day = ?1"),
            [day],
            |r| r.get(0),
        )
        .unwrap_or(i64::MAX)
    }

    /// Reports from one install today, for the daily limit.
    pub fn today_from(&self, install_id: &str, now: DateTime<Utc>) -> i64 {
        let since = now.timestamp() - 86_400;
        self.db()
            .query_row(
                "SELECT count(*) FROM reports WHERE install_id = ?1 AND received > ?2",
                params![install_id, since],
                |r| r.get(0),
            )
            .unwrap_or(0)
    }

    /// Counts the events; false when today's rows are at their limit.
    pub fn count(&self, events: &[Event], now: DateTime<Utc>) -> Result<bool, String> {
        let mut db = self.db();
        if Self::rows_on(&db, "stats", &day(now)) >= MAX_STAT_ROWS_PER_DAY {
            return Ok(false);
        }
        let tx = db.transaction().map_err(|e| e.to_string())?;
        for e in events {
            tx.execute(
                "INSERT INTO stats VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, 1)
                 ON CONFLICT DO UPDATE SET count = count + 1",
                params![
                    day(now),
                    e.source,
                    e.kind,
                    e.signature,
                    e.launcher_version,
                    e.os,
                    e.game_version,
                    e.loader,
                    e.loader_version,
                    e.analyzer_version,
                    // NULLs never conflict in SQLite: no code is the empty text.
                    e.exit_code.map(|c| c.to_string()).unwrap_or_default(),
                ],
            )
            .map_err(|e| e.to_string())?;
        }
        tx.commit().map(|()| true).map_err(|e| e.to_string())
    }

    /// Counts one answer; false when today's rows are at their limit.
    pub fn feedback(&self, f: &Feedback, now: DateTime<Utc>) -> Result<bool, String> {
        let db = self.db();
        if Self::rows_on(&db, "feedback", &day(now)) >= MAX_FEEDBACK_ROWS_PER_DAY {
            return Ok(false);
        }
        db.execute(
            "INSERT INTO feedback VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1)
                 ON CONFLICT DO UPDATE SET count = count + 1",
            params![
                day(now),
                f.kind,
                f.signature,
                f.analyzer_version,
                f.launcher_version,
                f.helpful
            ],
        )
        .map(|_| true)
        .map_err(|e| e.to_string())
    }

    /// The biggest groups of the last `days` days.
    pub fn groups(&self, days: i64, source: Option<&str>, limit: i64) -> Vec<Group> {
        let since = Utc::now().timestamp() - days * 86_400;
        let db = self.db();
        let Ok(mut q) = db.prepare(
            "SELECT source, kind, signature, count(*), count(DISTINCT install_id),
                    min(received), max(received), max(launcher_version)
             FROM reports WHERE received > ?1 AND (?2 IS NULL OR source = ?2)
             GROUP BY source, kind, signature ORDER BY count(*) DESC LIMIT ?3",
        ) else {
            return Vec::new();
        };
        q.query_map(params![since, source, limit], |r| {
            Ok(Group {
                source: r.get(0)?,
                kind: r.get(1)?,
                signature: r.get(2)?,
                reports: r.get(3)?,
                installs: r.get(4)?,
                first: time(r.get(5)?),
                last: time(r.get(6)?),
                latest_version: r.get(7)?,
            })
        })
        .map(|rows| rows.flatten().collect())
        .unwrap_or_default()
    }

    pub fn list(&self, kind: Option<&str>, signature: Option<&str>, limit: i64) -> Vec<Listed> {
        let db = self.db();
        let Ok(mut q) = db.prepare(
            "SELECT id, received, source, kind, signature, launcher_version, os,
                    trim(game_version || ' ' || loader || ' ' || loader_version), bytes
             FROM reports WHERE (?1 IS NULL OR kind = ?1) AND (?2 IS NULL OR signature = ?2)
             ORDER BY received DESC LIMIT ?3",
        ) else {
            return Vec::new();
        };
        q.query_map(params![kind, signature, limit], |r| {
            Ok(Listed {
                id: r.get(0)?,
                received: time(r.get(1)?),
                source: r.get(2)?,
                kind: r.get(3)?,
                signature: r.get(4)?,
                launcher_version: r.get(5)?,
                os: r.get(6)?,
                game: r.get(7)?,
                bytes: r.get(8)?,
            })
        })
        .map(|rows| rows.flatten().collect())
        .unwrap_or_default()
    }

    /// The stored report, as sent.
    pub fn get(&self, id: &str) -> Result<Option<Report>, String> {
        let path: Option<String> = self
            .db()
            .query_row("SELECT path FROM reports WHERE id = ?1", [id], |r| r.get(0))
            .optional()
            .map_err(|e| e.to_string())?;
        let Some(path) = path else {
            return Ok(None);
        };
        let file = fs::File::open(self.reports.join(path)).map_err(|e| e.to_string())?;
        let mut json = Vec::new();
        GzDecoder::new(file)
            .read_to_end(&mut json)
            .map_err(|e| e.to_string())?;
        serde_json::from_slice(&json)
            .map(Some)
            .map_err(|e| e.to_string())
    }

    /// Daily counts of the statistics and the feedback, newest first.
    pub fn summary(&self, days: i64) -> serde_json::Value {
        let since = day(Utc::now() - chrono::Duration::days(days));
        let db = self.db();
        let rows = |sql: &str| -> Vec<serde_json::Value> {
            let Ok(mut q) = db.prepare(sql) else {
                return Vec::new();
            };
            let columns: Vec<String> = q.column_names().iter().map(|c| (*c).to_owned()).collect();
            q.query_map([&since], |r| {
                let mut row = serde_json::Map::new();
                for (i, name) in columns.iter().enumerate() {
                    let value: rusqlite::types::Value = r.get(i)?;
                    let value = match value {
                        rusqlite::types::Value::Integer(n) => serde_json::json!(n),
                        rusqlite::types::Value::Text(t) => serde_json::json!(t),
                        _ => serde_json::Value::Null,
                    };
                    row.insert(name.clone(), value);
                }
                Ok(serde_json::Value::Object(row))
            })
            .map(|r| r.flatten().collect())
            .unwrap_or_default()
        };
        serde_json::json!({
            "causes": rows("SELECT source, kind, loader, sum(count) AS count FROM stats
                            WHERE day >= ?1 GROUP BY source, kind, loader ORDER BY count DESC LIMIT 50"),
            "feedback": rows("SELECT kind, sum(CASE WHEN helpful THEN count ELSE 0 END) AS helped,
                              sum(CASE WHEN helpful THEN 0 ELSE count END) AS not_helped
                              FROM feedback WHERE day >= ?1 GROUP BY kind ORDER BY not_helped DESC"),
        })
    }

    #[cfg(test)]
    pub fn fill_feedback_for_test(&self, day: &str, rows: i64) {
        let mut db = self.db();
        let tx = db.transaction().unwrap();
        for i in 0..rows {
            tx.execute(
                "INSERT INTO feedback VALUES (?1, 'k', ?2, 1, '0.1.0', 1, 1)",
                params![day, i.to_string()],
            )
            .unwrap();
        }
        tx.commit().unwrap();
    }

    /// Counts per cause for a public summary: only fixed words (source,
    /// cause, launcher version), never signatures, notes or files.
    pub fn overview(&self, days: i64) -> serde_json::Value {
        // The summary is public: a cause the launcher does not name, or a
        // version that is not one, is whatever a sender wrote, so it shows as
        // "other".
        let kind = |s: String| {
            if KINDS.contains(&s.as_str()) {
                s
            } else {
                "other".to_owned()
            }
        };
        let version = |s: String| {
            let (core, pre) = s.split_once('-').unwrap_or((&s, ""));
            let numbers = core.split('.').count() == 3
                && core.split('.').all(|p| {
                    !p.is_empty() && p.len() <= 5 && p.bytes().all(|b| b.is_ascii_digit())
                });
            let label = pre.len() <= 16
                && pre
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'.');
            if numbers && label {
                s
            } else {
                "other".to_owned()
            }
        };
        let since = Utc::now().timestamp() - days * 86_400;
        let since_day = day(Utc::now() - chrono::Duration::days(days));
        let word = |s: String| {
            if !s.is_empty()
                && s.len() <= 40
                && s.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.' | b'+'))
            {
                s
            } else {
                "other".to_owned()
            }
        };
        let db = self.db();
        let reports: Vec<serde_json::Value> = db
            .prepare(
                "SELECT source, kind, count(*), count(DISTINCT install_id), max(launcher_version)
                 FROM reports WHERE received > ?1 GROUP BY source, kind ORDER BY count(*) DESC",
            )
            .and_then(|mut q| {
                q.query_map([since], |r| {
                    Ok(serde_json::json!({
                        "source": word(r.get(0)?),
                        "kind": kind(r.get(1)?),
                        "reports": r.get::<_, i64>(2)?,
                        "installs": r.get::<_, i64>(3)?,
                        "newestLauncher": version(r.get(4)?),
                    }))
                })
                .map(|rows| rows.flatten().collect())
            })
            .unwrap_or_default();
        let counted: Vec<serde_json::Value> = db
            .prepare(
                "SELECT source, kind, sum(count) FROM stats WHERE day >= ?1
                 GROUP BY source, kind ORDER BY sum(count) DESC",
            )
            .and_then(|mut q| {
                q.query_map([&since_day], |r| {
                    Ok(serde_json::json!({
                        "source": word(r.get(0)?),
                        "kind": kind(r.get(1)?),
                        "count": r.get::<_, i64>(2)?,
                    }))
                })
                .map(|rows| rows.flatten().collect())
            })
            .unwrap_or_default();
        let answers: Vec<serde_json::Value> = db
            .prepare(
                "SELECT kind, sum(CASE WHEN helpful THEN count ELSE 0 END),
                        sum(CASE WHEN helpful THEN 0 ELSE count END)
                 FROM feedback WHERE day >= ?1 GROUP BY kind ORDER BY 3 DESC",
            )
            .and_then(|mut q| {
                q.query_map([&since_day], |r| {
                    Ok(serde_json::json!({
                        "kind": kind(r.get(0)?),
                        "helped": r.get::<_, i64>(1)?,
                        "notHelped": r.get::<_, i64>(2)?,
                    }))
                })
                .map(|rows| rows.flatten().collect())
            })
            .unwrap_or_default();
        serde_json::json!({
            "days": days,
            "reports": reports,
            "statistics": counted,
            "feedback": answers,
        })
    }

    /// Removes every report of one install (a player's request).
    pub fn forget(&self, install_id: &str) -> Result<usize, String> {
        let found: Vec<(String, String)> = {
            let db = self.db();
            let mut q = db
                .prepare("SELECT id, path FROM reports WHERE install_id = ?1")
                .map_err(|e| e.to_string())?;
            q.query_map([install_id], |r| Ok((r.get(0)?, r.get(1)?)))
                .map_err(|e| e.to_string())?
                .flatten()
                .collect()
        };
        for (id, path) in &found {
            let _ = fs::remove_file(self.reports.join(path));
            self.db()
                .execute("DELETE FROM reports WHERE id = ?1", [id])
                .map_err(|e| e.to_string())?;
        }
        Ok(found.len())
    }

    /// Removes reports older than `report_days` and counts older than `stat_days`.
    pub fn prune(&self, report_days: i64, stat_days: i64) -> Result<usize, String> {
        let now = Utc::now();
        let cutoff = now.timestamp() - report_days * 86_400;
        let old: Vec<(String, String)> = {
            let db = self.db();
            let mut q = db
                .prepare("SELECT id, path FROM reports WHERE received < ?1")
                .map_err(|e| e.to_string())?;
            q.query_map([cutoff], |r| Ok((r.get(0)?, r.get(1)?)))
                .map_err(|e| e.to_string())?
                .flatten()
                .collect()
        };
        for (id, path) in &old {
            let _ = fs::remove_file(self.reports.join(path));
            self.db()
                .execute("DELETE FROM reports WHERE id = ?1", [id])
                .map_err(|e| e.to_string())?;
        }
        let stat_cutoff = day(now - chrono::Duration::days(stat_days));
        let db = self.db();
        db.execute("DELETE FROM stats WHERE day < ?1", [&stat_cutoff])
            .map_err(|e| e.to_string())?;
        db.execute("DELETE FROM feedback WHERE day < ?1", [&stat_cutoff])
            .map_err(|e| e.to_string())?;
        Ok(old.len())
    }
}
