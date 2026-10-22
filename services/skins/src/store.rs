//! Skins and capes in SQLite (`data/skins.db`). Accounts are only IDs here;
//! nicks come from the account service. A texture is kept once, by its
//! SHA-256, while someone wears it or keeps it in a wardrobe; the last one to
//! let it go takes it away. Every query takes its values as parameters.

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
-- The wardrobe: skins and capes an account keeps to wear again, by a random
-- ID; one item per picture and kind. `worn` is when it was last put on (0:
-- never), `favourite` 0 or 1.
CREATE TABLE IF NOT EXISTS items (
    id TEXT PRIMARY KEY,
    account TEXT NOT NULL,
    kind TEXT NOT NULL,
    hash TEXT NOT NULL,
    model TEXT,
    frames INTEGER NOT NULL,
    frame_ms TEXT NOT NULL,
    name TEXT NOT NULL,
    favourite INTEGER NOT NULL,
    created INTEGER NOT NULL,
    worn INTEGER NOT NULL,
    UNIQUE (account, kind, hash)
);
CREATE INDEX IF NOT EXISTS items_hash ON items(hash);
-- Sets: a skin and a cape worn together, as they were saved (either may be
-- none), whatever happens to the items since.
CREATE TABLE IF NOT EXISTS sets (
    id TEXT PRIMARY KEY,
    account TEXT NOT NULL,
    name TEXT NOT NULL,
    favourite INTEGER NOT NULL,
    created INTEGER NOT NULL,
    worn INTEGER NOT NULL,
    skin_hash TEXT,
    skin_model TEXT,
    skin_frames INTEGER,
    skin_frame_ms TEXT,
    cape_hash TEXT,
    cape_frames INTEGER,
    cape_frame_ms TEXT
);
CREATE INDEX IF NOT EXISTS sets_account ON sets(account);
CREATE INDEX IF NOT EXISTS sets_skin ON sets(skin_hash);
CREATE INDEX IF NOT EXISTS sets_cape ON sets(cape_hash);
"#;

/// Most skins, capes and sets in one wardrobe.
pub const MAX_SKINS: i64 = 30;
pub const MAX_CAPES: i64 = 10;
pub const MAX_SETS: i64 = 20;

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

/// A skin or cape in a wardrobe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub id: String,
    pub texture: Worn,
    pub name: String,
    pub favourite: bool,
    pub created: i64,
    /// When it was last worn; 0 if never.
    pub worn: i64,
}

/// A skin and a cape kept to be worn together.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Set {
    pub id: String,
    pub name: String,
    pub favourite: bool,
    pub created: i64,
    pub worn: i64,
    pub skin: Option<Worn>,
    pub cape: Option<Worn>,
}

/// Why something was not kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refused {
    /// The wardrobe has as many of the kind as it may.
    Full,
    /// Nothing is worn to keep.
    NothingWorn,
}

/// What a change to the wardrobe found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kept {
    /// The item's or set's ID.
    Yes(String),
    No(Refused),
}

/// What an item or set is changed to; `None` keeps it.
#[derive(Debug, Clone, Default)]
pub struct Change<'a> {
    pub name: Option<&'a str>,
    pub favourite: Option<bool>,
}

pub struct Store {
    db: Mutex<Connection>,
}

type Result<T> = std::result::Result<T, String>;

fn err(e: rusqlite::Error) -> String {
    e.to_string()
}

/// Removes a texture nobody wears or keeps any more.
fn drop_unworn(db: &Connection, hash: &str) -> Result<()> {
    db.execute(
        "DELETE FROM textures WHERE hash = ?1
         AND NOT EXISTS (SELECT 1 FROM wearing WHERE hash = ?1)
         AND NOT EXISTS (SELECT 1 FROM items WHERE hash = ?1)
         AND NOT EXISTS (SELECT 1 FROM sets WHERE skin_hash = ?1 OR cape_hash = ?1)",
        [hash],
    )
    .map(|_| ())
    .map_err(err)
}

/// A new item's or set's ID: 32 random hex digits.
fn new_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

/// `default` or `slim` for a skin, none for a cape.
fn model_of(texture: &Texture) -> Option<&'static str> {
    (texture.kind == Kind::Skin).then_some(if texture.slim { "slim" } else { "default" })
}

/// Keeps a texture's PNG (once, by its hash).
fn keep_png(db: &Connection, texture: &Texture, now: i64) -> Result<()> {
    db.execute(
        "INSERT OR IGNORE INTO textures (hash, png, kind, width, height, created)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            texture.hash,
            texture.png,
            texture.kind.as_str(),
            texture.width,
            texture.height,
            now
        ],
    )
    .map(|_| ())
    .map_err(err)
}

/// The account wears this from now on; what it wore of the kind before goes
/// if nothing else names it. `frame_ms` is the JSON list.
#[allow(clippy::too_many_arguments)]
fn put_on(
    db: &Connection,
    account: &str,
    kind: Kind,
    hash: &str,
    model: Option<&str>,
    frames: u32,
    frame_ms: &str,
    now: i64,
) -> Result<()> {
    let before: Option<String> = db
        .query_row(
            "SELECT hash FROM wearing WHERE account = ?1 AND kind = ?2",
            params![account, kind.as_str()],
            |r| r.get(0),
        )
        .optional()
        .map_err(err)?;
    db.execute(
        "INSERT INTO wearing (account, kind, hash, model, frames, frame_ms, updated)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT (account, kind) DO UPDATE SET hash = ?3, model = ?4,
             frames = ?5, frame_ms = ?6, updated = ?7",
        params![account, kind.as_str(), hash, model, frames, frame_ms, now],
    )
    .map_err(err)?;
    if let Some(before) = before.filter(|h| h != hash) {
        drop_unworn(db, &before)?;
    }
    Ok(())
}

/// Takes off what the account wears of the kind; the texture goes if
/// nothing else names it.
fn take_off_one(db: &Connection, account: &str, kind: Kind) -> Result<()> {
    let hash: Option<String> = db
        .query_row(
            "DELETE FROM wearing WHERE account = ?1 AND kind = ?2 RETURNING hash",
            params![account, kind.as_str()],
            |r| r.get(0),
        )
        .optional()
        .map_err(err)?;
    if let Some(hash) = hash {
        drop_unworn(db, &hash)?;
    }
    Ok(())
}

/// One worn texture of the account, read from `wearing`.
fn worn_one(db: &Connection, account: &str, kind: Kind) -> Result<Option<Worn>> {
    db.query_row(
        "SELECT w.hash, w.model, t.width, t.height, w.frames, w.frame_ms, w.updated
         FROM wearing w JOIN textures t ON t.hash = w.hash
         WHERE w.account = ?1 AND w.kind = ?2",
        params![account, kind.as_str()],
        |r| {
            let frame_ms: String = r.get(5)?;
            Ok(Worn {
                kind,
                hash: r.get(0)?,
                model: r.get(1)?,
                width: r.get(2)?,
                height: r.get(3)?,
                frames: r.get(4)?,
                frame_ms: serde_json::from_str(&frame_ms).unwrap_or_default(),
                updated: r.get(6)?,
            })
        },
    )
    .optional()
    .map_err(err)
}

/// How many of the kind a wardrobe may keep.
fn max_items(kind: Kind) -> i64 {
    match kind {
        Kind::Skin => MAX_SKINS,
        Kind::Cape => MAX_CAPES,
    }
}

/// Keeps a texture in the account's wardrobe, or finds it there already (its
/// arms and durations then become these). Its PNG must be kept already.
#[allow(clippy::too_many_arguments)]
fn keep_item(
    db: &Connection,
    account: &str,
    kind: Kind,
    hash: &str,
    model: Option<&str>,
    frames: u32,
    frame_ms: &str,
    name: &str,
    now: i64,
) -> Result<Kept> {
    let found: Option<String> = db
        .query_row(
            "UPDATE items SET model = ?4, frames = ?5, frame_ms = ?6
             WHERE account = ?1 AND kind = ?2 AND hash = ?3 RETURNING id",
            params![account, kind.as_str(), hash, model, frames, frame_ms],
            |r| r.get(0),
        )
        .optional()
        .map_err(err)?;
    if let Some(id) = found {
        return Ok(Kept::Yes(id));
    }
    let count: i64 = db
        .query_row(
            "SELECT COUNT(*) FROM items WHERE account = ?1 AND kind = ?2",
            params![account, kind.as_str()],
            |r| r.get(0),
        )
        .map_err(err)?;
    if count >= max_items(kind) {
        return Ok(Kept::No(Refused::Full));
    }
    let id = new_id();
    db.execute(
        "INSERT INTO items (id, account, kind, hash, model, frames, frame_ms, name,
             favourite, created, worn)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 0, ?9, 0)",
        params![
            id,
            account,
            kind.as_str(),
            hash,
            model,
            frames,
            frame_ms,
            name,
            now
        ],
    )
    .map_err(err)?;
    Ok(Kept::Yes(id))
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
    /// goes if nothing else names it.
    pub fn wear(&self, account: &str, texture: &Texture, now: i64) -> Result<()> {
        let mut db = self.db();
        let tx = db.transaction().map_err(err)?;
        keep_png(&tx, texture, now)?;
        let frame_ms = serde_json::to_string(&texture.frame_ms).map_err(|e| e.to_string())?;
        put_on(
            &tx,
            account,
            texture.kind,
            &texture.hash,
            model_of(texture),
            texture.frames,
            &frame_ms,
            now,
        )?;
        tx.commit().map_err(err)
    }

    /// How many items of the kind the account keeps.
    pub fn items_of(&self, account: &str, kind: Kind) -> Result<i64> {
        self.db()
            .query_row(
                "SELECT COUNT(*) FROM items WHERE account = ?1 AND kind = ?2",
                params![account, kind.as_str()],
                |r| r.get(0),
            )
            .map_err(err)
    }

    /// Keeps an uploaded texture in the account's wardrobe without wearing it.
    pub fn add_item(&self, account: &str, texture: &Texture, name: &str, now: i64) -> Result<Kept> {
        let mut db = self.db();
        let tx = db.transaction().map_err(err)?;
        keep_png(&tx, texture, now)?;
        let frame_ms = serde_json::to_string(&texture.frame_ms).map_err(|e| e.to_string())?;
        let kept = keep_item(
            &tx,
            account,
            texture.kind,
            &texture.hash,
            model_of(texture),
            texture.frames,
            &frame_ms,
            name,
            now,
        )?;
        if kept == Kept::No(Refused::Full) {
            // The PNG kept above goes again if nothing names it.
            drop_unworn(&tx, &texture.hash)?;
        }
        tx.commit().map_err(err)?;
        Ok(kept)
    }

    /// Keeps what the account wears of the kind in its wardrobe.
    pub fn save_worn(&self, account: &str, kind: Kind, name: &str, now: i64) -> Result<Kept> {
        let mut db = self.db();
        let tx = db.transaction().map_err(err)?;
        let Some(worn) = worn_one(&tx, account, kind)? else {
            return Ok(Kept::No(Refused::NothingWorn));
        };
        let frame_ms = serde_json::to_string(&worn.frame_ms).map_err(|e| e.to_string())?;
        let kept = keep_item(
            &tx,
            account,
            kind,
            &worn.hash,
            worn.model.as_deref(),
            worn.frames,
            &frame_ms,
            name,
            now,
        )?;
        tx.commit().map_err(err)?;
        Ok(kept)
    }

    /// Renames an item or marks it; whether the account has it.
    pub fn change_item(&self, account: &str, id: &str, change: &Change<'_>) -> Result<bool> {
        self.db()
            .execute(
                "UPDATE items SET name = COALESCE(?3, name),
                     favourite = COALESCE(?4, favourite)
                 WHERE account = ?1 AND id = ?2",
                params![account, id, change.name, change.favourite.map(i64::from)],
            )
            .map(|n| n > 0)
            .map_err(err)
    }

    /// Takes an item out of the wardrobe (it stays on, if worn); whether the
    /// account had it.
    pub fn delete_item(&self, account: &str, id: &str) -> Result<bool> {
        let mut db = self.db();
        let tx = db.transaction().map_err(err)?;
        let hash: Option<String> = tx
            .query_row(
                "DELETE FROM items WHERE account = ?1 AND id = ?2 RETURNING hash",
                params![account, id],
                |r| r.get(0),
            )
            .optional()
            .map_err(err)?;
        if let Some(hash) = &hash {
            drop_unworn(&tx, hash)?;
        }
        tx.commit().map_err(err)?;
        Ok(hash.is_some())
    }

    /// Wears an item of the wardrobe; whether the account has it.
    pub fn wear_item(&self, account: &str, id: &str, now: i64) -> Result<bool> {
        let mut db = self.db();
        let tx = db.transaction().map_err(err)?;
        let item: Option<(String, String, Option<String>, u32, String)> = tx
            .query_row(
                "UPDATE items SET worn = ?3 WHERE account = ?1 AND id = ?2
                 RETURNING kind, hash, model, frames, frame_ms",
                params![account, id, now],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .optional()
            .map_err(err)?;
        let Some((kind, hash, model, frames, frame_ms)) = item else {
            return Ok(false);
        };
        let kind = Kind::parse(&kind).ok_or("an item of no kind")?;
        put_on(
            &tx,
            account,
            kind,
            &hash,
            model.as_deref(),
            frames,
            &frame_ms,
            now,
        )?;
        tx.commit().map_err(err)?;
        Ok(true)
    }

    /// Keeps what the account wears, skin and cape, as a set.
    pub fn save_set(&self, account: &str, name: &str, now: i64) -> Result<Kept> {
        let mut db = self.db();
        let tx = db.transaction().map_err(err)?;
        let skin = worn_one(&tx, account, Kind::Skin)?;
        let cape = worn_one(&tx, account, Kind::Cape)?;
        if skin.is_none() && cape.is_none() {
            return Ok(Kept::No(Refused::NothingWorn));
        }
        let count: i64 = tx
            .query_row(
                "SELECT COUNT(*) FROM sets WHERE account = ?1",
                [account],
                |r| r.get(0),
            )
            .map_err(err)?;
        if count >= MAX_SETS {
            return Ok(Kept::No(Refused::Full));
        }
        let list = |w: &Option<Worn>| -> Result<Option<String>> {
            w.as_ref()
                .map(|w| serde_json::to_string(&w.frame_ms).map_err(|e| e.to_string()))
                .transpose()
        };
        let id = new_id();
        tx.execute(
            "INSERT INTO sets (id, account, name, favourite, created, worn,
                 skin_hash, skin_model, skin_frames, skin_frame_ms,
                 cape_hash, cape_frames, cape_frame_ms)
             VALUES (?1, ?2, ?3, 0, ?4, 0, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                id,
                account,
                name,
                now,
                skin.as_ref().map(|w| &w.hash),
                skin.as_ref().and_then(|w| w.model.as_ref()),
                skin.as_ref().map(|w| w.frames),
                list(&skin)?,
                cape.as_ref().map(|w| &w.hash),
                cape.as_ref().map(|w| w.frames),
                list(&cape)?,
            ],
        )
        .map_err(err)?;
        tx.commit().map_err(err)?;
        Ok(Kept::Yes(id))
    }

    /// Renames a set or marks it; whether the account has it.
    pub fn change_set(&self, account: &str, id: &str, change: &Change<'_>) -> Result<bool> {
        self.db()
            .execute(
                "UPDATE sets SET name = COALESCE(?3, name),
                     favourite = COALESCE(?4, favourite)
                 WHERE account = ?1 AND id = ?2",
                params![account, id, change.name, change.favourite.map(i64::from)],
            )
            .map(|n| n > 0)
            .map_err(err)
    }

    /// Deletes a set; whether the account had it.
    pub fn delete_set(&self, account: &str, id: &str) -> Result<bool> {
        let mut db = self.db();
        let tx = db.transaction().map_err(err)?;
        let hashes: Option<(Option<String>, Option<String>)> = tx
            .query_row(
                "DELETE FROM sets WHERE account = ?1 AND id = ?2 RETURNING skin_hash, cape_hash",
                params![account, id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(err)?;
        let Some((skin, cape)) = hashes else {
            return Ok(false);
        };
        for hash in [skin, cape].into_iter().flatten() {
            drop_unworn(&tx, &hash)?;
        }
        tx.commit().map_err(err)?;
        Ok(true)
    }

    /// Wears a set as it was saved: its skin and its cape, or none of a kind
    /// it has none of. Whether the account has it.
    pub fn wear_set(&self, account: &str, id: &str, now: i64) -> Result<bool> {
        let mut db = self.db();
        let tx = db.transaction().map_err(err)?;
        let Some(set) = read_set(&tx, account, id)? else {
            return Ok(false);
        };
        tx.execute(
            "UPDATE sets SET worn = ?3 WHERE account = ?1 AND id = ?2",
            params![account, id, now],
        )
        .map_err(err)?;
        for (kind, look) in [(Kind::Skin, &set.skin), (Kind::Cape, &set.cape)] {
            match look {
                Some(w) => {
                    let frame_ms = serde_json::to_string(&w.frame_ms).map_err(|e| e.to_string())?;
                    put_on(
                        &tx,
                        account,
                        kind,
                        &w.hash,
                        w.model.as_deref(),
                        w.frames,
                        &frame_ms,
                        now,
                    )?;
                }
                None => take_off_one(&tx, account, kind)?,
            }
        }
        tx.commit().map_err(err)?;
        Ok(true)
    }

    /// The account's wardrobe, newest first.
    pub fn items(&self, account: &str) -> Result<Vec<Item>> {
        let db = self.db();
        let mut statement = db
            .prepare(
                "SELECT i.id, i.kind, i.hash, i.model, t.width, t.height, i.frames,
                     i.frame_ms, i.name, i.favourite, i.created, i.worn
                 FROM items i JOIN textures t ON t.hash = i.hash
                 WHERE i.account = ?1 ORDER BY i.created DESC, i.id",
            )
            .map_err(err)?;
        let rows: Vec<(String, Item)> = statement
            .query_map([account], |r| {
                let frame_ms: String = r.get(7)?;
                Ok((
                    r.get::<_, String>(1)?,
                    Item {
                        id: r.get(0)?,
                        texture: Worn {
                            kind: Kind::Skin,
                            hash: r.get(2)?,
                            model: r.get(3)?,
                            width: r.get(4)?,
                            height: r.get(5)?,
                            frames: r.get(6)?,
                            frame_ms: serde_json::from_str(&frame_ms).unwrap_or_default(),
                            updated: r.get(10)?,
                        },
                        name: r.get(8)?,
                        favourite: r.get::<_, i64>(9)? != 0,
                        created: r.get(10)?,
                        worn: r.get(11)?,
                    },
                ))
            })
            .and_then(Iterator::collect)
            .map_err(err)?;
        Ok(rows
            .into_iter()
            .filter_map(|(kind, mut item)| {
                item.texture.kind = Kind::parse(&kind)?;
                Some(item)
            })
            .collect())
    }

    /// The account's sets, newest first.
    pub fn sets(&self, account: &str) -> Result<Vec<Set>> {
        let db = self.db();
        let ids: Vec<String> = {
            let mut statement = db
                .prepare("SELECT id FROM sets WHERE account = ?1 ORDER BY created DESC, id")
                .map_err(err)?;
            statement
                .query_map([account], |r| r.get(0))
                .and_then(Iterator::collect)
                .map_err(err)?
        };
        let mut sets = Vec::with_capacity(ids.len());
        for id in ids {
            if let Some(set) = read_set(&db, account, &id)? {
                sets.push(set);
            }
        }
        Ok(sets)
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

    /// Everything about a deleted account goes: what it wears, its wardrobe
    /// and its sets.
    pub fn purge(&self, account: &str) -> Result<bool> {
        let had = self.forget(account, None)?;
        Ok(self.take_off(account, None)? || had)
    }

    /// The owner's removal: takes off what the account wears of the kind (or
    /// of both) and forgets those pictures in its wardrobe and sets too, so
    /// they cannot simply be put on again. Whether it wore anything.
    pub fn remove(&self, account: &str, kind: Option<Kind>) -> Result<bool> {
        let hashes: Vec<String> = self
            .worn(account)?
            .into_iter()
            .filter(|w| kind.is_none_or(|k| w.kind == k))
            .map(|w| w.hash)
            .collect();
        {
            let mut db = self.db();
            let tx = db.transaction().map_err(err)?;
            for hash in &hashes {
                tx.execute(
                    "DELETE FROM items WHERE account = ?1 AND hash = ?2",
                    params![account, hash],
                )
                .map_err(err)?;
                tx.execute(
                    "UPDATE sets SET skin_hash = NULL, skin_model = NULL, skin_frames = NULL,
                         skin_frame_ms = NULL WHERE account = ?1 AND skin_hash = ?2",
                    params![account, hash],
                )
                .map_err(err)?;
                tx.execute(
                    "UPDATE sets SET cape_hash = NULL, cape_frames = NULL, cape_frame_ms = NULL
                     WHERE account = ?1 AND cape_hash = ?2",
                    params![account, hash],
                )
                .map_err(err)?;
            }
            tx.execute(
                "DELETE FROM sets WHERE account = ?1 AND skin_hash IS NULL AND cape_hash IS NULL",
                [account],
            )
            .map_err(err)?;
            tx.commit().map_err(err)?;
        }
        self.take_off(account, kind)
    }

    /// Deletes the account's wardrobe items (of the kind, or all) and, for
    /// all, its sets; their textures go if nothing else names them.
    fn forget(&self, account: &str, kind: Option<Kind>) -> Result<bool> {
        let mut db = self.db();
        let tx = db.transaction().map_err(err)?;
        let mut hashes: Vec<String> = {
            let mut statement = tx
                .prepare(
                    "DELETE FROM items WHERE account = ?1 AND (?2 IS NULL OR kind = ?2)
                     RETURNING hash",
                )
                .map_err(err)?;
            statement
                .query_map(params![account, kind.map(Kind::as_str)], |r| r.get(0))
                .and_then(Iterator::collect)
                .map_err(err)?
        };
        if kind.is_none() {
            let mut statement = tx
                .prepare("DELETE FROM sets WHERE account = ?1 RETURNING skin_hash, cape_hash")
                .map_err(err)?;
            let pairs: Vec<(Option<String>, Option<String>)> = statement
                .query_map([account], |r| Ok((r.get(0)?, r.get(1)?)))
                .and_then(Iterator::collect)
                .map_err(err)?;
            drop(statement);
            hashes.extend(pairs.into_iter().flat_map(|(s, c)| [s, c]).flatten());
        }
        let had = !hashes.is_empty();
        hashes.sort();
        hashes.dedup();
        for hash in &hashes {
            drop_unworn(&tx, hash)?;
        }
        tx.commit().map_err(err)?;
        Ok(had)
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
            "wardrobes": one("SELECT COUNT(DISTINCT account) FROM items")?,
            "items": one("SELECT COUNT(*) FROM items")?,
            "sets": one("SELECT COUNT(*) FROM sets")?,
        }))
    }
}

/// A set of the account, with the sizes of its textures.
fn read_set(db: &Connection, account: &str, id: &str) -> Result<Option<Set>> {
    let row = db
        .query_row(
            "SELECT s.id, s.name, s.favourite, s.created, s.worn,
                 s.skin_hash, s.skin_model, s.skin_frames, s.skin_frame_ms,
                 sk.width, sk.height,
                 s.cape_hash, s.cape_frames, s.cape_frame_ms, ca.width, ca.height
             FROM sets s
             LEFT JOIN textures sk ON sk.hash = s.skin_hash
             LEFT JOIN textures ca ON ca.hash = s.cape_hash
             WHERE s.account = ?1 AND s.id = ?2",
            params![account, id],
            |r| {
                let created: i64 = r.get(3)?;
                let look = |kind: Kind,
                            hash: Option<String>,
                            model: Option<String>,
                            frames: Option<u32>,
                            frame_ms: Option<String>,
                            width: Option<u32>,
                            height: Option<u32>| {
                    Some(Worn {
                        kind,
                        hash: hash?,
                        model,
                        width: width?,
                        height: height?,
                        frames: frames?,
                        frame_ms: serde_json::from_str(&frame_ms?).unwrap_or_default(),
                        updated: created,
                    })
                };
                Ok(Set {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    favourite: r.get::<_, i64>(2)? != 0,
                    created,
                    worn: r.get(4)?,
                    skin: look(
                        Kind::Skin,
                        r.get(5)?,
                        r.get(6)?,
                        r.get(7)?,
                        r.get(8)?,
                        r.get(9)?,
                        r.get(10)?,
                    ),
                    cape: look(
                        Kind::Cape,
                        r.get(11)?,
                        None,
                        r.get(12)?,
                        r.get(13)?,
                        r.get(14)?,
                        r.get(15)?,
                    ),
                })
            },
        )
        .optional()
        .map_err(err)?;
    Ok(row)
}
