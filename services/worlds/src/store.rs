//! Rooms, members, invites, devices, host sessions, tickets and admissions in
//! SQLite (`data/worlds.db`). Accounts are oauth subjects; a member's nick is
//! the one their latest access token carried. Every query takes its values as
//! parameters.
//!
//! Leases are kept in seconds. A session whose lease ran out, or that was made
//! under another authority epoch (a restored database), ends the next time
//! anyone looks at it, and so do its admissions: nothing revives it.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::Serialize;

use crate::tickets::Ticket;

/// A host lease lasts this long; the host renews it every 10 seconds.
pub const LEASE: i64 = 30;
pub const MAX_DEVICES: i64 = 10;
/// Rooms one account owns, and rooms it is in.
pub const MAX_OWNED: i64 = 10;
pub const MAX_ROOMS: i64 = 100;
/// Active members of one room, its owner included.
pub const MAX_MEMBERS: i64 = 50;
/// Invites of one room that can still be used.
pub const MAX_INVITES: i64 = 20;
/// Ended sessions, used tickets and old invites are kept this long.
const KEEP: i64 = 30 * 86_400;

const SCHEMA: &str = r#"
PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;
CREATE TABLE IF NOT EXISTS devices (
    id TEXT PRIMARY KEY,
    account TEXT NOT NULL,
    -- Unpadded base64url of the Ed25519 public key.
    key TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    created INTEGER NOT NULL,
    revoked INTEGER
);
CREATE INDEX IF NOT EXISTS devices_account ON devices(account);
CREATE TABLE IF NOT EXISTS rooms (
    id TEXT PRIMARY KEY,
    owner TEXT NOT NULL,
    name TEXT NOT NULL,
    description TEXT NOT NULL,
    -- The world's ID on the owner's computer.
    world TEXT NOT NULL,
    minecraft TEXT NOT NULL,
    revision INTEGER NOT NULL DEFAULT 1,
    created INTEGER NOT NULL,
    updated INTEGER NOT NULL,
    deleted INTEGER
);
CREATE UNIQUE INDEX IF NOT EXISTS rooms_world ON rooms(owner, world) WHERE deleted IS NULL;
CREATE TABLE IF NOT EXISTS members (
    room TEXT NOT NULL REFERENCES rooms(id),
    account TEXT NOT NULL,
    nick TEXT NOT NULL,
    -- owner, manager or member
    role TEXT NOT NULL,
    -- active, left, removed or banned
    state TEXT NOT NULL,
    joined INTEGER NOT NULL,
    changed INTEGER NOT NULL,
    PRIMARY KEY (room, account)
);
CREATE INDEX IF NOT EXISTS members_account ON members(account, state);
CREATE TABLE IF NOT EXISTS invites (
    id TEXT PRIMARY KEY,
    room TEXT NOT NULL REFERENCES rooms(id),
    -- SHA-256 of the code; the code itself is shown once and not kept.
    digest TEXT NOT NULL UNIQUE,
    created_by TEXT NOT NULL,
    created INTEGER NOT NULL,
    expires INTEGER NOT NULL,
    max_uses INTEGER NOT NULL,
    uses INTEGER NOT NULL DEFAULT 0,
    revoked INTEGER
);
CREATE INDEX IF NOT EXISTS invites_room ON invites(room);
CREATE TABLE IF NOT EXISTS sessions (
    id TEXT PRIMARY KEY,
    room TEXT NOT NULL REFERENCES rooms(id),
    host_device TEXT NOT NULL,
    epoch INTEGER NOT NULL,
    authority TEXT NOT NULL,
    -- The host's tunnel endpoint for this session (64 hex digits).
    endpoint TEXT NOT NULL,
    max_players INTEGER NOT NULL,
    -- live, stopped, superseded, lease_lost, device_revoked or room_deleted
    state TEXT NOT NULL,
    lease_until INTEGER NOT NULL,
    started INTEGER NOT NULL,
    ended INTEGER,
    -- The world's mods: the catalog version of its room pack, and the
    -- pack's SHA-256 (none for a world without mods).
    pack_version TEXT,
    pack_sha256 TEXT,
    UNIQUE (room, epoch)
);
CREATE UNIQUE INDEX IF NOT EXISTS sessions_live ON sessions(room) WHERE ended IS NULL;
CREATE TABLE IF NOT EXISTS tickets (
    jti TEXT PRIMARY KEY,
    session TEXT NOT NULL,
    account TEXT NOT NULL,
    expires INTEGER NOT NULL,
    consumed INTEGER
);
CREATE TABLE IF NOT EXISTS admissions (
    id TEXT PRIMARY KEY,
    session TEXT NOT NULL REFERENCES sessions(id),
    room TEXT NOT NULL,
    account TEXT NOT NULL,
    device TEXT NOT NULL,
    created INTEGER NOT NULL,
    -- The member was removed or left: the host kicks them, then releases.
    revoked INTEGER,
    released INTEGER
);
-- One slot per account per room until the host releases it.
CREATE UNIQUE INDEX IF NOT EXISTS admissions_slot ON admissions(room, account) WHERE released IS NULL;
CREATE INDEX IF NOT EXISTS admissions_session ON admissions(session);
-- DPoP proof IDs seen while their proofs are fresh, so a captured proof
-- cannot be replayed, after a restart either.
CREATE TABLE IF NOT EXISTS proofs (
    jti TEXT PRIMARY KEY,
    iat INTEGER NOT NULL
);
-- Guests' tunnel keys the relay may carry, one per member and session,
-- renewed by the guest every 10 seconds.
CREATE TABLE IF NOT EXISTS relay_grants (
    account TEXT NOT NULL,
    session TEXT NOT NULL,
    device TEXT NOT NULL,
    endpoint TEXT NOT NULL,
    until INTEGER NOT NULL,
    PRIMARY KEY (account, session)
);
"#;

/// A refusal with its error code, or a database failure.
#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    No(&'static str),
    Db(String),
}

impl From<rusqlite::Error> for Error {
    fn from(e: rusqlite::Error) -> Self {
        Self::Db(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, Error>;

fn no<T>(code: &'static str) -> Result<T> {
    Err(Error::No(code))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Role {
    Member,
    Manager,
    Owner,
}

impl Role {
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "owner" => Some(Self::Owner),
            "manager" => Some(Self::Manager),
            "member" => Some(Self::Member),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Owner => "owner",
            Self::Manager => "manager",
            Self::Member => "member",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Device {
    pub id: String,
    pub name: String,
    pub created: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Room {
    pub id: String,
    pub name: String,
    pub description: String,
    pub minecraft: String,
    pub revision: i64,
    pub owner: String,
    pub owner_nick: String,
    /// The caller's role.
    pub role: String,
    /// The world's ID on the owner's computer, for the owner only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub world: Option<String>,
    pub members: i64,
    pub hosting: bool,
    pub online: i64,
    pub max_players: i64,
    pub created: i64,
    pub updated: i64,
    /// The mods the live session plays, for every member.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pack: Option<Pack>,
}

/// A world's mods as the catalog keeps them: the version of its room pack
/// (hidden from the catalog, found by this ID only) and the pack's SHA-256.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Pack {
    pub version: String,
    pub sha256: String,
}

fn pack_from(version: Option<String>, sha256: Option<String>) -> Option<Pack> {
    Some(Pack {
        version: version?,
        sha256: sha256?,
    })
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Member {
    pub account: String,
    pub nick: String,
    pub role: String,
    pub state: String,
    pub joined: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Invite {
    pub id: String,
    pub created: i64,
    pub expires: i64,
    pub max_uses: i64,
    pub uses: i64,
}

/// What someone holding an invite code sees before joining.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    pub room: String,
    pub name: String,
    pub owner_nick: String,
    pub members: i64,
    pub minecraft: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub id: String,
    pub room: String,
    pub epoch: i64,
    pub endpoint: String,
    pub max_players: i64,
    pub lease_until: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pack: Option<Pack>,
}

/// What a renewal tells the host.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Renewal {
    pub lease_until: i64,
    /// Nicks of the active members, for the whitelist.
    pub members: Vec<String>,
    /// Admissions to close now: their member left or was removed.
    pub revoked: Vec<String>,
}

/// A tunnel key the relay may carry until `until`, and whose it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelayGrant {
    pub endpoint: String,
    pub account: String,
    pub until: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Admission {
    pub id: String,
    pub account: String,
    pub nick: String,
}

pub struct Store {
    db: Mutex<Connection>,
    /// The authority epoch this process serves under (outside the database).
    authority: String,
}

fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// Ends sessions whose lease ran out or that belong to another authority,
/// and releases the admissions of every ended session.
fn expire(db: &Connection, authority: &str, now: i64) -> Result<()> {
    db.execute(
        "UPDATE sessions SET state = 'lease_lost', ended = ?1
         WHERE ended IS NULL AND (lease_until < ?1 OR authority <> ?2)",
        params![now, authority],
    )?;
    db.execute(
        "UPDATE admissions SET released = ?1 WHERE released IS NULL
         AND session IN (SELECT id FROM sessions WHERE ended IS NOT NULL)",
        [now],
    )?;
    Ok(())
}

fn end_session(db: &Connection, id: &str, state: &str, now: i64) -> Result<()> {
    db.execute(
        "UPDATE sessions SET state = ?2, ended = ?3 WHERE id = ?1 AND ended IS NULL",
        params![id, state, now],
    )?;
    db.execute(
        "UPDATE admissions SET released = ?2 WHERE session = ?1 AND released IS NULL",
        params![id, now],
    )?;
    Ok(())
}

/// The caller's role in a room that exists, or `not_found` (the same answer
/// whether the room is missing or the caller is not in it).
fn role_of(db: &Connection, account: &str, room: &str) -> Result<Role> {
    let role: Option<String> = db
        .query_row(
            "SELECT m.role FROM members m JOIN rooms r ON r.id = m.room
             WHERE m.room = ?1 AND m.account = ?2 AND m.state = 'active' AND r.deleted IS NULL",
            params![room, account],
            |r| r.get(0),
        )
        .optional()?;
    role.as_deref()
        .and_then(Role::parse)
        .map_or_else(|| no("not_found"), Ok)
}

fn at_least(db: &Connection, account: &str, room: &str, role: Role) -> Result<Role> {
    let mine = role_of(db, account, room)?;
    if mine < role {
        return no("forbidden");
    }
    Ok(mine)
}

fn bump(db: &Connection, room: &str, now: i64) -> Result<()> {
    db.execute(
        "UPDATE rooms SET revision = revision + 1, updated = ?2 WHERE id = ?1",
        params![room, now],
    )?;
    Ok(())
}

/// Marks a member's open admissions revoked, so the host closes them.
fn revoke_admissions(db: &Connection, room: &str, account: &str, now: i64) -> Result<()> {
    db.execute(
        "UPDATE admissions SET revoked = ?3
         WHERE room = ?1 AND account = ?2 AND released IS NULL AND revoked IS NULL",
        params![room, account, now],
    )?;
    Ok(())
}

const ROOM_SELECT: &str = "
SELECT r.id, r.name, r.description, r.minecraft, r.revision, r.owner, o.nick, m.role, r.world,
       r.created, r.updated,
       (SELECT COUNT(*) FROM members WHERE room = r.id AND state = 'active'),
       s.id, s.max_players,
       MAX((SELECT COUNT(*) FROM admissions a WHERE a.session = s.id AND a.released IS NULL),
           COALESCE(s.players, 0)),
       s.pack_version, s.pack_sha256
FROM rooms r
JOIN members m ON m.room = r.id AND m.account = ?1 AND m.state = 'active'
JOIN members o ON o.room = r.id AND o.account = r.owner
LEFT JOIN sessions s ON s.room = r.id AND s.ended IS NULL
WHERE r.deleted IS NULL";

fn room_from(r: &rusqlite::Row<'_>) -> rusqlite::Result<Room> {
    let role: String = r.get(7)?;
    let session: Option<String> = r.get(12)?;
    let world: String = r.get(8)?;
    Ok(Room {
        id: r.get(0)?,
        name: r.get(1)?,
        description: r.get(2)?,
        minecraft: r.get(3)?,
        revision: r.get(4)?,
        owner: r.get(5)?,
        owner_nick: r.get(6)?,
        world: (role == "owner").then_some(world),
        role,
        created: r.get(9)?,
        updated: r.get(10)?,
        members: r.get(11)?,
        hosting: session.is_some(),
        max_players: r.get::<_, Option<i64>>(13)?.unwrap_or(0),
        online: r.get::<_, Option<i64>>(14)?.unwrap_or(0),
        pack: pack_from(r.get(15)?, r.get(16)?),
    })
}

fn session_from(r: &rusqlite::Row<'_>) -> rusqlite::Result<Session> {
    Ok(Session {
        id: r.get(0)?,
        room: r.get(1)?,
        epoch: r.get(2)?,
        endpoint: r.get(3)?,
        max_players: r.get(4)?,
        lease_until: r.get(5)?,
        pack: pack_from(r.get(7)?, r.get(8)?),
    })
}

/// Columns added after the first release; SQLite adds them in place.
fn migrate(db: &Connection) -> rusqlite::Result<()> {
    let has_players: bool = db.query_row(
        "SELECT EXISTS (SELECT 1 FROM pragma_table_info('sessions') WHERE name = 'players')",
        [],
        |r| r.get(0),
    )?;
    if !has_players {
        db.execute_batch("ALTER TABLE sessions ADD COLUMN players INTEGER")?;
    }
    let has_pack: bool = db.query_row(
        "SELECT EXISTS (SELECT 1 FROM pragma_table_info('sessions') WHERE name = 'pack_version')",
        [],
        |r| r.get(0),
    )?;
    if !has_pack {
        db.execute_batch(
            "ALTER TABLE sessions ADD COLUMN pack_version TEXT;
             ALTER TABLE sessions ADD COLUMN pack_sha256 TEXT;",
        )?;
    }
    Ok(())
}

const SESSION_SELECT: &str =
    "SELECT id, room, epoch, endpoint, max_players, lease_until, host_device,
       pack_version, pack_sha256 FROM sessions";

impl Store {
    pub fn open(dir: &Path, authority: &str) -> std::result::Result<Self, String> {
        std::fs::create_dir_all(dir).map_err(|e| format!("creating {}: {e}", dir.display()))?;
        let path = dir.join("worlds.db");
        let db = Connection::open(&path).map_err(|e| format!("opening {}: {e}", path.display()))?;
        db.busy_timeout(std::time::Duration::from_secs(5))
            .map_err(|e| e.to_string())?;
        db.execute_batch(SCHEMA).map_err(|e| e.to_string())?;
        migrate(&db).map_err(|e| e.to_string())?;
        Ok(Self {
            db: Mutex::new(db),
            authority: authority.to_owned(),
        })
    }

    pub fn authority(&self) -> &str {
        &self.authority
    }

    fn db(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.db
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// A consistent copy of the database (`VACUUM INTO`, safe while the
    /// service runs) as `worlds-<time>.db` in `dir`, readable by this user
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
            "worlds-{}.db",
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
                    .is_some_and(|n| n.starts_with("worlds-") && n.ends_with(".db"))
            })
            .collect();
        copies.sort();
        let extra = copies.len().saturating_sub(keep);
        for old in &copies[..extra] {
            std::fs::remove_file(old).map_err(|e| e.to_string())?;
        }
        Ok(path)
    }

    /// Keeps a member's nick as their latest token has it.
    pub fn note_nick(&self, account: &str, nick: &str) -> Result<()> {
        self.db().execute(
            "UPDATE members SET nick = ?2 WHERE account = ?1 AND nick <> ?2",
            params![account, nick],
        )?;
        Ok(())
    }

    // Devices.

    /// Registers the device key the caller proved; the same key again only
    /// renames it.
    pub fn register_device(
        &self,
        account: &str,
        key: &str,
        name: &str,
        now: i64,
    ) -> Result<Device> {
        let db = self.db();
        let existing: Option<(String, String, Option<i64>, i64)> = db
            .query_row(
                "SELECT id, account, revoked, created FROM devices WHERE key = ?1",
                [key],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()?;
        if let Some((id, owner, revoked, created)) = existing {
            if owner != account || revoked.is_some() {
                return no("device_key_taken");
            }
            db.execute(
                "UPDATE devices SET name = ?2 WHERE id = ?1",
                params![id, name],
            )?;
            return Ok(Device {
                id,
                name: name.to_owned(),
                created,
            });
        }
        let count: i64 = db.query_row(
            "SELECT COUNT(*) FROM devices WHERE account = ?1 AND revoked IS NULL",
            [account],
            |r| r.get(0),
        )?;
        if count >= MAX_DEVICES {
            return no("device_limit");
        }
        let id = new_id();
        db.execute(
            "INSERT INTO devices (id, account, key, name, created) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![id, account, key, name, now],
        )?;
        Ok(Device {
            id,
            name: name.to_owned(),
            created: now,
        })
    }

    pub fn devices(&self, account: &str) -> Result<Vec<Device>> {
        let db = self.db();
        let mut statement = db.prepare(
            "SELECT id, name, created FROM devices
             WHERE account = ?1 AND revoked IS NULL ORDER BY created",
        )?;
        let devices = statement
            .query_map([account], |r| {
                Ok(Device {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    created: r.get(2)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        Ok(devices)
    }

    /// The caller's active device with this key.
    pub fn device_of(&self, account: &str, key: &str) -> Result<Option<String>> {
        Ok(self
            .db()
            .query_row(
                "SELECT id FROM devices WHERE account = ?1 AND key = ?2 AND revoked IS NULL",
                params![account, key],
                |r| r.get(0),
            )
            .optional()?)
    }

    /// Revokes a device: the sessions it hosts end and its admissions close.
    /// Returns the rooms that changed.
    pub fn revoke_device(&self, account: &str, id: &str, now: i64) -> Result<Vec<String>> {
        let mut db = self.db();
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let changed = tx.execute(
            "UPDATE devices SET revoked = ?3 WHERE id = ?1 AND account = ?2 AND revoked IS NULL",
            params![id, account, now],
        )?;
        if changed == 0 {
            return no("not_found");
        }
        let hosted: Vec<(String, String)> = {
            let mut statement = tx.prepare(
                "SELECT id, room FROM sessions WHERE host_device = ?1 AND ended IS NULL",
            )?;
            statement
                .query_map([id], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<rusqlite::Result<_>>()?
        };
        for (session, _) in &hosted {
            end_session(&tx, session, "device_revoked", now)?;
        }
        let mut rooms: Vec<String> = hosted.into_iter().map(|(_, room)| room).collect();
        {
            let mut statement = tx.prepare(
                "SELECT DISTINCT room FROM admissions WHERE device = ?1 AND released IS NULL",
            )?;
            let guest: Vec<String> = statement
                .query_map([id], |r| r.get(0))?
                .collect::<rusqlite::Result<_>>()?;
            rooms.extend(guest);
        }
        tx.execute(
            "UPDATE admissions SET revoked = ?2
             WHERE device = ?1 AND released IS NULL AND revoked IS NULL",
            params![id, now],
        )?;
        tx.commit()?;
        rooms.sort();
        rooms.dedup();
        Ok(rooms)
    }

    // Rooms.

    pub fn create_room(
        &self,
        owner: &str,
        nick: &str,
        draft: &RoomDraft<'_>,
        now: i64,
    ) -> Result<Room> {
        let id = new_id();
        {
            let mut db = self.db();
            let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let owned: i64 = tx.query_row(
                "SELECT COUNT(*) FROM rooms WHERE owner = ?1 AND deleted IS NULL",
                [owner],
                |r| r.get(0),
            )?;
            let joined: i64 = tx.query_row(
                "SELECT COUNT(*) FROM members m JOIN rooms r ON r.id = m.room
                 WHERE m.account = ?1 AND m.state = 'active' AND r.deleted IS NULL",
                [owner],
                |r| r.get(0),
            )?;
            if owned >= MAX_OWNED || joined >= MAX_ROOMS {
                return no("room_limit");
            }
            let taken: bool = tx.query_row(
                "SELECT EXISTS (SELECT 1 FROM rooms WHERE owner = ?1 AND world = ?2 AND deleted IS NULL)",
                params![owner, draft.world],
                |r| r.get(0),
            )?;
            if taken {
                return no("world_taken");
            }
            tx.execute(
                "INSERT INTO rooms (id, owner, name, description, world, minecraft, created, updated)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)",
                params![
                    id,
                    owner,
                    draft.name,
                    draft.description,
                    draft.world,
                    draft.minecraft,
                    now
                ],
            )?;
            tx.execute(
                "INSERT INTO members (room, account, nick, role, state, joined, changed)
                 VALUES (?1, ?2, ?3, 'owner', 'active', ?4, ?4)",
                params![id, owner, nick, now],
            )?;
            tx.commit()?;
        }
        self.room(owner, &id, now)
    }

    pub fn room(&self, account: &str, id: &str, now: i64) -> Result<Room> {
        let db = self.db();
        expire(&db, &self.authority, now)?;
        db.query_row(
            &format!("{ROOM_SELECT} AND r.id = ?2"),
            params![account, id],
            room_from,
        )
        .optional()?
        .map_or_else(|| no("not_found"), Ok)
    }

    pub fn rooms(&self, account: &str, now: i64) -> Result<Vec<Room>> {
        let db = self.db();
        expire(&db, &self.authority, now)?;
        let mut statement = db.prepare(&format!("{ROOM_SELECT} ORDER BY r.updated DESC"))?;
        let rooms = statement
            .query_map([account], room_from)?
            .collect::<rusqlite::Result<_>>()?;
        Ok(rooms)
    }

    /// Renames or describes a room, if `revision` is still the current one.
    pub fn update_room(
        &self,
        account: &str,
        id: &str,
        revision: i64,
        name: Option<&str>,
        description: Option<&str>,
        now: i64,
    ) -> Result<Room> {
        {
            let db = self.db();
            at_least(&db, account, id, Role::Manager)?;
            let changed = db.execute(
                "UPDATE rooms SET name = COALESCE(?3, name), description = COALESCE(?4, description),
                 revision = revision + 1, updated = ?5
                 WHERE id = ?1 AND revision = ?2 AND deleted IS NULL",
                params![id, revision, name, description, now],
            )?;
            if changed == 0 {
                return no("revision_conflict");
            }
        }
        self.room(account, id, now)
    }

    /// Deletes a room for everyone: its session ends, its invites stop.
    /// Returns the accounts that were in it.
    pub fn delete_room(&self, account: &str, id: &str, now: i64) -> Result<Vec<String>> {
        let mut db = self.db();
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        at_least(&tx, account, id, Role::Owner)?;
        let members = active_accounts(&tx, id)?;
        tx.execute(
            "UPDATE rooms SET deleted = ?2, updated = ?2 WHERE id = ?1",
            params![id, now],
        )?;
        let live: Option<String> = tx
            .query_row(
                "SELECT id FROM sessions WHERE room = ?1 AND ended IS NULL",
                [id],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(session) = live {
            end_session(&tx, &session, "room_deleted", now)?;
        }
        tx.execute(
            "UPDATE invites SET revoked = ?2 WHERE room = ?1 AND revoked IS NULL",
            params![id, now],
        )?;
        tx.commit()?;
        Ok(members)
    }

    /// Accounts active in a room (to tell them it changed).
    pub fn members_of(&self, room: &str) -> Result<Vec<String>> {
        active_accounts(&self.db(), room)
    }

    // Members.

    /// Managers and the owner see everyone who was ever in the room; members
    /// see the active ones.
    pub fn members(&self, account: &str, room: &str) -> Result<Vec<Member>> {
        let db = self.db();
        let role = role_of(&db, account, room)?;
        let sql = if role >= Role::Manager {
            "SELECT account, nick, role, state, joined FROM members WHERE room = ?1 ORDER BY joined"
        } else {
            "SELECT account, nick, role, state, joined FROM members
             WHERE room = ?1 AND state = 'active' ORDER BY joined"
        };
        let mut statement = db.prepare(sql)?;
        let members = statement
            .query_map([room], |r| {
                Ok(Member {
                    account: r.get(0)?,
                    nick: r.get(1)?,
                    role: r.get(2)?,
                    state: r.get(3)?,
                    joined: r.get(4)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        Ok(members)
    }

    /// Removes or bans a member, or lifts a ban (the person may then come
    /// back with a new invite). The owner acts on anyone else, a manager on
    /// members only.
    pub fn moderate(
        &self,
        actor: &str,
        room: &str,
        target: &str,
        action: Moderation,
        now: i64,
    ) -> Result<()> {
        let mut db = self.db();
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mine = at_least(&tx, actor, room, Role::Manager)?;
        if actor == target {
            return no("forbidden");
        }
        let theirs: Option<(String, String)> = tx
            .query_row(
                "SELECT role, state FROM members WHERE room = ?1 AND account = ?2",
                params![room, target],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let Some((role, state)) = theirs else {
            return no("not_found");
        };
        let role = Role::parse(&role).unwrap_or(Role::Owner);
        if role >= mine {
            return no("forbidden");
        }
        let next = match (action, state.as_str()) {
            (Moderation::Remove, "active") => "removed",
            (Moderation::Ban, "active" | "left" | "removed") => "banned",
            (Moderation::Unban, "banned") => "removed",
            _ => return no("not_found"),
        };
        tx.execute(
            "UPDATE members SET state = ?3, role = 'member', changed = ?4 WHERE room = ?1 AND account = ?2",
            params![room, target, next, now],
        )?;
        if state == "active" {
            revoke_admissions(&tx, room, target, now)?;
            bump(&tx, room, now)?;
        }
        tx.commit()?;
        Ok(())
    }

    /// The owner makes an active member a manager or back.
    pub fn set_role(
        &self,
        actor: &str,
        room: &str,
        target: &str,
        role: Role,
        now: i64,
    ) -> Result<()> {
        let db = self.db();
        at_least(&db, actor, room, Role::Owner)?;
        if role == Role::Owner {
            return no("bad_request");
        }
        let changed = db.execute(
            "UPDATE members SET role = ?3, changed = ?4
             WHERE room = ?1 AND account = ?2 AND state = 'active' AND role <> 'owner'",
            params![room, target, role.as_str(), now],
        )?;
        if changed == 0 {
            return no("not_found");
        }
        bump(&db, room, now)
    }

    pub fn leave(&self, account: &str, room: &str, now: i64) -> Result<()> {
        let mut db = self.db();
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if role_of(&tx, account, room)? == Role::Owner {
            return no("owner_cannot_leave");
        }
        tx.execute(
            "UPDATE members SET state = 'left', role = 'member', changed = ?3
             WHERE room = ?1 AND account = ?2",
            params![room, account, now],
        )?;
        revoke_admissions(&tx, room, account, now)?;
        bump(&tx, room, now)?;
        tx.commit()?;
        Ok(())
    }

    // Invites.

    pub fn create_invite(
        &self,
        actor: &str,
        room: &str,
        digest: &str,
        hours: i64,
        uses: i64,
        now: i64,
    ) -> Result<Invite> {
        let db = self.db();
        at_least(&db, actor, room, Role::Manager)?;
        let open: i64 = db.query_row(
            "SELECT COUNT(*) FROM invites
             WHERE room = ?1 AND revoked IS NULL AND expires > ?2 AND uses < max_uses",
            params![room, now],
            |r| r.get(0),
        )?;
        if open >= MAX_INVITES {
            return no("invite_limit");
        }
        let invite = Invite {
            id: new_id(),
            created: now,
            expires: now + hours * 3600,
            max_uses: uses,
            uses: 0,
        };
        db.execute(
            "INSERT INTO invites (id, room, digest, created_by, created, expires, max_uses)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![invite.id, room, digest, actor, now, invite.expires, uses],
        )?;
        Ok(invite)
    }

    /// Invites that can still be used.
    pub fn invites(&self, actor: &str, room: &str, now: i64) -> Result<Vec<Invite>> {
        let db = self.db();
        at_least(&db, actor, room, Role::Manager)?;
        let mut statement = db.prepare(
            "SELECT id, created, expires, max_uses, uses FROM invites
             WHERE room = ?1 AND revoked IS NULL AND expires > ?2 AND uses < max_uses
             ORDER BY created",
        )?;
        let invites = statement
            .query_map(params![room, now], |r| {
                Ok(Invite {
                    id: r.get(0)?,
                    created: r.get(1)?,
                    expires: r.get(2)?,
                    max_uses: r.get(3)?,
                    uses: r.get(4)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        Ok(invites)
    }

    pub fn revoke_invite(&self, actor: &str, room: &str, invite: &str, now: i64) -> Result<()> {
        let db = self.db();
        at_least(&db, actor, room, Role::Manager)?;
        let changed = db.execute(
            "UPDATE invites SET revoked = ?3 WHERE id = ?1 AND room = ?2 AND revoked IS NULL",
            params![invite, room, now],
        )?;
        if changed == 0 {
            return no("not_found");
        }
        Ok(())
    }

    /// The room behind a usable invite. Every unusable invite gets the same
    /// answer, so a code tells nothing more than whether it works.
    fn usable_invite(db: &Connection, digest: &str, now: i64) -> Result<(String, String)> {
        db.query_row(
            "SELECT i.id, i.room FROM invites i JOIN rooms r ON r.id = i.room
             WHERE i.digest = ?1 AND i.revoked IS NULL AND i.expires > ?2
             AND i.uses < i.max_uses AND r.deleted IS NULL",
            params![digest, now],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?
        .map_or_else(|| no("invite_invalid"), Ok)
    }

    pub fn preview(&self, digest: &str, now: i64) -> Result<Preview> {
        let db = self.db();
        let (_, room) = Self::usable_invite(&db, digest, now)?;
        Ok(db.query_row(
            "SELECT r.id, r.name, o.nick, r.minecraft,
             (SELECT COUNT(*) FROM members WHERE room = r.id AND state = 'active')
             FROM rooms r JOIN members o ON o.room = r.id AND o.account = r.owner
             WHERE r.id = ?1",
            [room],
            |r| {
                Ok(Preview {
                    room: r.get(0)?,
                    name: r.get(1)?,
                    owner_nick: r.get(2)?,
                    minecraft: r.get(3)?,
                    members: r.get(4)?,
                })
            },
        )?)
    }

    /// Joins the room of an invite. Someone already in it keeps their place
    /// without using the invite up; a banned account cannot come back.
    /// Returns the room.
    pub fn redeem(&self, account: &str, nick: &str, digest: &str, now: i64) -> Result<String> {
        let mut db = self.db();
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (invite, room) = Self::usable_invite(&tx, digest, now)?;
        let state: Option<String> = tx
            .query_row(
                "SELECT state FROM members WHERE room = ?1 AND account = ?2",
                params![room, account],
                |r| r.get(0),
            )
            .optional()?;
        match state.as_deref() {
            Some("active") => return Ok(room),
            Some("banned") => return no("banned"),
            _ => {}
        }
        let members: i64 = tx.query_row(
            "SELECT COUNT(*) FROM members WHERE room = ?1 AND state = 'active'",
            [&room],
            |r| r.get(0),
        )?;
        if members >= MAX_MEMBERS {
            return no("room_full");
        }
        let joined: i64 = tx.query_row(
            "SELECT COUNT(*) FROM members m JOIN rooms r ON r.id = m.room
             WHERE m.account = ?1 AND m.state = 'active' AND r.deleted IS NULL",
            [account],
            |r| r.get(0),
        )?;
        if joined >= MAX_ROOMS {
            return no("room_limit");
        }
        // The last use cannot be taken twice: the row changes only while a
        // use is left, inside this transaction.
        let used = tx.execute(
            "UPDATE invites SET uses = uses + 1 WHERE id = ?1 AND uses < max_uses",
            [&invite],
        )?;
        if used == 0 {
            return no("invite_invalid");
        }
        tx.execute(
            "INSERT INTO members (room, account, nick, role, state, joined, changed)
             VALUES (?1, ?2, ?3, 'member', 'active', ?4, ?4)
             ON CONFLICT (room, account) DO UPDATE SET
             nick = excluded.nick, role = 'member', state = 'active', joined = ?4, changed = ?4",
            params![room, account, nick, now],
        )?;
        bump(&tx, &room, now)?;
        tx.commit()?;
        Ok(room)
    }

    // Host sessions.

    /// The owner starts hosting from one of their devices. A session still
    /// live (the same owner on another start, or a host that crashed) is
    /// superseded: one room has one live session.
    #[allow(clippy::too_many_arguments)]
    pub fn start_session(
        &self,
        account: &str,
        room: &str,
        device: &str,
        endpoint: &str,
        max_players: i64,
        pack: Option<&Pack>,
        now: i64,
    ) -> Result<Session> {
        let mut db = self.db();
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        expire(&tx, &self.authority, now)?;
        at_least(&tx, account, room, Role::Owner)?;
        let live: Option<String> = tx
            .query_row(
                "SELECT id FROM sessions WHERE room = ?1 AND ended IS NULL",
                [room],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(old) = live {
            end_session(&tx, &old, "superseded", now)?;
        }
        let epoch: i64 = tx.query_row(
            "SELECT COALESCE(MAX(epoch), 0) + 1 FROM sessions WHERE room = ?1",
            [room],
            |r| r.get(0),
        )?;
        let session = Session {
            id: new_id(),
            room: room.to_owned(),
            epoch,
            endpoint: endpoint.to_owned(),
            max_players,
            lease_until: now + LEASE,
            pack: pack.cloned(),
        };
        tx.execute(
            "INSERT INTO sessions (id, room, host_device, epoch, authority, endpoint, max_players,
             state, lease_until, started, pack_version, pack_sha256)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'live', ?8, ?9, ?10, ?11)",
            params![
                session.id,
                room,
                device,
                epoch,
                self.authority,
                endpoint,
                max_players,
                session.lease_until,
                now,
                pack.map(|p| &p.version),
                pack.map(|p| &p.sha256)
            ],
        )?;
        tx.commit()?;
        Ok(session)
    }

    /// A live session hosted by `device`.
    fn hosted(db: &Connection, device: &str, session: &str) -> Result<Session> {
        let found: Option<(Session, String)> = db
            .query_row(
                &format!("{SESSION_SELECT} WHERE id = ?1 AND ended IS NULL"),
                [session],
                |r| Ok((session_from(r)?, r.get(6)?)),
            )
            .optional()?;
        match found {
            Some((session, host)) if host == device => Ok(session),
            Some(_) => no("not_found"),
            None => no("session_ended"),
        }
    }

    /// Extends the host's lease. A renewal after the lease ran out finds the
    /// session ended.
    /// Keeps the lease; `players` is the host's count of players on the
    /// server (its own game included), shown as the room's online count.
    pub fn renew(
        &self,
        device: &str,
        session: &str,
        players: Option<u32>,
        now: i64,
    ) -> Result<Renewal> {
        let db = self.db();
        expire(&db, &self.authority, now)?;
        let found = Self::hosted(&db, device, session)?;
        let lease_until = now + LEASE;
        db.execute(
            "UPDATE sessions SET lease_until = ?2, players = COALESCE(?3, players) WHERE id = ?1",
            params![session, lease_until, players],
        )?;
        let members = {
            let mut statement = db.prepare(
                "SELECT nick FROM members WHERE room = ?1 AND state = 'active' ORDER BY nick",
            )?;
            statement
                .query_map([&found.room], |r| r.get(0))?
                .collect::<rusqlite::Result<_>>()?
        };
        let revoked = {
            let mut statement = db.prepare(
                "SELECT id FROM admissions
                 WHERE session = ?1 AND released IS NULL AND revoked IS NOT NULL ORDER BY id",
            )?;
            statement
                .query_map([session], |r| r.get(0))?
                .collect::<rusqlite::Result<_>>()?
        };
        Ok(Renewal {
            lease_until,
            members,
            revoked,
        })
    }

    /// Ends hosting; returns the room.
    pub fn stop_session(&self, device: &str, session: &str, now: i64) -> Result<String> {
        let db = self.db();
        expire(&db, &self.authority, now)?;
        let found = Self::hosted(&db, device, session)?;
        end_session(&db, session, "stopped", now)?;
        Ok(found.room)
    }

    /// Records a ticket for an active member of a room that is being
    /// hosted; returns the session it is for.
    pub fn issue_ticket(
        &self,
        account: &str,
        room: &str,
        jti: &str,
        expires: i64,
        now: i64,
    ) -> Result<Session> {
        let db = self.db();
        expire(&db, &self.authority, now)?;
        role_of(&db, account, room)?;
        let session = db
            .query_row(
                &format!("{SESSION_SELECT} WHERE room = ?1 AND ended IS NULL"),
                [room],
                session_from,
            )
            .optional()?;
        let Some(session) = session else {
            return no("not_hosting");
        };
        db.execute(
            "INSERT INTO tickets (jti, session, account, expires) VALUES (?1, ?2, ?3, ?4)",
            params![jti, session.id, account, expires],
        )?;
        Ok(session)
    }

    /// The host consumes a ticket (already checked for its signature and
    /// time) for a guest whose tunnel key is `peer`. The ticket is used up
    /// whatever the answer.
    pub fn admit(
        &self,
        device: &str,
        session: &str,
        ticket: &Ticket,
        peer: &str,
        now: i64,
    ) -> Result<Admission> {
        let mut db = self.db();
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        expire(&tx, &self.authority, now)?;
        let found = Self::hosted(&tx, device, session)?;
        if ticket.ses != found.id
            || ticket.room != found.room
            || ticket.hep != found.epoch
            || ticket.aep != self.authority
        {
            return no("wrong_session");
        }
        let consumed = tx.execute(
            "UPDATE tickets SET consumed = ?2
             WHERE jti = ?1 AND consumed IS NULL AND session = ?3 AND account = ?4 AND expires > ?2",
            params![ticket.jti, now, found.id, ticket.sub],
        )?;
        if consumed == 0 {
            return no("ticket_invalid");
        }
        let outcome = Self::admit_member(&tx, &found, ticket, peer, now)?;
        tx.commit()?;
        outcome.map_or_else(no, Ok)
    }

    /// The checks after the ticket was used up; a refusal here still keeps
    /// the ticket used.
    fn admit_member(
        db: &Connection,
        session: &Session,
        ticket: &Ticket,
        peer: &str,
        now: i64,
    ) -> Result<std::result::Result<Admission, &'static str>> {
        if ticket.epk != peer {
            return Ok(Err("ticket_invalid"));
        }
        let device_ok: bool = db.query_row(
            "SELECT EXISTS (SELECT 1 FROM devices WHERE id = ?1 AND account = ?2 AND revoked IS NULL)",
            params![ticket.dev, ticket.sub],
            |r| r.get(0),
        )?;
        if !device_ok {
            return Ok(Err("ticket_invalid"));
        }
        let nick: Option<String> = db
            .query_row(
                "SELECT nick FROM members WHERE room = ?1 AND account = ?2 AND state = 'active'",
                params![session.room, ticket.sub],
                |r| r.get(0),
            )
            .optional()?;
        let Some(nick) = nick else {
            return Ok(Err("not_member"));
        };
        let taken: bool = db.query_row(
            "SELECT EXISTS (SELECT 1 FROM admissions
             WHERE room = ?1 AND account = ?2 AND released IS NULL)",
            params![session.room, ticket.sub],
            |r| r.get(0),
        )?;
        if taken {
            return Ok(Err("already_connected"));
        }
        let online: i64 = db.query_row(
            "SELECT COUNT(*) FROM admissions WHERE session = ?1 AND released IS NULL",
            [&session.id],
            |r| r.get(0),
        )?;
        if online >= session.max_players {
            return Ok(Err("room_full"));
        }
        let id = new_id();
        db.execute(
            "INSERT INTO admissions (id, session, room, account, device, created)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![id, session.id, session.room, ticket.sub, ticket.dev, now],
        )?;
        Ok(Ok(Admission {
            id,
            account: ticket.sub.clone(),
            nick,
        }))
    }

    /// The host says the player left: the slot is free again.
    /// Returns the room.
    pub fn release(
        &self,
        device: &str,
        session: &str,
        admission: &str,
        now: i64,
    ) -> Result<String> {
        let db = self.db();
        expire(&db, &self.authority, now)?;
        let found = Self::hosted(&db, device, session)?;
        let changed = db.execute(
            "UPDATE admissions SET released = ?3 WHERE id = ?1 AND session = ?2 AND released IS NULL",
            params![admission, session, now],
        )?;
        if changed == 0 {
            return no("not_found");
        }
        Ok(found.room)
    }

    /// Lets the relay carry a member's tunnel key for the room's live
    /// session; a new key replaces the member's old one. Returns how long it
    /// holds.
    pub fn enroll(
        &self,
        account: &str,
        device: &str,
        room: &str,
        endpoint: &str,
        now: i64,
    ) -> Result<i64> {
        let db = self.db();
        expire(&db, &self.authority, now)?;
        role_of(&db, account, room)?;
        let session: Option<String> = db
            .query_row(
                "SELECT id FROM sessions WHERE room = ?1 AND ended IS NULL",
                [room],
                |r| r.get(0),
            )
            .optional()?;
        let Some(session) = session else {
            return no("not_hosting");
        };
        let until = now + LEASE;
        db.execute(
            "INSERT INTO relay_grants (account, session, device, endpoint, until)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT (account, session) DO UPDATE SET
             device = excluded.device, endpoint = excluded.endpoint, until = excluded.until",
            params![account, session, device, endpoint, until],
        )?;
        Ok(until)
    }

    /// What the relay may carry now: the hosts of live sessions while their
    /// lease holds, and the enrolled keys of their active members.
    pub fn relay_allow(&self, now: i64) -> Result<Vec<RelayGrant>> {
        let db = self.db();
        expire(&db, &self.authority, now)?;
        let mut statement = db.prepare(
            "SELECT s.endpoint, r.owner, s.lease_until FROM sessions s
             JOIN rooms r ON r.id = s.room
             WHERE s.ended IS NULL AND s.lease_until >= ?1
             UNION ALL
             SELECT g.endpoint, g.account, MIN(g.until, s.lease_until) FROM relay_grants g
             JOIN sessions s ON s.id = g.session AND s.ended IS NULL
             JOIN members m ON m.room = s.room AND m.account = g.account AND m.state = 'active'
             JOIN devices d ON d.id = g.device AND d.revoked IS NULL
             WHERE g.until >= ?1 AND s.lease_until >= ?1",
        )?;
        let grants = statement
            .query_map([now], |r| {
                Ok(RelayGrant {
                    endpoint: r.get(0)?,
                    account: r.get(1)?,
                    until: r.get(2)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        Ok(grants)
    }

    /// Forgets a deleted account (D-064): the rooms it owns go with all
    /// they hold, and so do its places in other rooms, its devices, invites,
    /// tickets and relay keys. Its open admissions are revoked, so their
    /// hosts close them at the next renewal, and name no account any more.
    /// Returns the rooms that changed, each with the accounts that lost it.
    pub fn purge(&self, account: &str, now: i64) -> Result<Vec<(String, Vec<String>)>> {
        let mut db = self.db();
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let owned: Vec<String> = {
            let mut statement = tx.prepare("SELECT id FROM rooms WHERE owner = ?1")?;
            statement
                .query_map([account], |r| r.get(0))?
                .collect::<rusqlite::Result<_>>()?
        };
        let mut changed = Vec::new();
        for room in owned {
            let mut lost = active_accounts(&tx, &room)?;
            lost.retain(|a| a != account);
            for sql in [
                "DELETE FROM admissions WHERE session IN (SELECT id FROM sessions WHERE room = ?1)",
                "DELETE FROM tickets WHERE session IN (SELECT id FROM sessions WHERE room = ?1)",
                "DELETE FROM relay_grants WHERE session IN (SELECT id FROM sessions WHERE room = ?1)",
                "DELETE FROM sessions WHERE room = ?1",
                "DELETE FROM invites WHERE room = ?1",
                "DELETE FROM members WHERE room = ?1",
                "DELETE FROM rooms WHERE id = ?1",
            ] {
                tx.execute(sql, [&room])?;
            }
            changed.push((room, lost));
        }
        let joined: Vec<String> = {
            let mut statement =
                tx.prepare("SELECT room FROM members WHERE account = ?1 AND state = 'active'")?;
            statement
                .query_map([account], |r| r.get(0))?
                .collect::<rusqlite::Result<_>>()?
        };
        for room in joined {
            revoke_admissions(&tx, &room, account, now)?;
            bump(&tx, &room, now)?;
            changed.push((room, Vec::new()));
        }
        for sql in [
            "DELETE FROM members WHERE account = ?1",
            "DELETE FROM invites WHERE created_by = ?1",
            "DELETE FROM tickets WHERE account = ?1",
            "DELETE FROM relay_grants WHERE account = ?1",
            "DELETE FROM admissions WHERE account = ?1 AND released IS NOT NULL",
            "DELETE FROM devices WHERE account = ?1",
        ] {
            tx.execute(sql, [account])?;
        }
        tx.execute(
            "UPDATE admissions SET account = ?2 WHERE account = ?1",
            params![account, format!("deleted-{}", new_id())],
        )?;
        tx.commit()?;
        Ok(changed)
    }

    /// True the first time a DPoP proof's `jti` is seen.
    pub fn first_proof(&self, jti: &str, iat: i64) -> Result<bool> {
        let added = self.db().execute(
            "INSERT OR IGNORE INTO proofs (jti, iat) VALUES (?1, ?2)",
            params![jti, iat],
        )?;
        Ok(added == 1)
    }

    /// Ends what ran out and forgets what is old.
    pub fn sweep(&self, now: i64) -> Result<()> {
        let db = self.db();
        expire(&db, &self.authority, now)?;
        db.execute(
            "DELETE FROM proofs WHERE iat < ?1",
            [now - crate::auth::PROOF_WINDOW],
        )?;
        db.execute("DELETE FROM tickets WHERE expires < ?1", [now - 3600])?;
        db.execute("DELETE FROM relay_grants WHERE until < ?1", [now - 3600])?;
        db.execute(
            "DELETE FROM invites WHERE expires < ?1 OR revoked < ?1",
            [now - KEEP],
        )?;
        db.execute(
            "DELETE FROM admissions WHERE session IN
             (SELECT id FROM sessions WHERE ended < ?1)",
            [now - KEEP],
        )?;
        db.execute("DELETE FROM sessions WHERE ended < ?1", [now - KEEP])?;
        Ok(())
    }

    /// Numbers only, fit for a public log.
    pub fn counts(&self, now: i64) -> Result<serde_json::Value> {
        let db = self.db();
        let one = |sql: &str| -> Result<i64> { Ok(db.query_row(sql, [now], |r| r.get(0))?) };
        Ok(serde_json::json!({
            "rooms": one("SELECT COUNT(*) FROM rooms WHERE deleted IS NULL AND ?1 = ?1")?,
            "members": one("SELECT COUNT(*) FROM members m JOIN rooms r ON r.id = m.room
                            WHERE m.state = 'active' AND r.deleted IS NULL AND ?1 = ?1")?,
            "devices": one("SELECT COUNT(*) FROM devices WHERE revoked IS NULL AND ?1 = ?1")?,
            "invites": one("SELECT COUNT(*) FROM invites
                            WHERE revoked IS NULL AND expires > ?1 AND uses < max_uses")?,
            "hosting": one("SELECT COUNT(*) FROM sessions WHERE ended IS NULL AND lease_until >= ?1")?,
            "playing": one("SELECT COUNT(*) FROM admissions a JOIN sessions s ON s.id = a.session
                            WHERE a.released IS NULL AND s.ended IS NULL AND s.lease_until >= ?1")?,
        }))
    }
}

/// What a new room is made with (already checked).
pub struct RoomDraft<'a> {
    pub name: &'a str,
    pub description: &'a str,
    pub world: &'a str,
    pub minecraft: &'a str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Moderation {
    Remove,
    Ban,
    Unban,
}

fn active_accounts(db: &Connection, room: &str) -> Result<Vec<String>> {
    let mut statement =
        db.prepare("SELECT account FROM members WHERE room = ?1 AND state = 'active'")?;
    let accounts = statement
        .query_map([room], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    Ok(accounts)
}
