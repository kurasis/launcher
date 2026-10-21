use std::collections::{HashMap, HashSet};
use std::io::Write as _;

use serde_json::json;

use super::*;
use crate::mods::{Found, Version};
use crate::worlds::properties::WorldSettings;
use crate::worlds::world;

fn id(n: u8) -> String {
    format!("00000000-0000-4000-8000-{n:012}")
}

fn zip_of(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default();
    for (name, bytes) in entries {
        zip.start_file(*name, options).unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

fn fabric_jar(id: &str, environment: Option<&str>) -> Vec<u8> {
    let mut meta = json!({"schemaVersion": 1, "id": id, "version": "1.0"});
    if let Some(environment) = environment {
        meta["environment"] = environment.into();
    }
    zip_of(&[("fabric.mod.json", meta.to_string().as_bytes())])
}

fn sha256_of(bytes: &[u8]) -> String {
    hex::encode(sha2::Sha256::digest(bytes))
}

fn sha1_of(bytes: &[u8]) -> String {
    use sha1::Digest as _;
    hex::encode(sha1::Sha1::digest(bytes))
}

/// The catalog: files by SHA-256 and the sides of their projects.
#[derive(Default)]
struct Fake {
    files: HashMap<String, (String, String)>,
    projects: HashMap<String, Project>,
}

impl Fake {
    fn add(&mut self, n: u8, bytes: &[u8], client: &str, server: &str) {
        self.files.insert(sha256_of(bytes), (id(n), id(n + 100)));
        let project = Project::of_json(&json!({
            "id": id(n), "kind": "mod", "title": format!("Mod {n}"),
            "clientSide": client, "serverSide": server,
        }))
        .unwrap();
        self.projects.insert(id(n), project);
    }
}

fn not_found() -> LauncherError {
    // Not a message of the launcher's: kept out of the language catalogs.
    let text = "Not in the catalog.";
    LauncherError::new(ErrorCode::InvalidInput, text)
}

impl Catalog for Fake {
    async fn version(&self, _id: &str) -> LauncherResult<Version> {
        Err(not_found())
    }

    async fn project(&self, id: &str) -> LauncherResult<Project> {
        self.projects.get(id).cloned().ok_or_else(not_found)
    }

    async fn versions(
        &self,
        _project: &str,
        _game: Option<&str>,
        _loader: Option<&str>,
    ) -> LauncherResult<Vec<Version>> {
        Ok(Vec::new())
    }

    async fn fetch(&self, _id: &str) -> LauncherResult<()> {
        Ok(())
    }

    async fn lookup(&self, hashes: &[String]) -> LauncherResult<Vec<Found>> {
        Ok(hashes
            .iter()
            .filter_map(|h| {
                let (project_id, version_id) = self.files.get(h)?.clone();
                Some(Found {
                    hash: h.clone(),
                    project_id,
                    version_id,
                })
            })
            .collect())
    }
}

fn fabric() -> LoaderChoice {
    LoaderChoice {
        kind: LoaderKind::Fabric,
        version: "0.17.2".into(),
    }
}

struct Setup {
    _dir: tempfile::TempDir,
    root: DataRoot,
    game: PathBuf,
    catalog: Fake,
    jars: HashMap<&'static str, Vec<u8>>,
}

fn put(path: &Path, bytes: &[u8]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}

/// An installation with a mod for each side, a library the catalog places,
/// settings, and what a world never takes.
fn setup() -> Setup {
    let dir = tempfile::tempdir().unwrap();
    let root = DataRoot::new(dir.path().join("data")).unwrap();
    let game = dir.path().join("game");
    let mut jars = HashMap::new();
    jars.insert("fabric-api.jar", fabric_jar("fabric-api", Some("*")));
    jars.insert("sodium.jar", fabric_jar("sodium", Some("client")));
    jars.insert("ledger.jar", fabric_jar("ledger", Some("server")));
    jars.insert("library.jar", zip_of(&[("a.class", b"x")]));
    jars.insert("minimap.jar", fabric_jar("minimap", None));
    let mut catalog = Fake::default();
    catalog.add(1, &jars["fabric-api.jar"], "required", "required");
    // The jar says client: it wins over the catalog.
    catalog.add(2, &jars["sodium.jar"], "required", "required");
    catalog.add(3, &jars["ledger.jar"], "unsupported", "required");
    catalog.add(4, &jars["library.jar"], "unknown", "unknown");
    catalog.add(5, &jars["minimap.jar"], "required", "unsupported");
    for (name, bytes) in &jars {
        put(&game.join("mods").join(name), bytes);
    }
    put(
        &game.join("mods/launcherskins-fabric-1.21.1-0.3.0.jar"),
        b"ours",
    );
    put(&game.join("mods/old.jar.disabled"), b"off");
    put(&game.join("mods/1.21.1/nested.jar"), b"not loaded");
    put(&game.join("config/sodium.json"), b"{\"fps\":1}");
    put(&game.join("config/tool.exe"), b"MZ");
    put(&game.join("options.txt"), b"fov:1");
    put(&game.join("saves/Home/level.dat"), b"home");
    put(&game.join("resourcepacks/look.zip"), b"look");
    Setup {
        _dir: dir,
        root,
        game,
        catalog,
        jars,
    }
}

fn names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

fn file<'a>(pack: &'a WorldPack, path: &str) -> &'a PackFile {
    pack.files.iter().find(|f| f.path == path).unwrap()
}

#[tokio::test]
async fn a_generation_takes_catalog_mods_with_their_sides_and_the_settings() {
    let s = setup();
    let built = build(&s.catalog, &HashSet::new(), &s.game, &fabric())
        .await
        .unwrap();
    let pack = &built.pack;
    assert_eq!((pack.generation, &pack.loader), (1, &fabric()));
    let mut paths: Vec<&str> = pack.files.iter().map(|f| f.path.as_str()).collect();
    paths.sort_unstable();
    assert_eq!(
        paths,
        [
            "config/sodium.json",
            "mods/fabric-api.jar",
            "mods/ledger.jar",
            "mods/library.jar",
            "mods/minimap.jar",
            "mods/sodium.jar",
        ]
    );
    let side = |path| {
        let f = file(pack, path);
        (f.side, f.side_from)
    };
    assert_eq!(side("mods/sodium.jar"), (Side::Client, SideFrom::Jar));
    assert_eq!(side("mods/ledger.jar"), (Side::Server, SideFrom::Jar));
    assert_eq!(side("mods/fabric-api.jar"), (Side::Both, SideFrom::Catalog));
    assert_eq!(side("mods/minimap.jar"), (Side::Client, SideFrom::Catalog));
    assert_eq!(side("mods/library.jar"), (Side::Both, SideFrom::Default));
    assert_eq!(side("config/sodium.json"), (Side::Both, SideFrom::Default));
    let api = file(pack, "mods/fabric-api.jar");
    assert_eq!(api.project_id.as_deref(), Some(id(1).as_str()));
    assert_eq!(api.sha1.as_str(), sha1_of(&s.jars["fabric-api.jar"]));
    assert_eq!(api.sha512.len(), 128);
    assert_eq!(file(pack, "config/sodium.json").project_id, None);
}

#[tokio::test]
async fn a_mod_the_catalog_lacks_or_holds_back_stops_the_world() {
    let s = setup();
    put(&s.game.join("mods/handmade.jar"), b"mine");
    let error = build(&s.catalog, &HashSet::new(), &s.game, &fabric())
        .await
        .err()
        .unwrap();
    assert_eq!(error.code, ErrorCode::InvalidInput);
    assert_eq!(
        error.message,
        "These mods are not in the catalog: handmade.jar. Add them on the Mods tab by a link, or remove them from the installation."
    );
    fs::remove_file(s.game.join("mods/handmade.jar")).unwrap();

    let harmful: HashSet<String> = [sha1_of(&s.jars["minimap.jar"])].into();
    let error = build(&s.catalog, &harmful, &s.game, &fabric())
        .await
        .err()
        .unwrap();
    assert!(
        error.message.contains("harmful: minimap.jar."),
        "{}",
        error.message
    );
}

fn made(s: &Setup) -> WorldFile {
    world::create(&s.root, "Modded", "1.21.1", WorldSettings::default()).unwrap()
}

async fn stored(s: &Setup) -> (WorldFile, WorldPack) {
    let world = made(s);
    let built = build(&s.catalog, &HashSet::new(), &s.game, &fabric())
        .await
        .unwrap();
    let pack = store(&s.root, &world, built).unwrap();
    let mut world = world;
    world.pack = Some(pack.clone());
    world::save(&s.root, &world).unwrap();
    (world, pack)
}

#[tokio::test]
async fn the_server_gets_exactly_its_mods_and_keeps_its_own_settings() {
    let s = setup();
    let (world, pack) = stored(&s).await;
    for f in &pack.files {
        let kept = s.root.world_pack_dir(world.id).join(f.sha256.as_str());
        assert_eq!(sha256_of(&fs::read(kept).unwrap()), f.sha256.as_str());
    }
    let server = s.root.world_server_dir(world.id);
    put(&server.join("mods/stray.jar"), b"left over");
    fs::create_dir_all(server.join("mods/.fabric")).unwrap();
    place_server(&s.root, &world).unwrap();
    assert_eq!(
        names(&server.join("mods")),
        [".fabric", "fabric-api.jar", "ledger.jar", "library.jar"]
    );
    assert_eq!(
        fs::read(server.join("mods/fabric-api.jar")).unwrap(),
        s.jars["fabric-api.jar"]
    );
    assert_eq!(
        fs::read(server.join("config/sodium.json")).unwrap(),
        b"{\"fps\":1}"
    );

    // What the server changed in its settings stays; a changed mod does not.
    fs::write(server.join("config/sodium.json"), b"{\"fps\":2}").unwrap();
    fs::write(server.join("mods/ledger.jar"), b"swapped").unwrap();
    place_server(&s.root, &world).unwrap();
    assert_eq!(
        fs::read(server.join("config/sodium.json")).unwrap(),
        b"{\"fps\":2}"
    );
    assert_eq!(
        fs::read(server.join("mods/ledger.jar")).unwrap(),
        s.jars["ledger.jar"]
    );

    // A damaged copy is never put in place.
    let api = file(&pack, "mods/fabric-api.jar");
    fs::write(
        s.root.world_pack_dir(world.id).join(api.sha256.as_str()),
        b"bad",
    )
    .unwrap();
    fs::remove_file(server.join("mods/fabric-api.jar")).unwrap();
    assert_eq!(
        place_server(&s.root, &world).unwrap_err().code,
        ErrorCode::RepairRequired
    );
    assert!(!server.join("mods/fabric-api.jar").exists());
}

#[tokio::test]
async fn the_hosts_game_gets_the_client_mods_beside_the_skins_mod() {
    let s = setup();
    let (world, _) = stored(&s).await;
    let dir = tempfile::tempdir().unwrap();
    let game = dir.path().join("room");
    put(
        &game.join("mods/launcherskins-fabric-1.21.1-0.3.0.jar"),
        b"ours",
    );
    put(&game.join("mods/other.jar"), b"not the room's");
    put(&game.join("options.txt"), b"guest's own");
    place_client(&s.root, &world, &game).unwrap();
    assert_eq!(
        names(&game.join("mods")),
        [
            "fabric-api.jar",
            "launcherskins-fabric-1.21.1-0.3.0.jar",
            "library.jar",
            "minimap.jar",
            "sodium.jar"
        ]
    );
    assert_eq!(fs::read(game.join("options.txt")).unwrap(), b"guest's own");
    assert!(game.join("config/sodium.json").is_file());
}

#[tokio::test]
async fn a_tampered_record_is_refused_before_any_file_moves() {
    let s = setup();
    let (mut world, _) = stored(&s).await;
    let server = s.root.world_server_dir(world.id);
    let pack = world.pack.as_mut().unwrap();
    pack.files[0].path = "../outside.jar".into();
    assert_eq!(
        place_server(&s.root, &world).unwrap_err().code,
        ErrorCode::RepairRequired
    );
    assert!(!server.join("mods").exists());
    for path in ["mods/run.exe", "config/x.jar", "saves/x/level.dat", ""] {
        let pack = world.pack.as_mut().unwrap();
        pack.files[0].path = path.into();
        assert!(place_server(&s.root, &world).is_err(), "{path}");
    }
    // A link where a settings folder should be.
    #[cfg(unix)]
    {
        let (world, _) = stored(&s).await;
        let server = s.root.world_server_dir(world.id);
        let outside = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(outside.path(), server.join("config")).unwrap();
        assert!(place_server(&s.root, &world).is_err());
        assert_eq!(fs::read_dir(outside.path()).unwrap().count(), 0);
    }
}

#[tokio::test]
async fn the_generation_is_a_mrpack_with_sides_and_catalog_downloads() {
    let s = setup();
    let (world, pack) = stored(&s).await;
    let path = s.root.world_pack_dir(world.id).join("generation-1.mrpack");
    let bytes = fs::read(&path).unwrap();
    assert_eq!(
        pack.archive_sha256.as_ref().unwrap().as_str(),
        sha256_of(&bytes)
    );
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut entries: Vec<String> = (0..zip.len())
        .map(|i| zip.by_index(i).unwrap().name().to_owned())
        .collect();
    entries.sort();
    assert_eq!(
        entries,
        ["modrinth.index.json", "overrides/config/sodium.json"]
    );
    let mut text = String::new();
    zip.by_name("modrinth.index.json")
        .unwrap()
        .read_to_string(&mut text)
        .unwrap();
    let index: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(
        index["dependencies"],
        json!({"minecraft": "1.21.1", "fabric-loader": "0.17.2"})
    );
    let entry = |name: &str| {
        index["files"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["path"] == name)
            .unwrap()
            .clone()
    };
    assert_eq!(
        entry("mods/sodium.jar")["env"],
        json!({"client": "required", "server": "unsupported"})
    );
    assert_eq!(
        entry("mods/ledger.jar")["env"],
        json!({"client": "unsupported", "server": "required"})
    );
    let api = entry("mods/fabric-api.jar");
    let sha = sha256_of(&s.jars["fabric-api.jar"]);
    assert_eq!(
        api["downloads"],
        json!([
            format!("https://cdn-mods.lapi.sh/f/{sha}"),
            format!("https://files-mods.lapi.sh/f/{sha}")
        ])
    );
    assert_eq!(api["hashes"]["sha1"], sha1_of(&s.jars["fabric-api.jar"]));
    assert_eq!(index["files"].as_array().unwrap().len(), 5);
}

#[test]
fn sides_come_from_the_jar_then_the_catalog() {
    let dir = tempfile::tempdir().unwrap();
    let jar = |name: &str, bytes: &[u8]| {
        let path = dir.path().join(name);
        fs::write(&path, bytes).unwrap();
        jar_side(&path)
    };
    assert_eq!(
        jar("a.jar", &fabric_jar("a", Some("client"))),
        Some(Side::Client)
    );
    assert_eq!(
        jar("b.jar", &fabric_jar("b", Some("server"))),
        Some(Side::Server)
    );
    assert_eq!(jar("c.jar", &fabric_jar("c", Some("*"))), None);
    assert_eq!(jar("d.jar", &fabric_jar("d", None)), None);
    let quilt = json!({"schema_version": 1, "minecraft": {"environment": "dedicated_server"}});
    assert_eq!(
        jar(
            "e.jar",
            &zip_of(&[("quilt.mod.json", quilt.to_string().as_bytes())])
        ),
        Some(Side::Server)
    );
    assert_eq!(jar("f.jar", b"not a zip"), None);
    assert_eq!(
        jar("g.jar", &zip_of(&[("fabric.mod.json", b"{ junk")])),
        None
    );

    let project = |client: &str, server: &str| {
        Project::of_json(&json!({"id": id(1), "kind": "mod", "title": "x",
            "clientSide": client, "serverSide": server}))
        .unwrap()
    };
    assert_eq!(
        catalog_side(&project("required", "optional")),
        Some(Side::Both)
    );
    assert_eq!(
        catalog_side(&project("optional", "unsupported")),
        Some(Side::Client)
    );
    assert_eq!(
        catalog_side(&project("unknown", "unsupported")),
        Some(Side::Client)
    );
    assert_eq!(
        catalog_side(&project("unsupported", "required")),
        Some(Side::Server)
    );
    assert_eq!(catalog_side(&project("unsupported", "unsupported")), None);
    assert_eq!(catalog_side(&project("unknown", "unknown")), None);
}

#[test]
fn only_tested_loaders_and_releases_are_offered() {
    let fabric = fabric();
    for version in ["1.20.1", "1.21.1", "26.3"] {
        assert!(check_supported(&fabric, version).is_ok(), "{version}");
    }
    for version in ["1.21.4", "26.4-snapshot-5", "24w14a", "1.12.2", ""] {
        assert!(check_supported(&fabric, version).is_err(), "{version}");
    }
    let forge = LoaderChoice {
        kind: LoaderKind::Forge,
        version: "1.20.1-47.4.10".into(),
    };
    assert_eq!(
        check_supported(&forge, "1.20.1").unwrap_err().message,
        "Worlds with this mod loader come later; Fabric works now."
    );
    let junk = LoaderChoice {
        kind: LoaderKind::Fabric,
        version: "../0.17.2".into(),
    };
    assert!(check_supported(&junk, "1.21.1").is_err());
}
