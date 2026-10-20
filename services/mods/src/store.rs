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
    -- importing, ready, hidden, removed
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
    -- stored, quarantined
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

/// A project due to be read from its source again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Due {
    pub source: String,
    pub source_id: String,
    pub source_slug: String,
    pub kind: String,
    /// Players wanted it lately.
    pub wanted: bool,
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
    /// longest waiting first, or only `project` when given.
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
                "SELECT s.source, s.source_id, s.source_slug, p.kind, coalesce(w.at, 0) >= ?2
                 FROM project p JOIN project_source s ON s.project_id = p.id
                 LEFT JOIN project_wanted w ON w.project_id = p.id
                 WHERE p.status = 'ready' AND p.gone_at IS NULL AND (?5 IS NULL OR p.id = ?5)
                   AND s.fetched <= ?1 - CASE WHEN coalesce(w.at, 0) >= ?2 THEN ?3 ELSE ?4 END
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
                })
            },
        )
        .map_err(sql)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(sql)
    }

    /// Stores a project as its source describes it; returns our ID.
    pub fn upsert_project(&self, p: &SourceProject, now: i64) -> Result<String> {
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
        Ok(id)
    }

    pub fn set_project_status(&self, id: &str, status: &str) -> Result<()> {
        self.db()
            .execute(
                "UPDATE project SET status = ?2 WHERE id = ?1",
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
                let same = kept.iter().find(|k| match &f.sha1 {
                    Some(sha1) => k.sha1.as_ref() == Some(sha1),
                    None => k.name == f.name && k.size == f.size,
                });
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
                    params![id, ord as i64, f.name, f.size, sha1, sha512, sha256, f.primary, f.role, f.url],
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

    /// The blob of a file, if one with these source hashes is kept already.
    pub fn blob_by_hashes(
        &self,
        sha1: Option<&str>,
        sha512: Option<&str>,
    ) -> Result<Option<String>> {
        let db = self.db();
        if let Some(sha512) = sha512
            && let Some(found) = db
                .query_row(
                    "SELECT sha256 FROM blob WHERE sha512 = ?1 AND state = 'stored'",
                    [sha512],
                    |r| r.get(0),
                )
                .optional()
                .map_err(sql)?
        {
            return Ok(Some(found));
        }
        if let Some(sha1) = sha1 {
            return db
                .query_row(
                    "SELECT sha256 FROM blob WHERE sha1 = ?1 AND state = 'stored'",
                    [sha1],
                    |r| r.get(0),
                )
                .optional()
                .map_err(sql);
        }
        Ok(None)
    }

    pub fn add_blob(&self, hashed: &Hashed, media_type: &str, now: i64) -> Result<()> {
        self.db()
            .execute(
                "INSERT OR IGNORE INTO blob (sha256, size, sha1, sha512, media_type, state, stored)
                 VALUES (?1, ?2, ?3, ?4, ?5, 'stored', ?6)",
                params![
                    hashed.sha256,
                    hashed.size as i64,
                    hashed.sha1,
                    hashed.sha512,
                    media_type,
                    now
                ],
            )
            .map(|_| ())
            .map_err(sql)
    }

    /// Links a kept file to its place in a version.
    /// A file is kept; a source that gave no hashes gets ours.
    pub fn file_kept(&self, version: &str, ord: i64, sha256: &str) -> Result<()> {
        let db = self.db();
        db.execute(
            "UPDATE version_file SET sha256 = ?3,
               sha1 = coalesce(sha1, (SELECT sha1 FROM blob WHERE sha256 = ?3)),
               sha512 = coalesce(sha512, (SELECT sha512 FROM blob WHERE sha256 = ?3))
             WHERE version_id = ?1 AND ord = ?2",
            params![version, ord, sha256],
        )
        .map_err(sql)?;
        Self::refresh_files_state(&db, version)
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
        tx.commit().map_err(sql)
    }

    pub fn pack_files(&self, version: &str) -> Result<Vec<PackFileRow>> {
        let db = self.db();
        let mut q = db
            .prepare(
                "SELECT ord, size, sha1, sha512, sha256, origin_url
                 FROM pack_file WHERE version_id = ?1 ORDER BY ord",
            )
            .map_err(sql)?;
        q.query_map([version], |r| {
            Ok(PackFileRow {
                ord: r.get(0)?,
                size: r.get(1)?,
                sha1: r.get(2)?,
                sha512: r.get(3)?,
                sha256: r.get(4)?,
                origin_url: r.get(5)?,
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
                "SELECT path, size, sha256, optional FROM pack_file WHERE version_id = ?1 ORDER BY ord",
            )
            .map_err(sql)?;
        let list: Vec<serde_json::Value> = q
            .query_map([version], |r| {
                let path: String = r.get(0)?;
                let sha256: Option<String> = r.get(2)?;
                let name = path.rsplit('/').next().unwrap_or_default().to_owned();
                Ok(serde_json::json!({
                    "path": path,
                    "size": r.get::<_, i64>(1)?,
                    "sha256": sha256,
                    "url": sha256.as_deref().map(|s| files.file(s, &name)),
                    "optional": r.get::<_, bool>(3)?,
                }))
            })
            .map_err(sql)?
            .collect::<std::result::Result<_, _>>()
            .map_err(sql)?;
        let kept = list.iter().filter(|f| !f["sha256"].is_null()).count();
        pack["total"] = list.len().into();
        pack["kept"] = kept.into();
        pack["files"] = list.into();
        Ok(Some(pack))
    }

    pub fn mark_version_gone(&self, version: &str, now: i64) -> Result<()> {
        self.db()
            .execute(
                "UPDATE version SET gone_at = coalesce(gone_at, ?2) WHERE id = ?1",
                params![version, now],
            )
            .map(|_| ())
            .map_err(sql)
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
                "SELECT id FROM project WHERE (id = ?1 OR slug = ?1) AND status IN ('importing', 'ready')",
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
                    "SELECT file_name, size, sha1, sha512, sha256, is_primary, role, origin_url
                     FROM version_file WHERE version_id = ?1 ORDER BY ord",
                )
                .map_err(sql)?;
            q.query_map([id], |r| {
                let name: String = r.get(0)?;
                let sha256: Option<String> = r.get(4)?;
                Ok(serde_json::json!({
                    "name": name,
                    "size": r.get::<_, i64>(1)?,
                    "sha1": r.get::<_, Option<String>>(2)?,
                    "sha512": r.get::<_, Option<String>>(3)?,
                    "sha256": sha256,
                    "primary": r.get::<_, bool>(5)?,
                    "role": r.get::<_, String>(6)?,
                    "url": sha256.as_deref().map(|s| files.file(s, &name)),
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
