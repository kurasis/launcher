//! Sums by day in SQLite (`data/stats.db`), the random IDs of the batches
//! already counted, and the ads that showed each day. No row holds an
//! address, an ID of a launcher, or a request as it came. Every query takes
//! its values as parameters.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use chrono::{Datelike, Duration, NaiveDate};
use rusqlite::{Connection, OptionalExtension, params};

use crate::model::Period;
use crate::tally::Key;

/// Rows one day may add to a table, and different values one cut (or one
/// ad's event and cut) may have in a day; beyond them counts go to `other`,
/// so made-up values cannot fill the disk.
pub const MAX_ROWS_PER_DAY: i64 = 100_000;
pub const MAX_VALUES: i64 = 20_000;
pub const MAX_AD_VALUES: i64 = 300;
/// Batches counted per day at most, both kinds together.
pub const MAX_BATCHES_PER_DAY: i64 = 1_000_000;
/// Batch IDs are kept a day longer than a launcher keeps a batch.
pub const KEEP_BATCH_DAYS: i64 = 8;
/// Days of players' sums; older ones are summed by month.
pub const KEEP_DAILY_DAYS: i64 = 730;
/// Days of ad counts (disputes with an advertiser, the ad register).
pub const KEEP_AD_DAYS: i64 = 1095;

const SCHEMA: &str = "
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
CREATE TABLE IF NOT EXISTS players (
    day TEXT NOT NULL,
    -- d: the day's ping; w, m: the first of its week, month; n: the first ever.
    period TEXT NOT NULL,
    cut TEXT NOT NULL,
    value TEXT NOT NULL,
    count INTEGER NOT NULL,
    PRIMARY KEY (day, period, cut, value)
) WITHOUT ROWID;
-- Days older than two years, summed by month (YYYY-MM).
CREATE TABLE IF NOT EXISTS players_monthly (
    month TEXT NOT NULL,
    period TEXT NOT NULL,
    cut TEXT NOT NULL,
    value TEXT NOT NULL,
    count INTEGER NOT NULL,
    PRIMARY KEY (month, period, cut, value)
) WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS ads (
    day TEXT NOT NULL,
    ad INTEGER NOT NULL,
    event TEXT NOT NULL,
    -- all (value ''), country, ui, or version (of the event picked).
    cut TEXT NOT NULL,
    value TEXT NOT NULL,
    count INTEGER NOT NULL,
    PRIMARY KEY (day, ad, event, cut, value)
) WITHOUT ROWID;
-- The ads the ads service showed each day, with their name and erid.
CREATE TABLE IF NOT EXISTS ads_seen (
    day TEXT NOT NULL,
    ad INTEGER NOT NULL,
    name TEXT NOT NULL,
    erid TEXT,
    PRIMARY KEY (day, ad)
) WITHOUT ROWID;
-- Random IDs of the batches counted, by the day they came, only so that a
-- batch sent again is not counted twice.
CREATE TABLE IF NOT EXISTS batches (
    id BLOB PRIMARY KEY,
    day TEXT NOT NULL
) WITHOUT ROWID;
CREATE INDEX IF NOT EXISTS batches_day ON batches (day);
-- What was refused or left out, by a fixed word.
CREATE TABLE IF NOT EXISTS dropped (
    day TEXT NOT NULL,
    reason TEXT NOT NULL,
    count INTEGER NOT NULL,
    PRIMARY KEY (day, reason)
) WITHOUT ROWID;
";

/// Whether a batch is counted now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fresh {
    New,
    /// Counted before: it was sent again.
    Seen,
    /// The day's batches are at their limit.
    Full,
}

/// An ad the ads service shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShownAd {
    pub id: i64,
    pub name: String,
    pub erid: Option<String>,
}

pub struct Store {
    db: Mutex<Connection>,
    /// Batches counted on a day, so the limit needs no count of rows.
    batches: Mutex<Option<(NaiveDate, i64)>>,
}

fn err(e: rusqlite::Error) -> String {
    format!("database: {e}")
}

pub fn day_text(day: NaiveDate) -> String {
    day.format("%Y-%m-%d").to_string()
}

/// Days `from` to `to`, both included, as the database writes them.
fn span(from: NaiveDate, to: NaiveDate) -> (String, String) {
    (day_text(from), day_text(to))
}

impl Store {
    pub fn open(data: &Path) -> Result<Self, String> {
        std::fs::create_dir_all(data).map_err(|e| format!("creating {}: {e}", data.display()))?;
        let path = data.join("stats.db");
        let db = Connection::open(&path).map_err(err)?;
        db.busy_timeout(std::time::Duration::from_secs(10))
            .map_err(err)?;
        db.execute_batch(SCHEMA).map_err(err)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
        }
        Ok(Self {
            db: Mutex::new(db),
            batches: Mutex::new(None),
        })
    }

    fn db(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.db
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Notes a batch as counted, unless it was before or the day's batches
    /// are at their limit.
    pub fn first_time(&self, id: uuid::Uuid, today: NaiveDate) -> Result<Fresh, String> {
        let mut batches = self
            .batches
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let db = self.db();
        let counted = match *batches {
            Some((day, n)) if day == today => n,
            _ => db
                .query_row(
                    "SELECT count(*) FROM batches WHERE day = ?1",
                    [day_text(today)],
                    |r| r.get(0),
                )
                .map_err(err)?,
        };
        if counted >= MAX_BATCHES_PER_DAY {
            *batches = Some((today, counted));
            return Ok(Fresh::Full);
        }
        let added = db
            .execute(
                "INSERT OR IGNORE INTO batches (id, day) VALUES (?1, ?2)",
                params![id.as_bytes().as_slice(), day_text(today)],
            )
            .map_err(err)?;
        *batches = Some((today, counted + added as i64));
        Ok(if added == 1 { Fresh::New } else { Fresh::Seen })
    }

    /// Adds counts from memory. A new row past a limit goes to the `other`
    /// value of its cut instead; returns how many did.
    pub fn write(&self, counts: &HashMap<Key, i64>) -> Result<usize, String> {
        let mut keys: Vec<(&Key, i64)> = counts.iter().map(|(k, n)| (k, *n)).collect();
        keys.sort();
        let mut db = self.db();
        let tx = db.transaction().map_err(err)?;
        let mut limits = Limits::default();
        let mut folded = 0;
        for (key, n) in keys {
            match key {
                Key::Player {
                    day,
                    period,
                    cut,
                    value,
                } => {
                    let day = day_text(*day);
                    let period = period.code();
                    let update = |value: &str| {
                        tx.prepare_cached(
                            "UPDATE players SET count = count + ?5
                             WHERE day = ?1 AND period = ?2 AND cut = ?3 AND value = ?4",
                        )
                        .and_then(|mut q| q.execute(params![day, period, cut, value, n]))
                        .map_err(err)
                    };
                    let insert = |value: &str| {
                        tx.prepare_cached("INSERT INTO players VALUES (?1, ?2, ?3, ?4, ?5)")
                            .and_then(|mut q| q.execute(params![day, period, cut, value, n]))
                            .map_err(err)
                    };
                    if update(value)? > 0 {
                        continue;
                    }
                    if value == "other" || limits.player_room(&tx, &day, period, cut)? {
                        insert(value)?;
                    } else {
                        folded += 1;
                        if update("other")? > 0 {
                            continue;
                        }
                        insert("other")?;
                    }
                    limits.player_added(&day, period, cut);
                }
                Key::Ad {
                    day,
                    ad,
                    event,
                    cut,
                    value,
                } => {
                    let day = day_text(*day);
                    let update = |value: &str| {
                        tx.prepare_cached(
                            "UPDATE ads SET count = count + ?6
                             WHERE day = ?1 AND ad = ?2 AND event = ?3 AND cut = ?4 AND value = ?5",
                        )
                        .and_then(|mut q| q.execute(params![day, ad, event, cut, value, n]))
                        .map_err(err)
                    };
                    let insert = |value: &str| {
                        tx.prepare_cached("INSERT INTO ads VALUES (?1, ?2, ?3, ?4, ?5, ?6)")
                            .and_then(|mut q| q.execute(params![day, ad, event, cut, value, n]))
                            .map_err(err)
                    };
                    if update(value)? > 0 {
                        continue;
                    }
                    // The totals (`all`) are a few rows per ad and event.
                    if value == "other"
                        || *cut == "all"
                        || limits.ad_room(&tx, &day, *ad, event, cut)?
                    {
                        insert(value)?;
                    } else {
                        folded += 1;
                        if update("other")? > 0 {
                            continue;
                        }
                        insert("other")?;
                    }
                    limits.ad_added(&day, *ad, event, cut);
                }
                Key::Dropped { day, reason } => {
                    tx.prepare_cached(
                        "INSERT INTO dropped VALUES (?1, ?2, ?3)
                         ON CONFLICT DO UPDATE SET count = count + excluded.count",
                    )
                    .and_then(|mut q| q.execute(params![day_text(*day), reason, n]))
                    .map_err(err)?;
                }
            }
        }
        tx.commit().map_err(err)?;
        Ok(folded)
    }

    /// Keeps the ads showing on `day`, with their names and erids now.
    pub fn save_ads(&self, day: NaiveDate, ads: &[ShownAd]) -> Result<(), String> {
        let mut db = self.db();
        let tx = db.transaction().map_err(err)?;
        for ad in ads {
            tx.execute(
                "INSERT INTO ads_seen VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT DO UPDATE SET name = excluded.name, erid = excluded.erid",
                params![day_text(day), ad.id, ad.name, ad.erid],
            )
            .map_err(err)?;
        }
        tx.commit().map_err(err)
    }

    /// The ads seen on each day from `from` on.
    pub fn ads_seen(&self, from: NaiveDate) -> Result<BTreeMap<NaiveDate, HashSet<i64>>, String> {
        let db = self.db();
        let mut q = db
            .prepare("SELECT day, ad FROM ads_seen WHERE day >= ?1")
            .map_err(err)?;
        let rows = q
            .query_map([day_text(from)], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
            })
            .map_err(err)?;
        let mut seen: BTreeMap<NaiveDate, HashSet<i64>> = BTreeMap::new();
        for row in rows {
            let (day, ad) = row.map_err(err)?;
            if let Some(day) = crate::model::day(&day) {
                seen.entry(day).or_default().insert(ad);
            }
        }
        Ok(seen)
    }

    /// The newest name of each ad seen between the days.
    pub fn ad_names(&self, from: NaiveDate, to: NaiveDate) -> Result<HashMap<i64, String>, String> {
        let (from, to) = span(from, to);
        let db = self.db();
        let mut q = db
            .prepare("SELECT ad, name FROM ads_seen WHERE day >= ?1 AND day <= ?2 ORDER BY day")
            .map_err(err)?;
        let rows = q
            .query_map([from, to], |r| Ok((r.get(0)?, r.get(1)?)))
            .map_err(err)?;
        rows.collect::<rusqlite::Result<HashMap<_, _>>>()
            .map_err(err)
    }

    /// The name of one ad, from the last day it showed.
    pub fn ad_name(&self, ad: i64) -> Result<Option<String>, String> {
        self.db()
            .query_row(
                "SELECT name FROM ads_seen WHERE ad = ?1 ORDER BY day DESC LIMIT 1",
                [ad],
                |r| r.get(0),
            )
            .optional()
            .map_err(err)
    }

    /// Sum of one cut's counts over the days, per value, the biggest first.
    pub fn values(
        &self,
        from: NaiveDate,
        to: NaiveDate,
        period: Period,
        cut: &str,
    ) -> Result<Vec<(String, i64)>, String> {
        let (from, to) = span(from, to);
        let db = self.db();
        let mut q = db
            .prepare(
                "SELECT value, sum(count) FROM players
                 WHERE day >= ?1 AND day <= ?2 AND period = ?3 AND cut = ?4
                 GROUP BY value ORDER BY sum(count) DESC, value",
            )
            .map_err(err)?;
        let rows = q
            .query_map(params![from, to, period.code(), cut], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .map_err(err)?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(err)
    }

    /// Pings per day of a period: every ping has one country, so the
    /// country cut holds them all.
    pub fn daily(
        &self,
        from: NaiveDate,
        to: NaiveDate,
        period: Period,
    ) -> Result<BTreeMap<NaiveDate, i64>, String> {
        let (from_text, to_text) = span(from, to);
        let db = self.db();
        let mut q = db
            .prepare(
                "SELECT day, sum(count) FROM players
                 WHERE day >= ?1 AND day <= ?2 AND period = ?3 AND cut = 'country'
                 GROUP BY day",
            )
            .map_err(err)?;
        let rows = q
            .query_map(params![from_text, to_text, period.code()], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
            })
            .map_err(err)?;
        let mut days = BTreeMap::new();
        let mut day = from;
        while day <= to {
            days.insert(day, 0);
            day += Duration::days(1);
        }
        for row in rows {
            let (day, n) = row.map_err(err)?;
            if let Some(day) = crate::model::day(&day) {
                days.insert(day, n);
            }
        }
        Ok(days)
    }

    pub fn total(&self, from: NaiveDate, to: NaiveDate, period: Period) -> Result<i64, String> {
        Ok(self.daily(from, to, period)?.values().sum())
    }

    /// Launchers of one install week (`2026-W41`), from one country or all,
    /// that counted for `period` between the days.
    pub fn cohort(
        &self,
        week: &str,
        country: Option<&str>,
        period: Period,
        from: NaiveDate,
        to: NaiveDate,
    ) -> Result<i64, String> {
        let (from, to) = span(from, to);
        let prefix = format!("{week}/");
        let exact = country.map(|c| format!("{prefix}{c}"));
        self.db()
            .query_row(
                "SELECT coalesce(sum(count), 0) FROM players
                 WHERE day >= ?1 AND day <= ?2 AND period = ?3 AND cut = 'cohort'
                   AND (CASE WHEN ?5 IS NULL THEN substr(value, 1, length(?4)) = ?4
                             ELSE value = ?5 END)",
                params![from, to, period.code(), prefix, exact],
                |r| r.get(0),
            )
            .map_err(err)
    }

    /// Each ad's events between the days, for the `all` cut: (ad, event) → n.
    pub fn ad_totals(
        &self,
        from: NaiveDate,
        to: NaiveDate,
        ad: Option<i64>,
    ) -> Result<BTreeMap<(i64, String), i64>, String> {
        let (from, to) = span(from, to);
        let db = self.db();
        let mut q = db
            .prepare(
                "SELECT ad, event, sum(count) FROM ads
                 WHERE day >= ?1 AND day <= ?2 AND cut = 'all' AND (?3 IS NULL OR ad = ?3)
                 GROUP BY ad, event",
            )
            .map_err(err)?;
        let rows = q
            .query_map(params![from, to, ad], |r| {
                Ok(((r.get(0)?, r.get(1)?), r.get(2)?))
            })
            .map_err(err)?;
        rows.collect::<rusqlite::Result<BTreeMap<_, _>>>()
            .map_err(err)
    }

    /// One ad's events per day, for the `all` cut: day → event → n.
    pub fn ad_days(
        &self,
        from: NaiveDate,
        to: NaiveDate,
        ad: i64,
    ) -> Result<BTreeMap<String, BTreeMap<String, i64>>, String> {
        let (from, to) = span(from, to);
        let db = self.db();
        let mut q = db
            .prepare(
                "SELECT day, event, sum(count) FROM ads
                 WHERE day >= ?1 AND day <= ?2 AND ad = ?3 AND cut = 'all'
                 GROUP BY day, event ORDER BY day",
            )
            .map_err(err)?;
        let rows = q
            .query_map(params![from, to, ad], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, i64>(2)?,
                ))
            })
            .map_err(err)?;
        let mut days: BTreeMap<String, BTreeMap<String, i64>> = BTreeMap::new();
        for row in rows {
            let (day, event, n) = row.map_err(err)?;
            days.entry(day).or_default().insert(event, n);
        }
        Ok(days)
    }

    /// One ad's event by the values of a cut, the biggest first.
    pub fn ad_values(
        &self,
        from: NaiveDate,
        to: NaiveDate,
        ad: i64,
        event: &str,
        cut: &str,
    ) -> Result<Vec<(String, i64)>, String> {
        let (from, to) = span(from, to);
        let db = self.db();
        let mut q = db
            .prepare(
                "SELECT value, sum(count) FROM ads
                 WHERE day >= ?1 AND day <= ?2 AND ad = ?3 AND event = ?4 AND cut = ?5
                 GROUP BY value ORDER BY sum(count) DESC, value",
            )
            .map_err(err)?;
        let rows = q
            .query_map(params![from, to, ad, event, cut], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .map_err(err)?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(err)
    }

    /// Impressions of a month per erid (the erid each ad had that day):
    /// erid → (ads, shown, shown in Russia). Ads without one under "".
    pub fn by_erid(&self, month: &str) -> Result<BTreeMap<String, (i64, i64, i64)>, String> {
        let db = self.db();
        let mut q = db
            .prepare(
                "SELECT coalesce(s.erid, ''), count(DISTINCT a.ad),
                        sum(CASE WHEN a.cut = 'all' THEN a.count ELSE 0 END),
                        sum(CASE WHEN a.cut = 'country' AND a.value = 'RU' THEN a.count ELSE 0 END)
                 FROM ads a JOIN ads_seen s ON s.day = a.day AND s.ad = a.ad
                 WHERE substr(a.day, 1, 7) = ?1 AND a.event = 'shown'
                   AND (a.cut = 'all' OR (a.cut = 'country' AND a.value = 'RU'))
                 GROUP BY coalesce(s.erid, '')",
            )
            .map_err(err)?;
        let rows = q
            .query_map([month], |r| {
                Ok((r.get(0)?, (r.get(1)?, r.get(2)?, r.get(3)?)))
            })
            .map_err(err)?;
        rows.collect::<rusqlite::Result<BTreeMap<_, _>>>()
            .map_err(err)
    }

    /// Ads of a month with their erid, for the `ord` listing: erid → ids.
    pub fn erid_ads(&self, month: &str) -> Result<BTreeMap<String, Vec<i64>>, String> {
        let db = self.db();
        let mut q = db
            .prepare(
                "SELECT DISTINCT erid, ad FROM ads_seen
                 WHERE substr(day, 1, 7) = ?1 AND erid IS NOT NULL ORDER BY erid, ad",
            )
            .map_err(err)?;
        let rows = q
            .query_map([month], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
            })
            .map_err(err)?;
        let mut erids: BTreeMap<String, Vec<i64>> = BTreeMap::new();
        for row in rows {
            let (erid, ad) = row.map_err(err)?;
            erids.entry(erid).or_default().push(ad);
        }
        Ok(erids)
    }

    /// What was left out between the days, per reason.
    pub fn dropped(&self, from: NaiveDate, to: NaiveDate) -> Result<Vec<(String, i64)>, String> {
        let (from, to) = span(from, to);
        let db = self.db();
        let mut q = db
            .prepare(
                "SELECT reason, sum(count) FROM dropped WHERE day >= ?1 AND day <= ?2
                 GROUP BY reason ORDER BY sum(count) DESC",
            )
            .map_err(err)?;
        let rows = q
            .query_map([from, to], |r| Ok((r.get(0)?, r.get(1)?)))
            .map_err(err)?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(err)
    }

    /// Removes batch IDs after `KEEP_BATCH_DAYS`, sums the players' days
    /// older than `KEEP_DAILY_DAYS` by month, and removes ad counts after
    /// `KEEP_AD_DAYS`.
    pub fn prune(&self, today: NaiveDate) -> Result<(), String> {
        let before = |days: i64| day_text(today - Duration::days(days));
        let mut db = self.db();
        let tx = db.transaction().map_err(err)?;
        tx.execute(
            "DELETE FROM batches WHERE day < ?1",
            [before(KEEP_BATCH_DAYS)],
        )
        .map_err(err)?;
        let old = before(KEEP_DAILY_DAYS);
        tx.execute(
            "INSERT INTO players_monthly
             SELECT substr(day, 1, 7), period, cut, value, sum(count) FROM players
             WHERE day < ?1 GROUP BY substr(day, 1, 7), period, cut, value
             ON CONFLICT DO UPDATE SET count = count + excluded.count",
            [&old],
        )
        .map_err(err)?;
        tx.execute("DELETE FROM players WHERE day < ?1", [&old])
            .map_err(err)?;
        tx.execute("DELETE FROM dropped WHERE day < ?1", [&old])
            .map_err(err)?;
        let old_ads = before(KEEP_AD_DAYS);
        tx.execute("DELETE FROM ads WHERE day < ?1", [&old_ads])
            .map_err(err)?;
        tx.execute("DELETE FROM ads_seen WHERE day < ?1", [&old_ads])
            .map_err(err)?;
        tx.commit().map_err(err)
    }

    /// A copy of the database in `dir`; the newest `keep` copies stay.
    pub fn backup(&self, dir: &Path, keep: usize) -> Result<PathBuf, String> {
        std::fs::create_dir_all(dir).map_err(|e| format!("creating {}: {e}", dir.display()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
                .map_err(|e| e.to_string())?;
        }
        let name = format!(
            "stats-{}.db",
            chrono::Utc::now().format("%Y%m%d-%H%M%S-%3f")
        );
        let path = dir.join(name);
        self.db()
            .execute("VACUUM INTO ?1", [path.to_string_lossy()])
            .map_err(|e| format!("copying the database: {e}"))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
                .map_err(|e| e.to_string())?;
        }
        let mut copies: Vec<PathBuf> = std::fs::read_dir(dir)
            .map_err(|e| e.to_string())?
            .flatten()
            .map(|entry| entry.path())
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with("stats-") && n.ends_with(".db"))
            })
            .collect();
        copies.sort();
        let extra = copies.len().saturating_sub(keep);
        for old in &copies[..extra] {
            std::fs::remove_file(old).map_err(|e| e.to_string())?;
        }
        Ok(path)
    }

    #[cfg(test)]
    pub fn rows(&self, table: &str) -> i64 {
        // Tests name the table.
        self.db()
            .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
            .unwrap_or(-1)
    }
}

/// Rows and values per day as one write goes, read once per day and cut.
#[derive(Default)]
struct Limits {
    rows: HashMap<(&'static str, String), i64>,
    values: HashMap<String, i64>,
}

impl Limits {
    fn rows_of(
        &mut self,
        tx: &rusqlite::Transaction<'_>,
        table: &'static str,
        day: &str,
    ) -> Result<i64, String> {
        if let Some(n) = self.rows.get(&(table, day.to_owned())) {
            return Ok(*n);
        }
        // The names are this file's constants, never anyone's input.
        let n: i64 = tx
            .query_row(
                &format!("SELECT count(*) FROM {table} WHERE day = ?1"),
                [day],
                |r| r.get(0),
            )
            .map_err(err)?;
        self.rows.insert((table, day.to_owned()), n);
        Ok(n)
    }

    fn values_of(
        &mut self,
        key: String,
        count: impl FnOnce() -> Result<i64, String>,
    ) -> Result<i64, String> {
        if let Some(n) = self.values.get(&key) {
            return Ok(*n);
        }
        let n = count()?;
        self.values.insert(key, n);
        Ok(n)
    }

    fn player_room(
        &mut self,
        tx: &rusqlite::Transaction<'_>,
        day: &str,
        period: &str,
        cut: &str,
    ) -> Result<bool, String> {
        let rows = self.rows_of(tx, "players", day)?;
        let values = self.values_of(format!("p|{day}|{period}|{cut}"), || {
            tx.query_row(
                "SELECT count(*) FROM players WHERE day = ?1 AND period = ?2 AND cut = ?3",
                params![day, period, cut],
                |r| r.get(0),
            )
            .map_err(err)
        })?;
        Ok(rows < MAX_ROWS_PER_DAY && values < MAX_VALUES)
    }

    fn player_added(&mut self, day: &str, period: &str, cut: &str) {
        *self.rows.entry(("players", day.to_owned())).or_default() += 1;
        *self
            .values
            .entry(format!("p|{day}|{period}|{cut}"))
            .or_default() += 1;
    }

    fn ad_room(
        &mut self,
        tx: &rusqlite::Transaction<'_>,
        day: &str,
        ad: i64,
        event: &str,
        cut: &str,
    ) -> Result<bool, String> {
        let rows = self.rows_of(tx, "ads", day)?;
        let values = self.values_of(format!("a|{day}|{ad}|{event}|{cut}"), || {
            tx.query_row(
                "SELECT count(*) FROM ads WHERE day = ?1 AND ad = ?2 AND event = ?3 AND cut = ?4",
                params![day, ad, event, cut],
                |r| r.get(0),
            )
            .map_err(err)
        })?;
        Ok(rows < MAX_ROWS_PER_DAY && values < MAX_AD_VALUES)
    }

    fn ad_added(&mut self, day: &str, ad: i64, event: &str, cut: &str) {
        *self.rows.entry(("ads", day.to_owned())).or_default() += 1;
        *self
            .values
            .entry(format!("a|{day}|{ad}|{event}|{cut}"))
            .or_default() += 1;
    }
}

/// The first day of a month written `YYYY-MM`.
pub fn month_start(text: &str) -> Option<NaiveDate> {
    (text.len() == 7)
        .then(|| NaiveDate::parse_from_str(&format!("{text}-01"), "%Y-%m-%d").ok())
        .flatten()
}

/// The last day of the month `day` is in.
pub fn month_end(day: NaiveDate) -> NaiveDate {
    let (year, month) = if day.month() == 12 {
        (day.year() + 1, 1)
    } else {
        (day.year(), day.month() + 1)
    };
    NaiveDate::from_ymd_opt(year, month, 1).map_or(day, |next| next - Duration::days(1))
}
