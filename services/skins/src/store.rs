//! Skins and capes in SQLite (`data/skins.db`). Accounts are only IDs here;
//! nicks come from the account service. A texture is kept once, by its
//! SHA-256, while someone wears it; the last one to stop wearing it takes it
//! away. Every query takes its values as parameters.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use rusqlite::{Connection, OptionalExtension, params};

use crate::texture::{Kind, Texture};

const SCHEMA: &str = r#"
PRAGMA journal_mode = WAL;
-- Our own PNGs (re-encoded on upload), by the SHA-256 of the file.
CREATE TABLE IF NOT EXISTS textures (
    hash TEXT PRIMARY KEY,
    png BLOB NOT NULL,
    kind TEXT NOT NULL,
    width INTEGER NOT NULL,
    height INTEGER NOT NULL,
    created INTEGER NOT NULL
);
-- What each account wears: a skin and a cape at most. `model` is for skins
-- (default or slim); `frame_ms` is a JSON list, one per frame (empty for
-- one frame).
CREATE TABLE IF NOT EXISTS wearing (
    account TEXT NOT NULL,
    kind TEXT NOT NULL,
    hash TEXT NOT NULL,
    model TEXT,
    frames INTEGER NOT NULL,
    frame_ms TEXT NOT NULL,
    updated INTEGER NOT NULL,
    PRIMARY KEY (account, kind)
);
CREATE INDEX IF NOT EXISTS wearing_hash ON wearing(hash);
"#;

/// A texture an account wears.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Worn {
    pub kind: Kind,
    pub hash: String,
    /// `default` or `slim` for a skin; none for a cape.
    pub model: Option<String>,
    pub width: u32,
    pub height: u32,
    pub frames: u32,
    pub frame_ms: Vec<u32>,
    pub updated: i64,
}

pub struct Store {
    db: Mutex<Connection>,
}

type Result<T> = std::result::Result<T, String>;

fn err(e: rusqlite::Error) -> String {
    e.to_string()
}

/// Removes a texture nobody wears any more.
fn drop_unworn(db: &Connection, hash: &str) -> Result<()> {
    db.execute(
        "DELETE FROM textures WHERE hash = ?1
         AND NOT EXISTS (SELECT 1 FROM wearing WHERE hash = ?1)",
        [hash],
    )
    .map(|_| ())
    .map_err(err)
}

impl Store {
    pub fn open(dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(dir).map_err(|e| format!("creating {}: {e}", dir.display()))?;
        let path = dir.join("skins.db");
        let db = Connection::open(&path).map_err(|e| format!("opening {}: {e}", path.display()))?;
        db.busy_timeout(std::time::Duration::from_secs(5))
            .map_err(err)?;
        db.execute_batch(SCHEMA).map_err(err)?;
        Ok(Self { db: Mutex::new(db) })
    }

    /// A consistent copy of the database (`VACUUM INTO`, safe while the
    /// service runs) as `skins-<time>.db` in `dir`, readable by this user
    /// alone; only the newest `keep` copies stay.
    pub fn backup(&self, dir: &Path, keep: usize) -> Result<PathBuf> {
        std::fs::create_dir_all(dir).map_err(|e| format!("creating {}: {e}", dir.display()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
                .map_err(|e| e.to_string())?;
        }
        let name = format!(
            "skins-{}.db",
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
                    .is_some_and(|n| n.starts_with("skins-") && n.ends_with(".db"))
            })
            .collect();
        copies.sort();
        let extra = copies.len().saturating_sub(keep);
        for old in &copies[..extra] {
            std::fs::remove_file(old).map_err(|e| e.to_string())?;
        }
        Ok(path)
    }

    fn db(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.db
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// The account wears this texture from now on; the one it wore before
    /// goes if nobody else wears it.
    pub fn wear(&self, account: &str, texture: &Texture, now: i64) -> Result<()> {
        let mut db = self.db();
        let tx = db.transaction().map_err(err)?;
        let kind = texture.kind.as_str();
        let before: Option<String> = tx
            .query_row(
                "SELECT hash FROM wearing WHERE account = ?1 AND kind = ?2",
                params![account, kind],
                |r| r.get(0),
            )
            .optional()
            .map_err(err)?;
        tx.execute(
            "INSERT OR IGNORE INTO textures (hash, png, kind, width, height, created)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                texture.hash,
                texture.png,
                kind,
                texture.width,
                texture.height,
                now
            ],
        )
        .map_err(err)?;
        let model =
            (texture.kind == Kind::Skin).then_some(if texture.slim { "slim" } else { "default" });
        let frame_ms = serde_json::to_string(&texture.frame_ms).map_err(|e| e.to_string())?;
        tx.execute(
            "INSERT INTO wearing (account, kind, hash, model, frames, frame_ms, updated)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT (account, kind) DO UPDATE SET hash = ?3, model = ?4,
                 frames = ?5, frame_ms = ?6, updated = ?7",
            params![
                account,
                kind,
                texture.hash,
                model,
                texture.frames,
                frame_ms,
                now
            ],
        )
        .map_err(err)?;
        if let Some(before) = before.filter(|h| *h != texture.hash) {
            drop_unworn(&tx, &before)?;
        }
        tx.commit().map_err(err)
    }

    /// Takes off what the account wears of this kind (or of both); whether
    /// there was anything.
    pub fn take_off(&self, account: &str, kind: Option<Kind>) -> Result<bool> {
        let mut db = self.db();
        let tx = db.transaction().map_err(err)?;
        let hashes: Vec<String> = {
            let mut statement = tx
                .prepare(
                    "DELETE FROM wearing WHERE account = ?1 AND (?2 IS NULL OR kind = ?2)
                     RETURNING hash",
                )
                .map_err(err)?;
            statement
                .query_map(params![account, kind.map(Kind::as_str)], |r| r.get(0))
                .and_then(Iterator::collect)
                .map_err(err)?
        };
        for hash in &hashes {
            drop_unworn(&tx, hash)?;
        }
        tx.commit().map_err(err)?;
        Ok(!hashes.is_empty())
    }

    /// Everything about a deleted account goes.
    pub fn purge(&self, account: &str) -> Result<bool> {
        self.take_off(account, None)
    }

    /// What the account wears, skin first.
    pub fn worn(&self, account: &str) -> Result<Vec<Worn>> {
        let db = self.db();
        let mut statement = db
            .prepare(
                "SELECT w.kind, w.hash, w.model, t.width, t.height, w.frames, w.frame_ms, w.updated
                 FROM wearing w JOIN textures t ON t.hash = w.hash
                 WHERE w.account = ?1 ORDER BY w.kind DESC",
            )
            .map_err(err)?;
        let rows: Vec<(String, Worn)> = statement
            .query_map([account], |r| {
                let frame_ms: String = r.get(6)?;
                Ok((
                    r.get::<_, String>(0)?,
                    Worn {
                        kind: Kind::Skin,
                        hash: r.get(1)?,
                        model: r.get(2)?,
                        width: r.get(3)?,
                        height: r.get(4)?,
                        frames: r.get(5)?,
                        frame_ms: serde_json::from_str(&frame_ms).unwrap_or_default(),
                        updated: r.get(7)?,
                    },
                ))
            })
            .and_then(Iterator::collect)
            .map_err(err)?;
        Ok(rows
            .into_iter()
            .filter_map(|(kind, worn)| {
                Some(Worn {
                    kind: Kind::parse(&kind)?,
                    ..worn
                })
            })
            .collect())
    }

    /// A kept PNG.
    pub fn texture(&self, hash: &str) -> Result<Option<Vec<u8>>> {
        self.db()
            .query_row("SELECT png FROM textures WHERE hash = ?1", [hash], |r| {
                r.get(0)
            })
            .optional()
            .map_err(err)
    }

    /// Numbers only, fit for a public log.
    pub fn counts(&self) -> Result<serde_json::Value> {
        let db = self.db();
        let one = |sql: &str| -> Result<i64> { db.query_row(sql, [], |r| r.get(0)).map_err(err) };
        Ok(serde_json::json!({
            "accounts": one("SELECT COUNT(DISTINCT account) FROM wearing")?,
            "skins": one("SELECT COUNT(*) FROM wearing WHERE kind = 'skin'")?,
            "slim": one("SELECT COUNT(*) FROM wearing WHERE kind = 'skin' AND model = 'slim'")?,
            "capes": one("SELECT COUNT(*) FROM wearing WHERE kind = 'cape'")?,
            "animated": one("SELECT COUNT(*) FROM wearing WHERE frames > 1")?,
            "hd": one("SELECT COUNT(*) FROM wearing w JOIN textures t ON t.hash = w.hash WHERE t.width > 64")?,
            "textures": one("SELECT COUNT(*) FROM textures")?,
            "textureBytes": one("SELECT COALESCE(SUM(LENGTH(png)), 0) FROM textures")?,
        }))
    }
}
