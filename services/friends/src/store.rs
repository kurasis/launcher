//! Friends, requests, blocks, rooms and messages in SQLite
//! (`data/friends.db`). Accounts are only IDs here; nicks come from the
//! account service. Every query takes its values as parameters.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;

const SCHEMA: &str = r#"
PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;
CREATE TABLE IF NOT EXISTS requests (
    from_id TEXT NOT NULL,
    to_id TEXT NOT NULL,
    created INTEGER NOT NULL,
    PRIMARY KEY (from_id, to_id)
);
CREATE INDEX IF NOT EXISTS requests_to ON requests(to_id);
-- Both directions, so one index answers "friends of".
CREATE TABLE IF NOT EXISTS friends (
    a TEXT NOT NULL,
    b TEXT NOT NULL,
    since INTEGER NOT NULL,
    PRIMARY KEY (a, b)
);
CREATE TABLE IF NOT EXISTS blocks (
    blocker TEXT NOT NULL,
    blocked TEXT NOT NULL,
    created INTEGER NOT NULL,
    PRIMARY KEY (blocker, blocked)
);
CREATE TABLE IF NOT EXISTS rooms (
    id TEXT PRIMARY KEY,
    kind TEXT NOT NULL,
    -- "a|b" for the room of two friends, so there is one per pair.
    pair TEXT UNIQUE,
    name TEXT NOT NULL DEFAULT '',
    owner TEXT NOT NULL,
    created INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS members (
    room_id TEXT NOT NULL REFERENCES rooms(id) ON DELETE CASCADE,
    account_id TEXT NOT NULL,
    joined INTEGER NOT NULL,
    read_upto INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (room_id, account_id)
);
CREATE INDEX IF NOT EXISTS members_account ON members(account_id);
CREATE TABLE IF NOT EXISTS messages (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    room_id TEXT NOT NULL REFERENCES rooms(id) ON DELETE CASCADE,
    sender TEXT NOT NULL,
    body TEXT NOT NULL,
    created INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS messages_room ON messages(room_id, id);
CREATE TABLE IF NOT EXISTS reports (
    id INTEGER PRIMARY KEY,
    message_id INTEGER NOT NULL,
    reporter TEXT NOT NULL,
    reason TEXT NOT NULL,
    created INTEGER NOT NULL,
    UNIQUE (message_id, reporter)
);
-- What the owner did about an account: no writing until a time (mute), or
-- no friends at all (ban); `until` 0 is for good.
CREATE TABLE IF NOT EXISTS sanctions (
    account_id TEXT PRIMARY KEY,
    kind TEXT NOT NULL,
    until INTEGER NOT NULL,
    reason TEXT NOT NULL,
    created INTEGER NOT NULL
);
-- Changes made from the command line for the running service to tell:
-- a removed message, a sanction.
CREATE TABLE IF NOT EXISTS moderation (
    id INTEGER PRIMARY KEY,
    kind TEXT NOT NULL,
    room_id TEXT NOT NULL DEFAULT '',
    message_id INTEGER NOT NULL DEFAULT 0,
    account_id TEXT NOT NULL DEFAULT '',
    told INTEGER NOT NULL DEFAULT 0
);
-- Who does not show as online to friends.
CREATE TABLE IF NOT EXISTS hidden (
    account_id TEXT PRIMARY KEY
);
-- When an account first used friends: new accounts send fewer requests.
CREATE TABLE IF NOT EXISTS seen (
    account_id TEXT PRIMARY KEY,
    first INTEGER NOT NULL
);
-- Requests an account sent in the last day, for the daily limit (kept over
-- restarts, unlike the hourly one).
CREATE TABLE IF NOT EXISTS request_log (
    account_id TEXT NOT NULL,
    at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS request_log_account ON request_log(account_id, at);
-- Friend requests reported as spam (the request itself is dropped).
CREATE TABLE IF NOT EXISTS request_reports (
    id INTEGER PRIMARY KEY,
    reporter TEXT NOT NULL,
    sender TEXT NOT NULL,
    nick TEXT NOT NULL,
    created INTEGER NOT NULL,
    status TEXT NOT NULL DEFAULT 'open',
    note TEXT NOT NULL DEFAULT '',
    UNIQUE (reporter, sender)
);
"#;

/// Columns added to `reports` after it was first made: a copy of the
/// message (so the evidence stays when the message goes) and what the owner
/// decided.
const REPORT_COLUMNS: [(&str, &str); 6] = [
    ("room_id", "TEXT NOT NULL DEFAULT ''"),
    ("sender", "TEXT NOT NULL DEFAULT ''"),
    ("body", "TEXT NOT NULL DEFAULT ''"),
    ("status", "TEXT NOT NULL DEFAULT 'open'"),
    ("note", "TEXT NOT NULL DEFAULT ''"),
    ("resolved", "INTEGER NOT NULL DEFAULT 0"),
];

/// Brings a database made by an older version up to the schema.
fn migrate(db: &Connection) -> Result<()> {
    for (column, definition) in REPORT_COLUMNS {
        let exists = db
            .prepare("SELECT 1 FROM pragma_table_info('reports') WHERE name = ?1")
            .and_then(|mut s| s.exists([column]))
            .map_err(err)?;
        if !exists {
            db.execute_batch(&format!(
                "ALTER TABLE reports ADD COLUMN {column} {definition};"
            ))
            .map_err(err)?;
        }
    }
    // Reports from before: copy what is still there.
    db.execute_batch(
        "UPDATE reports SET room_id = m.room_id, sender = m.sender, body = m.body
         FROM messages m WHERE m.id = reports.message_id AND reports.sender = '';",
    )
    .map_err(err)
}

/// A change made from the command line, for the running service to tell.
pub struct Untold {
    pub id: i64,
    /// `message` (removed) or `sanction`.
    pub kind: String,
    pub room: String,
    pub message: i64,
    pub account: String,
}

/// A sanction on an account.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Sanction {
    pub kind: String,
    /// Unix time it ends; 0 for good.
    pub until: i64,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Requested {
    Sent,
    /// They had asked too: now friends.
    Friends,
    AlreadyFriends,
    AlreadySent,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub id: i64,
    pub room: String,
    pub sender: String,
    pub body: String,
    pub created: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Room {
    pub id: String,
    pub kind: String,
    pub name: String,
    pub owner: String,
    pub created: i64,
}

pub struct Store {
    db: Mutex<Connection>,
}

type Result<T> = std::result::Result<T, String>;

fn err(e: rusqlite::Error) -> String {
    e.to_string()
}

/// The two IDs of a pair in a fixed order.
pub fn pair(a: &str, b: &str) -> String {
    if a < b {
        format!("{a}|{b}")
    } else {
        format!("{b}|{a}")
    }
}

impl Store {
    pub fn open(dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(dir).map_err(|e| format!("creating {}: {e}", dir.display()))?;
        let path = dir.join("friends.db");
        let db = Connection::open(&path).map_err(|e| format!("opening {}: {e}", path.display()))?;
        db.busy_timeout(std::time::Duration::from_secs(5))
            .map_err(err)?;
        db.execute_batch(SCHEMA).map_err(err)?;
        migrate(&db)?;
        Ok(Self { db: Mutex::new(db) })
    }

    /// A consistent copy of the database (`VACUUM INTO`, safe while the
    /// service runs) as `friends-<time>.db` in `dir`, readable by this user
    /// alone; only the newest `keep` copies stay.
    pub fn backup(&self, dir: &Path, keep: usize) -> std::result::Result<PathBuf, String> {
        std::fs::create_dir_all(dir).map_err(|e| format!("creating {}: {e}", dir.display()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
                .map_err(|e| e.to_string())?;
        }
        let name = format!(
            "friends-{}.db",
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
                    .is_some_and(|n| n.starts_with("friends-") && n.ends_with(".db"))
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

    fn ids(&self, sql: &str, id: &str) -> Result<Vec<(String, i64)>> {
        let db = self.db();
        let mut statement = db.prepare(sql).map_err(err)?;
        statement
            .query_map([id], |r| Ok((r.get(0)?, r.get(1)?)))
            .and_then(Iterator::collect)
            .map_err(err)
    }

    pub fn friends(&self, me: &str) -> Result<Vec<(String, i64)>> {
        self.ids(
            "SELECT b, since FROM friends WHERE a = ?1 ORDER BY since",
            me,
        )
    }

    pub fn incoming(&self, me: &str) -> Result<Vec<(String, i64)>> {
        self.ids(
            "SELECT from_id, created FROM requests WHERE to_id = ?1 ORDER BY created",
            me,
        )
    }

    pub fn outgoing(&self, me: &str) -> Result<Vec<(String, i64)>> {
        self.ids(
            "SELECT to_id, created FROM requests WHERE from_id = ?1 ORDER BY created",
            me,
        )
    }

    pub fn blocked(&self, me: &str) -> Result<Vec<(String, i64)>> {
        self.ids(
            "SELECT blocked, created FROM blocks WHERE blocker = ?1 ORDER BY created",
            me,
        )
    }

    fn exists(&self, sql: &str, a: &str, b: &str) -> Result<bool> {
        self.db()
            .query_row(sql, params![a, b], |_| Ok(()))
            .optional()
            .map(|r| r.is_some())
            .map_err(err)
    }

    pub fn are_friends(&self, a: &str, b: &str) -> Result<bool> {
        self.exists("SELECT 1 FROM friends WHERE a = ?1 AND b = ?2", a, b)
    }

    /// `blocker` blocked `blocked`.
    pub fn blocks(&self, blocker: &str, blocked: &str) -> Result<bool> {
        self.exists(
            "SELECT 1 FROM blocks WHERE blocker = ?1 AND blocked = ?2",
            blocker,
            blocked,
        )
    }

    /// A request from `from` to `to`; theirs to us makes us friends at once.
    pub fn request(&self, from: &str, to: &str, now: i64) -> Result<Requested> {
        let mut db = self.db();
        let tx = db.transaction().map_err(err)?;
        let found = |sql: &str, a: &str, b: &str| -> Result<bool> {
            tx.query_row(sql, params![a, b], |_| Ok(()))
                .optional()
                .map(|r| r.is_some())
                .map_err(err)
        };
        if found("SELECT 1 FROM friends WHERE a = ?1 AND b = ?2", from, to)? {
            return Ok(Requested::AlreadyFriends);
        }
        if found(
            "SELECT 1 FROM requests WHERE from_id = ?1 AND to_id = ?2",
            from,
            to,
        )? {
            return Ok(Requested::AlreadySent);
        }
        let theirs = found(
            "SELECT 1 FROM requests WHERE from_id = ?1 AND to_id = ?2",
            to,
            from,
        )?;
        if theirs {
            tx.execute(
                "DELETE FROM requests WHERE from_id = ?1 AND to_id = ?2",
                params![to, from],
            )
            .map_err(err)?;
            tx.execute(
                "INSERT INTO friends (a, b, since) VALUES (?1, ?2, ?3), (?2, ?1, ?3)",
                params![from, to, now],
            )
            .map_err(err)?;
            tx.commit().map_err(err)?;
            return Ok(Requested::Friends);
        }
        tx.execute(
            "INSERT INTO requests (from_id, to_id, created) VALUES (?1, ?2, ?3)",
            params![from, to, now],
        )
        .map_err(err)?;
        tx.commit().map_err(err)?;
        Ok(Requested::Sent)
    }

    /// `me` accepts the request of `from`; false if there was none.
    pub fn accept(&self, me: &str, from: &str, now: i64) -> Result<bool> {
        let mut db = self.db();
        let tx = db.transaction().map_err(err)?;
        let removed = tx
            .execute(
                "DELETE FROM requests WHERE from_id = ?1 AND to_id = ?2",
                params![from, me],
            )
            .map_err(err)?;
        if removed == 0 {
            return Ok(false);
        }
        tx.execute(
            "INSERT OR IGNORE INTO friends (a, b, since) VALUES (?1, ?2, ?3), (?2, ?1, ?3)",
            params![me, from, now],
        )
        .map_err(err)?;
        tx.commit().map_err(err)?;
        Ok(true)
    }

    /// Removes a request between the two, either way; false if there was none.
    /// Declines every request waiting for `me`; how many.
    pub fn decline_all(&self, me: &str) -> Result<usize> {
        self.db()
            .execute("DELETE FROM requests WHERE to_id = ?1", [me])
            .map_err(err)
    }

    /// When the account first used friends (now, the first time).
    pub fn first_seen(&self, account: &str, now: i64) -> Result<i64> {
        let db = self.db();
        db.execute(
            "INSERT OR IGNORE INTO seen (account_id, first) VALUES (?1, ?2)",
            params![account, now],
        )
        .map_err(err)?;
        db.query_row(
            "SELECT first FROM seen WHERE account_id = ?1",
            [account],
            |r| r.get(0),
        )
        .map_err(err)
    }

    /// Counts a request toward the daily limit; false (and not counted)
    /// once `limit` were sent in the last day.
    pub fn count_request(&self, account: &str, limit: i64, now: i64) -> Result<bool> {
        let mut db = self.db();
        let tx = db.transaction().map_err(err)?;
        tx.execute(
            "DELETE FROM request_log WHERE account_id = ?1 AND at <= ?2",
            params![account, now - 86_400],
        )
        .map_err(err)?;
        let sent: i64 = tx
            .query_row(
                "SELECT COUNT(*) FROM request_log WHERE account_id = ?1",
                [account],
                |r| r.get(0),
            )
            .map_err(err)?;
        if sent >= limit {
            return Ok(false);
        }
        tx.execute(
            "INSERT INTO request_log (account_id, at) VALUES (?1, ?2)",
            params![account, now],
        )
        .map_err(err)?;
        tx.commit().map_err(err)?;
        Ok(true)
    }

    /// Reports a waiting request as spam and drops it; false when there is
    /// no such request.
    pub fn report_request(
        &self,
        reporter: &str,
        sender: &str,
        nick: &str,
        now: i64,
    ) -> Result<bool> {
        let mut db = self.db();
        let tx = db.transaction().map_err(err)?;
        let dropped = tx
            .execute(
                "DELETE FROM requests WHERE from_id = ?1 AND to_id = ?2",
                params![sender, reporter],
            )
            .map_err(err)?;
        if dropped == 0 {
            return Ok(false);
        }
        tx.execute(
            "INSERT OR IGNORE INTO request_reports (reporter, sender, nick, created) VALUES (?1, ?2, ?3, ?4)",
            params![reporter, sender, nick, now],
        )
        .map_err(err)?;
        tx.commit().map_err(err)?;
        Ok(true)
    }

    /// Requests reported as spam, newest first; only open ones unless `all`.
    pub fn request_reports(&self, all: bool, limit: i64) -> Result<Vec<serde_json::Value>> {
        let db = self.db();
        let mut statement = db
            .prepare(
                "SELECT id, created, reporter, sender, nick, status, note FROM request_reports
                 WHERE ?1 OR status = 'open' ORDER BY id DESC LIMIT ?2",
            )
            .map_err(err)?;
        statement
            .query_map(params![all, limit], |r| {
                Ok(serde_json::json!({
                    "requestReport": r.get::<_, i64>(0)?,
                    "at": r.get::<_, i64>(1)?,
                    "reporter": r.get::<_, String>(2)?,
                    "sender": r.get::<_, String>(3)?,
                    "nick": r.get::<_, String>(4)?,
                    "status": r.get::<_, String>(5)?,
                    "note": r.get::<_, String>(6)?,
                }))
            })
            .and_then(Iterator::collect)
            .map_err(err)
    }

    pub fn resolve_request_report(&self, id: i64, note: &str) -> Result<bool> {
        self.db()
            .execute(
                "UPDATE request_reports SET status = 'resolved', note = ?2 WHERE id = ?1",
                params![id, note],
            )
            .map(|n| n > 0)
            .map_err(err)
    }

    pub fn drop_request(&self, from: &str, to: &str) -> Result<bool> {
        self.db()
            .execute(
                "DELETE FROM requests WHERE from_id = ?1 AND to_id = ?2",
                params![from, to],
            )
            .map(|n| n > 0)
            .map_err(err)
    }

    pub fn unfriend(&self, a: &str, b: &str) -> Result<bool> {
        self.db()
            .execute(
                "DELETE FROM friends WHERE (a = ?1 AND b = ?2) OR (a = ?2 AND b = ?1)",
                params![a, b],
            )
            .map(|n| n > 0)
            .map_err(err)
    }

    /// Blocks: no friendship and no request between the two remain.
    pub fn block(&self, me: &str, other: &str, now: i64) -> Result<()> {
        let mut db = self.db();
        let tx = db.transaction().map_err(err)?;
        tx.execute(
            "DELETE FROM friends WHERE (a = ?1 AND b = ?2) OR (a = ?2 AND b = ?1)",
            params![me, other],
        )
        .map_err(err)?;
        tx.execute(
            "DELETE FROM requests WHERE (from_id = ?1 AND to_id = ?2) OR (from_id = ?2 AND to_id = ?1)",
            params![me, other],
        )
        .map_err(err)?;
        tx.execute(
            "INSERT OR IGNORE INTO blocks (blocker, blocked, created) VALUES (?1, ?2, ?3)",
            params![me, other, now],
        )
        .map_err(err)?;
        tx.commit().map_err(err)
    }

    pub fn unblock(&self, me: &str, other: &str) -> Result<bool> {
        self.db()
            .execute(
                "DELETE FROM blocks WHERE blocker = ?1 AND blocked = ?2",
                params![me, other],
            )
            .map(|n| n > 0)
            .map_err(err)
    }

    // Rooms.

    /// The room of two friends, made the first time.
    pub fn direct_room(&self, a: &str, b: &str, now: i64) -> Result<String> {
        let key = pair(a, b);
        let mut db = self.db();
        let tx = db.transaction().map_err(err)?;
        if let Some(id) = tx
            .query_row("SELECT id FROM rooms WHERE pair = ?1", [&key], |r| {
                r.get::<_, String>(0)
            })
            .optional()
            .map_err(err)?
        {
            return Ok(id);
        }
        let id = uuid::Uuid::new_v4().to_string();
        tx.execute(
            "INSERT INTO rooms (id, kind, pair, owner, created) VALUES (?1, 'direct', ?2, ?3, ?4)",
            params![id, key, a, now],
        )
        .map_err(err)?;
        tx.execute(
            "INSERT INTO members (room_id, account_id, joined) VALUES (?1, ?2, ?4), (?1, ?3, ?4)",
            params![id, a, b, now],
        )
        .map_err(err)?;
        tx.commit().map_err(err)?;
        Ok(id)
    }

    pub fn create_group(
        &self,
        owner: &str,
        name: &str,
        others: &[String],
        now: i64,
    ) -> Result<String> {
        let id = uuid::Uuid::new_v4().to_string();
        let mut db = self.db();
        let tx = db.transaction().map_err(err)?;
        tx.execute(
            "INSERT INTO rooms (id, kind, name, owner, created) VALUES (?1, 'group', ?2, ?3, ?4)",
            params![id, name, owner, now],
        )
        .map_err(err)?;
        for member in std::iter::once(owner).chain(others.iter().map(String::as_str)) {
            tx.execute(
                "INSERT OR IGNORE INTO members (room_id, account_id, joined) VALUES (?1, ?2, ?3)",
                params![id, member, now],
            )
            .map_err(err)?;
        }
        tx.commit().map_err(err)?;
        Ok(id)
    }

    pub fn room(&self, id: &str) -> Result<Option<Room>> {
        self.db()
            .query_row(
                "SELECT id, kind, name, owner, created FROM rooms WHERE id = ?1",
                [id],
                |r| {
                    Ok(Room {
                        id: r.get(0)?,
                        kind: r.get(1)?,
                        name: r.get(2)?,
                        owner: r.get(3)?,
                        created: r.get(4)?,
                    })
                },
            )
            .optional()
            .map_err(err)
    }

    pub fn members(&self, room: &str) -> Result<Vec<String>> {
        let db = self.db();
        let mut statement = db
            .prepare(
                "SELECT account_id FROM members WHERE room_id = ?1 ORDER BY joined, account_id",
            )
            .map_err(err)?;
        statement
            .query_map([room], |r| r.get(0))
            .and_then(Iterator::collect)
            .map_err(err)
    }

    pub fn is_member(&self, room: &str, account: &str) -> Result<bool> {
        self.exists(
            "SELECT 1 FROM members WHERE room_id = ?1 AND account_id = ?2",
            room,
            account,
        )
    }

    /// The rooms of an account: room, last message ID, unread count.
    pub fn rooms_of(&self, me: &str) -> Result<Vec<(Room, Option<Message>, i64)>> {
        let rooms: Vec<Room> = {
            let db = self.db();
            let mut statement = db
                .prepare(
                    "SELECT r.id, r.kind, r.name, r.owner, r.created FROM rooms r
                     JOIN members m ON m.room_id = r.id WHERE m.account_id = ?1",
                )
                .map_err(err)?;
            statement
                .query_map([me], |r| {
                    Ok(Room {
                        id: r.get(0)?,
                        kind: r.get(1)?,
                        name: r.get(2)?,
                        owner: r.get(3)?,
                        created: r.get(4)?,
                    })
                })
                .and_then(Iterator::collect)
                .map_err(err)?
        };
        let mut out = Vec::new();
        for room in rooms {
            let last = self.messages(&room.id, me, None, 1)?.pop();
            let unread = self.unread(&room.id, me)?;
            out.push((room, last, unread));
        }
        out.sort_by_key(|(room, last, _)| {
            std::cmp::Reverse(last.as_ref().map_or(room.created, |m| m.created))
        });
        Ok(out)
    }

    pub fn owned_groups(&self, owner: &str) -> Result<i64> {
        self.db()
            .query_row(
                "SELECT COUNT(*) FROM rooms WHERE owner = ?1 AND kind = 'group'",
                [owner],
                |r| r.get(0),
            )
            .map_err(err)
    }

    pub fn add_member(&self, room: &str, account: &str, now: i64) -> Result<bool> {
        self.db()
            .execute(
                "INSERT OR IGNORE INTO members (room_id, account_id, joined) VALUES (?1, ?2, ?3)",
                params![room, account, now],
            )
            .map(|n| n > 0)
            .map_err(err)
    }

    /// Takes a member out; a group left empty goes with its messages.
    pub fn remove_member(&self, room: &str, account: &str) -> Result<bool> {
        let db = self.db();
        let removed = db
            .execute(
                "DELETE FROM members WHERE room_id = ?1 AND account_id = ?2",
                params![room, account],
            )
            .map_err(err)?;
        db.execute(
            "DELETE FROM rooms WHERE id = ?1 AND kind = 'group' AND NOT EXISTS (SELECT 1 FROM members WHERE room_id = ?1)",
            [room],
        )
        .map_err(err)?;
        Ok(removed > 0)
    }

    pub fn set_owner(&self, room: &str, owner: &str) -> Result<()> {
        self.db()
            .execute(
                "UPDATE rooms SET owner = ?2 WHERE id = ?1",
                params![room, owner],
            )
            .map(drop)
            .map_err(err)
    }

    pub fn rename(&self, room: &str, name: &str) -> Result<()> {
        self.db()
            .execute(
                "UPDATE rooms SET name = ?2 WHERE id = ?1 AND kind = 'group'",
                params![room, name],
            )
            .map(drop)
            .map_err(err)
    }

    // Messages.

    pub fn post(&self, room: &str, sender: &str, body: &str, now: i64) -> Result<Message> {
        let db = self.db();
        db.execute(
            "INSERT INTO messages (room_id, sender, body, created) VALUES (?1, ?2, ?3, ?4)",
            params![room, sender, body, now],
        )
        .map_err(err)?;
        let id = db.last_insert_rowid();
        // One's own message is read.
        db.execute(
            "UPDATE members SET read_upto = ?3 WHERE room_id = ?1 AND account_id = ?2",
            params![room, sender, id],
        )
        .map_err(err)?;
        Ok(Message {
            id,
            room: room.to_owned(),
            sender: sender.to_owned(),
            body: body.to_owned(),
            created: now,
        })
    }

    /// Messages before `before` (newest first) since `reader` joined the
    /// room, leaving out senders `reader` blocked.
    pub fn messages(
        &self,
        room: &str,
        reader: &str,
        before: Option<i64>,
        limit: i64,
    ) -> Result<Vec<Message>> {
        let db = self.db();
        let mut statement = db
            .prepare(
                "SELECT id, room_id, sender, body, created FROM messages
                 WHERE room_id = ?1 AND id < ?2
                   AND created >= (SELECT joined FROM members WHERE room_id = ?1 AND account_id = ?3)
                   AND sender NOT IN (SELECT blocked FROM blocks WHERE blocker = ?3)
                 ORDER BY id DESC LIMIT ?4",
            )
            .map_err(err)?;
        statement
            .query_map(
                params![room, before.unwrap_or(i64::MAX), reader, limit],
                |r| {
                    Ok(Message {
                        id: r.get(0)?,
                        room: r.get(1)?,
                        sender: r.get(2)?,
                        body: r.get(3)?,
                        created: r.get(4)?,
                    })
                },
            )
            .and_then(Iterator::collect)
            .map_err(err)
    }

    /// Makes the account an old user of friends (tests of the new-account
    /// limit).
    #[cfg(test)]
    pub fn age_account(&self, account: &str, seconds: i64) {
        let db = self.db();
        db.execute(
            "UPDATE seen SET first = first - ?2 WHERE account_id = ?1",
            params![account, seconds],
        )
        .unwrap();
        db.execute(
            "UPDATE request_log SET at = at - ?2 WHERE account_id = ?1",
            params![account, seconds],
        )
        .unwrap();
    }

    /// `count` requests from made-up accounts waiting for `to` (tests).
    #[cfg(test)]
    pub fn fill_requests_to(&self, to: &str, count: usize, now: i64) {
        let db = self.db();
        for i in 0..count {
            db.execute(
                "INSERT INTO requests (from_id, to_id, created) VALUES (?1, ?2, ?3)",
                params![format!("00000000-0000-4000-9000-{i:012}"), to, now],
            )
            .unwrap();
        }
    }

    /// Moves a room's messages and members back in time (tests of join times).
    #[cfg(test)]
    pub fn age_room(&self, room: &str, seconds: i64) {
        let db = self.db();
        for sql in [
            "UPDATE messages SET created = created - ?2 WHERE room_id = ?1",
            "UPDATE members SET joined = joined - ?2 WHERE room_id = ?1",
        ] {
            db.execute(sql, params![room, seconds]).unwrap();
        }
    }

    pub fn mark_read(&self, room: &str, me: &str, upto: i64) -> Result<()> {
        self.db()
            .execute(
                "UPDATE members SET read_upto = MAX(read_upto, ?3) WHERE room_id = ?1 AND account_id = ?2",
                params![room, me, upto],
            )
            .map(drop)
            .map_err(err)
    }

    pub fn unread(&self, room: &str, me: &str) -> Result<i64> {
        self.db()
            .query_row(
                "SELECT COUNT(*) FROM messages m JOIN members s ON s.room_id = m.room_id AND s.account_id = ?2
                 WHERE m.room_id = ?1 AND m.id > s.read_upto AND m.created >= s.joined AND m.sender != ?2
                   AND m.sender NOT IN (SELECT blocked FROM blocks WHERE blocker = ?2)",
                params![room, me],
                |r| r.get(0),
            )
            .map_err(err)
    }

    /// The room and sender of a message.
    pub fn message_place(&self, id: i64) -> Result<Option<(String, String)>> {
        self.db()
            .query_row(
                "SELECT room_id, sender FROM messages WHERE id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(err)
    }

    /// Files a report with a copy of the message, so the evidence stays
    /// when the message goes.
    pub fn report(&self, message: i64, reporter: &str, reason: &str, now: i64) -> Result<()> {
        self.db()
            .execute(
                "INSERT OR IGNORE INTO reports (message_id, reporter, reason, created, room_id, sender, body)
                 SELECT ?1, ?2, ?3, ?4, room_id, sender, body FROM messages WHERE id = ?1",
                params![message, reporter, reason, now],
            )
            .map(drop)
            .map_err(err)
    }

    /// Reports for the owner, newest first; only open ones unless `all`.
    pub fn reports(&self, all: bool, limit: i64) -> Result<Vec<serde_json::Value>> {
        let db = self.db();
        let mut statement = db
            .prepare(
                "SELECT r.id, r.created, r.reason, r.reporter, r.message_id, r.room_id, r.sender, r.body,
                        r.status, r.note, (SELECT COUNT(*) FROM messages m WHERE m.id = r.message_id)
                 FROM reports r WHERE ?1 OR r.status = 'open' ORDER BY r.id DESC LIMIT ?2",
            )
            .map_err(err)?;
        statement
            .query_map(params![all, limit], |r| {
                Ok(serde_json::json!({
                    "report": r.get::<_, i64>(0)?,
                    "at": r.get::<_, i64>(1)?,
                    "reason": r.get::<_, String>(2)?,
                    "reporter": r.get::<_, String>(3)?,
                    "message": r.get::<_, i64>(4)?,
                    "room": r.get::<_, String>(5)?,
                    "sender": r.get::<_, String>(6)?,
                    "body": r.get::<_, String>(7)?,
                    "status": r.get::<_, String>(8)?,
                    "note": r.get::<_, String>(9)?,
                    "messageStillThere": r.get::<_, i64>(10)? > 0,
                }))
            })
            .and_then(Iterator::collect)
            .map_err(err)
    }

    /// Marks a report handled, with a note; false when there is none.
    pub fn resolve(&self, report: i64, note: &str, now: i64) -> Result<bool> {
        self.db()
            .execute(
                "UPDATE reports SET status = 'resolved', note = ?2, resolved = ?3 WHERE id = ?1",
                params![report, note, now],
            )
            .map(|n| n > 0)
            .map_err(err)
    }

    /// Removes a message for everyone; its reports keep their copy. The
    /// running service tells the room.
    pub fn remove_message(&self, id: i64) -> Result<bool> {
        let mut db = self.db();
        let tx = db.transaction().map_err(err)?;
        let room: Option<String> = tx
            .query_row("SELECT room_id FROM messages WHERE id = ?1", [id], |r| {
                r.get(0)
            })
            .optional()
            .map_err(err)?;
        let Some(room) = room else {
            return Ok(false);
        };
        tx.execute("DELETE FROM messages WHERE id = ?1", [id])
            .map_err(err)?;
        tx.execute(
            "INSERT INTO moderation (kind, room_id, message_id) VALUES ('message', ?1, ?2)",
            params![room, id],
        )
        .map_err(err)?;
        tx.commit().map_err(err)?;
        Ok(true)
    }

    /// Mutes or bans an account until `until` (0: for good).
    pub fn sanction(
        &self,
        account: &str,
        kind: &str,
        until: i64,
        reason: &str,
        now: i64,
    ) -> Result<()> {
        let mut db = self.db();
        let tx = db.transaction().map_err(err)?;
        tx.execute(
            "INSERT INTO sanctions (account_id, kind, until, reason, created) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT (account_id) DO UPDATE SET kind = ?2, until = ?3, reason = ?4, created = ?5",
            params![account, kind, until, reason, now],
        )
        .map_err(err)?;
        tx.execute(
            "INSERT INTO moderation (kind, account_id) VALUES ('sanction', ?1)",
            [account],
        )
        .map_err(err)?;
        tx.commit().map_err(err)
    }

    pub fn lift(&self, account: &str) -> Result<bool> {
        self.db()
            .execute("DELETE FROM sanctions WHERE account_id = ?1", [account])
            .map(|n| n > 0)
            .map_err(err)
    }

    /// The account's sanction now in force, if any.
    pub fn sanction_of(&self, account: &str, now: i64) -> Result<Option<Sanction>> {
        self.db()
            .query_row(
                "SELECT kind, until, reason FROM sanctions WHERE account_id = ?1 AND (until = 0 OR until > ?2)",
                params![account, now],
                |r| {
                    Ok(Sanction {
                        kind: r.get(0)?,
                        until: r.get(1)?,
                        reason: r.get(2)?,
                    })
                },
            )
            .optional()
            .map_err(err)
    }

    /// Sanctions in force: account, sanction.
    pub fn sanctions(&self, now: i64) -> Result<Vec<(String, Sanction)>> {
        let db = self.db();
        let mut statement = db
            .prepare(
                "SELECT account_id, kind, until, reason FROM sanctions
                 WHERE until = 0 OR until > ?1 ORDER BY created DESC",
            )
            .map_err(err)?;
        statement
            .query_map([now], |r| {
                Ok((
                    r.get(0)?,
                    Sanction {
                        kind: r.get(1)?,
                        until: r.get(2)?,
                        reason: r.get(3)?,
                    },
                ))
            })
            .and_then(Iterator::collect)
            .map_err(err)
    }

    /// Changes from the command line not yet told.
    pub fn untold(&self) -> Result<Vec<Untold>> {
        let db = self.db();
        let mut statement = db
            .prepare("SELECT id, kind, room_id, message_id, account_id FROM moderation WHERE told = 0 ORDER BY id")
            .map_err(err)?;
        statement
            .query_map([], |r| {
                Ok(Untold {
                    id: r.get(0)?,
                    kind: r.get(1)?,
                    room: r.get(2)?,
                    message: r.get(3)?,
                    account: r.get(4)?,
                })
            })
            .and_then(Iterator::collect)
            .map_err(err)
    }

    pub fn told(&self, id: i64) -> Result<()> {
        self.db()
            .execute("DELETE FROM moderation WHERE id = ?1", [id])
            .map(drop)
            .map_err(err)
    }

    // Presence.

    pub fn hidden(&self, account: &str) -> Result<bool> {
        self.exists(
            "SELECT 1 FROM hidden WHERE account_id = ?1 AND ?2 = ?2",
            account,
            "",
        )
    }

    pub fn set_hidden(&self, account: &str, hidden: bool) -> Result<()> {
        let sql = if hidden {
            "INSERT OR IGNORE INTO hidden (account_id) VALUES (?1)"
        } else {
            "DELETE FROM hidden WHERE account_id = ?1"
        };
        self.db().execute(sql, [account]).map(drop).map_err(err)
    }

    /// Messages an account wrote, oldest first, for its export.
    pub fn written_by(&self, sender: &str, limit: i64) -> Result<Vec<Message>> {
        let db = self.db();
        let mut statement = db
            .prepare(
                "SELECT id, room_id, sender, body, created FROM messages WHERE sender = ?1
                 ORDER BY id LIMIT ?2",
            )
            .map_err(err)?;
        statement
            .query_map(params![sender, limit], |r| {
                Ok(Message {
                    id: r.get(0)?,
                    room: r.get(1)?,
                    sender: r.get(2)?,
                    body: r.get(3)?,
                    created: r.get(4)?,
                })
            })
            .and_then(Iterator::collect)
            .map_err(err)
    }

    // A deleted account.

    /// Removes everything of an account that was deleted: requests, friends,
    /// blocks, its messages, its place in rooms (a group passes to who joined
    /// first; a room of two goes) and the reports it filed. Reports about its
    /// messages keep their copy. Returns who should reload: id, what changed.
    pub fn purge(&self, account: &str) -> Result<Vec<String>> {
        let mut db = self.db();
        let tx = db.transaction().map_err(err)?;
        let mut touched: Vec<String> = {
            let mut people = tx
                .prepare(
                    "SELECT b FROM friends WHERE a = ?1
                     UNION SELECT to_id FROM requests WHERE from_id = ?1
                     UNION SELECT from_id FROM requests WHERE to_id = ?1
                     UNION SELECT m2.account_id FROM members m1 JOIN members m2 ON m2.room_id = m1.room_id
                           WHERE m1.account_id = ?1 AND m2.account_id != ?1",
                )
                .map_err(err)?;
            people
                .query_map([account], |r| r.get(0))
                .and_then(Iterator::collect)
                .map_err(err)?
        };
        let rooms: Vec<(String, String, String)> = {
            let mut statement = tx
                .prepare(
                    "SELECT r.id, r.kind, r.owner FROM rooms r JOIN members m ON m.room_id = r.id
                     WHERE m.account_id = ?1",
                )
                .map_err(err)?;
            statement
                .query_map([account], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
                .and_then(Iterator::collect)
                .map_err(err)?
        };
        for (room, kind, owner) in rooms {
            if kind == "direct" {
                tx.execute("DELETE FROM rooms WHERE id = ?1", [&room])
                    .map_err(err)?;
                continue;
            }
            tx.execute(
                "DELETE FROM members WHERE room_id = ?1 AND account_id = ?2",
                params![room, account],
            )
            .map_err(err)?;
            if owner == account {
                tx.execute(
                    "UPDATE rooms SET owner = (SELECT account_id FROM members WHERE room_id = ?1
                     ORDER BY joined, account_id LIMIT 1) WHERE id = ?1",
                    [&room],
                )
                .map_err(err)?;
            }
            tx.execute(
                "DELETE FROM rooms WHERE id = ?1 AND NOT EXISTS (SELECT 1 FROM members WHERE room_id = ?1)",
                [&room],
            )
            .map_err(err)?;
        }
        for sql in [
            "DELETE FROM messages WHERE sender = ?1",
            "DELETE FROM friends WHERE a = ?1 OR b = ?1",
            "DELETE FROM requests WHERE from_id = ?1 OR to_id = ?1",
            "DELETE FROM blocks WHERE blocker = ?1 OR blocked = ?1",
            "DELETE FROM reports WHERE reporter = ?1",
            "DELETE FROM sanctions WHERE account_id = ?1",
            "DELETE FROM hidden WHERE account_id = ?1",
            "DELETE FROM seen WHERE account_id = ?1",
            "DELETE FROM request_log WHERE account_id = ?1",
            "DELETE FROM request_reports WHERE reporter = ?1",
        ] {
            tx.execute(sql, [account]).map_err(err)?;
        }
        tx.commit().map_err(err)?;
        touched.sort();
        touched.dedup();
        Ok(touched)
    }

    /// Numbers only: fit for a public log.
    pub fn counts(&self) -> Result<serde_json::Value> {
        let db = self.db();
        let one = |sql: &str| -> Result<i64> { db.query_row(sql, [], |r| r.get(0)).map_err(err) };
        Ok(serde_json::json!({
            "friendships": one("SELECT COUNT(*) / 2 FROM friends")?,
            "requests": one("SELECT COUNT(*) FROM requests")?,
            "blocks": one("SELECT COUNT(*) FROM blocks")?,
            "rooms": one("SELECT COUNT(*) FROM rooms")?,
            "groups": one("SELECT COUNT(*) FROM rooms WHERE kind = 'group'")?,
            "messages": one("SELECT COUNT(*) FROM messages")?,
            "reports": one("SELECT COUNT(*) FROM reports")?,
        }))
    }
}
