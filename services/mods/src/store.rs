//! The catalog in SQLite (`data/db/catalog.db`): projects, versions, files,
//! dependencies, pictures, players' requests and the work queue. Every query
//! takes its values as parameters.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;

use crate::blobs::Hashed;
use crate::model::{SourceProject, SourceVersion};

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS project (
    id TEXT PRIMARY KEY,
    kind TEXT NOT NULL,
    slug TEXT NOT NULL UNIQUE,
    title TEXT NOT NULL,
    summary TEXT NOT NULL DEFAULT '',
    body TEXT NOT NULL DEFAULT '',
    license TEXT NOT NULL DEFAULT '',
    icon_sha256 TEXT,
    icon_type TEXT,
    icon_thumb TEXT,
    client_side TEXT NOT NULL DEFAULT 'unknown',
    server_side TEXT NOT NULL DEFAULT 'unknown',
    -- importing, ready, hidden, removed, room (a world's own, never shown)
    status TEXT NOT NULL,
    downloads INTEGER NOT NULL DEFAULT 0,
    followers INTEGER NOT NULL DEFAULT 0,
    downloads_ours INTEGER NOT NULL DEFAULT 0,
    published INTEGER NOT NULL DEFAULT 0,
    updated INTEGER NOT NULL DEFAULT 0,
    imported INTEGER NOT NULL,
    gone_at INTEGER
);
CREATE TABLE IF NOT EXISTS project_source (
    project_id TEXT NOT NULL REFERENCES project(id),
    source TEXT NOT NULL,
    source_id TEXT NOT NULL,
    source_slug TEXT NOT NULL,
    url TEXT NOT NULL,
    raw_sha256 TEXT,
    fetched INTEGER NOT NULL,
    distribution TEXT NOT NULL DEFAULT 'allowed',
    PRIMARY KEY (source, source_id)
);
CREATE INDEX IF NOT EXISTS project_source_project ON project_source(project_id);
-- When players last looked at a project or had it installed (to the hour),
-- so those are checked for new versions more often.
CREATE TABLE IF NOT EXISTS project_wanted (
    project_id TEXT PRIMARY KEY REFERENCES project(id),
    at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS project_author (
    project_id TEXT NOT NULL REFERENCES project(id),
    ord INTEGER NOT NULL,
    name TEXT NOT NULL,
    role TEXT NOT NULL DEFAULT '',
    PRIMARY KEY (project_id, ord)
);
CREATE TABLE IF NOT EXISTS project_category (
    project_id TEXT NOT NULL REFERENCES project(id),
    category TEXT NOT NULL,
    PRIMARY KEY (project_id, category)
);
CREATE INDEX IF NOT EXISTS project_category_category ON project_category(category);
CREATE TABLE IF NOT EXISTS version (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES project(id),
    number TEXT NOT NULL,
    name TEXT NOT NULL,
    changelog TEXT NOT NULL DEFAULT '',
    channel TEXT NOT NULL,
    published INTEGER NOT NULL,
    downloads INTEGER NOT NULL DEFAULT 0,
    source TEXT NOT NULL,
    source_id TEXT NOT NULL,
    -- none, partial, all: how many of its files are kept here
    files_state TEXT NOT NULL DEFAULT 'none',
    gone_at INTEGER,
    UNIQUE (source, source_id)
);
CREATE INDEX IF NOT EXISTS version_project ON version(project_id, published DESC);
CREATE TABLE IF NOT EXISTS version_game (
    version_id TEXT NOT NULL REFERENCES version(id),
    game TEXT NOT NULL,
    PRIMARY KEY (version_id, game)
);
CREATE INDEX IF NOT EXISTS version_game_game ON version_game(game);
CREATE TABLE IF NOT EXISTS version_loader (
    version_id TEXT NOT NULL REFERENCES version(id),
    loader TEXT NOT NULL,
    PRIMARY KEY (version_id, loader)
);
CREATE INDEX IF NOT EXISTS version_loader_loader ON version_loader(loader);
CREATE TABLE IF NOT EXISTS blob (
    sha256 TEXT PRIMARY KEY,
    size INTEGER NOT NULL,
    sha1 TEXT NOT NULL,
    sha512 TEXT NOT NULL,
    media_type TEXT NOT NULL,
    -- stored, quarantined; forgotten once the owner deleted a held file
    state TEXT NOT NULL,
    stored INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS blob_sha1 ON blob(sha1);
CREATE INDEX IF NOT EXISTS blob_sha512 ON blob(sha512);
CREATE TABLE IF NOT EXISTS version_file (
    version_id TEXT NOT NULL REFERENCES version(id),
    ord INTEGER NOT NULL,
    file_name TEXT NOT NULL,
    size INTEGER NOT NULL,
    sha1 TEXT,
    sha512 TEXT,
    -- set once the file is kept here
    sha256 TEXT,
    is_primary INTEGER NOT NULL,
    role TEXT NOT NULL,
    origin_url TEXT NOT NULL,
    PRIMARY KEY (version_id, ord)
);
CREATE INDEX IF NOT EXISTS version_file_sha1 ON version_file(sha1);
CREATE INDEX IF NOT EXISTS version_file_sha256 ON version_file(sha256);
CREATE TABLE IF NOT EXISTS dependency (
    version_id TEXT NOT NULL REFERENCES version(id),
    ord INTEGER NOT NULL,
    kind TEXT NOT NULL,
    source TEXT NOT NULL,
    source_project TEXT,
    source_version TEXT,
    PRIMARY KEY (version_id, ord)
);
CREATE TABLE IF NOT EXISTS gallery (
    project_id TEXT NOT NULL REFERENCES project(id),
    ord INTEGER NOT NULL,
    origin_url TEXT NOT NULL,
    title TEXT NOT NULL DEFAULT '',
    description TEXT NOT NULL DEFAULT '',
    sha256 TEXT,
    media_type TEXT,
    thumb_sha256 TEXT,
    PRIMARY KEY (project_id, ord)
);
CREATE TABLE IF NOT EXISTS import_request (
    id TEXT PRIMARY KEY,
    account_id TEXT NOT NULL,
    input TEXT NOT NULL,
    -- queued, choosing, importing, ready, failed
    state TEXT NOT NULL,
    error TEXT,
    project_id TEXT,
    candidates TEXT,
    bytes INTEGER NOT NULL DEFAULT 0,
    created INTEGER NOT NULL,
    finished INTEGER
);
CREATE INDEX IF NOT EXISTS import_request_account ON import_request(account_id, created);
CREATE TABLE IF NOT EXISTS job (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    key TEXT NOT NULL UNIQUE,
    kind TEXT NOT NULL,
    payload TEXT NOT NULL,
    request_id TEXT,
    project_id TEXT,
    -- queued, running, done, failed
    state TEXT NOT NULL,
    attempts INTEGER NOT NULL DEFAULT 0,
    run_after INTEGER NOT NULL,
    error TEXT,
    created INTEGER NOT NULL,
    finished INTEGER
);
CREATE INDEX IF NOT EXISTS job_ready ON job(state, run_after);
CREATE INDEX IF NOT EXISTS job_request ON job(request_id, state);
-- Pictures by the address they came from, so a picture is fetched once.
CREATE TABLE IF NOT EXISTS image_origin (
    origin_url TEXT PRIMARY KEY,
    sha256 TEXT NOT NULL,
    media_type TEXT NOT NULL
);
-- Projects joined into another: the same project on two sources, known by a
-- file they share. Their IDs and slugs lead to the one they joined.
CREATE TABLE IF NOT EXISTS project_joined (
    project_id TEXT PRIMARY KEY,
    into_id TEXT NOT NULL
);
-- A modpack version's index, read from its kept archive (stage 3).
CREATE TABLE IF NOT EXISTS pack (
    version_id TEXT PRIMARY KEY REFERENCES version(id),
    archive_sha256 TEXT NOT NULL,
    game TEXT NOT NULL,
    loader TEXT,
    loader_version TEXT,
    skipped INTEGER NOT NULL DEFAULT 0,
    read INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS pack_file (
    version_id TEXT NOT NULL REFERENCES version(id),
    ord INTEGER NOT NULL,
    path TEXT NOT NULL,
    size INTEGER NOT NULL,
    sha1 TEXT NOT NULL,
    sha512 TEXT NOT NULL,
    -- set once the file is kept here
    sha256 TEXT,
    optional INTEGER NOT NULL,
    origin_url TEXT NOT NULL,
    PRIMARY KEY (version_id, ord)
);
-- What waits for the owner's decision (lapi-mods review): a file the
-- scanner held back (kind scan, subject its SHA-256) or a version its source
-- took down (kind gone, subject the version).
CREATE TABLE IF NOT EXISTS review (
    id INTEGER PRIMARY KEY,
    kind TEXT NOT NULL,
    subject TEXT NOT NULL,
    created INTEGER NOT NULL,
    decided INTEGER,
    decision TEXT,
    UNIQUE (kind, subject)
);
-- Files the owner blocked (lapi-mods block), by SHA-256 or by SHA-1 from a
-- public report, also before they are here: a blocked file fetched again
-- goes straight into quarantine, and launchers warn about it. `key` orders
-- the list launchers read: the SHA-256, or one made from the SHA-1.
CREATE TABLE IF NOT EXISTS blocked (
    key TEXT PRIMARY KEY,
    sha256 TEXT,
    sha1 TEXT,
    reason TEXT NOT NULL DEFAULT '',
    at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS blocked_sha256 ON blocked(sha256);
CREATE INDEX IF NOT EXISTS blocked_sha1 ON blocked(sha1);
-- Times and numbers of the service's own upkeep (copies in R2).
CREATE TABLE IF NOT EXISTS upkeep (
    key TEXT PRIMARY KEY,
    value INTEGER NOT NULL
);
-- What the owner decided and when (block, unblock, keep, remove, forget).
CREATE TABLE IF NOT EXISTS decision (
    id INTEGER PRIMARY KEY,
    at INTEGER NOT NULL,
    action TEXT NOT NULL,
    subject TEXT NOT NULL,
    reason TEXT NOT NULL DEFAULT ''
);
CREATE INDEX IF NOT EXISTS decision_subject ON decision(subject);
-- Players' own packs (own.rs): who published each, and each version with
-- the bytes it brought, for the limits per account.
CREATE TABLE IF NOT EXISTS own_pack (
    project_id TEXT PRIMARY KEY,
    account_id TEXT NOT NULL,
    created INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS own_pack_account ON own_pack(account_id, created);
CREATE TABLE IF NOT EXISTS own_version (
    version_id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL,
    account_id TEXT NOT NULL,
    bytes INTEGER NOT NULL,
    created INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS own_version_account ON own_version(account_id, created);
-- Players' reports about an own pack, one per account; they put the pack
-- before the owner (review kind `report`).
CREATE TABLE IF NOT EXISTS report (
    project_id TEXT NOT NULL,
    account_id TEXT NOT NULL,
    reason TEXT NOT NULL,
    text TEXT NOT NULL DEFAULT '',
    created INTEGER NOT NULL,
    PRIMARY KEY (project_id, account_id)
);
-- Projects from sites that are not catalogs, as the owner described them
-- (external.rs), and the versions the owner added to them by hand.
CREATE TABLE IF NOT EXISTS external_project (
    source_id TEXT PRIMARY KEY,
    kind TEXT NOT NULL,
    slug TEXT NOT NULL,
    title TEXT NOT NULL,
    summary TEXT NOT NULL DEFAULT '',
    page TEXT NOT NULL,
    icon TEXT,
    days INTEGER NOT NULL,
    added INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS external_version (
    source_id TEXT NOT NULL,
    file_url TEXT NOT NULL,
    file_name TEXT NOT NULL,
    number TEXT NOT NULL,
    channel TEXT NOT NULL,
    games TEXT NOT NULL,
    loaders TEXT NOT NULL,
    added INTEGER NOT NULL,
    PRIMARY KEY (source_id, file_url)
);
CREATE VIRTUAL TABLE IF NOT EXISTS project_fts USING fts5(
    project_id UNINDEXED, title, summary, authors, categories, slug,
    tokenize = 'unicode61 remove_diacritics 2'
);
"#;

pub struct Store {
    db: Mutex<Connection>,
    path: PathBuf,
}

pub type Result<T> = std::result::Result<T, String>;

/// Reasons with their counts, and the newest texts.
pub type Reports = (Vec<(String, i64)>, Vec<String>);

/// A kept file as the owner sees it (`lapi-mods find`).
#[derive(Debug, Clone)]
pub struct BlobInfo {
    pub sha256: String,
    pub sha1: String,
    pub size: i64,
    pub media_type: String,
    pub state: String,
    pub verdict: String,
    pub signs: String,
    pub allowed: bool,
    pub held: Option<i64>,
}

/// Whether `source` owns a project's card: Modrinth's when the project has
/// it, else the source that brought the project first.
fn card_owner(tx: &rusqlite::Transaction<'_>, project: &str, source: &str) -> Result<bool> {
    let owner: Option<String> = tx
        .query_row(
            "SELECT source FROM project_source WHERE project_id = ?1
             ORDER BY source = 'modrinth' DESC, rowid LIMIT 1",
            [project],
            |r| r.get(0),
        )
        .optional()
        .map_err(sql)?;
    Ok(owner.is_none_or(|o| o == source))
}

/// Columns added after the first release, to databases made before them.
const ADDED: &[(&str, &str, &str)] = &[
    // The scanner's rules a blob was last scanned under (0: never).
    ("blob", "scanned", "INTEGER NOT NULL DEFAULT 0"),
    // clean, note, suspicious, malicious; blocked or allowed by the owner.
    ("blob", "verdict", "TEXT NOT NULL DEFAULT 'clean'"),
    // The scanner's signs, comma-separated fixed words.
    ("blob", "signs", "TEXT NOT NULL DEFAULT ''"),
    // Kept by the owner's decision: no later scan holds it back.
    ("blob", "allowed", "INTEGER NOT NULL DEFAULT 0"),
    // When it was held back.
    ("blob", "held", "INTEGER"),
    // Its copy in R2: '' none, 'public' served from there, 'private' a held
    // file's copy (r2.rs).
    ("blob", "copy", "TEXT NOT NULL DEFAULT ''"),
    // Days between checks of the project's sources, as the owner set them:
    // NULL the usual rule, 0 never (external.rs).
    ("project", "recheck_days", "INTEGER"),
];

fn migrate(db: &Connection) -> Result<()> {
    for (table, column, kind) in ADDED {
        let there: bool = db
            .query_row(
                &format!("SELECT count(*) FROM pragma_table_info('{table}') WHERE name = ?1"),
                [column],
                |r| r.get::<_, i64>(0),
            )
            .map_err(sql)?
            > 0;
        if !there {
            db.execute_batch(&format!("ALTER TABLE {table} ADD COLUMN {column} {kind}"))
                .map_err(sql)?;
        }
    }
    db.execute_batch(
        "CREATE INDEX IF NOT EXISTS blob_scanned ON blob(scanned);
         CREATE INDEX IF NOT EXISTS blob_held ON blob(held);
         CREATE INDEX IF NOT EXISTS blob_copy ON blob(copy, state);",
    )
    .map_err(sql)
}

fn sql(e: rusqlite::Error) -> String {
    format!("database: {e}")
}

/// A gallery picture kept already: its address, SHA-256, type and small copy.
type KeptPicture = (String, Option<String>, Option<String>, Option<String>);

/// A job from the queue.
#[derive(Debug, Clone)]
pub struct Job {
    pub id: i64,
    pub kind: String,
    pub payload: serde_json::Value,
    pub request_id: Option<String>,
    pub attempts: i64,
}

/// A file of a version that is kept here, as `upsert_versions` finds it.
struct KeptFile {
    sha1: Option<String>,
    sha512: Option<String>,
    sha256: String,
    name: String,
    size: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileRow {
    pub ord: i64,
    pub name: String,
    pub size: i64,
    pub sha1: Option<String>,
    pub sha512: Option<String>,
    pub sha256: Option<String>,
    pub primary: bool,
    pub role: String,
    pub origin_url: String,
}

/// A file a modpack lists.
#[derive(Debug, Clone)]
pub struct PackFileRow {
    pub ord: i64,
    /// Empty for a CurseForge pack's file not found yet.
    pub path: String,
    pub size: i64,
    pub sha1: String,
    pub sha512: String,
    pub sha256: Option<String>,
    pub origin_url: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Request {
    pub id: String,
    pub input: String,
    pub state: String,
    pub error: Option<String>,
    pub project_id: Option<String>,
    pub candidates: Option<serde_json::Value>,
    pub created: i64,
    pub finished: Option<i64>,
}

/// What a search asks for, already checked.
#[derive(Debug, Clone, Default)]
pub struct Search {
    pub words: Vec<String>,
    pub kind: Option<String>,
    pub game: Option<String>,
    pub loader: Option<String>,
    pub category: Option<String>,
    pub sort: &'static str,
    pub offset: i64,
    pub limit: i64,
}

/// A project the owner added from a site that is not a catalog.
#[derive(Debug, Clone)]
pub struct ExternalRow {
    pub source_id: String,
    pub kind: String,
    pub days: i64,
    pub project: Option<String>,
    pub versions: i64,
    pub kept: i64,
    pub fetched: Option<i64>,
}

/// A project due to be read from its source again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Due {
    pub source: String,
    pub source_id: String,
    pub source_slug: String,
    pub kind: String,
    /// Players wanted it lately.
    pub wanted: bool,
    /// Days between checks, as the owner set them.
    pub days: Option<i64>,
}

impl Store {
    pub fn open(data: &Path) -> Result<Self> {
        // nginx reads the kept files next to it; the database is ours alone.
        let dir = data.join("db");
        std::fs::create_dir_all(&dir).map_err(|e| format!("creating {}: {e}", dir.display()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))
                .map_err(|e| e.to_string())?;
        }
        let path = dir.join("catalog.db");
        let db = Connection::open(&path).map_err(sql)?;
        db.busy_timeout(std::time::Duration::from_secs(10))
            .map_err(sql)?;
        db.execute_batch(
            "PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON; PRAGMA synchronous = NORMAL;",
        )
        .map_err(sql)?;
        db.execute_batch(SCHEMA).map_err(sql)?;
        migrate(&db)?;
        Ok(Self {
            db: Mutex::new(db),
            path,
        })
    }

    fn db(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.db
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    // ----- projects and versions -----

    pub fn project_by_source(&self, source: &str, source_id: &str) -> Result<Option<String>> {
        self.db()
            .query_row(
                "SELECT project_id FROM project_source WHERE source = ?1 AND source_id = ?2",
                params![source, source_id],
                |r| r.get(0),
            )
            .optional()
            .map_err(sql)
    }

    /// A project's ID at a source.
    pub fn source_id(&self, project: &str, source: &str) -> Result<Option<String>> {
        self.db()
            .query_row(
                "SELECT source_id FROM project_source WHERE project_id = ?1 AND source = ?2",
                params![project, source],
                |r| r.get(0),
            )
            .optional()
            .map_err(sql)
    }

    /// A project by its source's ID or slug.
    pub fn project_by_source_ref(&self, source: &str, reference: &str) -> Result<Option<String>> {
        self.db()
            .query_row(
                "SELECT project_id FROM project_source WHERE source = ?1 AND (source_id = ?2 OR source_slug = lower(?2))
                 LIMIT 1",
                params![source, reference],
                |r| r.get(0),
            )
            .optional()
            .map_err(sql)
    }

    pub fn project_status(&self, id: &str) -> Result<Option<String>> {
        self.db()
            .query_row("SELECT status FROM project WHERE id = ?1", [id], |r| {
                r.get(0)
            })
            .optional()
            .map_err(sql)
    }

    /// A project's versions from a source: our ID and theirs.
    pub fn version_sources(&self, project: &str, source: &str) -> Result<Vec<(String, String)>> {
        let db = self.db();
        let mut q = db
            .prepare("SELECT id, source_id FROM version WHERE project_id = ?1 AND source = ?2")
            .map_err(sql)?;
        q.query_map(params![project, source], |r| Ok((r.get(0)?, r.get(1)?)))
            .map_err(sql)?
            .collect::<std::result::Result<_, _>>()
            .map_err(sql)
    }

    /// A picture kept already from this address: its SHA-256 and type.
    pub fn image_by_origin(&self, url: &str) -> Result<Option<(String, String)>> {
        self.db()
            .query_row(
                "SELECT sha256, media_type FROM image_origin WHERE origin_url = ?1",
                [url],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(sql)
    }

    pub fn set_image_origin(&self, url: &str, sha256: &str, media_type: &str) -> Result<()> {
        self.db()
            .execute(
                "INSERT OR REPLACE INTO image_origin (origin_url, sha256, media_type) VALUES (?1, ?2, ?3)",
                params![url, sha256, media_type],
            )
            .map(|_| ())
            .map_err(sql)
    }

    /// When the project's metadata was last fetched from this source.
    pub fn fetched(&self, source: &str, source_id: &str) -> Result<Option<i64>> {
        self.db()
            .query_row(
                "SELECT fetched FROM project_source WHERE source = ?1 AND source_id = ?2",
                params![source, source_id],
                |r| r.get(0),
            )
            .optional()
            .map_err(sql)
    }

    /// Notes that players want a project now; written at most once an hour.
    pub fn note_wanted(&self, project: &str, now: i64) -> Result<()> {
        self.db()
            .execute(
                "INSERT INTO project_wanted (project_id, at) SELECT id, ?2 FROM project WHERE id = ?1
                 ON CONFLICT (project_id) DO UPDATE SET at = excluded.at WHERE at < excluded.at - 3600",
                params![project, now],
            )
            .map(|_| ())
            .map_err(sql)
    }

    /// Projects whose source was last read before their time: `wanted_fresh`
    /// for those wanted since `wanted_since`, `idle_fresh` for the rest;
    /// longest waiting first, or only `project` when given. Days the owner
    /// set for a project are its time on an external source, and at most
    /// its time on another; 0 days is never.
    pub fn due_projects(
        &self,
        now: i64,
        wanted_since: i64,
        wanted_fresh: i64,
        idle_fresh: i64,
        project: Option<&str>,
        limit: usize,
    ) -> Result<Vec<Due>> {
        let db = self.db();
        let mut q = db
            .prepare(
                "SELECT s.source, s.source_id, s.source_slug, p.kind, coalesce(w.at, 0) >= ?2,
                   p.recheck_days
                 FROM project p JOIN project_source s ON s.project_id = p.id
                 LEFT JOIN project_wanted w ON w.project_id = p.id
                 WHERE p.status = 'ready' AND p.gone_at IS NULL AND (?5 IS NULL OR p.id = ?5)
                   AND s.source != 'own' AND coalesce(p.recheck_days, -1) != 0
                   AND s.fetched <= ?1 - CASE
                     WHEN s.source = 'external' THEN coalesce(p.recheck_days * 86400, ?4)
                     WHEN p.recheck_days > 0 THEN min(p.recheck_days * 86400,
                       CASE WHEN coalesce(w.at, 0) >= ?2 THEN ?3 ELSE ?4 END)
                     ELSE CASE WHEN coalesce(w.at, 0) >= ?2 THEN ?3 ELSE ?4 END
                   END
                 ORDER BY s.fetched LIMIT ?6",
            )
            .map_err(sql)?;
        q.query_map(
            params![
                now,
                wanted_since,
                wanted_fresh,
                idle_fresh,
                project,
                limit as i64
            ],
            |r| {
                Ok(Due {
                    source: r.get(0)?,
                    source_id: r.get(1)?,
                    source_slug: r.get(2)?,
                    kind: r.get(3)?,
                    wanted: r.get(4)?,
                    days: r.get(5)?,
                })
            },
        )
        .map_err(sql)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(sql)
    }

    /// Stores a project as its source describes it; returns our ID.
    /// Records a source's project; its ID, and whether this source owns the
    /// card (title, description, pictures). A project on two sources shows
    /// Modrinth's card; the other source only adds its versions.
    pub fn upsert_project(&self, p: &SourceProject, now: i64) -> Result<(String, bool)> {
        let mut db = self.db();
        let tx = db.transaction().map_err(sql)?;
        let existing: Option<String> = tx
            .query_row(
                "SELECT project_id FROM project_source WHERE source = ?1 AND source_id = ?2",
                params![p.source, p.source_id],
                |r| r.get(0),
            )
            .optional()
            .map_err(sql)?;
        let id = match existing {
            Some(id) if !card_owner(&tx, &id, p.source)? => {
                tx.execute(
                    "UPDATE project_source SET source_slug = ?3, url = ?4, fetched = ?5
                     WHERE source = ?1 AND source_id = ?2",
                    params![p.source, p.source_id, p.slug, p.url, now],
                )
                .map_err(sql)?;
                tx.commit().map_err(sql)?;
                return Ok((id, false));
            }
            Some(id) => {
                tx.execute(
                    "UPDATE project SET kind = ?2, title = ?3, summary = ?4, body = ?5, license = ?6,
                       client_side = ?7, server_side = ?8, downloads = ?9, followers = ?10,
                       published = ?11, updated = ?12, gone_at = NULL
                     WHERE id = ?1",
                    params![
                        id,
                        p.kind,
                        p.title,
                        p.summary,
                        p.body,
                        p.license,
                        p.client_side,
                        p.server_side,
                        p.downloads,
                        p.followers,
                        p.published,
                        p.updated
                    ],
                )
                .map_err(sql)?;
                tx.execute(
                    "UPDATE project_source SET source_slug = ?3, url = ?4, fetched = ?5
                     WHERE source = ?1 AND source_id = ?2",
                    params![p.source, p.source_id, p.slug, p.url, now],
                )
                .map_err(sql)?;
                id
            }
            None => {
                let id = uuid::Uuid::now_v7().to_string();
                // The source's slug when it is free; another source's project
                // may have it already.
                let taken: bool = tx
                    .query_row("SELECT 1 FROM project WHERE slug = ?1", [&p.slug], |_| {
                        Ok(())
                    })
                    .optional()
                    .map_err(sql)?
                    .is_some();
                let slug = if taken {
                    format!("{}-{}-{}", p.slug, p.source, &id[id.len() - 6..])
                } else {
                    p.slug.clone()
                };
                tx.execute(
                    "INSERT INTO project (id, kind, slug, title, summary, body, license, client_side,
                       server_side, status, downloads, followers, published, updated, imported)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 'importing', ?10, ?11, ?12, ?13, ?14)",
                    params![
                        id,
                        p.kind,
                        slug,
                        p.title,
                        p.summary,
                        p.body,
                        p.license,
                        p.client_side,
                        p.server_side,
                        p.downloads,
                        p.followers,
                        p.published,
                        p.updated,
                        now
                    ],
                )
                .map_err(sql)?;
                tx.execute(
                    "INSERT INTO project_source (project_id, source, source_id, source_slug, url, fetched)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    params![id, p.source, p.source_id, p.slug, p.url, now],
                )
                .map_err(sql)?;
                id
            }
        };
        tx.execute("DELETE FROM project_author WHERE project_id = ?1", [&id])
            .map_err(sql)?;
        for (ord, (name, role)) in p.authors.iter().enumerate() {
            tx.execute(
                "INSERT INTO project_author (project_id, ord, name, role) VALUES (?1, ?2, ?3, ?4)",
                params![id, ord as i64, name, role],
            )
            .map_err(sql)?;
        }
        tx.execute("DELETE FROM project_category WHERE project_id = ?1", [&id])
            .map_err(sql)?;
        for category in &p.categories {
            tx.execute(
                "INSERT OR IGNORE INTO project_category (project_id, category) VALUES (?1, ?2)",
                params![id, category],
            )
            .map_err(sql)?;
        }
        // Pictures already kept stay when their address is the same.
        let old: Vec<KeptPicture> = {
            let mut q = tx
                .prepare("SELECT origin_url, sha256, media_type, thumb_sha256 FROM gallery WHERE project_id = ?1")
                .map_err(sql)?;
            q.query_map([&id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
                .map_err(sql)?
                .collect::<std::result::Result<_, _>>()
                .map_err(sql)?
        };
        tx.execute("DELETE FROM gallery WHERE project_id = ?1", [&id])
            .map_err(sql)?;
        for (ord, image) in p.gallery.iter().enumerate() {
            let kept = old.iter().find(|(url, ..)| *url == image.url);
            tx.execute(
                "INSERT INTO gallery (project_id, ord, origin_url, title, description, sha256, media_type, thumb_sha256)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    id,
                    ord as i64,
                    image.url,
                    image.title,
                    image.description,
                    kept.and_then(|k| k.1.clone()),
                    kept.and_then(|k| k.2.clone()),
                    kept.and_then(|k| k.3.clone())
                ],
            )
            .map_err(sql)?;
        }
        let slug: String = tx
            .query_row("SELECT slug FROM project WHERE id = ?1", [&id], |r| {
                r.get(0)
            })
            .map_err(sql)?;
        tx.execute("DELETE FROM project_fts WHERE project_id = ?1", [&id])
            .map_err(sql)?;
        tx.execute(
            "INSERT INTO project_fts (project_id, title, summary, authors, categories, slug)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                id,
                p.title,
                p.summary,
                p.authors
                    .iter()
                    .map(|(n, _)| n.as_str())
                    .collect::<Vec<_>>()
                    .join(" "),
                p.categories.join(" "),
                slug.replace(['-', '_'], " ")
            ],
        )
        .map_err(sql)?;
        tx.commit().map_err(sql)?;
        Ok((id, true))
    }

    /// Joins the project of a version with another project of the same kind
    /// that has a file with these hashes, when the two come from different
    /// sources: the same project on Modrinth and CurseForge. The one on
    /// Modrinth (else the older) stays; the other's sources and versions move
    /// to it, and its ID and slug lead to it. Returns (stays, joined).
    pub fn join_twins(
        &self,
        version: &str,
        sha256: &str,
        sha1: &str,
        now: i64,
    ) -> Result<Option<(String, String)>> {
        let mut db = self.db();
        let tx = db.transaction().map_err(sql)?;
        let Some((me, kind)): Option<(String, String)> = tx
            .query_row(
                "SELECT p.id, p.kind FROM version v JOIN project p ON p.id = v.project_id
                 WHERE v.id = ?1 AND p.status IN ('importing', 'ready')",
                [version],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(sql)?
        else {
            return Ok(None);
        };
        let others: Vec<String> = {
            let mut q = tx
                .prepare(
                    "SELECT DISTINCT v.project_id FROM version_file g
                     JOIN version v ON v.id = g.version_id
                     JOIN project p ON p.id = v.project_id
                     WHERE (g.sha256 = ?1 OR g.sha1 = ?2) AND v.project_id != ?3 AND p.kind = ?4
                       AND p.status IN ('importing', 'ready')
                     LIMIT 4",
                )
                .map_err(sql)?;
            q.query_map(params![sha256, sha1, me, kind], |r| r.get(0))
                .map_err(sql)?
                .collect::<std::result::Result<_, _>>()
                .map_err(sql)?
        };
        let sources = |id: &str| -> Result<Vec<String>> {
            let mut q = tx
                .prepare("SELECT source FROM project_source WHERE project_id = ?1")
                .map_err(sql)?;
            q.query_map([id], |r| r.get(0))
                .map_err(sql)?
                .collect::<std::result::Result<_, _>>()
                .map_err(sql)
        };
        let mine = sources(&me)?;
        let mut pair = None;
        for other in others {
            let theirs = sources(&other)?;
            // Two projects of one source are two projects.
            if theirs.iter().any(|s| mine.contains(s)) {
                continue;
            }
            let older = |id: &str| -> Result<i64> {
                tx.query_row("SELECT imported FROM project WHERE id = ?1", [id], |r| {
                    r.get(0)
                })
                .map_err(sql)
            };
            let me_stays = if mine.iter().any(|s| s == "modrinth") {
                true
            } else if theirs.iter().any(|s| s == "modrinth") {
                false
            } else {
                older(&me)? <= older(&other)?
            };
            pair = Some(if me_stays {
                (me.clone(), other)
            } else {
                (other, me.clone())
            });
            break;
        }
        let Some((stays, gone)) = pair else {
            return Ok(None);
        };
        for statement in [
            "UPDATE project_source SET project_id = ?1 WHERE project_id = ?2",
            "UPDATE version SET project_id = ?1 WHERE project_id = ?2",
            "INSERT OR IGNORE INTO project_category (project_id, category)
               SELECT ?1, category FROM project_category WHERE project_id = ?2",
            "INSERT INTO project_wanted (project_id, at) SELECT ?1, at FROM project_wanted WHERE project_id = ?2
               ON CONFLICT (project_id) DO UPDATE SET at = max(at, excluded.at)",
            "UPDATE import_request SET project_id = ?1 WHERE project_id = ?2",
            "UPDATE job SET project_id = ?1 WHERE project_id = ?2",
            "UPDATE project SET downloads_ours = downloads_ours
               + (SELECT downloads_ours FROM project WHERE id = ?2) WHERE id = ?1",
            "UPDATE project_joined SET into_id = ?1 WHERE into_id = ?2",
            "INSERT OR REPLACE INTO project_joined (project_id, into_id) VALUES (?2, ?1)",
        ] {
            tx.execute(statement, params![stays, gone]).map_err(sql)?;
        }
        for table in [
            "project_category",
            "project_wanted",
            "project_author",
            "gallery",
            "project_fts",
        ] {
            tx.execute(
                &format!("DELETE FROM {table} WHERE project_id = ?1"),
                [&gone],
            )
            .map_err(sql)?;
        }
        tx.execute(
            "UPDATE project SET status = 'joined', gone_at = coalesce(gone_at, ?2) WHERE id = ?1",
            params![gone, now],
        )
        .map_err(sql)?;
        tx.commit().map_err(sql)?;
        Ok(Some((stays, gone)))
    }

    pub fn set_project_status(&self, id: &str, status: &str) -> Result<()> {
        self.db()
            .execute(
                // A project the owner removed stays removed.
                "UPDATE project SET status = ?2 WHERE id = ?1 AND status != 'removed'",
                params![id, status],
            )
            .map(|_| ())
            .map_err(sql)
    }

    pub fn set_body(&self, id: &str, body: &str) -> Result<()> {
        self.db()
            .execute(
                "UPDATE project SET body = ?2 WHERE id = ?1",
                params![id, body],
            )
            .map(|_| ())
            .map_err(sql)
    }

    pub fn body(&self, id: &str) -> Result<String> {
        self.db()
            .query_row("SELECT body FROM project WHERE id = ?1", [id], |r| r.get(0))
            .map_err(sql)
    }

    pub fn set_icon(
        &self,
        id: &str,
        sha256: &str,
        media_type: &str,
        thumb: Option<&str>,
    ) -> Result<()> {
        self.db()
            .execute(
                "UPDATE project SET icon_sha256 = ?2, icon_type = ?3, icon_thumb = ?4 WHERE id = ?1",
                params![id, sha256, media_type, thumb],
            )
            .map(|_| ())
            .map_err(sql)
    }

    pub fn set_gallery_image(
        &self,
        project: &str,
        url: &str,
        sha256: &str,
        media_type: &str,
        thumb: Option<&str>,
    ) -> Result<()> {
        self.db()
            .execute(
                "UPDATE gallery SET sha256 = ?3, media_type = ?4, thumb_sha256 = ?5
                 WHERE project_id = ?1 AND origin_url = ?2",
                params![project, url, sha256, media_type, thumb],
            )
            .map(|_| ())
            .map_err(sql)
    }

    /// Stores a project's versions (metadata only); returns our IDs in order.
    /// Files already kept stay linked when their SHA-1 is the same.
    pub fn upsert_versions(
        &self,
        project: &str,
        source: &str,
        versions: &[SourceVersion],
    ) -> Result<Vec<String>> {
        let mut db = self.db();
        let tx = db.transaction().map_err(sql)?;
        let mut ids = Vec::with_capacity(versions.len());
        for v in versions {
            let existing: Option<String> = tx
                .query_row(
                    "SELECT id FROM version WHERE source = ?1 AND source_id = ?2",
                    params![source, v.source_id],
                    |r| r.get(0),
                )
                .optional()
                .map_err(sql)?;
            let id = match existing {
                Some(id) => {
                    tx.execute(
                        "UPDATE version SET number = ?2, name = ?3,
                           changelog = CASE WHEN ?8 THEN ?4 ELSE changelog END, channel = ?5,
                           published = ?6, downloads = ?7, gone_at = NULL WHERE id = ?1",
                        params![
                            id,
                            v.number,
                            v.name,
                            v.changelog,
                            v.channel,
                            v.published,
                            v.downloads,
                            v.details
                        ],
                    )
                    .map_err(sql)?;
                    id
                }
                None => {
                    let id = uuid::Uuid::now_v7().to_string();
                    tx.execute(
                        "INSERT INTO version (id, project_id, number, name, changelog, channel, published,
                           downloads, source, source_id)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                        params![
                            id,
                            project,
                            v.number,
                            v.name,
                            v.changelog,
                            v.channel,
                            v.published,
                            v.downloads,
                            source,
                            v.source_id
                        ],
                    )
                    .map_err(sql)?;
                    id
                }
            };
            tx.execute("DELETE FROM version_game WHERE version_id = ?1", [&id])
                .map_err(sql)?;
            for game in &v.games {
                tx.execute(
                    "INSERT OR IGNORE INTO version_game (version_id, game) VALUES (?1, ?2)",
                    params![id, game],
                )
                .map_err(sql)?;
            }
            tx.execute("DELETE FROM version_loader WHERE version_id = ?1", [&id])
                .map_err(sql)?;
            for loader in &v.loaders {
                tx.execute(
                    "INSERT OR IGNORE INTO version_loader (version_id, loader) VALUES (?1, ?2)",
                    params![id, loader],
                )
                .map_err(sql)?;
            }
            let kept: Vec<KeptFile> = {
                let mut q = tx
                    .prepare(
                        "SELECT sha1, sha512, sha256, file_name, size FROM version_file
                         WHERE version_id = ?1 AND sha256 IS NOT NULL",
                    )
                    .map_err(sql)?;
                q.query_map([&id], |r| {
                    Ok(KeptFile {
                        sha1: r.get(0)?,
                        sha512: r.get(1)?,
                        sha256: r.get(2)?,
                        name: r.get(3)?,
                        size: r.get(4)?,
                    })
                })
                .map_err(sql)?
                .collect::<std::result::Result<_, _>>()
                .map_err(sql)?
            };
            tx.execute("DELETE FROM version_file WHERE version_id = ?1", [&id])
                .map_err(sql)?;
            for (ord, f) in v.files.iter().enumerate() {
                // The same file: by its SHA-1, or (a source with no hashes) by
                // its name and size.
                // its name and size; by its name alone when the source gives
                // no size either (external.rs).
                let same = kept.iter().find(|k| match &f.sha1 {
                    Some(sha1) => k.sha1.as_ref() == Some(sha1),
                    None => k.name == f.name && (f.size == 0 || k.size == f.size),
                });
                let size = match same {
                    Some(k) if f.size == 0 => k.size,
                    _ => f.size,
                };
                let sha1 = f.sha1.clone().or_else(|| same.and_then(|k| k.sha1.clone()));
                let sha512 = f
                    .sha512
                    .clone()
                    .or_else(|| same.and_then(|k| k.sha512.clone()));
                let sha256 = same.map(|k| k.sha256.clone());
                tx.execute(
                    "INSERT INTO version_file (version_id, ord, file_name, size, sha1, sha512, sha256,
                       is_primary, role, origin_url)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                    params![id, ord as i64, f.name, size, sha1, sha512, sha256, f.primary, f.role, f.url],
                )
                .map_err(sql)?;
            }
            if !v.details {
                Self::refresh_files_state(&tx, &id)?;
                ids.push(id);
                continue;
            }
            tx.execute("DELETE FROM dependency WHERE version_id = ?1", [&id])
                .map_err(sql)?;
            for (ord, d) in v.dependencies.iter().enumerate() {
                tx.execute(
                    "INSERT INTO dependency (version_id, ord, kind, source, source_project, source_version)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    params![id, ord as i64, d.kind, source, d.project, d.version],
                )
                .map_err(sql)?;
            }
            Self::refresh_files_state(&tx, &id)?;
            ids.push(id);
        }
        tx.commit().map_err(sql)?;
        Ok(ids)
    }

    fn refresh_files_state(db: &Connection, version: &str) -> Result<()> {
        let (all, kept): (i64, i64) = db
            .query_row(
                "SELECT count(*), count(sha256) FROM version_file WHERE version_id = ?1",
                [version],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(sql)?;
        let state = if all > 0 && kept == all {
            "all"
        } else if kept > 0 {
            "partial"
        } else {
            "none"
        };
        db.execute(
            "UPDATE version SET files_state = ?2 WHERE id = ?1",
            params![version, state],
        )
        .map_err(sql)?;
        Ok(())
    }

    pub fn version_by_source(&self, source: &str, source_id: &str) -> Result<Option<String>> {
        self.db()
            .query_row(
                "SELECT id FROM version WHERE source = ?1 AND source_id = ?2",
                params![source, source_id],
                |r| r.get(0),
            )
            .optional()
            .map_err(sql)
    }

    /// A version's project and files_state.
    pub fn version_state(&self, id: &str) -> Result<Option<(String, String)>> {
        self.db()
            .query_row(
                "SELECT project_id, files_state FROM version WHERE id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(sql)
    }

    pub fn files(&self, version: &str) -> Result<Vec<FileRow>> {
        let db = self.db();
        let mut q = db
            .prepare(
                "SELECT ord, file_name, size, sha1, sha512, sha256, is_primary, role, origin_url
                 FROM version_file WHERE version_id = ?1 ORDER BY ord",
            )
            .map_err(sql)?;
        q.query_map([version], |r| {
            Ok(FileRow {
                ord: r.get(0)?,
                name: r.get(1)?,
                size: r.get(2)?,
                sha1: r.get(3)?,
                sha512: r.get(4)?,
                sha256: r.get(5)?,
                primary: r.get(6)?,
                role: r.get(7)?,
                origin_url: r.get(8)?,
            })
        })
        .map_err(sql)?
        .collect::<std::result::Result<_, _>>()
        .map_err(sql)
    }

    /// The blob of a file, if one with these source hashes is here already,
    /// served or held back.
    pub fn blob_by_hashes(
        &self,
        sha1: Option<&str>,
        sha512: Option<&str>,
    ) -> Result<Option<String>> {
        let db = self.db();
        if let Some(sha512) = sha512
            && let Some(found) = db
                .query_row("SELECT sha256 FROM blob WHERE sha512 = ?1", [sha512], |r| {
                    r.get(0)
                })
                .optional()
                .map_err(sql)?
        {
            return Ok(Some(found));
        }
        if let Some(sha1) = sha1 {
            return db
                .query_row("SELECT sha256 FROM blob WHERE sha1 = ?1", [sha1], |r| {
                    r.get(0)
                })
                .optional()
                .map_err(sql);
        }
        Ok(None)
    }

    /// Records a kept file; with a scan, its verdict, and held back when
    /// the verdict says so.
    pub fn add_blob(
        &self,
        hashed: &Hashed,
        media_type: &str,
        scan: Option<&crate::scan::Verdict>,
        now: i64,
    ) -> Result<()> {
        let level = scan.map_or(crate::scan::Level::Clean, |v| v.level);
        self.db()
            .execute(
                "INSERT OR IGNORE INTO blob
                     (sha256, size, sha1, sha512, media_type, state, stored, scanned, verdict, signs, held)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![
                    hashed.sha256,
                    hashed.size as i64,
                    hashed.sha1,
                    hashed.sha512,
                    media_type,
                    if level.held() { "quarantined" } else { "stored" },
                    now,
                    crate::scan::RULES,
                    level.as_str(),
                    scan.map(crate::scan::Verdict::signs_text).unwrap_or_default(),
                    level.held().then_some(now),
                ],
            )
            .map(|_| ())
            .map_err(sql)
    }

    /// Whether a kept file is served (`stored`) or held back (`quarantined`).
    pub fn blob_state(&self, sha256: &str) -> Result<Option<String>> {
        self.db()
            .query_row("SELECT state FROM blob WHERE sha256 = ?1", [sha256], |r| {
                r.get(0)
            })
            .optional()
            .map_err(sql)
    }

    /// Served archives scanned under older rules, oldest first.
    pub fn to_rescan(&self, limit: i64) -> Result<Vec<(String, String)>> {
        let db = self.db();
        let mut q = db
            .prepare(
                "SELECT sha256, media_type FROM blob
                 WHERE scanned < ?1 AND state = 'stored' AND allowed = 0
                   AND media_type IN ('application/java-archive', 'application/zip')
                 ORDER BY stored LIMIT ?2",
            )
            .map_err(sql)?;
        q.query_map(params![crate::scan::RULES, limit], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .map_err(sql)?
        .collect::<std::result::Result<_, _>>()
        .map_err(sql)
    }

    /// A scan's result for a kept file; `held` once it is out of service.
    pub fn set_scanned(
        &self,
        sha256: &str,
        scan: &crate::scan::Verdict,
        held: bool,
        now: i64,
    ) -> Result<()> {
        self.db()
            .execute(
                "UPDATE blob SET scanned = ?2, verdict = ?3, signs = ?4,
                     state = CASE WHEN ?5 THEN 'quarantined' ELSE state END,
                     held = CASE WHEN ?5 THEN ?6 ELSE held END
                 WHERE sha256 = ?1",
                params![
                    sha256,
                    crate::scan::RULES,
                    scan.level.as_str(),
                    scan.signs_text(),
                    held,
                    now
                ],
            )
            .map(|_| ())
            .map_err(sql)
    }

    /// As if every kept file were scanned under older rules.
    #[cfg(test)]
    pub fn forget_scans(&self) {
        self.db()
            .execute("UPDATE blob SET scanned = 0", [])
            .unwrap();
    }

    /// What waits for the owner: kind and subject.
    #[cfg(test)]
    pub fn reviews(&self) -> Vec<(String, String)> {
        let db = self.db();
        let mut q = db
            .prepare("SELECT kind, subject FROM review WHERE decided IS NULL ORDER BY id")
            .unwrap();
        q.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .map(std::result::Result::unwrap)
            .collect()
    }

    /// Puts something before the owner, once.
    pub fn add_review(&self, kind: &str, subject: &str, now: i64) -> Result<()> {
        self.db()
            .execute(
                "INSERT OR IGNORE INTO review (kind, subject, created) VALUES (?1, ?2, ?3)",
                params![kind, subject, now],
            )
            .map(|_| ())
            .map_err(sql)
    }

    /// Files known to be harmful since `since`: what a launcher warns about
    /// when one is installed already. The scanner's malware and what the
    /// owner blocked; files only suspected stay off the list until the owner
    /// decides. Each with its `key`, the order of the list.
    pub fn harmful_since(
        &self,
        since: i64,
        after: &str,
        limit: i64,
    ) -> Result<Vec<(serde_json::Value, String)>> {
        let db = self.db();
        let mut q = db
            .prepare(
                "SELECT key, sha256, sha1, sha512, since FROM (
                     SELECT sha256 AS key, sha256, sha1, sha512, held AS since FROM blob
                     WHERE state != 'stored' AND verdict = 'malicious' AND held IS NOT NULL
                     UNION ALL
                     SELECT key, sha256, sha1, NULL, at FROM blocked WHERE sha1 IS NOT NULL)
                 WHERE since > ?1 OR (since = ?1 AND key > ?2)
                 ORDER BY since, key LIMIT ?3",
            )
            .map_err(sql)?;
        q.query_map(params![since, after, limit], |r| {
            Ok((
                serde_json::json!({
                    "sha256": r.get::<_, Option<String>>(1)?,
                    "sha1": r.get::<_, String>(2)?,
                    "sha512": r.get::<_, Option<String>>(3)?,
                    "since": r.get::<_, i64>(4)?,
                }),
                r.get::<_, String>(0)?,
            ))
        })
        .map_err(sql)?
        .collect::<std::result::Result<_, _>>()
        .map_err(sql)
    }

    // ----- the owner's decisions (lapi-mods block, review, …) -----

    /// Whether a file with these hashes is blocked.
    pub fn is_blocked(&self, sha256: &str, sha1: &str) -> Result<bool> {
        self.db()
            .query_row(
                "SELECT 1 FROM blocked WHERE sha256 = ?1 OR sha1 = ?2 LIMIT 1",
                params![sha256, sha1],
                |_| Ok(()),
            )
            .optional()
            .map(|found| found.is_some())
            .map_err(sql)
    }

    /// A blocked file is here now: its block learns the hashes it lacked.
    pub fn complete_block(&self, sha256: &str, sha1: &str) -> Result<()> {
        self.db()
            .execute(
                "UPDATE blocked SET sha256 = coalesce(sha256, ?1), sha1 = coalesce(sha1, ?2)
                 WHERE sha256 = ?1 OR sha1 = ?2",
                params![sha256, sha1],
            )
            .map(|_| ())
            .map_err(sql)
    }

    /// Blocks a file by its SHA-256 or SHA-1; the other comes from the
    /// kept file when it is here. Returns whether it was not blocked yet.
    pub fn block(
        &self,
        sha256: Option<&str>,
        sha1: Option<&str>,
        reason: &str,
        now: i64,
    ) -> Result<bool> {
        let key = match (sha256, sha1) {
            (Some(sha256), _) => sha256.to_owned(),
            (None, Some(sha1)) => {
                use sha2::Digest;
                hex::encode(sha2::Sha256::digest(format!("sha1:{sha1}")))
            }
            (None, None) => return Ok(false),
        };
        let db = self.db();
        let there: bool = db
            .query_row(
                "SELECT 1 FROM blocked WHERE key = ?1 OR sha256 = ?2 OR sha1 = ?3",
                params![key, sha256, sha1],
                |_| Ok(()),
            )
            .optional()
            .map_err(sql)?
            .is_some();
        if there {
            return Ok(false);
        }
        db.execute(
            "INSERT INTO blocked (key, sha256, sha1, reason, at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![key, sha256, sha1, reason, now],
        )
        .map_err(sql)?;
        Ok(true)
    }

    /// Lifts a block given by any of its hashes; the number lifted.
    pub fn unblock(&self, hash: &str) -> Result<usize> {
        self.db()
            .execute(
                "DELETE FROM blocked WHERE key = ?1 OR sha256 = ?1 OR sha1 = ?1",
                [hash],
            )
            .map_err(sql)
    }

    /// A kept file as the owner sees it.
    pub fn blob_info(&self, sha256: &str) -> Result<Option<BlobInfo>> {
        self.db()
            .query_row(
                "SELECT sha256, sha1, size, media_type, state, verdict, signs, allowed, held
                 FROM blob WHERE sha256 = ?1",
                [sha256],
                |r| {
                    Ok(BlobInfo {
                        sha256: r.get(0)?,
                        sha1: r.get(1)?,
                        size: r.get(2)?,
                        media_type: r.get(3)?,
                        state: r.get(4)?,
                        verdict: r.get(5)?,
                        signs: r.get(6)?,
                        allowed: r.get(7)?,
                        held: r.get(8)?,
                    })
                },
            )
            .optional()
            .map_err(sql)
    }

    /// A kept file is out of service; `verdict` replaces the scanner's.
    pub fn set_held(&self, sha256: &str, verdict: Option<&str>, now: i64) -> Result<()> {
        self.db()
            .execute(
                "UPDATE blob SET
                     state = CASE WHEN state = 'forgotten' THEN state ELSE 'quarantined' END,
                     verdict = coalesce(?2, verdict), held = coalesce(held, ?3),
                     allowed = CASE WHEN ?2 IS NULL THEN allowed ELSE 0 END
                 WHERE sha256 = ?1",
                params![sha256, verdict, now],
            )
            .map(|_| ())
            .map_err(sql)
    }

    /// The owner let a held file go: served again, never held by a scan.
    pub fn set_allowed(&self, sha256: &str) -> Result<()> {
        self.db()
            .execute(
                "UPDATE blob SET state = 'stored', verdict = 'allowed', allowed = 1, held = NULL
                 WHERE sha256 = ?1 AND state != 'forgotten'",
                [sha256],
            )
            .map(|_| ())
            .map_err(sql)
    }

    /// A held file deleted for good: it is not fetched again.
    pub fn set_forgotten(&self, sha256: &str) -> Result<()> {
        self.db()
            .execute(
                "UPDATE blob SET state = 'forgotten' WHERE sha256 = ?1 AND state = 'quarantined'",
                [sha256],
            )
            .map(|_| ())
            .map_err(sql)
    }

    /// The names a kept file is served under, for clearing caches.
    pub fn served_names(&self, sha256: &str) -> Result<Vec<String>> {
        let db = self.db();
        let mut q = db
            .prepare(
                "SELECT file_name FROM version_file WHERE sha256 = ?1
                 UNION SELECT path FROM pack_file WHERE sha256 = ?1
                 LIMIT 200",
            )
            .map_err(sql)?;
        let names: Vec<String> = q
            .query_map([sha256], |r| r.get(0))
            .map_err(sql)?
            .collect::<std::result::Result<_, _>>()
            .map_err(sql)?;
        let mut names: Vec<String> = names
            .into_iter()
            .map(|n| n.rsplit('/').next().unwrap_or_default().to_owned())
            .filter(|n| !n.is_empty())
            .collect();
        names.sort();
        names.dedup();
        Ok(names)
    }

    /// Writes down what the owner decided.
    pub fn log_decision(&self, action: &str, subject: &str, reason: &str, now: i64) -> Result<()> {
        self.db()
            .execute(
                "INSERT INTO decision (at, action, subject, reason) VALUES (?1, ?2, ?3, ?4)",
                params![now, action, subject, reason],
            )
            .map(|_| ())
            .map_err(sql)
    }

    /// The owner's decisions about a subject, newest first.
    pub fn decisions(&self, subject: &str) -> Result<Vec<(i64, String, String)>> {
        let db = self.db();
        let mut q = db
            .prepare(
                "SELECT at, action, reason FROM decision WHERE subject = ?1
                 ORDER BY id DESC LIMIT 20",
            )
            .map_err(sql)?;
        q.query_map([subject], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .map_err(sql)?
            .collect::<std::result::Result<_, _>>()
            .map_err(sql)
    }

    /// What waits for the owner, oldest first: id, kind, subject, created.
    pub fn open_reviews(&self, limit: i64) -> Result<Vec<(i64, String, String, i64)>> {
        let db = self.db();
        let mut q = db
            .prepare(
                "SELECT id, kind, subject, created FROM review WHERE decided IS NULL
                 ORDER BY id LIMIT ?1",
            )
            .map_err(sql)?;
        q.query_map([limit], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
        })
        .map_err(sql)?
        .collect::<std::result::Result<_, _>>()
        .map_err(sql)
    }

    /// How many wait for the owner, by kind.
    pub fn review_counts(&self) -> Result<Vec<(String, i64)>> {
        let db = self.db();
        let mut q = db
            .prepare(
                "SELECT kind, count(*) FROM review WHERE decided IS NULL GROUP BY kind ORDER BY kind",
            )
            .map_err(sql)?;
        q.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .map_err(sql)?
            .collect::<std::result::Result<_, _>>()
            .map_err(sql)
    }

    /// One item of the queue: kind, subject and whether it is decided.
    pub fn review_item(&self, id: i64) -> Result<Option<(String, String, bool)>> {
        self.db()
            .query_row(
                "SELECT kind, subject, decided IS NOT NULL FROM review WHERE id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()
            .map_err(sql)
    }

    /// Closes the open items about a subject (of any kind) with a decision.
    pub fn decide_reviews(&self, subject: &str, decision: &str, now: i64) -> Result<()> {
        self.db()
            .execute(
                "UPDATE review SET decided = ?3, decision = ?2 WHERE subject = ?1 AND decided IS NULL",
                params![subject, decision, now],
            )
            .map(|_| ())
            .map_err(sql)
    }

    // ----- players' own packs -----

    /// The account that published an own pack.
    pub fn own_account(&self, project: &str) -> Result<Option<String>> {
        self.db()
            .query_row(
                "SELECT account_id FROM own_pack WHERE project_id = ?1",
                [project],
                |r| r.get(0),
            )
            .optional()
            .map_err(sql)
    }

    /// An account's own packs: new ones and versions since `since`, and the
    /// bytes of all its versions.
    pub fn own_usage(&self, account: &str, since: i64) -> Result<(i64, i64, i64)> {
        let db = self.db();
        let packs: i64 = db
            .query_row(
                "SELECT count(*) FROM own_pack WHERE account_id = ?1 AND created >= ?2",
                params![account, since],
                |r| r.get(0),
            )
            .map_err(sql)?;
        let (versions, bytes): (i64, i64) = db
            .query_row(
                "SELECT coalesce(sum(created >= ?2), 0), coalesce(sum(bytes), 0)
                 FROM own_version WHERE account_id = ?1",
                params![account, since],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(sql)?;
        Ok((packs, versions, bytes))
    }

    /// Whether an own pack has a version with this number already.
    pub fn has_number(&self, project: &str, number: &str) -> Result<bool> {
        self.db()
            .query_row(
                "SELECT 1 FROM version WHERE project_id = ?1 AND number = ?2",
                params![project, number],
                |_| Ok(()),
            )
            .optional()
            .map(|found| found.is_some())
            .map_err(sql)
    }

    /// Records who published an own pack's version (and the pack, when new).
    pub fn add_own(
        &self,
        project: &str,
        version: &str,
        account: &str,
        bytes: u64,
        now: i64,
    ) -> Result<()> {
        let mut db = self.db();
        let tx = db.transaction().map_err(sql)?;
        tx.execute(
            "INSERT OR IGNORE INTO own_pack (project_id, account_id, created) VALUES (?1, ?2, ?3)",
            params![project, account, now],
        )
        .map_err(sql)?;
        tx.execute(
            "INSERT OR REPLACE INTO own_version (version_id, project_id, account_id, bytes, created)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![version, project, account, bytes as i64, now],
        )
        .map_err(sql)?;
        tx.commit().map_err(sql)
    }

    /// An account's own packs, newest first.
    pub fn own_packs(&self, account: &str) -> Result<Vec<serde_json::Value>> {
        let db = self.db();
        let mut q = db
            .prepare(
                "SELECT p.id, p.slug, p.title, p.status, o.created,
                   (SELECT count(*) FROM version v WHERE v.project_id = p.id),
                   (SELECT v.number FROM version v WHERE v.project_id = p.id
                    ORDER BY v.published DESC, v.id DESC LIMIT 1)
                 FROM own_pack o JOIN project p ON p.id = o.project_id
                 WHERE o.account_id = ?1 AND p.status != 'room' ORDER BY o.created DESC LIMIT 200",
            )
            .map_err(sql)?;
        q.query_map([account], |r| {
            Ok(serde_json::json!({
                "id": r.get::<_, String>(0)?,
                "slug": r.get::<_, String>(1)?,
                "title": r.get::<_, String>(2)?,
                "status": r.get::<_, String>(3)?,
                "created": r.get::<_, i64>(4)?,
                "versions": r.get::<_, i64>(5)?,
                "latest": r.get::<_, Option<String>>(6)?,
            }))
        })
        .map_err(sql)?
        .collect::<std::result::Result<_, _>>()
        .map_err(sql)
    }

    /// Hides an own pack or shows it again, as its author asks; a pack the
    /// owner removed, or one still importing, stays as it is.
    pub fn set_own_hidden(&self, project: &str, hidden: bool) -> Result<bool> {
        let (from, to) = if hidden {
            ("ready", "hidden")
        } else {
            ("hidden", "ready")
        };
        self.db()
            .execute(
                "UPDATE project SET status = ?3 WHERE id = ?1 AND status IN (?2, ?3)",
                params![project, from, to],
            )
            .map(|n| n > 0)
            .map_err(sql)
    }

    /// A player's report about an own pack. A new one (the first from this
    /// account) opens the owner's review again; returns whether it was new.
    pub fn add_report(
        &self,
        project: &str,
        account: &str,
        reason: &str,
        text: &str,
        now: i64,
    ) -> Result<bool> {
        let mut db = self.db();
        let tx = db.transaction().map_err(sql)?;
        let added = tx
            .execute(
                "INSERT OR IGNORE INTO report (project_id, account_id, reason, text, created)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![project, account, reason, text, now],
            )
            .map_err(sql)?
            > 0;
        if added {
            tx.execute(
                "INSERT INTO review (kind, subject, created) VALUES ('report', ?1, ?2)
                 ON CONFLICT (kind, subject) DO UPDATE SET decided = NULL, decision = NULL",
                params![project, now],
            )
            .map_err(sql)?;
        }
        tx.commit().map_err(sql)?;
        Ok(added)
    }

    /// The reports about a pack: each reason with how many sent it, and the
    /// newest texts.
    pub fn reports(&self, project: &str) -> Result<Reports> {
        let db = self.db();
        let mut q = db
            .prepare(
                "SELECT reason, count(*) FROM report WHERE project_id = ?1
                 GROUP BY reason ORDER BY count(*) DESC, reason",
            )
            .map_err(sql)?;
        let reasons = q
            .query_map([project], |r| Ok((r.get(0)?, r.get(1)?)))
            .map_err(sql)?
            .collect::<std::result::Result<_, _>>()
            .map_err(sql)?;
        let mut q = db
            .prepare(
                "SELECT text FROM report WHERE project_id = ?1 AND text != ''
                 ORDER BY created DESC LIMIT 5",
            )
            .map_err(sql)?;
        let texts = q
            .query_map([project], |r| r.get(0))
            .map_err(sql)?
            .collect::<std::result::Result<_, _>>()
            .map_err(sql)?;
        Ok((reasons, texts))
    }

    /// The SHA-256 of a version's kept files.
    pub fn version_blobs(&self, version: &str) -> Result<Vec<String>> {
        let db = self.db();
        let mut q = db
            .prepare(
                "SELECT DISTINCT sha256 FROM version_file WHERE version_id = ?1 AND sha256 IS NOT NULL",
            )
            .map_err(sql)?;
        q.query_map([version], |r| r.get(0))
            .map_err(sql)?
            .collect::<std::result::Result<_, _>>()
            .map_err(sql)
    }

    /// A project by its ID, slug or a source's ID or slug.
    pub fn find_project(&self, reference: &str) -> Result<Option<String>> {
        self.db()
            .query_row(
                "SELECT id FROM project WHERE id = ?1 OR slug = lower(?1)
                 UNION ALL
                 SELECT project_id FROM project_source WHERE source_id = ?1 OR source_slug = lower(?1)
                 LIMIT 1",
                [reference],
                |r| r.get(0),
            )
            .optional()
            .map_err(sql)
    }

    /// Takes a project out of the catalog at its author's request: hidden
    /// from search and the API, never brought back by an import. Returns the
    /// kept files no other project needs (its own, and those only its packs
    /// use), for quarantine.
    pub fn remove_project(&self, id: &str) -> Result<Vec<String>> {
        let mut db = self.db();
        let tx = db.transaction().map_err(sql)?;
        tx.execute("UPDATE project SET status = 'removed' WHERE id = ?1", [id])
            .map_err(sql)?;
        let only: Vec<String> = {
            let mut q = tx
                .prepare(
                    "WITH mine(sha256) AS (
                         SELECT f.sha256 FROM version_file f JOIN version v ON v.id = f.version_id
                         WHERE v.project_id = ?1 AND f.sha256 IS NOT NULL
                         UNION
                         SELECT p.sha256 FROM pack_file p JOIN version v ON v.id = p.version_id
                         WHERE v.project_id = ?1 AND p.sha256 IS NOT NULL),
                     used(sha256) AS (
                         SELECT f.sha256 FROM version_file f JOIN version v ON v.id = f.version_id
                         JOIN project o ON o.id = v.project_id
                         WHERE v.project_id != ?1 AND o.status != 'removed' AND f.sha256 IS NOT NULL
                         UNION
                         SELECT p.sha256 FROM pack_file p JOIN version v ON v.id = p.version_id
                         JOIN project o ON o.id = v.project_id
                         WHERE v.project_id != ?1 AND o.status != 'removed' AND p.sha256 IS NOT NULL)
                     SELECT sha256 FROM mine WHERE sha256 NOT IN (SELECT sha256 FROM used)
                     ORDER BY sha256",
                )
                .map_err(sql)?;
            q.query_map([id], |r| r.get(0))
                .map_err(sql)?
                .collect::<std::result::Result<_, _>>()
                .map_err(sql)?
        };
        tx.commit().map_err(sql)?;
        Ok(only)
    }

    /// What the owner reads about a project: lines of text.
    pub fn project_report(&self, id: &str) -> Result<Vec<String>> {
        let db = self.db();
        let mut out = Vec::new();
        let head = db
            .query_row(
                "SELECT kind, slug, title, status, gone_at FROM project WHERE id = ?1",
                [id],
                |r| {
                    Ok(format!(
                        "project {id} {} {} \"{}\" {}{}",
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, String>(3)?,
                        if r.get::<_, Option<i64>>(4)?.is_some() {
                            " gone"
                        } else {
                            ""
                        }
                    ))
                },
            )
            .optional()
            .map_err(sql)?;
        let Some(head) = head else {
            return Ok(out);
        };
        out.push(head);
        let mut q = db
            .prepare("SELECT source, source_id, url FROM project_source WHERE project_id = ?1")
            .map_err(sql)?;
        for row in q
            .query_map([id], |r| {
                Ok(format!(
                    "  source {} {} {}",
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?
                ))
            })
            .map_err(sql)?
        {
            out.push(row.map_err(sql)?);
        }
        let mut q = db
            .prepare(
                "SELECT v.id, v.number, v.gone_at IS NOT NULL, f.file_name, f.sha256,
                        b.state, b.verdict, b.signs
                 FROM version v
                 LEFT JOIN version_file f ON f.version_id = v.id
                 LEFT JOIN blob b ON b.sha256 = f.sha256
                 WHERE v.id IN (SELECT id FROM version WHERE project_id = ?1
                                ORDER BY published DESC LIMIT 30)
                 ORDER BY v.published DESC, v.id, f.ord",
            )
            .map_err(sql)?;
        let rows = q
            .query_map([id], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, bool>(2)?,
                    r.get::<_, Option<String>>(3)?,
                    r.get::<_, Option<String>>(4)?,
                    r.get::<_, Option<String>>(5)?,
                    r.get::<_, Option<String>>(6)?,
                    r.get::<_, Option<String>>(7)?,
                ))
            })
            .map_err(sql)?;
        let mut last = String::new();
        for row in rows {
            let (version, number, gone, name, sha256, state, verdict, signs) = row.map_err(sql)?;
            if version != last {
                out.push(format!(
                    "  version {version} {number}{}",
                    if gone { " gone" } else { "" }
                ));
                last = version;
            }
            if let Some(name) = name {
                out.push(format!(
                    "    file {} {} {} {} {name}",
                    sha256.as_deref().unwrap_or("-"),
                    state.as_deref().unwrap_or("not-kept"),
                    verdict.as_deref().unwrap_or("-"),
                    signs.filter(|s| !s.is_empty()).as_deref().unwrap_or("-"),
                ));
            }
        }
        Ok(out)
    }

    /// Where a kept file is used: lines of text.
    pub fn file_uses(&self, sha256: &str) -> Result<Vec<String>> {
        let db = self.db();
        let mut out = Vec::new();
        let mut q = db
            .prepare(
                "SELECT 'version', v.id, p.slug, v.number, f.file_name FROM version_file f
                 JOIN version v ON v.id = f.version_id JOIN project p ON p.id = v.project_id
                 WHERE f.sha256 = ?1
                 UNION ALL
                 SELECT 'pack', v.id, p.slug, v.number, k.path FROM pack_file k
                 JOIN version v ON v.id = k.version_id JOIN project p ON p.id = v.project_id
                 WHERE k.sha256 = ?1
                 LIMIT 50",
            )
            .map_err(sql)?;
        for row in q
            .query_map([sha256], |r| {
                Ok(format!(
                    "  in {} {} {} {} {}",
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?
                ))
            })
            .map_err(sql)?
        {
            out.push(row.map_err(sql)?);
        }
        Ok(out)
    }

    /// The block of a file, by any of its hashes: its key and reason.
    pub fn block_of(&self, hash: &str) -> Result<Option<(String, String)>> {
        self.db()
            .query_row(
                "SELECT key, reason FROM blocked WHERE key = ?1 OR sha256 = ?1 OR sha1 = ?1",
                [hash],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(sql)
    }

    /// A version in a few words: its project's slug and its number.
    pub fn version_brief(&self, version: &str) -> Result<Option<String>> {
        self.db()
            .query_row(
                "SELECT p.slug || ' ' || v.number FROM version v
                 JOIN project p ON p.id = v.project_id WHERE v.id = ?1",
                [version],
                |r| r.get(0),
            )
            .optional()
            .map_err(sql)
    }

    // ----- copies in R2 (r2.rs) -----

    /// Kept files whose copy in R2 is not what their state wants, oldest
    /// first: sha256, media type, state, copy. Pictures stay on our disk.
    pub fn to_copy(&self, limit: i64) -> Result<Vec<(String, String, String, String)>> {
        let db = self.db();
        let mut q = db
            .prepare(
                "SELECT sha256, media_type, state, copy FROM blob
                 WHERE media_type NOT LIKE 'image/%' AND (
                     (state = 'stored' AND copy != 'public')
                     OR (state != 'stored' AND copy = 'public')
                     OR (state = 'quarantined' AND copy = '')
                     OR (state = 'forgotten' AND copy = 'private'))
                 ORDER BY stored LIMIT ?1",
            )
            .map_err(sql)?;
        q.query_map([limit], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
        })
        .map_err(sql)?
        .collect::<std::result::Result<_, _>>()
        .map_err(sql)
    }

    /// Where a kept file's copy in R2 is now.
    pub fn set_copy(&self, sha256: &str, copy: &str) -> Result<()> {
        self.db()
            .execute(
                "UPDATE blob SET copy = ?2 WHERE sha256 = ?1",
                params![sha256, copy],
            )
            .map(|_| ())
            .map_err(sql)
    }

    /// Where a kept file's copy in R2 is.
    pub fn copy_of(&self, sha256: &str) -> Result<Option<String>> {
        self.db()
            .query_row("SELECT copy FROM blob WHERE sha256 = ?1", [sha256], |r| {
                r.get(0)
            })
            .optional()
            .map_err(sql)
    }

    /// Files served from R2, picked at random: sha256 and size.
    pub fn sample_public(&self, limit: i64) -> Result<Vec<(String, i64)>> {
        let db = self.db();
        let mut q = db
            .prepare(
                "SELECT sha256, size FROM blob WHERE copy = 'public' AND state = 'stored'
                 ORDER BY random() LIMIT ?1",
            )
            .map_err(sql)?;
        q.query_map([limit], |r| Ok((r.get(0)?, r.get(1)?)))
            .map_err(sql)?
            .collect::<std::result::Result<_, _>>()
            .map_err(sql)
    }

    /// Kept files with a copy in R2, for bringing the disk back: sha256,
    /// state, copy, media type.
    pub fn copied(&self, after: &str, limit: i64) -> Result<Vec<(String, String, String, String)>> {
        let db = self.db();
        let mut q = db
            .prepare(
                "SELECT sha256, state, copy, media_type FROM blob
                 WHERE copy != '' AND sha256 > ?1 ORDER BY sha256 LIMIT ?2",
            )
            .map_err(sql)?;
        q.query_map(params![after, limit], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
        })
        .map_err(sql)?
        .collect::<std::result::Result<_, _>>()
        .map_err(sql)
    }

    pub fn upkeep(&self, key: &str) -> Result<Option<i64>> {
        self.db()
            .query_row("SELECT value FROM upkeep WHERE key = ?1", [key], |r| {
                r.get(0)
            })
            .optional()
            .map_err(sql)
    }

    pub fn set_upkeep(&self, key: &str, value: i64) -> Result<()> {
        self.db()
            .execute(
                "INSERT INTO upkeep (key, value) VALUES (?1, ?2)
                 ON CONFLICT (key) DO UPDATE SET value = excluded.value",
                params![key, value],
            )
            .map(|_| ())
            .map_err(sql)
    }

    /// The SHA-256 of a kept file known by its SHA-1.
    pub fn blob_by_sha1(&self, sha1: &str) -> Result<Option<String>> {
        self.db()
            .query_row("SELECT sha256 FROM blob WHERE sha1 = ?1", [sha1], |r| {
                r.get(0)
            })
            .optional()
            .map_err(sql)
    }

    /// Links a kept file to its place in a version.
    /// A file is kept; a source that gave no hashes gets ours, and one that
    /// gave no size our size.
    pub fn file_kept(&self, version: &str, ord: i64, sha256: &str) -> Result<()> {
        let db = self.db();
        db.execute(
            "UPDATE version_file SET sha256 = ?3,
               sha1 = coalesce(sha1, (SELECT sha1 FROM blob WHERE sha256 = ?3)),
               sha512 = coalesce(sha512, (SELECT sha512 FROM blob WHERE sha256 = ?3)),
               size = CASE WHEN size > 0 THEN size
                 ELSE coalesce((SELECT size FROM blob WHERE sha256 = ?3), 0) END
             WHERE version_id = ?1 AND ord = ?2",
            params![version, ord, sha256],
        )
        .map_err(sql)?;
        Self::refresh_files_state(&db, version)
    }

    // ----- external projects -----

    /// Records (or changes) a project the owner describes.
    pub fn set_external(&self, card: &crate::external::Card, now: i64) -> Result<()> {
        self.db()
            .execute(
                "INSERT INTO external_project (source_id, kind, slug, title, summary, page, icon, days, added)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                 ON CONFLICT (source_id) DO UPDATE SET kind = ?2, slug = ?3, title = ?4,
                   summary = ?5, page = ?6, icon = ?7, days = ?8",
                params![
                    card.source_id,
                    card.kind,
                    card.slug,
                    card.title,
                    card.summary,
                    card.page,
                    card.icon,
                    card.days,
                    now
                ],
            )
            .map(|_| ())
            .map_err(sql)
    }

    pub fn external(&self, source_id: &str) -> Result<Option<crate::external::Card>> {
        self.db()
            .query_row(
                "SELECT source_id, kind, slug, title, summary, page, icon, days
                 FROM external_project WHERE source_id = ?1",
                [source_id],
                |r| {
                    Ok(crate::external::Card {
                        source_id: r.get(0)?,
                        kind: r.get(1)?,
                        slug: r.get(2)?,
                        title: r.get(3)?,
                        summary: r.get(4)?,
                        page: r.get(5)?,
                        icon: r.get(6)?,
                        days: r.get(7)?,
                    })
                },
            )
            .optional()
            .map_err(sql)
    }

    /// The owner's projects: ID at the source, kind, days, our ID once
    /// imported, its versions and how many of them are kept, when last read.
    pub fn externals(&self) -> Result<Vec<ExternalRow>> {
        let db = self.db();
        let mut q = db
            .prepare(
                "SELECT e.source_id, e.kind, coalesce(p.recheck_days, e.days), s.project_id,
                   (SELECT count(*) FROM version v WHERE v.project_id = s.project_id
                      AND v.source = 'external' AND v.gone_at IS NULL),
                   (SELECT count(*) FROM version v WHERE v.project_id = s.project_id
                      AND v.source = 'external' AND v.files_state = 'all'),
                   s.fetched
                 FROM external_project e
                 LEFT JOIN project_source s ON s.source = 'external' AND s.source_id = e.source_id
                 LEFT JOIN project p ON p.id = s.project_id
                 ORDER BY e.added",
            )
            .map_err(sql)?;
        q.query_map([], |r| {
            Ok(ExternalRow {
                source_id: r.get(0)?,
                kind: r.get(1)?,
                days: r.get(2)?,
                project: r.get(3)?,
                versions: r.get(4)?,
                kept: r.get(5)?,
                fetched: r.get(6)?,
            })
        })
        .map_err(sql)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(sql)
    }

    /// Adds (or changes) a version the owner gives by its file's address.
    pub fn add_external_version(
        &self,
        source_id: &str,
        added: &crate::external::Added,
    ) -> Result<()> {
        self.db()
            .execute(
                "INSERT INTO external_version (source_id, file_url, file_name, number, channel,
                   games, loaders, added)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                 ON CONFLICT (source_id, file_url) DO UPDATE SET file_name = ?3, number = ?4,
                   channel = ?5, games = ?6, loaders = ?7",
                params![
                    source_id,
                    added.file_url,
                    added.file_name,
                    added.number,
                    added.channel,
                    added.games.join(","),
                    added.loaders.join(","),
                    added.added
                ],
            )
            .map(|_| ())
            .map_err(sql)
    }

    pub fn external_versions(&self, source_id: &str) -> Result<Vec<crate::external::Added>> {
        let db = self.db();
        let mut q = db
            .prepare(
                "SELECT file_url, file_name, number, channel, games, loaders, added
                 FROM external_version WHERE source_id = ?1 ORDER BY added",
            )
            .map_err(sql)?;
        let split = |text: String| -> Vec<String> {
            text.split(',')
                .filter(|w| !w.is_empty())
                .map(str::to_owned)
                .collect()
        };
        q.query_map([source_id], |r| {
            Ok(crate::external::Added {
                file_url: r.get(0)?,
                file_name: r.get(1)?,
                number: r.get(2)?,
                channel: r.get(3)?,
                games: split(r.get(4)?),
                loaders: split(r.get(5)?),
                added: r.get(6)?,
            })
        })
        .map_err(sql)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(sql)
    }

    /// Days between checks of a project's sources: `None` the usual rule.
    pub fn set_recheck_days(&self, project: &str, days: Option<i64>) -> Result<()> {
        self.db()
            .execute(
                "UPDATE project SET recheck_days = ?2 WHERE id = ?1",
                params![project, days],
            )
            .map(|_| ())
            .map_err(sql)
    }

    /// Days between checks, unless the owner set them already.
    pub fn default_recheck_days(&self, project: &str, days: i64) -> Result<()> {
        self.db()
            .execute(
                "UPDATE project SET recheck_days = ?2 WHERE id = ?1 AND recheck_days IS NULL",
                params![project, days],
            )
            .map(|_| ())
            .map_err(sql)
    }

    pub fn recheck_days(&self, project: &str) -> Result<Option<i64>> {
        self.db()
            .query_row(
                "SELECT recheck_days FROM project WHERE id = ?1",
                [project],
                |r| r.get(0),
            )
            .optional()
            .map(Option::flatten)
            .map_err(sql)
    }

    /// The source a version came from.
    pub fn version_source(&self, version: &str) -> Result<Option<String>> {
        self.db()
            .query_row("SELECT source FROM version WHERE id = ?1", [version], |r| {
                r.get(0)
            })
            .optional()
            .map_err(sql)
    }

    // ----- modpacks -----

    /// Whether a modpack version's index was read.
    pub fn has_pack(&self, version: &str) -> Result<bool> {
        self.db()
            .query_row(
                "SELECT 1 FROM pack WHERE version_id = ?1",
                [version],
                |_| Ok(()),
            )
            .optional()
            .map(|found| found.is_some())
            .map_err(sql)
    }

    /// Records a modpack version's index.
    pub fn set_pack(
        &self,
        version: &str,
        archive: &str,
        index: &crate::pack::Index,
        now: i64,
    ) -> Result<()> {
        let mut db = self.db();
        let tx = db.transaction().map_err(sql)?;
        tx.execute("DELETE FROM pack_file WHERE version_id = ?1", [version])
            .map_err(sql)?;
        tx.execute(
            "INSERT OR REPLACE INTO pack (version_id, archive_sha256, game, loader, loader_version, skipped, read)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                version,
                archive,
                index.game,
                index.loader.as_ref().map(|l| &l.0),
                index.loader.as_ref().map(|l| &l.1),
                index.skipped as i64,
                now
            ],
        )
        .map_err(sql)?;
        for (ord, file) in index.files.iter().enumerate() {
            tx.execute(
                "INSERT INTO pack_file (version_id, ord, path, size, sha1, sha512, optional, origin_url)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    version,
                    ord as i64,
                    file.path,
                    file.size as i64,
                    file.sha1,
                    file.sha512,
                    file.optional,
                    file.url
                ],
            )
            .map_err(sql)?;
        }
        // A CurseForge pack's files: no path, size or hashes until found.
        for (n, file) in index.curseforge.iter().enumerate() {
            tx.execute(
                "INSERT INTO pack_file (version_id, ord, path, size, sha1, sha512, optional, origin_url)
                 VALUES (?1, ?2, '', 0, '', '', ?3, ?4)",
                params![
                    version,
                    (index.files.len() + n) as i64,
                    file.optional,
                    crate::curseforge::download_url(file.project, file.file)
                ],
            )
            .map_err(sql)?;
        }
        tx.commit().map_err(sql)
    }

    /// A CurseForge pack's file, found and kept.
    pub fn pack_file_found(
        &self,
        version: &str,
        ord: i64,
        path: &str,
        hashed: &crate::blobs::Hashed,
    ) -> Result<()> {
        self.db()
            .execute(
                "UPDATE pack_file SET path = ?3, size = ?4, sha1 = ?5, sha512 = ?6, sha256 = ?7
                 WHERE version_id = ?1 AND ord = ?2",
                params![
                    version,
                    ord,
                    path,
                    hashed.size as i64,
                    hashed.sha1,
                    hashed.sha512,
                    hashed.sha256
                ],
            )
            .map(|_| ())
            .map_err(sql)
    }

    /// A pack's file left out after all (not something a pack may bring).
    pub fn pack_file_left_out(&self, version: &str, ord: i64) -> Result<()> {
        let mut db = self.db();
        let tx = db.transaction().map_err(sql)?;
        let gone = tx
            .execute(
                "DELETE FROM pack_file WHERE version_id = ?1 AND ord = ?2",
                params![version, ord],
            )
            .map_err(sql)?;
        tx.execute(
            "UPDATE pack SET skipped = skipped + ?2 WHERE version_id = ?1",
            params![version, gone as i64],
        )
        .map_err(sql)?;
        tx.commit().map_err(sql)
    }

    /// A file fetched from this address before, by any version or pack, served
    /// or held back.
    pub fn kept_by_origin(&self, url: &str) -> Result<Option<crate::blobs::Hashed>> {
        self.db()
            .query_row(
                "SELECT b.sha256, b.size, b.sha1, b.sha512 FROM blob b
                 WHERE b.sha256 IN (
                     SELECT sha256 FROM version_file WHERE origin_url = ?1 AND sha256 IS NOT NULL
                     UNION SELECT sha256 FROM pack_file WHERE origin_url = ?1 AND sha256 IS NOT NULL)
                 LIMIT 1",
                [url],
                |r| {
                    Ok(crate::blobs::Hashed {
                        sha256: r.get(0)?,
                        size: u64::try_from(r.get::<_, i64>(1)?).unwrap_or(0),
                        sha1: r.get(2)?,
                        sha512: r.get(3)?,
                    })
                },
            )
            .optional()
            .map_err(sql)
    }

    /// The kind of a version's project (`mod`, `modpack`, …).
    pub fn version_kind(&self, version: &str) -> Result<Option<String>> {
        self.db()
            .query_row(
                "SELECT p.kind FROM version v JOIN project p ON p.id = v.project_id WHERE v.id = ?1",
                [version],
                |r| r.get(0),
            )
            .optional()
            .map_err(sql)
    }

    pub fn pack_files(&self, version: &str) -> Result<Vec<PackFileRow>> {
        let db = self.db();
        let mut q = db
            .prepare(
                "SELECT ord, path, size, sha1, sha512, sha256, origin_url
                 FROM pack_file WHERE version_id = ?1 ORDER BY ord",
            )
            .map_err(sql)?;
        q.query_map([version], |r| {
            Ok(PackFileRow {
                ord: r.get(0)?,
                path: r.get(1)?,
                size: r.get(2)?,
                sha1: r.get(3)?,
                sha512: r.get(4)?,
                sha256: r.get(5)?,
                origin_url: r.get(6)?,
            })
        })
        .map_err(sql)?
        .collect::<std::result::Result<_, _>>()
        .map_err(sql)
    }

    pub fn pack_file_kept(&self, version: &str, ord: i64, sha256: &str) -> Result<()> {
        self.db()
            .execute(
                "UPDATE pack_file SET sha256 = ?3 WHERE version_id = ?1 AND ord = ?2",
                params![version, ord, sha256],
            )
            .map(|_| ())
            .map_err(sql)
    }

    /// A modpack version's index with our addresses of its files, once read.
    pub fn pack_json(
        &self,
        version: &str,
        files: &crate::Urls,
    ) -> Result<Option<serde_json::Value>> {
        let db = self.db();
        let base = db
            .query_row(
                "SELECT game, loader, loader_version, skipped FROM pack WHERE version_id = ?1",
                [version],
                |r| {
                    let loader: Option<String> = r.get(1)?;
                    let loader_version: Option<String> = r.get(2)?;
                    Ok(serde_json::json!({
                        "game": r.get::<_, String>(0)?,
                        "loader": loader.map(|kind| serde_json::json!({
                            "kind": kind,
                            "version": loader_version,
                        })),
                        "skipped": r.get::<_, i64>(3)?,
                    }))
                },
            )
            .optional()
            .map_err(sql)?;
        let Some(mut pack) = base else {
            return Ok(None);
        };
        let mut q = db
            .prepare(
                "SELECT p.path, p.size, p.sha256, p.optional, b.state, b.copy FROM pack_file p
                 LEFT JOIN blob b ON b.sha256 = p.sha256
                 WHERE p.version_id = ?1 ORDER BY p.ord",
            )
            .map_err(sql)?;
        let list: Vec<serde_json::Value> = q
            .query_map([version], |r| {
                let path: String = r.get(0)?;
                let sha256: Option<String> = r.get(2)?;
                let held = r
                    .get::<_, Option<String>>(4)?
                    .as_deref()
                    .is_some_and(|s| s != "stored");
                let name = path.rsplit('/').next().unwrap_or_default().to_owned();
                let public = r.get::<_, Option<String>>(5)?.as_deref() == Some("public");
                Ok(serde_json::json!({
                    "path": path,
                    "size": r.get::<_, i64>(1)?,
                    "sha256": sha256,
                    // A held file has no address: nobody downloads it.
                    "url": sha256.as_deref().filter(|_| !held).map(|s| files.served(s, &name, public)),
                    "held": held,
                    "optional": r.get::<_, bool>(3)?,
                }))
            })
            .map_err(sql)?
            .collect::<std::result::Result<_, _>>()
            .map_err(sql)?;
        let total = list.len();
        // A CurseForge pack's files not found yet are counted, not listed.
        let list: Vec<serde_json::Value> = list.into_iter().filter(|f| f["path"] != "").collect();
        let kept = list.iter().filter(|f| !f["sha256"].is_null()).count();
        pack["total"] = total.into();
        pack["kept"] = kept.into();
        pack["files"] = list.into();
        Ok(Some(pack))
    }

    /// Marks a version its source took down. One with files kept here goes
    /// before the owner: a source may have taken it down as harmful, and it
    /// stays offered until the owner decides (D-066).
    pub fn mark_version_gone(&self, version: &str, now: i64) -> Result<()> {
        let mut db = self.db();
        let tx = db.transaction().map_err(sql)?;
        let newly = tx
            .execute(
                "UPDATE version SET gone_at = ?2 WHERE id = ?1 AND gone_at IS NULL",
                params![version, now],
            )
            .map_err(sql)?
            > 0;
        if newly {
            tx.execute(
                "INSERT OR IGNORE INTO review (kind, subject, created)
                 SELECT 'gone', ?1, ?2 WHERE EXISTS (
                     SELECT 1 FROM version_file WHERE version_id = ?1 AND sha256 IS NOT NULL)",
                params![version, now],
            )
            .map_err(sql)?;
        }
        tx.commit().map_err(sql)
    }

    // ----- the queue -----

    /// Queues a job unless one with the key is waiting or running. A finished
    /// one runs again when `again_after` seconds have passed since (or at
    /// once with 0); `None` never.
    #[allow(clippy::too_many_arguments)]
    pub fn enqueue(
        &self,
        key: &str,
        kind: &str,
        payload: &serde_json::Value,
        request: Option<&str>,
        project: Option<&str>,
        now: i64,
        again_after: Option<i64>,
    ) -> Result<bool> {
        let db = self.db();
        let inserted = db
            .execute(
                "INSERT OR IGNORE INTO job (key, kind, payload, request_id, project_id, state, run_after, created)
                 VALUES (?1, ?2, ?3, ?4, ?5, 'queued', ?6, ?6)",
                params![key, kind, payload.to_string(), request, project, now],
            )
            .map_err(sql)?;
        if inserted > 0 {
            return Ok(true);
        }
        let Some(after) = again_after else {
            return Ok(false);
        };
        let again = db
            .execute(
                "UPDATE job SET state = 'queued', attempts = 0, run_after = ?2, error = NULL,
                   payload = ?3, request_id = coalesce(?4, request_id), finished = NULL
                 WHERE key = ?1 AND state IN ('done', 'failed') AND coalesce(finished, 0) <= ?2 - ?5",
                params![key, now, payload.to_string(), request, after],
            )
            .map_err(sql)?;
        Ok(again > 0)
    }

    /// A job's state and error, by its key.
    pub fn job_state(&self, key: &str) -> Result<Option<(String, Option<String>)>> {
        self.db()
            .query_row("SELECT state, error FROM job WHERE key = ?1", [key], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .optional()
            .map_err(sql)
    }

    /// Takes the next waiting job.
    pub fn claim(&self, now: i64) -> Result<Option<Job>> {
        let db = self.db();
        db.query_row(
            "UPDATE job SET state = 'running', attempts = attempts + 1
             WHERE id = (SELECT id FROM job WHERE state = 'queued' AND run_after <= ?1 ORDER BY run_after, id LIMIT 1)
             RETURNING id, kind, payload, request_id, attempts",
            [now],
            |r| {
                let payload: String = r.get(2)?;
                Ok(Job {
                    id: r.get(0)?,
                    kind: r.get(1)?,
                    payload: serde_json::from_str(&payload).unwrap_or_default(),
                    request_id: r.get(3)?,
                    attempts: r.get(4)?,
                })
            },
        )
        .optional()
        .map_err(sql)
    }

    pub fn job_done(&self, id: i64, now: i64) -> Result<()> {
        self.db()
            .execute(
                "UPDATE job SET state = 'done', error = NULL, finished = ?2 WHERE id = ?1",
                params![id, now],
            )
            .map(|_| ())
            .map_err(sql)
    }

    /// Fails a job for good, or puts it back to run at `retry_at`.
    pub fn job_failed(&self, id: i64, error: &str, retry_at: Option<i64>, now: i64) -> Result<()> {
        let db = self.db();
        match retry_at {
            Some(at) => db.execute(
                "UPDATE job SET state = 'queued', error = ?2, run_after = ?3 WHERE id = ?1",
                params![id, error, at],
            ),
            None => db.execute(
                "UPDATE job SET state = 'failed', error = ?2, finished = ?3 WHERE id = ?1",
                params![id, error, now],
            ),
        }
        .map(|_| ())
        .map_err(sql)
    }

    /// Jobs a stop left running wait again.
    pub fn requeue_running(&self) -> Result<usize> {
        self.db()
            .execute(
                "UPDATE job SET state = 'queued' WHERE state = 'running'",
                [],
            )
            .map_err(sql)
    }

    /// Unfinished jobs of a request, and the error of a failed one.
    pub fn request_jobs(&self, request: &str) -> Result<(i64, Option<String>)> {
        let db = self.db();
        let pending: i64 = db
            .query_row(
                "SELECT count(*) FROM job WHERE request_id = ?1 AND state IN ('queued', 'running')",
                [request],
                |r| r.get(0),
            )
            .map_err(sql)?;
        let failed: Option<String> = db
            .query_row(
                "SELECT error FROM job WHERE request_id = ?1 AND state = 'failed' AND kind IN ('resolve', 'project')
                 LIMIT 1",
                [request],
                |r| r.get(0),
            )
            .optional()
            .map_err(sql)?;
        Ok((pending, failed))
    }

    // ----- players' requests -----

    pub fn create_request(&self, id: &str, account: &str, input: &str, now: i64) -> Result<()> {
        self.db()
            .execute(
                "INSERT INTO import_request (id, account_id, input, state, created) VALUES (?1, ?2, ?3, 'queued', ?4)",
                params![id, account, input, now],
            )
            .map(|_| ())
            .map_err(sql)
    }

    pub fn request(&self, id: &str) -> Result<Option<(String, Request)>> {
        self.db()
            .query_row(
                "SELECT account_id, id, input, state, error, project_id, candidates, created, finished
                 FROM import_request WHERE id = ?1",
                [id],
                |r| {
                    let candidates: Option<String> = r.get(6)?;
                    Ok((
                        r.get(0)?,
                        Request {
                            id: r.get(1)?,
                            input: r.get(2)?,
                            state: r.get(3)?,
                            error: r.get(4)?,
                            project_id: r.get(5)?,
                            candidates: candidates.and_then(|c| serde_json::from_str(&c).ok()),
                            created: r.get(7)?,
                            finished: r.get(8)?,
                        },
                    ))
                },
            )
            .optional()
            .map_err(sql)
    }

    pub fn requests_of(&self, account: &str, limit: i64) -> Result<Vec<Request>> {
        let ids: Vec<String> = {
            let db = self.db();
            let mut q = db
                .prepare("SELECT id FROM import_request WHERE account_id = ?1 ORDER BY created DESC LIMIT ?2")
                .map_err(sql)?;
            q.query_map(params![account, limit], |r| r.get(0))
                .map_err(sql)?
                .collect::<std::result::Result<_, _>>()
                .map_err(sql)?
        };
        let mut out = Vec::new();
        for id in ids {
            if let Some((_, r)) = self.request(&id)? {
                out.push(r);
            }
        }
        Ok(out)
    }

    /// Requests of an account since a time, and the bytes they brought.
    pub fn account_usage(&self, account: &str, since: i64) -> Result<(i64, i64)> {
        self.db()
            .query_row(
                "SELECT count(*), coalesce(sum(bytes), 0) FROM import_request WHERE account_id = ?1 AND created >= ?2",
                params![account, since],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(sql)
    }

    pub fn set_request(
        &self,
        id: &str,
        state: &str,
        error: Option<&str>,
        project: Option<&str>,
        candidates: Option<&serde_json::Value>,
        now: i64,
    ) -> Result<()> {
        let finished = matches!(state, "ready" | "failed").then_some(now);
        self.db()
            .execute(
                "UPDATE import_request SET state = ?2, error = ?3, project_id = coalesce(?4, project_id),
                   candidates = coalesce(?5, candidates), finished = ?6 WHERE id = ?1",
                params![id, state, error, project, candidates.map(|c| c.to_string()), finished],
            )
            .map(|_| ())
            .map_err(sql)
    }

    pub fn add_request_bytes(&self, id: &str, bytes: u64) -> Result<()> {
        self.db()
            .execute(
                "UPDATE import_request SET bytes = bytes + ?2 WHERE id = ?1",
                params![id, bytes as i64],
            )
            .map(|_| ())
            .map_err(sql)
    }

    /// Requests still being worked on.
    pub fn open_requests(&self) -> Result<Vec<String>> {
        let db = self.db();
        let mut q = db
            .prepare("SELECT id FROM import_request WHERE state IN ('queued', 'importing')")
            .map_err(sql)?;
        q.query_map([], |r| r.get(0))
            .map_err(sql)?
            .collect::<std::result::Result<_, _>>()
            .map_err(sql)
    }

    // ----- reading the catalog -----

    /// A project by our ID or slug.
    pub fn project_id(&self, reference: &str) -> Result<Option<String>> {
        self.db()
            .query_row(
                "SELECT id FROM project WHERE (id = ?1 OR slug = ?1) AND status IN ('importing', 'ready')
                 UNION ALL
                 SELECT j.into_id FROM project_joined j JOIN project p ON p.id = j.project_id
                 WHERE p.id = ?1 OR p.slug = ?1
                 LIMIT 1",
                [reference],
                |r| r.get(0),
            )
            .optional()
            .map_err(sql)
    }

    pub fn project_json(&self, id: &str, files: &crate::Urls) -> Result<Option<serde_json::Value>> {
        let db = self.db();
        let base = db
            .query_row(
                "SELECT id, kind, slug, title, summary, body, license, icon_sha256, icon_type, icon_thumb,
                   client_side, server_side, status, downloads, followers, downloads_ours, published, updated,
                   imported, gone_at
                 FROM project WHERE id = ?1",
                [id],
                |r| {
                    let icon: Option<String> = r.get(7)?;
                    let icon_type: Option<String> = r.get(8)?;
                    let thumb: Option<String> = r.get(9)?;
                    Ok(serde_json::json!({
                        "id": r.get::<_, String>(0)?,
                        "kind": r.get::<_, String>(1)?,
                        "slug": r.get::<_, String>(2)?,
                        "title": r.get::<_, String>(3)?,
                        "summary": r.get::<_, String>(4)?,
                        "body": r.get::<_, String>(5)?,
                        "license": r.get::<_, String>(6)?,
                        "icon": icon.as_deref().map(|s| files.image(s, icon_type.as_deref().unwrap_or(""))),
                        "iconThumb": thumb.as_deref().map(|s| files.image(s, "image/webp")),
                        "clientSide": r.get::<_, String>(10)?,
                        "serverSide": r.get::<_, String>(11)?,
                        "status": r.get::<_, String>(12)?,
                        "downloads": r.get::<_, i64>(13)?,
                        "followers": r.get::<_, i64>(14)?,
                        "downloadsHere": r.get::<_, i64>(15)?,
                        "published": r.get::<_, i64>(16)?,
                        "updated": r.get::<_, i64>(17)?,
                        "imported": r.get::<_, i64>(18)?,
                        "goneAt": r.get::<_, Option<i64>>(19)?,
                    }))
                },
            )
            .optional()
            .map_err(sql)?;
        let Some(mut project) = base else {
            return Ok(None);
        };
        let authors: Vec<serde_json::Value> = {
            let mut q = db
                .prepare("SELECT name, role FROM project_author WHERE project_id = ?1 ORDER BY ord")
                .map_err(sql)?;
            q.query_map([id], |r| {
                Ok(serde_json::json!({ "name": r.get::<_, String>(0)?, "role": r.get::<_, String>(1)? }))
            })
            .map_err(sql)?
            .collect::<std::result::Result<_, _>>()
            .map_err(sql)?
        };
        let categories: Vec<String> = {
            let mut q = db
                .prepare(
                    "SELECT category FROM project_category WHERE project_id = ?1 ORDER BY category",
                )
                .map_err(sql)?;
            q.query_map([id], |r| r.get(0))
                .map_err(sql)?
                .collect::<std::result::Result<_, _>>()
                .map_err(sql)?
        };
        let sources: Vec<serde_json::Value> = {
            let mut q = db
                .prepare("SELECT source, source_id, source_slug, url FROM project_source WHERE project_id = ?1")
                .map_err(sql)?;
            q.query_map([id], |r| {
                Ok(serde_json::json!({
                    "source": r.get::<_, String>(0)?,
                    "id": r.get::<_, String>(1)?,
                    "slug": r.get::<_, String>(2)?,
                    "url": r.get::<_, String>(3)?,
                }))
            })
            .map_err(sql)?
            .collect::<std::result::Result<_, _>>()
            .map_err(sql)?
        };
        let gallery: Vec<serde_json::Value> = {
            let mut q = db
                .prepare(
                    "SELECT sha256, media_type, thumb_sha256, title, description FROM gallery
                     WHERE project_id = ?1 AND sha256 IS NOT NULL ORDER BY ord",
                )
                .map_err(sql)?;
            q.query_map([id], |r| {
                let sha: String = r.get(0)?;
                let media: Option<String> = r.get(1)?;
                let thumb: Option<String> = r.get(2)?;
                Ok(serde_json::json!({
                    "url": files.image(&sha, media.as_deref().unwrap_or("")),
                    "thumb": thumb.as_deref().map(|t| files.image(t, "image/jpeg")),
                    "title": r.get::<_, String>(3)?,
                    "description": r.get::<_, String>(4)?,
                }))
            })
            .map_err(sql)?
            .collect::<std::result::Result<_, _>>()
            .map_err(sql)?
        };
        project["authors"] = authors.into();
        project["categories"] = categories.into();
        project["sources"] = sources.into();
        project["gallery"] = gallery.into();
        Ok(Some(project))
    }

    fn version_json(
        db: &Connection,
        id: &str,
        files: &crate::Urls,
    ) -> Result<Option<serde_json::Value>> {
        let base = db
            .query_row(
                "SELECT id, project_id, number, name, changelog, channel, published, downloads, files_state, gone_at
                 FROM version WHERE id = ?1",
                [id],
                |r| {
                    Ok(serde_json::json!({
                        "id": r.get::<_, String>(0)?,
                        "projectId": r.get::<_, String>(1)?,
                        "number": r.get::<_, String>(2)?,
                        "name": r.get::<_, String>(3)?,
                        "changelog": r.get::<_, String>(4)?,
                        "channel": r.get::<_, String>(5)?,
                        "published": r.get::<_, i64>(6)?,
                        "downloads": r.get::<_, i64>(7)?,
                        // all: kept here; partial or none: fetched on the first install.
                        "saved": r.get::<_, String>(8)?,
                        "goneAt": r.get::<_, Option<i64>>(9)?,
                    }))
                },
            )
            .optional()
            .map_err(sql)?;
        let Some(mut version) = base else {
            return Ok(None);
        };
        let list = |query: &str| -> Result<Vec<String>> {
            let mut q = db.prepare(query).map_err(sql)?;
            q.query_map([id], |r| r.get(0))
                .map_err(sql)?
                .collect::<std::result::Result<_, _>>()
                .map_err(sql)
        };
        version["games"] =
            list("SELECT game FROM version_game WHERE version_id = ?1 ORDER BY game")?.into();
        version["loaders"] =
            list("SELECT loader FROM version_loader WHERE version_id = ?1 ORDER BY loader")?.into();
        let file_list: Vec<serde_json::Value> = {
            let mut q = db
                .prepare(
                    "SELECT f.file_name, f.size, f.sha1, f.sha512, f.sha256, f.is_primary, f.role, f.origin_url, b.state,
                            b.copy
                     FROM version_file f LEFT JOIN blob b ON b.sha256 = f.sha256
                     WHERE f.version_id = ?1 ORDER BY f.ord",
                )
                .map_err(sql)?;
            q.query_map([id], |r| {
                let name: String = r.get(0)?;
                let sha256: Option<String> = r.get(4)?;
                let held = r
                    .get::<_, Option<String>>(8)?
                    .as_deref()
                    .is_some_and(|s| s != "stored");
                let public = r.get::<_, Option<String>>(9)?.as_deref() == Some("public");
                Ok(serde_json::json!({
                    "name": name,
                    "size": r.get::<_, i64>(1)?,
                    "sha1": r.get::<_, Option<String>>(2)?,
                    "sha512": r.get::<_, Option<String>>(3)?,
                    "sha256": sha256,
                    "primary": r.get::<_, bool>(5)?,
                    "role": r.get::<_, String>(6)?,
                    "url": sha256.as_deref().filter(|_| !held).map(|s| files.served(s, &name, public)),
                    "held": held,
                    "originUrl": r.get::<_, String>(7)?,
                }))
            })
            .map_err(sql)?
            .collect::<std::result::Result<_, _>>()
            .map_err(sql)?
        };
        version["files"] = file_list.into();
        let dependencies: Vec<serde_json::Value> = {
            let mut q = db
                .prepare(
                    "SELECT d.kind, d.source, d.source_project, d.source_version, ps.project_id, v.id, v.project_id
                     FROM dependency d
                     LEFT JOIN project_source ps ON ps.source = d.source AND ps.source_id = d.source_project
                     LEFT JOIN version v ON v.source = d.source AND v.source_id = d.source_version
                     WHERE d.version_id = ?1 ORDER BY d.ord",
                )
                .map_err(sql)?;
            q.query_map([id], |r| {
                let project: Option<String> = r.get(4)?;
                let version_project: Option<String> = r.get(6)?;
                Ok(serde_json::json!({
                    "kind": r.get::<_, String>(0)?,
                    "projectId": project.or(version_project),
                    "versionId": r.get::<_, Option<String>>(5)?,
                    "source": r.get::<_, String>(1)?,
                    "sourceProject": r.get::<_, Option<String>>(2)?,
                    "sourceVersion": r.get::<_, Option<String>>(3)?,
                }))
            })
            .map_err(sql)?
            .collect::<std::result::Result<_, _>>()
            .map_err(sql)?
        };
        version["dependencies"] = dependencies.into();
        Ok(Some(version))
    }

    pub fn version(&self, id: &str, files: &crate::Urls) -> Result<Option<serde_json::Value>> {
        Self::version_json(&self.db(), id, files)
    }

    /// A project's versions, newest first, for a game version and a loader.
    pub fn versions(
        &self,
        project: &str,
        game: Option<&str>,
        loader: Option<&str>,
        files: &crate::Urls,
    ) -> Result<Vec<serde_json::Value>> {
        let db = self.db();
        let ids: Vec<String> = {
            let mut q = db
                .prepare(
                    "SELECT v.id FROM version v WHERE v.project_id = ?1
                       AND (?2 IS NULL OR EXISTS (SELECT 1 FROM version_game g WHERE g.version_id = v.id AND g.game = ?2))
                       AND (?3 IS NULL OR EXISTS (SELECT 1 FROM version_loader l WHERE l.version_id = v.id AND l.loader = ?3))
                       -- A joined project's file on both sources is shown once, as Modrinth's.
                       AND NOT (v.source != 'modrinth' AND EXISTS (
                         SELECT 1 FROM version_file f
                         LEFT JOIN blob b ON b.sha256 = f.sha256
                         JOIN version_file g ON g.sha256 = f.sha256 OR g.sha1 = b.sha1
                         JOIN version w ON w.id = g.version_id
                         WHERE f.version_id = v.id AND w.project_id = v.project_id AND w.source = 'modrinth'))
                     ORDER BY v.published DESC LIMIT 2000",
                )
                .map_err(sql)?;
            q.query_map(params![project, game, loader], |r| r.get(0))
                .map_err(sql)?
                .collect::<std::result::Result<_, _>>()
                .map_err(sql)?
        };
        let mut out = Vec::with_capacity(ids.len());
        for id in ids {
            if let Some(v) = Self::version_json(&db, &id, files)? {
                out.push(v);
            }
        }
        Ok(out)
    }

    /// Projects matching a search, with the total.
    pub fn search(&self, s: &Search, files: &crate::Urls) -> Result<(i64, Vec<serde_json::Value>)> {
        let db = self.db();
        let fts = (!s.words.is_empty()).then(|| {
            s.words
                .iter()
                .map(|w| format!("\"{}\"*", w.replace('"', "")))
                .collect::<Vec<_>>()
                .join(" ")
        });
        let order = match s.sort {
            "updated" => "p.updated DESC",
            "new" => "p.imported DESC",
            "title" => "p.title COLLATE NOCASE",
            "downloads" => "p.downloads DESC",
            _ if fts.is_some() => "bm25(project_fts), p.downloads DESC",
            _ => "p.downloads DESC",
        };
        let from = if fts.is_some() {
            "project_fts JOIN project p ON p.id = project_fts.project_id"
        } else {
            "project p"
        };
        // FTS5's MATCH cannot sit under an OR: without words, ?1 is NULL and
        // the FTS table is not in the query.
        let words = if fts.is_some() {
            "project_fts MATCH ?1"
        } else {
            "?1 IS NULL"
        };
        let filter = format!(
            "{from} WHERE p.status IN ('importing', 'ready')
               AND {words}
               AND (?2 IS NULL OR p.kind = ?2)
               AND (?3 IS NULL OR EXISTS (SELECT 1 FROM version v JOIN version_game g ON g.version_id = v.id
                                         WHERE v.project_id = p.id AND g.game = ?3))
               AND (?4 IS NULL OR EXISTS (SELECT 1 FROM version v JOIN version_loader l ON l.version_id = v.id
                                         WHERE v.project_id = p.id AND l.loader = ?4))
               AND (?5 IS NULL OR EXISTS (SELECT 1 FROM project_category c WHERE c.project_id = p.id AND c.category = ?5))"
        );
        let total: i64 = db
            .query_row(
                &format!("SELECT count(*) FROM {filter}"),
                params![fts, s.kind, s.game, s.loader, s.category],
                |r| r.get(0),
            )
            .map_err(sql)?;
        let mut q = db
            .prepare(&format!(
                "SELECT p.id, p.kind, p.slug, p.title, p.summary, p.icon_thumb, p.downloads, p.status, p.updated,
                   (SELECT name FROM project_author a WHERE a.project_id = p.id ORDER BY ord LIMIT 1),
                   (SELECT group_concat(source) FROM project_source s WHERE s.project_id = p.id)
                 FROM {filter} ORDER BY {order} LIMIT ?6 OFFSET ?7"
            ))
            .map_err(sql)?;
        let hits = q
            .query_map(
                params![fts, s.kind, s.game, s.loader, s.category, s.limit, s.offset],
                |r| {
                    let thumb: Option<String> = r.get(5)?;
                    let sources: Option<String> = r.get(10)?;
                    Ok(serde_json::json!({
                        "id": r.get::<_, String>(0)?,
                        "kind": r.get::<_, String>(1)?,
                        "slug": r.get::<_, String>(2)?,
                        "title": r.get::<_, String>(3)?,
                        "summary": r.get::<_, String>(4)?,
                        "iconThumb": thumb.as_deref().map(|t| files.image(t, "image/webp")),
                        "downloads": r.get::<_, i64>(6)?,
                        "status": r.get::<_, String>(7)?,
                        "updated": r.get::<_, i64>(8)?,
                        "author": r.get::<_, Option<String>>(9)?,
                        "sources": sources.map(|s| s.split(',').map(str::to_owned).collect::<Vec<_>>()).unwrap_or_default(),
                    }))
                },
            )
            .map_err(sql)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(sql)?;
        Ok((total, hits))
    }

    /// Kept files by SHA-1 or SHA-256, with their version and project.
    pub fn lookup(&self, hashes: &[String]) -> Result<Vec<serde_json::Value>> {
        let db = self.db();
        let mut out = Vec::new();
        let mut q = db
            .prepare(
                "SELECT f.sha1, f.sha256, f.version_id, v.project_id, f.file_name FROM version_file f
                 JOIN version v ON v.id = f.version_id
                 WHERE f.sha1 = ?1 OR f.sha256 = ?1 LIMIT 1",
            )
            .map_err(sql)?;
        for hash in hashes {
            if let Some(found) = q
                .query_row([hash], |r| {
                    Ok(serde_json::json!({
                        "hash": hash,
                        "sha1": r.get::<_, Option<String>>(0)?,
                        "sha256": r.get::<_, Option<String>>(1)?,
                        "versionId": r.get::<_, String>(2)?,
                        "projectId": r.get::<_, String>(3)?,
                        "fileName": r.get::<_, String>(4)?,
                    }))
                })
                .optional()
                .map_err(sql)?
            {
                out.push(found);
            }
        }
        Ok(out)
    }

    /// Numbers only, fit for a public workflow log.
    pub fn counts(&self) -> Result<serde_json::Value> {
        let db = self.db();
        let one =
            |query: &str| -> Result<i64> { db.query_row(query, [], |r| r.get(0)).map_err(sql) };
        // Failed jobs by kind and our error code: words of our own only.
        let mut failed_by = serde_json::Map::new();
        let mut query = db
            .prepare(
                "SELECT kind, coalesce(error, ''), count(*) FROM job WHERE state = 'failed'
                 GROUP BY 1, 2 ORDER BY 3 DESC LIMIT 40",
            )
            .map_err(sql)?;
        let rows = query
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, i64>(2)?,
                ))
            })
            .map_err(sql)?;
        for row in rows {
            let (kind, error, count) = row.map_err(sql)?;
            let word = |w: &str| {
                if !w.is_empty()
                    && w.len() <= 40
                    && w.bytes().all(|b| b.is_ascii_lowercase() || b == b'_')
                {
                    w.to_owned()
                } else {
                    "other".to_owned()
                }
            };
            failed_by.insert(format!("{}:{}", word(&kind), word(&error)), count.into());
        }
        Ok(serde_json::json!({
            "projects": one("SELECT count(*) FROM project")?,
            "projectsReady": one("SELECT count(*) FROM project WHERE status = 'ready'")?,
            "versions": one("SELECT count(*) FROM version")?,
            "versionsKept": one("SELECT count(*) FROM version WHERE files_state = 'all'")?,
            "blobs": one("SELECT count(*) FROM blob")?,
            "blobsGb": one("SELECT coalesce(sum(size), 0) FROM blob")? as f64 / crate::blobs::GB as f64,
            "requests": one("SELECT count(*) FROM import_request")?,
            "requestsFailed": one("SELECT count(*) FROM import_request WHERE state = 'failed'")?,
            "jobsWaiting": one("SELECT count(*) FROM job WHERE state IN ('queued', 'running')")?,
            "jobsFailed": one("SELECT count(*) FROM job WHERE state = 'failed'")?,
            "jobsFailedBy": failed_by,
            "blobsHeld": one("SELECT count(*) FROM blob WHERE state = 'quarantined'")?,
            "blobsNoted": one("SELECT count(*) FROM blob WHERE verdict = 'note'")?,
            "blobsToRescan": one(&format!(
                "SELECT count(*) FROM blob WHERE scanned < {} AND state = 'stored' AND allowed = 0
                 AND media_type IN ('application/java-archive', 'application/zip')",
                crate::scan::RULES
            ))?,
            "reviewOpen": one("SELECT count(*) FROM review WHERE decided IS NULL")?,
            "blocked": one("SELECT count(*) FROM blocked")?,
            "blobsForgotten": one("SELECT count(*) FROM blob WHERE state = 'forgotten'")?,
            "projectsRemoved": one("SELECT count(*) FROM project WHERE status = 'removed'")?,
            "ownPacks": one("SELECT count(*) FROM own_pack")?,
            "ownVersions": one("SELECT count(*) FROM own_version")?,
            "reports": one("SELECT count(*) FROM report")?,
            "blobsInR2": one("SELECT count(*) FROM blob WHERE copy = 'public'")?,
            "blobsHeldInR2": one("SELECT count(*) FROM blob WHERE copy = 'private'")?,
            "blobsToCopy": one(
                "SELECT count(*) FROM blob WHERE media_type NOT LIKE 'image/%' AND (
                     (state = 'stored' AND copy != 'public')
                     OR (state != 'stored' AND copy = 'public')
                     OR (state = 'quarantined' AND copy = '')
                     OR (state = 'forgotten' AND copy = 'private'))"
            )?,
            "r2Checked": one("SELECT coalesce(max(value), 0) FROM upkeep WHERE key = 'r2_checked'")?,
            "r2Mismatched": one("SELECT coalesce(max(value), 0) FROM upkeep WHERE key = 'r2_mismatched'")?,
            "databaseCopied": one("SELECT coalesce(max(value), 0) FROM upkeep WHERE key = 'database_copied'")?,
        }))
    }

    /// A copy of the database in `dir`; the newest `keep` copies stay.
    pub fn backup(&self, dir: &Path, keep: usize) -> Result<PathBuf> {
        std::fs::create_dir_all(dir).map_err(|e| format!("creating {}: {e}", dir.display()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
                .map_err(|e| e.to_string())?;
        }
        let path = dir.join(format!(
            "catalog-{}.db",
            chrono::Utc::now().format("%Y%m%d-%H%M%S-%3f")
        ));
        self.db()
            .execute("VACUUM INTO ?1", [path.to_string_lossy()])
            .map_err(|e| format!("copying the database: {e}"))?;
        let mut copies: Vec<PathBuf> = std::fs::read_dir(dir)
            .map_err(|e| e.to_string())?
            .flatten()
            .map(|entry| entry.path())
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with("catalog-") && n.ends_with(".db"))
            })
            .collect();
        copies.sort();
        while copies.len() > keep {
            let _ = std::fs::remove_file(copies.remove(0));
        }
        Ok(path)
    }
}
