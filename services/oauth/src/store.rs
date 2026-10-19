//! Accounts, sessions and one-time tokens in SQLite (`data/accounts.db`).
//!
//! Every query takes its values as parameters, never as text in the SQL.
//! Times are Unix seconds. Tokens are stored as their SHA-256 only.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use rusqlite::{Connection, OptionalExtension, params};

const SCHEMA: &str = r#"
PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;
CREATE TABLE IF NOT EXISTS accounts (
    id TEXT PRIMARY KEY,
    nick TEXT NOT NULL,
    nick_key TEXT NOT NULL UNIQUE,
    email TEXT NOT NULL,
    email_key TEXT NOT NULL UNIQUE,
    email_verified INTEGER NOT NULL DEFAULT 0,
    password TEXT NOT NULL,
    lang TEXT NOT NULL DEFAULT 'en',
    created INTEGER NOT NULL,
    reg_ip TEXT NOT NULL,
    reg_country TEXT NOT NULL,
    disabled INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS requests (
    hash TEXT PRIMARY KEY,
    csrf TEXT NOT NULL,
    state TEXT NOT NULL,
    challenge TEXT NOT NULL,
    device TEXT NOT NULL,
    lang TEXT NOT NULL,
    expires INTEGER NOT NULL,
    used INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS codes (
    hash TEXT PRIMARY KEY,
    account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    challenge TEXT NOT NULL,
    device TEXT NOT NULL,
    expires INTEGER NOT NULL,
    used INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS sessions (
    id TEXT PRIMARY KEY,
    account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    device TEXT NOT NULL,
    country TEXT NOT NULL,
    created INTEGER NOT NULL,
    last_used INTEGER NOT NULL,
    expires INTEGER NOT NULL,
    revoked INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS sessions_account ON sessions(account_id);
CREATE TABLE IF NOT EXISTS refresh_tokens (
    hash TEXT PRIMARY KEY,
    session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    created INTEGER NOT NULL,
    used INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS refresh_session ON refresh_tokens(session_id);
CREATE TABLE IF NOT EXISTS email_tokens (
    hash TEXT PRIMARY KEY,
    account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    kind TEXT NOT NULL,
    email TEXT NOT NULL,
    expires INTEGER NOT NULL,
    used INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS failures (
    account_id TEXT PRIMARY KEY REFERENCES accounts(id) ON DELETE CASCADE,
    count INTEGER NOT NULL,
    since INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS events (
    id INTEGER PRIMARY KEY,
    account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    at INTEGER NOT NULL,
    kind TEXT NOT NULL,
    country TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS events_account ON events(account_id, at);
CREATE TABLE IF NOT EXISTS outbox (
    id INTEGER PRIMARY KEY,
    to_addr TEXT NOT NULL,
    subject TEXT NOT NULL,
    body TEXT NOT NULL,
    created INTEGER NOT NULL,
    sent INTEGER,
    attempts INTEGER NOT NULL DEFAULT 0,
    last_error TEXT,
    next_try INTEGER NOT NULL DEFAULT 0
);
-- Accounts deleted, for friends.lapi.sh to clean out; kept 90 days.
CREATE TABLE IF NOT EXISTS deleted (
    id TEXT PRIMARY KEY,
    at INTEGER NOT NULL
);
"#;

/// Failed sign-ins counted per account in this window.
pub const FAILURE_WINDOW: i64 = 15 * 60;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Account {
    pub id: String,
    pub nick: String,
    pub email: String,
    pub email_verified: bool,
    pub password: String,
    pub lang: String,
    pub created: i64,
    pub reg_ip: String,
    pub reg_country: String,
    pub disabled: bool,
}

pub struct NewAccount<'a> {
    pub nick: &'a str,
    pub email: &'a str,
    pub password_hash: &'a str,
    pub lang: &'a str,
    pub ip: &'a str,
    pub country: &'a str,
    pub now: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Taken {
    Nick,
    Email,
}

/// A sign-in in progress: what `/authorize` was asked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub csrf: String,
    pub state: String,
    pub challenge: String,
    pub device: String,
    pub lang: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    pub id: String,
    pub device: String,
    pub country: String,
    pub created: i64,
    pub last_used: i64,
}

/// What a presented refresh token turned out to be.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refresh {
    /// Valid: the session and its account.
    Fresh { session: String, account: String },
    /// Used before: someone else has a copy. The session is ended.
    Reused { account: String },
    /// Unknown, expired, or its session ended.
    Invalid,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mail {
    pub id: i64,
    pub to: String,
    pub subject: String,
    pub body: String,
}

pub struct Store {
    db: Mutex<Connection>,
}

fn account_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Account> {
    Ok(Account {
        id: row.get(0)?,
        nick: row.get(1)?,
        email: row.get(2)?,
        email_verified: row.get(3)?,
        password: row.get(4)?,
        lang: row.get(5)?,
        created: row.get(6)?,
        reg_ip: row.get(7)?,
        reg_country: row.get(8)?,
        disabled: row.get(9)?,
    })
}

/// Brings a database made by an older version up to the schema.
fn migrate(db: &Connection) -> Result<(), String> {
    let has_next_try = db
        .prepare("SELECT 1 FROM pragma_table_info('outbox') WHERE name = 'next_try'")
        .and_then(|mut s| s.exists([]))
        .map_err(|e| e.to_string())?;
    if !has_next_try {
        db.execute_batch(
            "ALTER TABLE outbox ADD COLUMN next_try INTEGER NOT NULL DEFAULT 0;
             UPDATE outbox SET body = '' WHERE sent IS NOT NULL;",
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

const ACCOUNT_COLUMNS: &str =
    "id, nick, email, email_verified, password, lang, created, reg_ip, reg_country, disabled";

impl Store {
    pub fn open(dir: &Path) -> Result<Self, String> {
        std::fs::create_dir_all(dir).map_err(|e| format!("creating {}: {e}", dir.display()))?;
        let path = dir.join("accounts.db");
        let db = Connection::open(&path).map_err(|e| format!("opening {}: {e}", path.display()))?;
        db.busy_timeout(std::time::Duration::from_secs(5))
            .map_err(|e| e.to_string())?;
        db.execute_batch(SCHEMA).map_err(|e| e.to_string())?;
        migrate(&db)?;
        Ok(Self { db: Mutex::new(db) })
    }

    /// A consistent copy of the database (`VACUUM INTO`, safe while the
    /// service runs) as `accounts-<time>.db` in `dir`, readable by this user
    /// alone; only the newest `keep` copies stay.
    pub fn backup(&self, dir: &Path, keep: usize) -> Result<PathBuf, String> {
        std::fs::create_dir_all(dir).map_err(|e| format!("creating {}: {e}", dir.display()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
                .map_err(|e| e.to_string())?;
        }
        let name = format!(
            "accounts-{}.db",
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
                    .is_some_and(|n| n.starts_with("accounts-") && n.ends_with(".db"))
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

    pub fn create_account(&self, new: &NewAccount<'_>) -> Result<Result<Account, Taken>, String> {
        let mut db = self.db();
        let tx = db.transaction().map_err(|e| e.to_string())?;
        let nick_key = crate::rules::nick_key(new.nick);
        let email_key = crate::rules::email_key(new.email);
        let exists = |sql: &str, key: &str| -> Result<bool, String> {
            tx.query_row(sql, [key], |_| Ok(()))
                .optional()
                .map(|r| r.is_some())
                .map_err(|e| e.to_string())
        };
        if exists("SELECT 1 FROM accounts WHERE nick_key = ?1", &nick_key)? {
            return Ok(Err(Taken::Nick));
        }
        if exists("SELECT 1 FROM accounts WHERE email_key = ?1", &email_key)? {
            return Ok(Err(Taken::Email));
        }
        let account = Account {
            id: uuid::Uuid::new_v4().to_string(),
            nick: new.nick.to_owned(),
            email: new.email.to_owned(),
            email_verified: false,
            password: new.password_hash.to_owned(),
            lang: new.lang.to_owned(),
            created: new.now,
            reg_ip: new.ip.to_owned(),
            reg_country: new.country.to_owned(),
            disabled: false,
        };
        tx.execute(
            "INSERT INTO accounts (id, nick, nick_key, email, email_key, password, lang, created, reg_ip, reg_country)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                account.id,
                account.nick,
                nick_key,
                account.email,
                email_key,
                account.password,
                account.lang,
                account.created,
                account.reg_ip,
                account.reg_country
            ],
        )
        .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(Ok(account))
    }

    /// By nick or address, without regard to case.
    pub fn find_login(&self, login: &str) -> Result<Option<Account>, String> {
        let key = login.trim().to_ascii_lowercase();
        let column = if key.contains('@') {
            "email_key"
        } else {
            "nick_key"
        };
        self.db()
            .query_row(
                &format!("SELECT {ACCOUNT_COLUMNS} FROM accounts WHERE {column} = ?1"),
                [key],
                account_row,
            )
            .optional()
            .map_err(|e| e.to_string())
    }

    pub fn account(&self, id: &str) -> Result<Option<Account>, String> {
        self.db()
            .query_row(
                &format!("SELECT {ACCOUNT_COLUMNS} FROM accounts WHERE id = ?1"),
                [id],
                account_row,
            )
            .optional()
            .map_err(|e| e.to_string())
    }

    pub fn set_nick(&self, id: &str, nick: &str) -> Result<Result<(), Taken>, String> {
        let key = crate::rules::nick_key(nick);
        let db = self.db();
        let other: Option<String> = db
            .query_row("SELECT id FROM accounts WHERE nick_key = ?1", [&key], |r| {
                r.get(0)
            })
            .optional()
            .map_err(|e| e.to_string())?;
        if other.is_some_and(|other| other != id) {
            return Ok(Err(Taken::Nick));
        }
        db.execute(
            "UPDATE accounts SET nick = ?2, nick_key = ?3 WHERE id = ?1",
            params![id, nick, key],
        )
        .map_err(|e| e.to_string())?;
        Ok(Ok(()))
    }

    /// A new password also ends every reset link still out.
    pub fn set_password(&self, id: &str, hash: &str) -> Result<(), String> {
        let mut db = self.db();
        let tx = db.transaction().map_err(|e| e.to_string())?;
        tx.execute(
            "UPDATE accounts SET password = ?2 WHERE id = ?1",
            params![id, hash],
        )
        .map_err(|e| e.to_string())?;
        tx.execute(
            "UPDATE email_tokens SET used = 1 WHERE account_id = ?1 AND kind = 'reset'",
            [id],
        )
        .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())
    }

    /// Confirms `email` for the account if it is still its address.
    pub fn verify_email(&self, id: &str, email: &str) -> Result<bool, String> {
        self.db()
            .execute(
                "UPDATE accounts SET email_verified = 1 WHERE id = ?1 AND email_key = ?2",
                params![id, crate::rules::email_key(email)],
            )
            .map(|n| n > 0)
            .map_err(|e| e.to_string())
    }

    pub fn set_email(&self, id: &str, email: &str) -> Result<Result<(), Taken>, String> {
        let key = crate::rules::email_key(email);
        let db = self.db();
        let other: Option<String> = db
            .query_row(
                "SELECT id FROM accounts WHERE email_key = ?1",
                [&key],
                |r| r.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        if other.is_some_and(|other| other != id) {
            return Ok(Err(Taken::Email));
        }
        db.execute(
            "UPDATE accounts SET email = ?2, email_key = ?3, email_verified = 0 WHERE id = ?1",
            params![id, email, key],
        )
        .map_err(|e| e.to_string())?;
        // Links sent to the old address end with it.
        db.execute(
            "UPDATE email_tokens SET used = 1 WHERE account_id = ?1",
            [id],
        )
        .map_err(|e| e.to_string())?;
        Ok(Ok(()))
    }

    /// Of these sessions, those that no longer work: ended, run out, unknown,
    /// or of an account that is switched off.
    pub fn ended_sessions(&self, ids: &[String], now: i64) -> Result<Vec<String>, String> {
        let db = self.db();
        let mut statement = db
            .prepare(
                "SELECT 1 FROM sessions s JOIN accounts a ON a.id = s.account_id
                 WHERE s.id = ?1 AND s.revoked = 0 AND s.expires > ?2 AND a.disabled = 0",
            )
            .map_err(|e| e.to_string())?;
        let mut ended = Vec::new();
        for id in ids {
            if !statement
                .exists(params![id, now])
                .map_err(|e| e.to_string())?
            {
                ended.push(id.clone());
            }
        }
        Ok(ended)
    }

    /// Deletes an account and everything kept about it (sessions, tokens,
    /// links, events, letters waiting for it); friends.lapi.sh learns of it
    /// from the `deleted` list.
    pub fn delete_account(&self, id: &str, now: i64) -> Result<bool, String> {
        let mut db = self.db();
        let tx = db.transaction().map_err(|e| e.to_string())?;
        let email: Option<String> = tx
            .query_row("SELECT email FROM accounts WHERE id = ?1", [id], |r| {
                r.get(0)
            })
            .optional()
            .map_err(|e| e.to_string())?;
        let Some(email) = email else {
            return Ok(false);
        };
        tx.execute(
            "DELETE FROM outbox WHERE to_addr = ?1 COLLATE NOCASE",
            [&email],
        )
        .map_err(|e| e.to_string())?;
        tx.execute("DELETE FROM accounts WHERE id = ?1", [id])
            .map_err(|e| e.to_string())?;
        tx.execute(
            "INSERT OR REPLACE INTO deleted (id, at) VALUES (?1, ?2)",
            params![id, now],
        )
        .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(true)
    }

    /// Accounts deleted after `since`: ID and when, oldest first.
    pub fn deleted_since(&self, since: i64) -> Result<Vec<(String, i64)>, String> {
        let db = self.db();
        let mut statement = db
            .prepare("SELECT id, at FROM deleted WHERE at > ?1 ORDER BY at LIMIT 1000")
            .map_err(|e| e.to_string())?;
        statement
            .query_map([since], |r| Ok((r.get(0)?, r.get(1)?)))
            .and_then(Iterator::collect)
            .map_err(|e| e.to_string())
    }

    /// Everything kept about an account, for the account itself.
    pub fn export(&self, id: &str, now: i64) -> Result<serde_json::Value, String> {
        let account = self.account(id)?.ok_or("no such account")?;
        let sessions: Vec<serde_json::Value> = self
            .sessions(id, now)?
            .into_iter()
            .map(|s| {
                serde_json::json!({
                    "device": s.device, "country": s.country,
                    "created": s.created, "lastUsed": s.last_used,
                })
            })
            .collect();
        let events: Vec<serde_json::Value> = self
            .events(id, 10_000)?
            .into_iter()
            .map(|(at, kind, country)| serde_json::json!({ "at": at, "kind": kind, "country": country }))
            .collect();
        Ok(serde_json::json!({
            "id": account.id,
            "nick": account.nick,
            "email": account.email,
            "emailConfirmed": account.email_verified,
            "language": account.lang,
            "created": account.created,
            "registeredFrom": { "address": account.reg_ip, "country": account.reg_country },
            "switchedOff": account.disabled,
            "sessions": sessions,
            "events": events,
        }))
    }

    pub fn set_disabled(&self, id: &str, disabled: bool) -> Result<bool, String> {
        self.db()
            .execute(
                "UPDATE accounts SET disabled = ?2 WHERE id = ?1",
                params![id, disabled],
            )
            .map(|n| n > 0)
            .map_err(|e| e.to_string())
    }

    // Sign-ins in progress and codes.

    pub fn put_request(&self, hash: &str, request: &Request, expires: i64) -> Result<(), String> {
        self.db()
            .execute(
                "INSERT INTO requests (hash, csrf, state, challenge, device, lang, expires) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    hash,
                    request.csrf,
                    request.state,
                    request.challenge,
                    request.device,
                    request.lang,
                    expires
                ],
            )
            .map(drop)
            .map_err(|e| e.to_string())
    }

    /// A sign-in in progress that has not ended or expired.
    pub fn request(&self, hash: &str, now: i64) -> Result<Option<Request>, String> {
        self.db()
            .query_row(
                "SELECT csrf, state, challenge, device, lang FROM requests WHERE hash = ?1 AND used = 0 AND expires > ?2",
                params![hash, now],
                |r| {
                    Ok(Request {
                        csrf: r.get(0)?,
                        state: r.get(1)?,
                        challenge: r.get(2)?,
                        device: r.get(3)?,
                        lang: r.get(4)?,
                    })
                },
            )
            .optional()
            .map_err(|e| e.to_string())
    }

    /// Ends a sign-in in progress with a code for the account; false when it
    /// had ended already (a second submit).
    pub fn finish_request(
        &self,
        hash: &str,
        code_hash: &str,
        account: &str,
        request: &Request,
        expires: i64,
    ) -> Result<bool, String> {
        let mut db = self.db();
        let tx = db.transaction().map_err(|e| e.to_string())?;
        let ended = tx
            .execute(
                "UPDATE requests SET used = 1 WHERE hash = ?1 AND used = 0",
                [hash],
            )
            .map_err(|e| e.to_string())?;
        if ended == 0 {
            return Ok(false);
        }
        tx.execute(
            "INSERT INTO codes (hash, account_id, challenge, device, expires) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![code_hash, account, request.challenge, request.device, expires],
        )
        .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(true)
    }

    /// Uses a code once: its account, challenge and device.
    pub fn take_code(
        &self,
        hash: &str,
        now: i64,
    ) -> Result<Option<(String, String, String)>, String> {
        let db = self.db();
        let found: Option<(String, String, String)> = db
            .query_row(
                "SELECT account_id, challenge, device FROM codes WHERE hash = ?1 AND used = 0 AND expires > ?2",
                params![hash, now],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        // Used even when the verifier then does not match: a code is tried once.
        db.execute("UPDATE codes SET used = 1 WHERE hash = ?1", [hash])
            .map_err(|e| e.to_string())?;
        Ok(found)
    }

    // Sessions and refresh tokens.

    pub fn create_session(
        &self,
        account: &str,
        device: &str,
        country: &str,
        refresh_hash: &str,
        now: i64,
        expires: i64,
    ) -> Result<String, String> {
        let id = uuid::Uuid::new_v4().to_string();
        let mut db = self.db();
        let tx = db.transaction().map_err(|e| e.to_string())?;
        tx.execute(
            "INSERT INTO sessions (id, account_id, device, country, created, last_used, expires) VALUES (?1, ?2, ?3, ?4, ?5, ?5, ?6)",
            params![id, account, device, country, now, expires],
        )
        .map_err(|e| e.to_string())?;
        tx.execute(
            "INSERT INTO refresh_tokens (hash, session_id, created) VALUES (?1, ?2, ?3)",
            params![refresh_hash, id, now],
        )
        .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(id)
    }

    /// Uses a refresh token once and puts `next` in its place. A token used
    /// before ends its session: either side may be a thief.
    pub fn rotate(
        &self,
        hash: &str,
        next: &str,
        now: i64,
        expires: i64,
    ) -> Result<Refresh, String> {
        let mut db = self.db();
        let tx = db.transaction().map_err(|e| e.to_string())?;
        let found: Option<(String, bool, String, bool, i64, bool)> = tx
            .query_row(
                "SELECT t.session_id, t.used, s.account_id, s.revoked, s.expires, a.disabled
                 FROM refresh_tokens t JOIN sessions s ON s.id = t.session_id JOIN accounts a ON a.id = s.account_id
                 WHERE t.hash = ?1",
                [hash],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        let Some((session, used, account, revoked, session_expires, disabled)) = found else {
            return Ok(Refresh::Invalid);
        };
        if used {
            tx.execute("UPDATE sessions SET revoked = 1 WHERE id = ?1", [&session])
                .map_err(|e| e.to_string())?;
            tx.commit().map_err(|e| e.to_string())?;
            return Ok(if revoked {
                Refresh::Invalid
            } else {
                Refresh::Reused { account }
            });
        }
        if revoked || disabled || session_expires <= now {
            return Ok(Refresh::Invalid);
        }
        tx.execute("UPDATE refresh_tokens SET used = 1 WHERE hash = ?1", [hash])
            .map_err(|e| e.to_string())?;
        tx.execute(
            "INSERT INTO refresh_tokens (hash, session_id, created) VALUES (?1, ?2, ?3)",
            params![next, session, now],
        )
        .map_err(|e| e.to_string())?;
        tx.execute(
            "UPDATE sessions SET last_used = ?2, expires = ?3 WHERE id = ?1",
            params![session, now, expires],
        )
        .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(Refresh::Fresh { session, account })
    }

    /// Ends the session of a refresh token (sign-out); its account if any.
    pub fn revoke_by_refresh(&self, hash: &str) -> Result<Option<String>, String> {
        let db = self.db();
        let found: Option<(String, String)> = db
            .query_row(
                "SELECT s.id, s.account_id FROM refresh_tokens t JOIN sessions s ON s.id = t.session_id WHERE t.hash = ?1",
                [hash],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        if let Some((session, _)) = &found {
            db.execute("UPDATE sessions SET revoked = 1 WHERE id = ?1", [session])
                .map_err(|e| e.to_string())?;
        }
        Ok(found.map(|(_, account)| account))
    }

    pub fn session_alive(&self, id: &str, account: &str, now: i64) -> Result<bool, String> {
        self.db()
            .query_row(
                "SELECT 1 FROM sessions s JOIN accounts a ON a.id = s.account_id
                 WHERE s.id = ?1 AND s.account_id = ?2 AND s.revoked = 0 AND s.expires > ?3 AND a.disabled = 0",
                params![id, account, now],
                |_| Ok(()),
            )
            .optional()
            .map(|r| r.is_some())
            .map_err(|e| e.to_string())
    }

    pub fn sessions(&self, account: &str, now: i64) -> Result<Vec<Session>, String> {
        let db = self.db();
        let mut statement = db
            .prepare(
                "SELECT id, device, country, created, last_used FROM sessions
                 WHERE account_id = ?1 AND revoked = 0 AND expires > ?2 ORDER BY last_used DESC LIMIT 100",
            )
            .map_err(|e| e.to_string())?;
        statement
            .query_map(params![account, now], |r| {
                Ok(Session {
                    id: r.get(0)?,
                    device: r.get(1)?,
                    country: r.get(2)?,
                    created: r.get(3)?,
                    last_used: r.get(4)?,
                })
            })
            .and_then(Iterator::collect)
            .map_err(|e| e.to_string())
    }

    /// Ends one of the account's sessions; false if it is not the account's.
    pub fn revoke_session(&self, account: &str, id: &str) -> Result<bool, String> {
        self.db()
            .execute(
                "UPDATE sessions SET revoked = 1 WHERE id = ?1 AND account_id = ?2",
                params![id, account],
            )
            .map(|n| n > 0)
            .map_err(|e| e.to_string())
    }

    /// Ends every session of the account but `keep`.
    pub fn revoke_all(&self, account: &str, keep: Option<&str>) -> Result<usize, String> {
        self.db()
            .execute(
                "UPDATE sessions SET revoked = 1 WHERE account_id = ?1 AND revoked = 0 AND id IS NOT ?2",
                params![account, keep],
            )
            .map_err(|e| e.to_string())
    }

    // Failed sign-ins.

    pub fn failures(&self, account: &str, now: i64) -> Result<i64, String> {
        self.db()
            .query_row(
                "SELECT count FROM failures WHERE account_id = ?1 AND since > ?2",
                params![account, now - FAILURE_WINDOW],
                |r| r.get(0),
            )
            .optional()
            .map(|c| c.unwrap_or(0))
            .map_err(|e| e.to_string())
    }

    pub fn fail(&self, account: &str, now: i64) -> Result<(), String> {
        self.db()
            .execute(
                "INSERT INTO failures (account_id, count, since) VALUES (?1, 1, ?2)
                 ON CONFLICT(account_id) DO UPDATE SET
                   count = CASE WHEN since > ?2 - ?3 THEN count + 1 ELSE 1 END,
                   since = CASE WHEN since > ?2 - ?3 THEN since ELSE ?2 END",
                params![account, now, FAILURE_WINDOW],
            )
            .map(drop)
            .map_err(|e| e.to_string())
    }

    pub fn clear_failures(&self, account: &str) -> Result<(), String> {
        self.db()
            .execute("DELETE FROM failures WHERE account_id = ?1", [account])
            .map(drop)
            .map_err(|e| e.to_string())
    }

    // Events, email links and letters.

    pub fn event(&self, account: &str, kind: &str, country: &str, now: i64) -> Result<(), String> {
        self.db()
            .execute(
                "INSERT INTO events (account_id, at, kind, country) VALUES (?1, ?2, ?3, ?4)",
                params![account, now, kind, country],
            )
            .map(drop)
            .map_err(|e| e.to_string())
    }

    pub fn events(&self, account: &str, limit: i64) -> Result<Vec<(i64, String, String)>, String> {
        let db = self.db();
        let mut statement = db
            .prepare("SELECT at, kind, country FROM events WHERE account_id = ?1 ORDER BY at DESC, id DESC LIMIT ?2")
            .map_err(|e| e.to_string())?;
        statement
            .query_map(params![account, limit], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })
            .and_then(Iterator::collect)
            .map_err(|e| e.to_string())
    }

    /// Events of this kind for the account since `since`.
    pub fn count_events(&self, account: &str, kind: &str, since: i64) -> Result<i64, String> {
        self.db()
            .query_row(
                "SELECT COUNT(*) FROM events WHERE account_id = ?1 AND kind = ?2 AND at > ?3",
                params![account, kind, since],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())
    }

    pub fn put_email_token(
        &self,
        hash: &str,
        account: &str,
        kind: &str,
        email: &str,
        expires: i64,
    ) -> Result<(), String> {
        self.db()
            .execute(
                "INSERT INTO email_tokens (hash, account_id, kind, email, expires) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![hash, account, kind, email, expires],
            )
            .map(drop)
            .map_err(|e| e.to_string())
    }

    /// An unused, unexpired email link of this kind: its account and address.
    pub fn email_token(
        &self,
        hash: &str,
        kind: &str,
        now: i64,
    ) -> Result<Option<(String, String)>, String> {
        self.db()
            .query_row(
                "SELECT account_id, email FROM email_tokens WHERE hash = ?1 AND kind = ?2 AND used = 0 AND expires > ?3",
                params![hash, kind, now],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(|e| e.to_string())
    }

    /// Uses an email link; false when it was used already.
    pub fn use_email_token(&self, hash: &str) -> Result<bool, String> {
        self.db()
            .execute(
                "UPDATE email_tokens SET used = 1 WHERE hash = ?1 AND used = 0",
                [hash],
            )
            .map(|n| n > 0)
            .map_err(|e| e.to_string())
    }

    pub fn queue_mail(&self, to: &str, subject: &str, body: &str, now: i64) -> Result<(), String> {
        self.db()
            .execute(
                "INSERT INTO outbox (to_addr, subject, body, created) VALUES (?1, ?2, ?3, ?4)",
                params![to, subject, body, now],
            )
            .map(drop)
            .map_err(|e| e.to_string())
    }

    /// Letters to send: not sent, due, newer than 3 days.
    pub fn pending_mail(&self, now: i64, limit: i64) -> Result<Vec<Mail>, String> {
        let db = self.db();
        let mut statement = db
            .prepare(
                "SELECT id, to_addr, subject, body FROM outbox
                 WHERE sent IS NULL AND body != '' AND next_try <= ?1 AND created > ?2
                 ORDER BY id LIMIT ?3",
            )
            .map_err(|e| e.to_string())?;
        statement
            .query_map(params![now, now - 3 * 86_400, limit], |r| {
                Ok(Mail {
                    id: r.get(0)?,
                    to: r.get(1)?,
                    subject: r.get(2)?,
                    body: r.get(3)?,
                })
            })
            .and_then(Iterator::collect)
            .map_err(|e| e.to_string())
    }

    /// A sent letter keeps no body; a failed one waits 20 s, 40 s, 80 s and so
    /// on, an hour at most, until it is 3 days old.
    pub fn mail_done(&self, id: i64, error: Option<&str>, now: i64) -> Result<(), String> {
        let db = self.db();
        match error {
            None => db.execute(
                "UPDATE outbox SET sent = ?2, body = '', attempts = attempts + 1, last_error = NULL WHERE id = ?1",
                params![id, now],
            ),
            Some(error) => db.execute(
                "UPDATE outbox SET attempts = attempts + 1, last_error = ?2,
                 next_try = ?3 + MIN(3600, 20 << MIN(attempts, 8)) WHERE id = ?1",
                params![id, error, now],
            ),
        }
        .map(drop)
        .map_err(|e| e.to_string())
    }

    /// Waiting letters whose body may be unsealed: id, recipient, body.
    pub fn unsealed_mail(&self) -> Result<Vec<(i64, String, String)>, String> {
        let db = self.db();
        let mut statement = db
            .prepare("SELECT id, to_addr, body FROM outbox WHERE sent IS NULL AND body != '' AND body NOT LIKE 'sealed:%'")
            .map_err(|e| e.to_string())?;
        statement
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .and_then(Iterator::collect)
            .map_err(|e| e.to_string())
    }

    pub fn set_mail_body(&self, id: i64, body: &str) -> Result<(), String> {
        self.db()
            .execute(
                "UPDATE outbox SET body = ?2 WHERE id = ?1",
                params![id, body],
            )
            .map(drop)
            .map_err(|e| e.to_string())
    }

    /// A letter that can no longer be opened (its key is gone).
    pub fn mail_dropped(&self, id: i64) -> Result<(), String> {
        self.db()
            .execute(
                "UPDATE outbox SET body = '', last_error = 'cannot be opened' WHERE id = ?1",
                [id],
            )
            .map(drop)
            .map_err(|e| e.to_string())
    }

    /// The outbox in numbers, for `lapi-oauth mail-check`.
    pub fn mail_counts(&self, now: i64) -> Result<serde_json::Value, String> {
        let db = self.db();
        let one = |sql: &str| -> Result<Option<i64>, String> {
            db.query_row(sql, [now], |r| r.get(0))
                .map_err(|e| e.to_string())
        };
        Ok(serde_json::json!({
            "waiting": one("SELECT COUNT(*) FROM outbox WHERE sent IS NULL AND body != '' AND created > ?1 - 3 * 86400")?,
            "waitingAfterAFailure": one("SELECT COUNT(*) FROM outbox WHERE sent IS NULL AND body != '' AND attempts > 0 AND created > ?1 - 3 * 86400")?,
            "sentLastDay": one("SELECT COUNT(*) FROM outbox WHERE sent > ?1 - 86400")?,
            "givenUpLastWeek": one("SELECT COUNT(*) FROM outbox WHERE sent IS NULL AND (body = '' OR created <= ?1 - 3 * 86400) AND created > ?1 - 7 * 86400")?,
            "minutesSinceLastSent": one("SELECT (?1 - MAX(sent)) / 60 FROM outbox")?,
        }))
    }

    /// Drops what has ended: sign-ins in progress, codes, links, old tokens
    /// and sessions, sent letters after 30 days, events after a year.
    pub fn prune(&self, now: i64) -> Result<(), String> {
        // Used refresh tokens stay to catch a reused one; after a session's
        // whole life without a refresh they would not work anyway.
        let keep_used = crate::app::SESSION_SECONDS;
        let db = self.db();
        db.execute_batch(&format!(
            "DELETE FROM requests WHERE expires < {now} - 3600;
             DELETE FROM codes WHERE expires < {now} - 3600;
             DELETE FROM email_tokens WHERE expires < {now} - 86400;
             DELETE FROM sessions WHERE expires < {now} OR (revoked = 1 AND last_used < {now} - 30 * 86400);
             DELETE FROM outbox WHERE (sent IS NOT NULL AND sent < {now} - 30 * 86400) OR created < {now} - 30 * 86400;
             DELETE FROM events WHERE at < {now} - 365 * 86400;
             DELETE FROM deleted WHERE at < {now} - 90 * 86400;
             DELETE FROM failures WHERE since < {now} - 86400;
             DELETE FROM refresh_tokens WHERE used = 1 AND created < {now} - {keep_used};"
        ))
        .map_err(|e| e.to_string())
    }

    /// Counts only: fit for a public workflow summary.
    pub fn counts(&self, now: i64) -> Result<serde_json::Value, String> {
        let db = self.db();
        let one = |sql: &str| -> Result<i64, String> {
            db.query_row(sql, [now], |r| r.get(0))
                .map_err(|e| e.to_string())
        };
        Ok(serde_json::json!({
            "accounts": one("SELECT COUNT(*) FROM accounts WHERE ?1 = ?1")?,
            "verified": one("SELECT COUNT(*) FROM accounts WHERE email_verified = 1 AND ?1 = ?1")?,
            "registeredLastDay": one("SELECT COUNT(*) FROM accounts WHERE created > ?1 - 86400")?,
            "activeSessions": one("SELECT COUNT(*) FROM sessions WHERE revoked = 0 AND expires > ?1")?,
            "lettersWaiting": one("SELECT COUNT(*) FROM outbox WHERE sent IS NULL AND body != '' AND created > ?1 - 3 * 86400")?,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_used_refresh_tokens_are_pruned() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        let now = 100 * 86_400 * 365;
        let keep = crate::app::SESSION_SECONDS;
        {
            let db = store.db();
            db.execute_batch("PRAGMA foreign_keys = OFF;").unwrap();
            for (hash, created, used) in [
                ("old-used", now - keep - 1, 1),
                ("recent-used", now - keep + 60, 1),
                ("old-unused", now - keep - 1, 0),
            ] {
                db.execute(
                    "INSERT INTO refresh_tokens (hash, session_id, created, used) VALUES (?1, 's', ?2, ?3)",
                    params![hash, created, used],
                )
                .unwrap();
            }
        }
        store.prune(now).unwrap();
        let db = store.db();
        let mut left: Vec<String> = db
            .prepare("SELECT hash FROM refresh_tokens ORDER BY hash")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        left.sort();
        assert_eq!(left, ["old-unused", "recent-used"]);
    }
}
