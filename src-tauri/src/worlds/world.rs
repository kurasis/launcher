//! World directories and `world.json` (PLAN W3, ADAPTATION A6).
//!
//! ```text
//! worlds/<worldId>/world.json        this file, atomically replaced
//! worlds/<worldId>/world.json.bak    the last good version
//! worlds/<worldId>/server/           the server's working directory
//! ```
//!
//! `world.json` records what the world runs (the version and the pinned server
//! JAR), what the player chose, whether the EULA was accepted, and the operation
//! that is changing the world, written before the change starts so an
//! interrupted one is found on the next start.

use std::path::Path;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::identity::Nick;
use super::properties::{self, RunBinding, WorldSettings};
use super::tuple;
use crate::error::{ErrorCode, LauncherError, LauncherResult};
use crate::storage::atomic;
use crate::storage::document::{self, Loaded, SCHEMA_VERSION};
use crate::storage::paths::{DataRoot, Sha1, Sha256, WorldId};

pub const MAX_NAME_CHARS: usize = 64;
/// Worlds one launcher keeps; a bound for listing, not a product limit.
pub const MAX_WORLDS: usize = 256;

/// The server JAR a world runs, verified on download (SHA-1 from Mojang) and
/// recorded by SHA-256 as well.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerPin {
    pub jar_sha1: Sha1,
    pub jar_sha256: Sha256,
    pub jar_size: u64,
    pub java_major: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MutationKind {
    InstallServer,
    Host,
    Import,
    Backup,
    Restore,
}

/// An operation changing this world, recorded before it starts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Mutation {
    pub kind: MutationKind,
    pub operation_id: String,
    pub started_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorldFile {
    pub schema_version: u64,
    pub id: WorldId,
    pub name: String,
    pub minecraft: String,
    pub settings: WorldSettings,
    pub created_at: DateTime<Utc>,
    #[serde(default)]
    pub server: Option<ServerPin>,
    #[serde(default)]
    pub eula_accepted_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub mutation: Option<Mutation>,
}

/// A world name: 1–64 characters after trimming, no control characters.
pub fn validate_name(name: &str) -> LauncherResult<String> {
    let name = name.trim();
    if name.is_empty()
        || name.chars().count() > MAX_NAME_CHARS
        || name.chars().any(char::is_control)
    {
        return Err(LauncherError::invalid_input(
            "A world name has 1 to 64 characters without line breaks.",
        ));
    }
    Ok(name.to_owned())
}

fn io(error: &std::io::Error) -> LauncherError {
    LauncherError::from_io(error, "Saving the world")
}

fn world_missing() -> LauncherError {
    LauncherError::new(ErrorCode::InstanceNotFound, "This world no longer exists.")
}

/// Creates the directory and `world.json` of a new world.
pub fn create(
    root: &DataRoot,
    name: &str,
    minecraft: &str,
    settings: WorldSettings,
) -> LauncherResult<WorldFile> {
    let name = validate_name(name)?;
    settings.validate()?;
    if tuple::find(minecraft).is_none() {
        return Err(LauncherError::invalid_input(
            "Worlds are not available for this Minecraft version yet.",
        ));
    }
    if list(root).len() >= MAX_WORLDS {
        return Err(LauncherError::invalid_input(
            "This launcher already keeps the most worlds it can.",
        ));
    }
    let world = WorldFile {
        schema_version: SCHEMA_VERSION,
        id: WorldId::generate(),
        name,
        minecraft: minecraft.to_owned(),
        settings,
        created_at: Utc::now(),
        server: None,
        eula_accepted_at: None,
        mutation: None,
    };
    std::fs::create_dir_all(root.world_server_dir(world.id)).map_err(|e| io(&e))?;
    save(root, &world)?;
    Ok(world)
}

pub fn save(root: &DataRoot, world: &WorldFile) -> LauncherResult<()> {
    let path = root.world_file(world.id);
    document::save(&path, world, Some(&backup_path(&path))).map_err(|e| io(&e))
}

fn backup_path(path: &Path) -> std::path::PathBuf {
    path.with_extension("json.bak")
}

pub fn load(root: &DataRoot, id: WorldId) -> LauncherResult<WorldFile> {
    let path = root.world_file(id);
    let loaded = document::load::<WorldFile>(&path, Some(&backup_path(&path)))
        .map_err(|e| LauncherError::from_io(&e, "Reading the world"))?;
    match loaded {
        Loaded::Valid(world) | Loaded::RecoveredFromBackup { value: world, .. }
            if world.id == id =>
        {
            Ok(world)
        }
        Loaded::Missing => Err(world_missing()),
        Loaded::UnsupportedSchema { .. } => Err(LauncherError::new(
            ErrorCode::UnsupportedSchema,
            "This world was saved by a newer launcher.",
        )),
        _ => Err(LauncherError::new(
            ErrorCode::StorageError,
            "This world's record is damaged.",
        )),
    }
}

/// Every readable world, by name. Entries that are not world directories are
/// skipped, never removed.
pub fn list(root: &DataRoot) -> Vec<WorldFile> {
    let Ok(entries) = std::fs::read_dir(root.worlds_dir()) else {
        return Vec::new();
    };
    let mut worlds: Vec<WorldFile> = entries
        .filter_map(Result::ok)
        .take(MAX_WORLDS * 4)
        .filter_map(|entry| entry.file_name().to_str()?.parse::<WorldId>().ok())
        .filter_map(|id| load(root, id).ok())
        .take(MAX_WORLDS)
        .collect();
    worlds.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then(a.created_at.cmp(&b.created_at))
    });
    worlds
}

/// Records that the player accepted the Minecraft EULA for this world.
pub fn accept_eula(root: &DataRoot, id: WorldId) -> LauncherResult<WorldFile> {
    let mut world = load(root, id)?;
    if world.eula_accepted_at.is_none() {
        world.eula_accepted_at = Some(Utc::now());
        save(root, &world)?;
    }
    Ok(world)
}

/// Whitelist entry as the server stores it.
#[derive(Serialize)]
struct WhitelistEntry<'a> {
    uuid: String,
    name: &'a str,
}

/// Writes the files the server reads at start: `server.properties`,
/// `whitelist.json` with the members' offline UUIDs, and `eula.txt`. Refuses
/// before the player accepted the EULA.
pub fn prepare_run(
    root: &DataRoot,
    world: &WorldFile,
    binding: &RunBinding,
    members: &[Nick],
) -> LauncherResult<()> {
    if world.eula_accepted_at.is_none() {
        return Err(LauncherError::new(
            ErrorCode::InvalidInput,
            "Accept the Minecraft EULA to host this world.",
        ));
    }
    world.settings.validate()?;
    let dir = root.world_server_dir(world.id);
    std::fs::create_dir_all(&dir).map_err(|e| io(&e))?;
    atomic::write(
        &dir.join("server.properties"),
        properties::render(&world.settings, binding).as_bytes(),
    )
    .map_err(|e| io(&e))?;
    let mut nicks: Vec<&Nick> = members.iter().collect();
    nicks.sort();
    nicks.dedup();
    let whitelist: Vec<WhitelistEntry<'_>> = nicks
        .into_iter()
        .map(|nick| WhitelistEntry {
            uuid: nick.offline_uuid().hyphenated().to_string(),
            name: nick.as_str(),
        })
        .collect();
    let mut json = serde_json::to_vec_pretty(&whitelist).map_err(|_| {
        LauncherError::new(ErrorCode::Internal, "The whitelist could not be written.")
    })?;
    json.push(b'\n');
    atomic::write(&dir.join("whitelist.json"), &json).map_err(|e| io(&e))?;
    atomic::write(
        &dir.join("eula.txt"),
        b"# Accepted by the player in the launcher.\neula=true\n",
    )
    .map_err(|e| io(&e))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secrets::SecretString;

    fn root() -> (tempfile::TempDir, DataRoot) {
        let dir = tempfile::tempdir().unwrap();
        let root = DataRoot::new(dir.path().to_path_buf()).unwrap();
        (dir, root)
    }

    fn binding() -> RunBinding {
        RunBinding {
            port: 41000,
            rcon_port: 41001,
            rcon_password: SecretString::new("pw"),
        }
    }

    #[test]
    fn create_load_and_list() {
        let (_dir, root) = root();
        let world = create(&root, "  Наш мир  ", "1.21.1", WorldSettings::default()).unwrap();
        assert_eq!(world.name, "Наш мир");
        assert!(root.world_server_dir(world.id).is_dir());
        assert_eq!(load(&root, world.id).unwrap(), world);
        let second = create(&root, "Another", "1.21.1", WorldSettings::default()).unwrap();
        let names: Vec<_> = list(&root).into_iter().map(|w| w.name).collect();
        assert_eq!(names, ["Another", "Наш мир"]);
        std::fs::create_dir_all(root.worlds_dir().join("not-a-world")).unwrap();
        assert_eq!(list(&root).len(), 2);
        assert!(load(&root, WorldId::generate()).is_err());
        assert_ne!(world.id, second.id);
    }

    #[test]
    fn create_validates_input() {
        let (_dir, root) = root();
        for name in ["", "   ", "a\nb", &"n".repeat(65)] {
            assert!(create(&root, name, "1.21.1", WorldSettings::default()).is_err());
        }
        assert!(create(&root, "x", "1.20.4", WorldSettings::default()).is_err());
        let bad = WorldSettings {
            max_players: 0,
            ..WorldSettings::default()
        };
        assert!(create(&root, "x", "1.21.1", bad).is_err());
        assert!(list(&root).is_empty());
    }

    #[test]
    fn a_record_for_another_id_is_not_loaded() {
        let (_dir, root) = root();
        let a = create(&root, "A", "1.21.1", WorldSettings::default()).unwrap();
        let b = WorldId::generate();
        std::fs::create_dir_all(root.world_dir(b)).unwrap();
        std::fs::copy(root.world_file(a.id), root.world_file(b)).unwrap();
        assert!(load(&root, b).is_err());
    }

    #[test]
    fn a_damaged_record_falls_back_to_the_last_good_one() {
        let (_dir, root) = root();
        let mut world = create(&root, "A", "1.21.1", WorldSettings::default()).unwrap();
        world.name = "B".into();
        save(&root, &world).unwrap();
        std::fs::write(root.world_file(world.id), b"{ not json").unwrap();
        assert_eq!(load(&root, world.id).unwrap().name, "A");
    }

    #[test]
    fn run_files_need_the_eula() {
        let (_dir, root) = root();
        let world = create(&root, "A", "1.21.1", WorldSettings::default()).unwrap();
        let nick = Nick::parse("Notch").unwrap();
        assert!(prepare_run(&root, &world, &binding(), std::slice::from_ref(&nick)).is_err());
        let dir = root.world_server_dir(world.id);
        assert!(!dir.join("eula.txt").exists());

        let world = accept_eula(&root, world.id).unwrap();
        prepare_run(&root, &world, &binding(), &[nick.clone(), nick]).unwrap();
        let eula = std::fs::read_to_string(dir.join("eula.txt")).unwrap();
        assert!(eula.lines().any(|l| l == "eula=true"));
        let whitelist: serde_json::Value =
            serde_json::from_slice(&std::fs::read(dir.join("whitelist.json")).unwrap()).unwrap();
        assert_eq!(
            whitelist,
            serde_json::json!([{"uuid": "b50ad385-829d-3141-a216-7e7d7539ba7f", "name": "Notch"}])
        );
        let props = std::fs::read_to_string(dir.join("server.properties")).unwrap();
        assert!(props.lines().any(|l| l == "server-port=41000"));
    }
}
