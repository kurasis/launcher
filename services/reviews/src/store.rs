//! Reviews in SQLite (`reviews.db`), their files beside it in
//! `files/<review>/` (text gzip, pictures as re-encoded), and the service's
//! secret (`secret`, 0600) that signs challenges and hashes networks.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use hmac::{Hmac, Mac};
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::model::Checked;

/// Kept this long, in days (D-070).
pub const KEEP_REVIEWS_DAYS: i64 = 180;
pub const KEEP_FILES_DAYS: i64 = 90;
pub const KEEP_SPAM_DAYS: i64 = 14;
/// The network hash goes after this; bans made from it stay.
pub const KEEP_NET_DAYS: i64 = 30;
const DAY: i64 = 86_400;

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS reviews (
    id INTEGER PRIMARY KEY,
    received INTEGER NOT NULL,
    kind TEXT NOT NULL,
    category TEXT NOT NULL,
    subcategory TEXT NOT NULL,
    message TEXT NOT NULL,
    rating INTEGER,
    launcher_version TEXT NOT NULL,
    os TEXT NOT NULL,
    language TEXT NOT NULL,
    install_id TEXT,
    account TEXT,
    key_hash TEXT,
    net TEXT,
    message_hash TEXT NOT NULL,
    diagnostic_id TEXT,
    ad INTEGER,
    status TEXT NOT NULL DEFAULT 'new',
    suspect INTEGER NOT NULL DEFAULT 0,
    reply TEXT,
    replied INTEGER,
    note TEXT,
    bytes INTEGER NOT NULL DEFAULT 0,
    files_gone INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS reviews_received ON reviews(received);
CREATE INDEX IF NOT EXISTS reviews_install ON reviews(install_id);
CREATE INDEX IF NOT EXISTS reviews_account ON reviews(account);
CREATE INDEX IF NOT EXISTS reviews_net ON reviews(net);
CREATE TABLE IF NOT EXISTS files (
    review INTEGER NOT NULL,
    n INTEGER NOT NULL,
    name TEXT NOT NULL,
    kind TEXT NOT NULL,
    stored TEXT NOT NULL,
    bytes INTEGER NOT NULL,
    PRIMARY KEY (review, n)
);
CREATE TABLE IF NOT EXISTS bans (
    id INTEGER PRIMARY KEY,
    kind TEXT NOT NULL,
    value TEXT NOT NULL,
    made INTEGER NOT NULL,
    until INTEGER,
    reason TEXT
);
CREATE TABLE IF NOT EXISTS settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
";

pub const STATUSES: [&str; 5] = ["new", "read", "answered", "closed", "spam"];
pub const BAN_KINDS: [&str; 3] = ["install", "account", "net"];

fn err(e: rusqlite::Error) -> String {
    format!("database: {e}")
}

fn io(e: std::io::Error) -> String {
    format!("files: {e}")
}

#[cfg(unix)]
fn private(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode));
}

#[cfg(not(unix))]
fn private(_: &Path, _: u32) {}

/// Who sent a review, as far as limits and bans need it.
#[derive(Debug, Clone, Default)]
pub struct Sender {
    pub install_id: Option<String>,
    pub account: Option<String>,
    /// [`Store::net`] of the address.
    pub net: String,
}

/// A file ready to be written: its name, `text` or `image`, and bytes.
pub struct Upload {
    pub name: String,
    pub kind: &'static str,
    pub bytes: Vec<u8>,
}

/// One line of `list`.
#[derive(Debug, Serialize)]
pub struct Line {
    pub id: i64,
    pub received: i64,
    pub kind: String,
    pub category: String,
    pub subcategory: String,
    pub status: String,
    pub suspect: bool,
    pub files: i64,
    pub start: String,
}

/// All of one review, for `show`.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Review {
    pub id: i64,
    pub received: i64,
    pub kind: String,
    pub category: String,
    pub subcategory: String,
    pub message: String,
    pub rating: Option<i64>,
    pub launcher_version: String,
    pub os: String,
    pub language: String,
    pub install_id: Option<String>,
    pub account: Option<String>,
    pub diagnostic_id: Option<String>,
    pub ad: Option<i64>,
    pub status: String,
    pub suspect: bool,
    pub reply: Option<String>,
    pub replied: Option<i64>,
    pub note: Option<String>,
    pub files: Vec<FileInfo>,
    pub files_gone: bool,
}

#[derive(Debug, Serialize)]
pub struct FileInfo {
    pub name: String,
    pub kind: String,
    pub bytes: i64,
}

/// What the player sees of their review.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct Status {
    pub id: i64,
    pub received: i64,
    /// `sent`, `read`, `answered`, `closed`, or `gone` (deleted or unknown).
    pub status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reply: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub replied: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct Ban {
    pub id: i64,
    pub kind: String,
    pub made: i64,
    pub until: Option<i64>,
    pub reason: Option<String>,
}

/// Filters of `list`.
#[derive(Debug, Default)]
pub struct Filter<'a> {
    pub status: Option<&'a str>,
    pub kind: Option<&'a str>,
    pub category: Option<&'a str>,
    pub since: Option<i64>,
    /// Suspected spam is left out unless asked for (or `status` is given).
    pub suspect: bool,
    pub limit: i64,
}

pub struct Store {
    db: Mutex<Connection>,
    files: PathBuf,
    secret: [u8; 32],
}

/// SHA-256 of a review's key, as stored.
pub fn key_hash(key: &str) -> String {
    hex::encode(Sha256::digest(key.as_bytes()))
}

fn gzip(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(bytes).map_err(io)?;
    encoder.finish().map_err(io)
}

/// The kind a player sees for a stored status.
fn shown(status: &str) -> &'static str {
    match status {
        "new" => "sent",
        "answered" => "answered",
        "closed" => "closed",
        _ => "read",
    }
}

impl Store {
    pub fn open(data: &Path) -> Result<Self, String> {
        std::fs::create_dir_all(data).map_err(|e| format!("creating {}: {e}", data.display()))?;
        let path = data.join("reviews.db");
        let db = Connection::open(&path).map_err(err)?;
        db.busy_timeout(std::time::Duration::from_secs(10))
            .map_err(err)?;
        db.execute_batch("PRAGMA journal_mode=WAL;").map_err(err)?;
        db.execute_batch(SCHEMA).map_err(err)?;
        private(&path, 0o600);
        let files = data.join("files");
        std::fs::create_dir_all(&files).map_err(io)?;
        private(&files, 0o700);
        let secret_path = data.join("secret");
        let secret = match std::fs::read(&secret_path) {
            Ok(bytes) if bytes.len() == 32 => bytes.try_into().expect("32 bytes"),
            _ => {
                let mut secret = [0u8; 32];
                getrandom::fill(&mut secret).map_err(|e| e.to_string())?;
                let partial = data.join("secret.new");
                std::fs::write(&partial, secret).map_err(io)?;
                private(&partial, 0o600);
                std::fs::rename(&partial, &secret_path).map_err(io)?;
                secret
            }
        };
        Ok(Self {
            db: Mutex::new(db),
            files,
            secret,
        })
    }

    fn db(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.db
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub fn secret(&self) -> [u8; 32] {
        self.secret
    }

    /// A network (an address, or its /64) as it is kept: a keyed hash that
    /// cannot be turned back into the address.
    pub fn net(&self, subject: &str) -> String {
        let mut mac = Hmac::<Sha256>::new_from_slice(&self.secret).expect("any key length");
        mac.update(b"net|");
        mac.update(subject.as_bytes());
        hex::encode(&mac.finalize().into_bytes()[..16])
    }

    pub fn setting(&self, key: &str) -> Result<Option<String>, String> {
        self.db()
            .query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| {
                r.get(0)
            })
            .optional()
            .map_err(err)
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<(), String> {
        self.db()
            .execute(
                "INSERT INTO settings (key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                [key, value],
            )
            .map(|_| ())
            .map_err(err)
    }

    /// Reviews since `since` from this install, account or network.
    pub fn sent_since(&self, column: &str, value: &str, since: i64) -> Result<i64, String> {
        let column = match column {
            "install" => "install_id",
            "account" => "account",
            _ => "net",
        };
        self.db()
            .query_row(
                &format!("SELECT COUNT(*) FROM reviews WHERE {column} = ?1 AND received >= ?2"),
                params![value, since],
                |r| r.get(0),
            )
            .map_err(err)
    }

    pub fn all_since(&self, since: i64) -> Result<i64, String> {
        self.db()
            .query_row(
                "SELECT COUNT(*) FROM reviews WHERE received >= ?1",
                [since],
                |r| r.get(0),
            )
            .map_err(err)
    }

    /// Bytes of the stored files.
    pub fn stored(&self) -> Result<i64, String> {
        self.db()
            .query_row(
                "SELECT COALESCE(SUM(bytes), 0) FROM reviews WHERE files_gone = 0",
                [],
                |r| r.get(0),
            )
            .map_err(err)
    }

    /// Whether a ban in force names this sender.
    pub fn banned(&self, sender: &Sender, now: i64) -> Result<bool, String> {
        let db = self.db();
        let check = |kind: &str, value: &str| -> Result<bool, String> {
            db.query_row(
                "SELECT COUNT(*) FROM bans WHERE kind = ?1 AND value = ?2
                 AND (until IS NULL OR until > ?3)",
                params![kind, value, now],
                |r| r.get::<_, i64>(0),
            )
            .map(|n| n > 0)
            .map_err(err)
        };
        if let Some(install) = &sender.install_id
            && check("install", install)?
        {
            return Ok(true);
        }
        if let Some(account) = &sender.account
            && check("account", account)?
        {
            return Ok(true);
        }
        check("net", &sender.net)
    }

    /// The number the next review would get: what a banned sender is told.
    pub fn next_id(&self) -> Result<i64, String> {
        self.db()
            .query_row("SELECT COALESCE(MAX(id), 0) + 1 FROM reviews", [], |r| {
                r.get(0)
            })
            .map_err(err)
    }

    /// Stores a review and its files; returns its number.
    pub fn add(
        &self,
        review: &Checked,
        sender: &Sender,
        key: &str,
        files: Vec<Upload>,
        now: i64,
    ) -> Result<i64, String> {
        let message_hash = hex::encode(Sha256::digest(review.message.to_lowercase().as_bytes()));
        let mut stored = Vec::new();
        for (n, file) in files.into_iter().enumerate() {
            let bytes = if file.kind == "text" {
                gzip(&file.bytes)?
            } else {
                file.bytes
            };
            stored.push((n as i64 + 1, file.name, file.kind, bytes));
        }
        let total: i64 = stored.iter().map(|f| f.3.len() as i64).sum();
        let mut db = self.db();
        let duplicate: i64 = db
            .query_row(
                "SELECT COUNT(*) FROM reviews WHERE net = ?1 AND message_hash = ?2 AND received >= ?3",
                params![sender.net, message_hash, now - DAY],
                |r| r.get(0),
            )
            .map_err(err)?;
        let suspect = duplicate > 0 || crate::model::looks_like_spam(&review.message);
        let tx = db.transaction().map_err(err)?;
        tx.execute(
            "INSERT INTO reviews (received, kind, category, subcategory, message, rating,
                launcher_version, os, language, install_id, account, key_hash, net, message_hash,
                diagnostic_id, ad, suspect, bytes)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)",
            params![
                now,
                review.kind,
                review.category,
                review.subcategory,
                review.message,
                review.rating,
                review.launcher_version,
                review.os,
                review.language,
                sender.install_id,
                sender.account,
                key_hash(key),
                sender.net,
                message_hash,
                review.diagnostic_id,
                review.ad,
                suspect,
                total,
            ],
        )
        .map_err(err)?;
        let id = tx.last_insert_rowid();
        if !stored.is_empty() {
            let dir = self.files.join(id.to_string());
            // A folder left by a review that was rolled back is not ours.
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).map_err(io)?;
            for (n, name, kind, bytes) in &stored {
                let file = if *kind == "text" {
                    format!("{n}.gz")
                } else {
                    format!("{n}.{}", name.rsplit('.').next().unwrap_or("png"))
                };
                std::fs::write(dir.join(&file), bytes).map_err(io)?;
                tx.execute(
                    "INSERT INTO files (review, n, name, kind, stored, bytes) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    params![id, n, name, kind, file, bytes.len() as i64],
                )
                .map_err(err)?;
            }
        }
        tx.commit().map_err(err)?;
        Ok(id)
    }

    /// Everything kept about an account's reviews (D-064): what it wrote and
    /// sent along, the answers, and the names and sizes of its files. The
    /// owner's notes and the spam marks stay out.
    pub fn export(&self, account: &str) -> Result<Vec<serde_json::Value>, String> {
        let db = self.db();
        let mut statement = db
            .prepare(
                "SELECT id, received, kind, category, subcategory, message, rating,
                   launcher_version, os, language, install_id, diagnostic_id, ad, status,
                   reply, replied
                 FROM reviews WHERE account = ?1 ORDER BY id LIMIT 10000",
            )
            .map_err(err)?;
        let mut reviews: Vec<serde_json::Value> = statement
            .query_map([account], |r| {
                let status: String = r.get(13)?;
                let shown = shown(&status);
                let answered = shown == "answered" || shown == "closed";
                Ok(serde_json::json!({
                    "id": r.get::<_, i64>(0)?,
                    "received": r.get::<_, i64>(1)?,
                    "kind": r.get::<_, String>(2)?,
                    "category": r.get::<_, String>(3)?,
                    "subcategory": r.get::<_, String>(4)?,
                    "message": r.get::<_, String>(5)?,
                    "rating": r.get::<_, Option<i64>>(6)?,
                    "launcherVersion": r.get::<_, String>(7)?,
                    "os": r.get::<_, String>(8)?,
                    "language": r.get::<_, String>(9)?,
                    "installId": r.get::<_, Option<String>>(10)?,
                    "diagnosticId": r.get::<_, Option<String>>(11)?,
                    "ad": r.get::<_, Option<i64>>(12)?,
                    "status": shown,
                    "reply": if answered { r.get::<_, Option<String>>(14)? } else { None },
                    "replied": if answered { r.get::<_, Option<i64>>(15)? } else { None },
                }))
            })
            .map_err(err)?
            .collect::<rusqlite::Result<_>>()
            .map_err(err)?;
        let mut files = db
            .prepare("SELECT name, kind, bytes FROM files WHERE review = ?1 ORDER BY n")
            .map_err(err)?;
        for review in &mut reviews {
            let id = review["id"].as_i64().unwrap_or_default();
            let list: Vec<serde_json::Value> = files
                .query_map([id], |r| {
                    Ok(serde_json::json!({
                        "name": r.get::<_, String>(0)?,
                        "kind": r.get::<_, String>(1)?,
                        "bytes": r.get::<_, i64>(2)?,
                    }))
                })
                .map_err(err)?
                .collect::<rusqlite::Result<_>>()
                .map_err(err)?;
            review["files"] = serde_json::Value::Array(list);
        }
        Ok(reviews)
    }

    /// The player's reviews: those whose key they hold, and those of their
    /// account.
    pub fn statuses(
        &self,
        keys: &[(i64, String)],
        account: Option<&str>,
    ) -> Result<Vec<Status>, String> {
        let db = self.db();
        let row = |r: &rusqlite::Row<'_>| -> rusqlite::Result<Status> {
            let status: String = r.get(2)?;
            let shown = shown(&status);
            Ok(Status {
                id: r.get(0)?,
                received: r.get(1)?,
                status: shown,
                reply: if shown == "answered" || shown == "closed" {
                    r.get(3)?
                } else {
                    None
                },
                replied: if shown == "answered" || shown == "closed" {
                    r.get(4)?
                } else {
                    None
                },
            })
        };
        let mut out = Vec::new();
        for (id, key) in keys {
            let found = db
                .query_row(
                    "SELECT id, received, status, reply, replied FROM reviews WHERE id = ?1 AND key_hash = ?2",
                    params![id, key_hash(key)],
                    row,
                )
                .optional()
                .map_err(err)?;
            out.push(found.unwrap_or(Status {
                id: *id,
                received: 0,
                status: "gone",
                reply: None,
                replied: None,
            }));
        }
        if let Some(account) = account {
            let mut statement = db
                .prepare(
                    "SELECT id, received, status, reply, replied FROM reviews
                     WHERE account = ?1 ORDER BY id DESC LIMIT 100",
                )
                .map_err(err)?;
            for status in statement.query_map([account], row).map_err(err)?.flatten() {
                if !out.iter().any(|s| s.id == status.id) {
                    out.push(status);
                }
            }
        }
        Ok(out)
    }

    /// Deletes a review its sender asked to delete (key or account).
    pub fn delete_own(
        &self,
        id: i64,
        key: Option<&str>,
        account: Option<&str>,
    ) -> Result<bool, String> {
        let found: Option<(Option<String>, Option<String>)> = self
            .db()
            .query_row(
                "SELECT key_hash, account FROM reviews WHERE id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(err)?;
        let Some((hash, owner)) = found else {
            return Ok(false);
        };
        let by_key = key.is_some_and(|k| hash.as_deref() == Some(key_hash(k).as_str()));
        let by_account = account.is_some() && owner.as_deref() == account;
        if !by_key && !by_account {
            return Ok(false);
        }
        self.remove(id)?;
        Ok(true)
    }

    /// Forgets a deleted account (D-064): the reviews it sent stay without
    /// it and without their files (logs and pictures can name the player),
    /// and its ban goes. Returns how many reviews it had.
    pub fn purge(&self, account: &str) -> Result<usize, String> {
        let ids = self.ids("SELECT id FROM reviews WHERE account = ?1", [account])?;
        for id in &ids {
            self.drop_files(*id)?;
        }
        let db = self.db();
        db.execute(
            "UPDATE reviews SET account = NULL WHERE account = ?1",
            [account],
        )
        .map_err(err)?;
        db.execute(
            "DELETE FROM bans WHERE kind = 'account' AND value = ?1",
            [account],
        )
        .map_err(err)?;
        Ok(ids.len())
    }

    /// A review's files, gone for good; the review stays.
    fn drop_files(&self, id: i64) -> Result<(), String> {
        match std::fs::remove_dir_all(self.files.join(id.to_string())) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(io(e)),
            _ => {}
        }
        let db = self.db();
        db.execute("DELETE FROM files WHERE review = ?1", [id])
            .map_err(err)?;
        db.execute("UPDATE reviews SET files_gone = 1 WHERE id = ?1", [id])
            .map_err(err)?;
        Ok(())
    }

    /// A review and its files, gone for good.
    pub fn remove(&self, id: i64) -> Result<(), String> {
        {
            let db = self.db();
            db.execute("DELETE FROM files WHERE review = ?1", [id])
                .map_err(err)?;
            db.execute("DELETE FROM reviews WHERE id = ?1", [id])
                .map_err(err)?;
        }
        match std::fs::remove_dir_all(self.files.join(id.to_string())) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(io(e)),
            _ => Ok(()),
        }
    }

    pub fn list(&self, filter: &Filter<'_>) -> Result<Vec<Line>, String> {
        let mut sql = String::from(
            "SELECT id, received, kind, category, subcategory, status, suspect,
                (SELECT COUNT(*) FROM files WHERE review = reviews.id), substr(message, 1, 60)
             FROM reviews WHERE 1 = 1",
        );
        let mut values: Vec<rusqlite::types::Value> = Vec::new();
        if let Some(status) = filter.status {
            sql.push_str(" AND status = ?");
            values.push(status.to_owned().into());
        } else if !filter.suspect {
            sql.push_str(" AND suspect = 0 AND status != 'spam'");
        }
        if let Some(kind) = filter.kind {
            sql.push_str(" AND kind = ?");
            values.push(kind.to_owned().into());
        }
        if let Some(category) = filter.category {
            sql.push_str(" AND category = ?");
            values.push(category.to_owned().into());
        }
        if let Some(since) = filter.since {
            sql.push_str(" AND received >= ?");
            values.push(since.into());
        }
        sql.push_str(" ORDER BY id DESC LIMIT ?");
        values.push(filter.limit.clamp(1, 1000).into());
        let db = self.db();
        let mut statement = db.prepare(&sql).map_err(err)?;
        let lines = statement
            .query_map(rusqlite::params_from_iter(values), |r| {
                Ok(Line {
                    id: r.get(0)?,
                    received: r.get(1)?,
                    kind: r.get(2)?,
                    category: r.get(3)?,
                    subcategory: r.get(4)?,
                    status: r.get(5)?,
                    suspect: r.get::<_, i64>(6)? != 0,
                    files: r.get(7)?,
                    start: r.get::<_, String>(8)?.replace(['\n', '\t'], " "),
                })
            })
            .map_err(err)?
            .flatten()
            .collect();
        Ok(lines)
    }

    pub fn get(&self, id: i64) -> Result<Option<Review>, String> {
        let db = self.db();
        let review = db
            .query_row(
                "SELECT id, received, kind, category, subcategory, message, rating,
                    launcher_version, os, language, install_id, account,
                    diagnostic_id, ad, status, suspect, reply, replied, note, files_gone
                 FROM reviews WHERE id = ?1",
                [id],
                |r| {
                    Ok(Review {
                        id: r.get(0)?,
                        received: r.get(1)?,
                        kind: r.get(2)?,
                        category: r.get(3)?,
                        subcategory: r.get(4)?,
                        message: r.get(5)?,
                        rating: r.get(6)?,
                        launcher_version: r.get(7)?,
                        os: r.get(8)?,
                        language: r.get(9)?,
                        install_id: r.get(10)?,
                        account: r.get(11)?,
                        diagnostic_id: r.get(12)?,
                        ad: r.get(13)?,
                        status: r.get(14)?,
                        suspect: r.get::<_, i64>(15)? != 0,
                        reply: r.get(16)?,
                        replied: r.get(17)?,
                        note: r.get(18)?,
                        files: Vec::new(),
                        files_gone: r.get::<_, i64>(19)? != 0,
                    })
                },
            )
            .optional()
            .map_err(err)?;
        let Some(mut review) = review else {
            return Ok(None);
        };
        let mut statement = db
            .prepare("SELECT name, kind, bytes FROM files WHERE review = ?1 ORDER BY n")
            .map_err(err)?;
        review.files = statement
            .query_map([id], |r| {
                Ok(FileInfo {
                    name: r.get(0)?,
                    kind: r.get(1)?,
                    bytes: r.get(2)?,
                })
            })
            .map_err(err)?
            .flatten()
            .collect();
        Ok(Some(review))
    }

    /// One file of a review as sent: text unpacked, a picture as stored.
    pub fn file(&self, id: i64, name: &str) -> Result<Option<(String, Vec<u8>)>, String> {
        let found: Option<(String, String)> = self
            .db()
            .query_row(
                "SELECT kind, stored FROM files WHERE review = ?1 AND name = ?2",
                params![id, name],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(err)?;
        let Some((kind, stored)) = found else {
            return Ok(None);
        };
        let bytes = std::fs::read(self.files.join(id.to_string()).join(stored)).map_err(io)?;
        if kind != "text" {
            return Ok(Some((kind, bytes)));
        }
        let mut text = Vec::new();
        flate2::read::GzDecoder::new(bytes.as_slice())
            .take(crate::model::MAX_TEXT_BYTES as u64 + 1)
            .read_to_end(&mut text)
            .map_err(io)?;
        Ok(Some((kind, text)))
    }

    fn update(&self, sql: &str, values: impl rusqlite::Params) -> Result<(), String> {
        match self.db().execute(sql, values).map_err(err)? {
            0 => Err("no such review".to_owned()),
            _ => Ok(()),
        }
    }

    pub fn set_status(&self, id: i64, status: &str) -> Result<(), String> {
        if !STATUSES.contains(&status) {
            return Err(format!("status is one of {}", STATUSES.join(", ")));
        }
        self.update(
            "UPDATE reviews SET status = ?2 WHERE id = ?1",
            params![id, status],
        )
    }

    /// Marks a new review read (the owner opened it).
    pub fn seen(&self, id: i64) -> Result<(), String> {
        self.db()
            .execute(
                "UPDATE reviews SET status = 'read' WHERE id = ?1 AND status = 'new'",
                [id],
            )
            .map(|_| ())
            .map_err(err)
    }

    pub fn reply(&self, id: i64, text: &str, now: i64) -> Result<(), String> {
        self.update(
            "UPDATE reviews SET reply = ?2, replied = ?3, status = 'answered' WHERE id = ?1",
            params![id, text, now],
        )
    }

    pub fn note(&self, id: i64, text: &str) -> Result<(), String> {
        self.update(
            "UPDATE reviews SET note = ?2 WHERE id = ?1",
            params![id, text],
        )
    }

    /// Bans the install, account or network that sent a review; returns the
    /// ban's number.
    pub fn ban(
        &self,
        kind: &str,
        review: i64,
        until: Option<i64>,
        reason: Option<&str>,
        now: i64,
    ) -> Result<i64, String> {
        let column = match kind {
            "install" => "install_id",
            "account" => "account",
            "net" => "net",
            _ => return Err(format!("ban one of {}", BAN_KINDS.join(", "))),
        };
        let db = self.db();
        let value: Option<String> = db
            .query_row(
                &format!("SELECT {column} FROM reviews WHERE id = ?1"),
                [review],
                |r| r.get(0),
            )
            .optional()
            .map_err(err)?
            .ok_or("no such review")?;
        let value = value.ok_or(format!("review {review} has no {kind} (any more)"))?;
        db.execute(
            "INSERT INTO bans (kind, value, made, until, reason) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![kind, value, now, until, reason],
        )
        .map_err(err)?;
        Ok(db.last_insert_rowid())
    }

    pub fn bans(&self, now: i64) -> Result<Vec<Ban>, String> {
        let db = self.db();
        let mut statement = db
            .prepare(
                "SELECT id, kind, made, until, reason FROM bans
                 WHERE until IS NULL OR until > ?1 ORDER BY id",
            )
            .map_err(err)?;
        let bans = statement
            .query_map([now], |r| {
                Ok(Ban {
                    id: r.get(0)?,
                    kind: r.get(1)?,
                    made: r.get(2)?,
                    until: r.get(3)?,
                    reason: r.get(4)?,
                })
            })
            .map_err(err)?
            .flatten()
            .collect();
        Ok(bans)
    }

    pub fn lift(&self, ban: i64) -> Result<(), String> {
        match self
            .db()
            .execute("DELETE FROM bans WHERE id = ?1", [ban])
            .map_err(err)?
        {
            0 => Err("no such ban".to_owned()),
            _ => Ok(()),
        }
    }

    fn ids(&self, sql: &str, values: impl rusqlite::Params) -> Result<Vec<i64>, String> {
        let db = self.db();
        let mut statement = db.prepare(sql).map_err(err)?;
        let ids = statement
            .query_map(values, |r| r.get(0))
            .map_err(err)?
            .flatten()
            .collect();
        Ok(ids)
    }

    /// Deletes every review of an install; returns how many.
    pub fn forget(&self, install: &str) -> Result<usize, String> {
        let ids = self.ids("SELECT id FROM reviews WHERE install_id = ?1", [install])?;
        for id in &ids {
            self.remove(*id)?;
        }
        Ok(ids.len())
    }

    /// Removes what is past its time: reviews, then files, spam, network
    /// hashes and ended bans. Returns (reviews removed, file sets removed).
    pub fn prune(&self, now: i64) -> Result<(usize, usize), String> {
        let old = self.ids(
            "SELECT id FROM reviews WHERE received < ?1
                OR ((status = 'spam' OR suspect = 1) AND status NOT IN ('answered', 'closed') AND received < ?2)",
            params![now - KEEP_REVIEWS_DAYS * DAY, now - KEEP_SPAM_DAYS * DAY],
        )?;
        for id in &old {
            self.remove(*id)?;
        }
        let stale = self.ids(
            "SELECT id FROM reviews WHERE files_gone = 0 AND received < ?1",
            [now - KEEP_FILES_DAYS * DAY],
        )?;
        for id in &stale {
            self.drop_files(*id)?;
        }
        let db = self.db();
        db.execute(
            "UPDATE reviews SET net = NULL WHERE received < ?1",
            [now - KEEP_NET_DAYS * DAY],
        )
        .map_err(err)?;
        db.execute(
            "DELETE FROM bans WHERE until IS NOT NULL AND until <= ?1",
            [now],
        )
        .map_err(err)?;
        Ok((old.len(), stale.len()))
    }

    /// Numbers only, nothing a player wrote: fit for a public workflow log.
    pub fn overview(&self, days: i64, now: i64) -> Result<serde_json::Value, String> {
        let since = now - days.clamp(1, 3650) * DAY;
        let db = self.db();
        let group = |column: &str| -> Result<serde_json::Map<String, serde_json::Value>, String> {
            let mut statement = db
                .prepare(&format!(
                    "SELECT {column}, COUNT(*) FROM reviews WHERE received >= ?1 GROUP BY {column} ORDER BY 2 DESC"
                ))
                .map_err(err)?;
            let rows = statement
                .query_map([since], |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
                })
                .map_err(err)?
                .flatten()
                .map(|(k, n)| (k, n.into()))
                .collect();
            Ok(rows)
        };
        let total: i64 = db
            .query_row(
                "SELECT COUNT(*) FROM reviews WHERE received >= ?1",
                [since],
                |r| r.get(0),
            )
            .map_err(err)?;
        let suspect: i64 = db
            .query_row(
                "SELECT COUNT(*) FROM reviews WHERE received >= ?1 AND suspect = 1",
                [since],
                |r| r.get(0),
            )
            .map_err(err)?;
        let stored: i64 = db
            .query_row(
                "SELECT COALESCE(SUM(bytes), 0) FROM reviews WHERE files_gone = 0",
                [],
                |r| r.get(0),
            )
            .map_err(err)?;
        Ok(serde_json::json!({
            "days": days,
            "reviews": total,
            "suspect": suspect,
            "byKind": group("kind")?,
            "byCategory": group("category")?,
            "byStatus": group("status")?,
            "storedMb": stored / (1024 * 1024),
        }))
    }

    /// A copy of the database in `dir`; the newest `keep` copies stay. The
    /// files are not in it (they go after 90 days anyway).
    pub fn backup(&self, dir: &Path, keep: usize) -> Result<PathBuf, String> {
        std::fs::create_dir_all(dir).map_err(|e| format!("creating {}: {e}", dir.display()))?;
        private(dir, 0o700);
        let name = format!(
            "reviews-{}.db",
            chrono::Utc::now().format("%Y%m%d-%H%M%S-%3f")
        );
        let path = dir.join(name);
        self.db()
            .execute("VACUUM INTO ?1", [path.to_string_lossy()])
            .map_err(|e| format!("copying the database: {e}"))?;
        private(&path, 0o600);
        let mut copies: Vec<PathBuf> = std::fs::read_dir(dir)
            .map_err(io)?
            .flatten()
            .map(|entry| entry.path())
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with("reviews-") && n.ends_with(".db"))
            })
            .collect();
        copies.sort();
        let extra = copies.len().saturating_sub(keep);
        for old in &copies[..extra] {
            std::fs::remove_file(old).map_err(io)?;
        }
        Ok(path)
    }
}
