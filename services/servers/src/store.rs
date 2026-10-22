//! Ads, their last ping, their players online by hour and the Minecraft
//! releases in SQLite (`data/servers.db`). Only the owner's commands on the
//! server write ads; the service writes pings and releases. Every query
//! takes its values as parameters.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;

/// Ads sent to the launcher at most, the heaviest first.
pub const MAX_LIVE: usize = 50;

const SCHEMA: &str = r#"
PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;
CREATE TABLE IF NOT EXISTS ads (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    address TEXT NOT NULL,
    -- The version the Play button starts by default, and the range of
    -- releases the server lets in (both ends included).
    version TEXT NOT NULL,
    oldest TEXT NOT NULL,
    newest TEXT NOT NULL,
    -- JSON: {"*": default text, "en"|"ru"|"ar": text in that language}.
    texts TEXT NOT NULL,
    starts INTEGER NOT NULL,
    ends INTEGER NOT NULL,
    weight INTEGER NOT NULL,
    place TEXT NOT NULL,
    erid TEXT,
    paused INTEGER NOT NULL DEFAULT 0,
    -- The SHA-256 of an icon the owner gave, over the server's own.
    icon TEXT,
    created INTEGER NOT NULL,
    updated INTEGER NOT NULL,
    -- JSON: the languages whose players see the ad; NULL for everyone.
    langs TEXT,
    -- The SHA-256 of a 16:9 picture the owner gave, shown over the icon.
    banner TEXT
);
CREATE INDEX IF NOT EXISTS ads_ends ON ads(ends);
CREATE TABLE IF NOT EXISTS pings (
    ad INTEGER PRIMARY KEY REFERENCES ads(id) ON DELETE CASCADE,
    at INTEGER NOT NULL,
    up INTEGER NOT NULL,
    online INTEGER,
    max INTEGER,
    protocol INTEGER,
    -- The server's own words about its version; shown to the owner only.
    server_version TEXT,
    -- The last good icon stays when a ping fails.
    icon TEXT,
    -- Whether the server wants a Microsoft sign-in (online mode): 1, 0, or
    -- NULL when unknown; checked at `checked_at`, about once a day.
    licensed INTEGER,
    checked_at INTEGER
);
-- Players online at each hourly ping (D-071): whether an ad brought
-- players, before it started and while it shows. Goes with the ad.
CREATE TABLE IF NOT EXISTS online (
    ad INTEGER NOT NULL REFERENCES ads(id) ON DELETE CASCADE,
    -- Unix seconds of the hour's start.
    hour INTEGER NOT NULL,
    up INTEGER NOT NULL,
    online INTEGER,
    max INTEGER,
    PRIMARY KEY (ad, hour)
) WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS releases (
    id TEXT PRIMARY KEY,
    -- 0 is the newest release.
    rank INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS meta (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
"#;

/// Columns added since the first release, for databases made before them.
const ADDED: [(&str, &str, &str); 4] = [
    ("ads", "langs", "TEXT"),
    ("ads", "banner", "TEXT"),
    ("pings", "licensed", "INTEGER"),
    ("pings", "checked_at", "INTEGER"),
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Ad {
    pub id: i64,
    pub name: String,
    pub address: String,
    pub version: String,
    pub oldest: String,
    pub newest: String,
    pub texts: BTreeMap<String, String>,
    pub starts: i64,
    pub ends: i64,
    pub weight: i64,
    pub place: String,
    pub erid: Option<String>,
    pub paused: bool,
    pub icon: Option<String>,
    /// The languages whose players see the ad; empty for everyone.
    pub langs: Vec<String>,
    pub banner: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Ping {
    pub at: i64,
    pub up: bool,
    pub online: Option<i64>,
    pub max: Option<i64>,
    pub protocol: Option<i64>,
    pub server_version: Option<String>,
    pub icon: Option<String>,
    /// Whether the server wants a Microsoft sign-in; None when unknown.
    pub licensed: Option<bool>,
    /// When that was checked; None keeps the last check.
    pub checked_at: Option<i64>,
}

/// Players online in one hour, from that hour's ping.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Hour {
    /// Unix seconds of the hour's start.
    pub at: i64,
    pub up: bool,
    pub online: Option<i64>,
    pub max: Option<i64>,
}

/// What an ad's state is at a moment.
pub fn state(ad: &Ad, now: i64) -> &'static str {
    if ad.ends <= now {
        "ended"
    } else if ad.paused {
        "paused"
    } else if ad.starts > now {
        "scheduled"
    } else {
        "live"
    }
}

pub struct Store {
    db: Mutex<Connection>,
}

fn err(e: rusqlite::Error) -> String {
    format!("database: {e}")
}

const COLUMNS: &str = "id, name, address, version, oldest, newest, texts, starts, ends, weight, place, erid, paused, icon, langs, banner";

fn langs_column(langs: &[String]) -> Option<String> {
    (!langs.is_empty()).then(|| serde_json::to_string(langs).unwrap_or_default())
}

fn ad_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Ad> {
    let texts: String = row.get(6)?;
    Ok(Ad {
        id: row.get(0)?,
        name: row.get(1)?,
        address: row.get(2)?,
        version: row.get(3)?,
        oldest: row.get(4)?,
        newest: row.get(5)?,
        texts: serde_json::from_str(&texts).unwrap_or_default(),
        starts: row.get(7)?,
        ends: row.get(8)?,
        weight: row.get(9)?,
        place: row.get(10)?,
        erid: row.get(11)?,
        paused: row.get::<_, i64>(12)? != 0,
        icon: row.get(13)?,
        langs: row
            .get::<_, Option<String>>(14)?
            .and_then(|langs| serde_json::from_str(&langs).ok())
            .unwrap_or_default(),
        banner: row.get(15)?,
    })
}

/// Adds a column a database made before it lacks.
fn add_column(db: &Connection, table: &str, column: &str, kind: &str) -> Result<(), String> {
    let found: Option<i64> = db
        .query_row(
            "SELECT 1 FROM pragma_table_info(?1) WHERE name = ?2",
            [table, column],
            |row| row.get(0),
        )
        .optional()
        .map_err(err)?;
    if found.is_none() {
        // The names are this file's constants, never anyone's input.
        db.execute_batch(&format!("ALTER TABLE {table} ADD COLUMN {column} {kind}"))
            .map_err(err)?;
    }
    Ok(())
}

impl Store {
    pub fn open(data: &Path) -> Result<Self, String> {
        std::fs::create_dir_all(data).map_err(|e| format!("creating {}: {e}", data.display()))?;
        let path = data.join("servers.db");
        let db = Connection::open(&path).map_err(err)?;
        db.busy_timeout(std::time::Duration::from_secs(10))
            .map_err(err)?;
        db.execute_batch(SCHEMA).map_err(err)?;
        for (table, column, kind) in ADDED {
            add_column(&db, table, column, kind)?;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
        }
        Ok(Self { db: Mutex::new(db) })
    }

    fn db(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.db
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub fn insert(&self, ad: &Ad, now: i64) -> Result<i64, String> {
        let db = self.db();
        db.execute(
            "INSERT INTO ads (name, address, version, oldest, newest, texts, starts, ends, weight,
                place, erid, paused, icon, created, updated, langs, banner)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?14, ?15, ?16)",
            params![
                ad.name,
                ad.address,
                ad.version,
                ad.oldest,
                ad.newest,
                serde_json::to_string(&ad.texts).map_err(|e| e.to_string())?,
                ad.starts,
                ad.ends,
                ad.weight,
                ad.place,
                ad.erid,
                i64::from(ad.paused),
                ad.icon,
                now,
                langs_column(&ad.langs),
                ad.banner,
            ],
        )
        .map_err(err)?;
        Ok(db.last_insert_rowid())
    }

    pub fn update(&self, ad: &Ad, now: i64) -> Result<(), String> {
        let changed = self
            .db()
            .execute(
                "UPDATE ads SET name = ?2, address = ?3, version = ?4, oldest = ?5, newest = ?6,
                    texts = ?7, starts = ?8, ends = ?9, weight = ?10, place = ?11, erid = ?12,
                    paused = ?13, icon = ?14, updated = ?15, langs = ?16, banner = ?17
                 WHERE id = ?1",
                params![
                    ad.id,
                    ad.name,
                    ad.address,
                    ad.version,
                    ad.oldest,
                    ad.newest,
                    serde_json::to_string(&ad.texts).map_err(|e| e.to_string())?,
                    ad.starts,
                    ad.ends,
                    ad.weight,
                    ad.place,
                    ad.erid,
                    i64::from(ad.paused),
                    ad.icon,
                    now,
                    langs_column(&ad.langs),
                    ad.banner,
                ],
            )
            .map_err(err)?;
        if changed == 0 {
            return Err(format!("no ad {}", ad.id));
        }
        Ok(())
    }

    pub fn remove(&self, id: i64) -> Result<(), String> {
        let db = self.db();
        db.execute("DELETE FROM pings WHERE ad = ?1", [id])
            .map_err(err)?;
        db.execute("DELETE FROM online WHERE ad = ?1", [id])
            .map_err(err)?;
        match db
            .execute("DELETE FROM ads WHERE id = ?1", [id])
            .map_err(err)?
        {
            0 => Err(format!("no ad {id}")),
            _ => Ok(()),
        }
    }

    pub fn ad(&self, id: i64) -> Result<Option<Ad>, String> {
        self.db()
            .query_row(
                &format!("SELECT {COLUMNS} FROM ads WHERE id = ?1"),
                [id],
                ad_row,
            )
            .optional()
            .map_err(err)
    }

    /// Every ad, the newest first.
    pub fn all(&self) -> Result<Vec<Ad>, String> {
        let db = self.db();
        let mut statement = db
            .prepare(&format!("SELECT {COLUMNS} FROM ads ORDER BY id DESC"))
            .map_err(err)?;
        let rows = statement.query_map([], ad_row).map_err(err)?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(err)
    }

    /// Ads shown at `now` in `place` to players of `lang`, the heaviest
    /// first, at most `MAX_LIVE`.
    pub fn live(&self, place: &str, lang: &str, now: i64) -> Result<Vec<Ad>, String> {
        let db = self.db();
        let mut statement = db
            .prepare(&format!(
                "SELECT {COLUMNS} FROM ads
                 WHERE place = ?1 AND paused = 0 AND starts <= ?2 AND ends > ?2
                 ORDER BY weight DESC, id ASC"
            ))
            .map_err(err)?;
        let rows = statement
            .query_map(params![place, now], ad_row)
            .map_err(err)?;
        let mut live = Vec::new();
        for ad in rows {
            let ad = ad.map_err(err)?;
            if ad.langs.is_empty() || ad.langs.iter().any(|l| l == lang) {
                live.push(ad);
                if live.len() == MAX_LIVE {
                    break;
                }
            }
        }
        Ok(live)
    }

    /// Ads worth a ping: every one not ended, also those still to start, so
    /// their players online before the ad are known (D-071).
    pub fn to_ping(&self, now: i64) -> Result<Vec<Ad>, String> {
        let db = self.db();
        let mut statement = db
            .prepare(&format!(
                "SELECT {COLUMNS} FROM ads WHERE ends > ?1 ORDER BY id"
            ))
            .map_err(err)?;
        let rows = statement.query_map([now], ad_row).map_err(err)?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(err)
    }

    /// The ad's players online by hour since `since` (unix seconds), the
    /// oldest first: (hour, up, online, max).
    pub fn online(&self, ad: i64, since: i64) -> Result<Vec<Hour>, String> {
        let db = self.db();
        let mut statement = db
            .prepare(
                "SELECT hour, up, online, max FROM online WHERE ad = ?1 AND hour >= ?2
                 ORDER BY hour",
            )
            .map_err(err)?;
        let rows = statement
            .query_map(params![ad, since], |row| {
                Ok(Hour {
                    at: row.get(0)?,
                    up: row.get::<_, i64>(1)? != 0,
                    online: row.get(2)?,
                    max: row.get(3)?,
                })
            })
            .map_err(err)?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(err)
    }

    pub fn ping(&self, ad: i64) -> Result<Option<Ping>, String> {
        self.db()
            .query_row(
                "SELECT at, up, online, max, protocol, server_version, icon, licensed, checked_at
                 FROM pings WHERE ad = ?1",
                [ad],
                |row| {
                    Ok(Ping {
                        at: row.get(0)?,
                        up: row.get::<_, i64>(1)? != 0,
                        online: row.get(2)?,
                        max: row.get(3)?,
                        protocol: row.get(4)?,
                        server_version: row.get(5)?,
                        icon: row.get(6)?,
                        licensed: row.get::<_, Option<i64>>(7)?.map(|l| l != 0),
                        checked_at: row.get(8)?,
                    })
                },
            )
            .optional()
            .map_err(err)
    }

    /// Keeps a ping, and its players online as the hour's. A failed one
    /// keeps the icon of the last good one, and one without a sign-in check
    /// keeps the last check.
    pub fn save_ping(&self, ad: i64, ping: &Ping) -> Result<(), String> {
        let last = self.ping(ad)?;
        let icon = ping
            .icon
            .clone()
            .or_else(|| last.as_ref().and_then(|p| p.icon.clone()));
        let (licensed, checked_at) = match ping.checked_at {
            Some(at) => (ping.licensed, Some(at)),
            None => last
                .as_ref()
                .map(|p| (p.licensed, p.checked_at))
                .unwrap_or_default(),
        };
        self.db()
            .execute(
                "INSERT INTO pings (ad, at, up, online, max, protocol, server_version, icon,
                    licensed, checked_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                 ON CONFLICT(ad) DO UPDATE SET at = ?2, up = ?3, online = ?4, max = ?5,
                    protocol = ?6, server_version = ?7, icon = ?8, licensed = ?9,
                    checked_at = ?10",
                params![
                    ad,
                    ping.at,
                    i64::from(ping.up),
                    ping.online,
                    ping.max,
                    ping.protocol,
                    ping.server_version,
                    icon,
                    licensed.map(i64::from),
                    checked_at,
                ],
            )
            .map_err(err)?;
        let (online, max) = if ping.up {
            (ping.online, ping.max)
        } else {
            (None, None)
        };
        self.db()
            .execute(
                "INSERT INTO online (ad, hour, up, online, max) VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(ad, hour) DO UPDATE SET up = ?3, online = ?4, max = ?5",
                params![
                    ad,
                    ping.at - ping.at.rem_euclid(3600),
                    i64::from(ping.up),
                    online,
                    max
                ],
            )
            .map(|_| ())
            .map_err(err)
    }

    /// Pictures any ad still uses: its icon, its server's, its banner.
    pub fn used_icons(&self) -> Result<Vec<String>, String> {
        let db = self.db();
        let mut statement = db
            .prepare(
                "SELECT icon FROM ads WHERE icon IS NOT NULL
                 UNION SELECT icon FROM pings WHERE icon IS NOT NULL
                 UNION SELECT banner FROM ads WHERE banner IS NOT NULL",
            )
            .map_err(err)?;
        let rows = statement
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(err)?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(err)
    }

    /// Replaces the list of releases, the newest first.
    pub fn set_releases(&self, ids: &[String], now: i64) -> Result<(), String> {
        let mut db = self.db();
        let tx = db.transaction().map_err(err)?;
        tx.execute("DELETE FROM releases", []).map_err(err)?;
        for (rank, id) in ids.iter().enumerate() {
            tx.execute(
                "INSERT OR IGNORE INTO releases (id, rank) VALUES (?1, ?2)",
                params![id, rank as i64],
            )
            .map_err(err)?;
        }
        tx.execute(
            "INSERT INTO meta (key, value) VALUES ('releases_at', ?1)
             ON CONFLICT(key) DO UPDATE SET value = ?1",
            [now.to_string()],
        )
        .map_err(err)?;
        tx.commit().map_err(err)
    }

    /// When the releases were last fetched.
    pub fn releases_at(&self) -> Result<Option<i64>, String> {
        let value: Option<String> = self
            .db()
            .query_row(
                "SELECT value FROM meta WHERE key = 'releases_at'",
                [],
                |r| r.get(0),
            )
            .optional()
            .map_err(err)?;
        Ok(value.and_then(|v| v.parse().ok()))
    }

    pub fn rank(&self, id: &str) -> Result<Option<i64>, String> {
        self.db()
            .query_row("SELECT rank FROM releases WHERE id = ?1", [id], |r| {
                r.get(0)
            })
            .optional()
            .map_err(err)
    }

    /// Releases from `newest` down to `oldest`, both included.
    pub fn range(&self, oldest: &str, newest: &str) -> Result<Vec<String>, String> {
        let (Some(low), Some(high)) = (self.rank(oldest)?, self.rank(newest)?) else {
            return Ok(Vec::new());
        };
        let db = self.db();
        let mut statement = db
            .prepare("SELECT id FROM releases WHERE rank >= ?1 AND rank <= ?2 ORDER BY rank")
            .map_err(err)?;
        let rows = statement
            .query_map(params![high, low], |row| row.get::<_, String>(0))
            .map_err(err)?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(err)
    }

    pub fn counts(&self, now: i64) -> Result<serde_json::Value, String> {
        let ads = self.all()?;
        let mut counts = BTreeMap::<&str, i64>::new();
        for ad in &ads {
            *counts.entry(state(ad, now)).or_default() += 1;
        }
        let db = self.db();
        let up: i64 = db
            .query_row(
                "SELECT COUNT(*) FROM pings JOIN ads ON ads.id = pings.ad
                 WHERE pings.up = 1 AND ads.ends > ?1",
                [now],
                |r| r.get(0),
            )
            .map_err(err)?;
        let releases: i64 = db
            .query_row("SELECT COUNT(*) FROM releases", [], |r| r.get(0))
            .map_err(err)?;
        Ok(serde_json::json!({
            "ads": ads.len(),
            "live": counts.get("live").copied().unwrap_or(0),
            "scheduled": counts.get("scheduled").copied().unwrap_or(0),
            "paused": counts.get("paused").copied().unwrap_or(0),
            "ended": counts.get("ended").copied().unwrap_or(0),
            "answering": up,
            "releases": releases,
        }))
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
            "servers-{}.db",
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
                    .is_some_and(|n| n.starts_with("servers-") && n.ends_with(".db"))
            })
            .collect();
        copies.sort();
        let extra = copies.len().saturating_sub(keep);
        for old in &copies[..extra] {
            std::fs::remove_file(old).map_err(|e| e.to_string())?;
        }
        Ok(path)
    }
}
