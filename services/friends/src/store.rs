//! Friends, requests, blocks, rooms and messages in SQLite
//! (`data/friends.db`). Accounts are only IDs here; nicks come from the
//! account service. Every query takes its values as parameters.

use std::path::Path;
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
"#;

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
        Ok(Self { db: Mutex::new(db) })
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

    /// Messages before `before` (newest first), leaving out senders `reader`
    /// blocked.
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
                 WHERE m.room_id = ?1 AND m.id > s.read_upto AND m.sender != ?2
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

    pub fn report(&self, message: i64, reporter: &str, reason: &str, now: i64) -> Result<()> {
        self.db()
            .execute(
                "INSERT OR IGNORE INTO reports (message_id, reporter, reason, created) VALUES (?1, ?2, ?3, ?4)",
                params![message, reporter, reason, now],
            )
            .map(drop)
            .map_err(err)
    }

    /// Reports for the owner: report, message, room, sender, reporter.
    pub fn reports(&self, limit: i64) -> Result<Vec<serde_json::Value>> {
        let db = self.db();
        let mut statement = db
            .prepare(
                "SELECT r.id, r.created, r.reason, r.reporter, m.id, m.room_id, m.sender, m.body, m.created
                 FROM reports r LEFT JOIN messages m ON m.id = r.message_id ORDER BY r.id DESC LIMIT ?1",
            )
            .map_err(err)?;
        statement
            .query_map([limit], |r| {
                Ok(serde_json::json!({
                    "report": r.get::<_, i64>(0)?,
                    "at": r.get::<_, i64>(1)?,
                    "reason": r.get::<_, String>(2)?,
                    "reporter": r.get::<_, String>(3)?,
                    "message": r.get::<_, Option<i64>>(4)?,
                    "room": r.get::<_, Option<String>>(5)?,
                    "sender": r.get::<_, Option<String>>(6)?,
                    "body": r.get::<_, Option<String>>(7)?,
                    "sent": r.get::<_, Option<i64>>(8)?,
                }))
            })
            .and_then(Iterator::collect)
            .map_err(err)
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
