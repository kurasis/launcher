use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use super::*;
use crate::backup::store::Reason;
use crate::loader::LoaderKind;
use crate::storage::paths::Sha1;
use crate::worlds::pack::SideFrom;
use crate::worlds::properties::WorldSettings;

fn fabric(version: &str) -> LoaderChoice {
    LoaderChoice {
        kind: LoaderKind::Fabric,
        version: version.into(),
    }
}

fn put(path: &Path, bytes: &[u8]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}

fn read(path: &Path) -> Vec<u8> {
    fs::read(path).unwrap()
}

struct Setup {
    _dir: tempfile::TempDir,
    root: DataRoot,
    /// The installation the generations come from, one folder each.
    games: PathBuf,
    world: WorldFile,
}

impl Setup {
    fn server(&self) -> PathBuf {
        self.root.world_server_dir(self.world.id)
    }

    fn store(&self) -> PathBuf {
        self.root.world_pack_dir(self.world.id)
    }

    fn load(&self) -> WorldFile {
        world::load(&self.root, self.world.id).unwrap()
    }
}

/// A file of a generation, written into the installation `game`.
fn entry(
    game: &Path,
    path: &str,
    bytes: &[u8],
    side: Side,
    project: Option<u8>,
) -> (PackFile, PathBuf) {
    let full = game.join(path);
    put(&full, bytes);
    let (sha1, sha512, sha256) = hashes_of(&full).unwrap();
    let file = PackFile {
        path: path.to_owned(),
        sha1: Sha1::parse(&sha1).unwrap(),
        sha256: Sha256::parse(&sha256).unwrap(),
        sha512,
        size: bytes.len() as u64,
        side,
        side_from: SideFrom::Jar,
        project_id: project.map(|n| format!("00000000-0000-4000-8000-{n:012}")),
        version_id: project.map(|n| format!("00000000-0000-4000-9000-{n:012}")),
        title: None,
        version_number: None,
    };
    (file, full)
}

fn built(loader: LoaderChoice, entries: Vec<(PackFile, PathBuf)>) -> Built {
    let (files, sources): (Vec<PackFile>, Vec<PathBuf>) = entries.into_iter().unzip();
    Built {
        pack: WorldPack {
            generation: 1,
            loader,
            files,
            made_at: chrono::Utc::now(),
            archive_sha256: None,
            server_manifest: None,
            room: None,
            source: None,
        },
        sources,
    }
}

/// Generation 1: Fabric API, a server mod and two settings; the world has
/// been played, and the host changed one of the settings on the server.
fn setup() -> Setup {
    let dir = tempfile::tempdir().unwrap();
    let root = DataRoot::new(dir.path().join("data")).unwrap();
    let games = dir.path().join("games");
    let game = games.join("1");
    let mut world =
        world::create(&root, "Modded base", "1.21.1", WorldSettings::default()).unwrap();
    let first = built(
        fabric("0.17.2"),
        vec![
            entry(&game, "mods/fabric-api.jar", b"api 1", Side::Both, Some(1)),
            entry(&game, "mods/ledger.jar", b"ledger 1", Side::Server, Some(2)),
            entry(
                &game,
                "config/mine.json",
                b"{\"spawn\":1}",
                Side::Both,
                None,
            ),
            entry(
                &game,
                "config/theirs.json",
                b"{\"rate\":1}",
                Side::Both,
                None,
            ),
        ],
    );
    let generation = pack::store(&root, &world, first).unwrap();
    world.pack = Some(generation);
    world::save(&root, &world).unwrap();
    let s = Setup {
        _dir: dir,
        root,
        games,
        world,
    };
    pack::place_server(&s.root, &s.world).unwrap();
    put(&s.server().join("config/mine.json"), b"{\"spawn\":7}");
    put(&s.server().join("world/level.dat"), b"level before");
    s
}

/// Generation 2: Fabric API updated, the server mod gone, a client mod
/// added, both settings in new versions.
fn second(s: &Setup) -> Built {
    let game = s.games.join("2");
    built(
        fabric("0.17.2"),
        vec![
            entry(&game, "mods/fabric-api.jar", b"api 2", Side::Both, Some(1)),
            entry(&game, "mods/sodium.jar", b"sodium", Side::Client, Some(3)),
            entry(
                &game,
                "config/mine.json",
                b"{\"spawn\":2}",
                Side::Both,
                None,
            ),
            entry(
                &game,
                "config/theirs.json",
                b"{\"rate\":2}",
                Side::Both,
                None,
            ),
        ],
    )
}

fn names(list: &[ModChange]) -> Vec<&str> {
    list.iter().map(|c| c.name.as_str()).collect()
}

#[test]
fn an_update_shows_its_mods_and_asks_only_for_settings_the_host_changed() {
    let s = setup();
    let shown = preview(&s.root, &s.world, &second(&s)).unwrap();
    assert_eq!((shown.generation, shown.next), (1, 2));
    assert!(!shown.same && !shown.loader_changed);
    assert_eq!(names(&shown.added), ["sodium.jar"]);
    assert_eq!(names(&shown.removed), ["ledger.jar"]);
    assert_eq!(names(&shown.updated), ["fabric-api.jar"]);
    assert_eq!(shown.updated[0].from.as_deref(), Some("fabric-api.jar"));
    // The host never touched theirs.json: it takes the new version.
    assert_eq!(shown.settings, 1);
    assert_eq!(shown.conflicts.len(), 1);
    let conflict = &shown.conflicts[0];
    assert_eq!(conflict.path, "config/mine.json");
    let text = |s: &Shown| s.text.clone().unwrap();
    assert_eq!(text(conflict.base.as_ref().unwrap()), "{\"spawn\":1}");
    assert_eq!(text(&conflict.mine), "{\"spawn\":7}");
    assert_eq!(text(&conflict.theirs), "{\"spawn\":2}");

    // The same installation again changes nothing.
    let game = s.games.join("1");
    let same = built(
        fabric("0.17.2"),
        s.world
            .pack
            .as_ref()
            .unwrap()
            .files
            .iter()
            .map(|f| (f.clone(), game.join(&f.path)))
            .collect(),
    );
    assert!(preview(&s.root, &s.world, &same).unwrap().same);
}

#[test]
fn an_update_from_another_loader_or_without_answers_is_refused() {
    let s = setup();
    let mut forge = second(&s);
    forge.pack.loader = LoaderChoice {
        kind: LoaderKind::Forge,
        version: "52.0.1".into(),
    };
    assert!(preview(&s.root, &s.world, &forge).is_err());

    let shown = preview(&s.root, &s.world, &second(&s)).unwrap();
    let none = HashMap::new();
    assert!(stage(&s.root, &s.world, second(&s), &shown.fingerprint, &none).is_err());
    let answered = HashMap::from([("config/mine.json".to_owned(), Keep::Mine)]);
    assert!(stage(&s.root, &s.world, second(&s), "another", &answered).is_err());
    // The server's setting changed after it was shown.
    put(&s.server().join("config/mine.json"), b"{\"spawn\":8}");
    assert!(stage(&s.root, &s.world, second(&s), &shown.fingerprint, &answered).is_err());
}

/// Generation 2 staged, kept with a backup and committed; `keep` answers
/// the changed setting.
fn updated(s: &Setup, keep: Keep) -> String {
    let shown = preview(&s.root, &s.world, &second(s)).unwrap();
    let choices = HashMap::from([("config/mine.json".to_owned(), keep)]);
    let staged = stage(&s.root, &s.world, second(s), &shown.fingerprint, &choices).unwrap();
    let backup = safety::backup(&s.root, s.world.id, Reason::BeforeUpdate)
        .unwrap()
        .unwrap()
        .id;
    begin(&s.root, s.world.id).unwrap();
    let trial = make_trial(&s.root, s.world.id, &staged, Some(&backup)).unwrap();
    let _ = fs::remove_dir_all(s.root.world_dir(trial));
    let manifest = Sha1::parse("0123456789abcdef0123456789abcdef01234567").unwrap();
    commit(
        &s.root,
        s.world.id,
        staged,
        Some(manifest),
        Some(backup.clone()),
    )
    .unwrap();
    backup
}

#[test]
fn the_copy_an_update_is_tried_on_holds_the_backup_and_the_answers() {
    let s = setup();
    let shown = preview(&s.root, &s.world, &second(&s)).unwrap();
    let choices = HashMap::from([("config/mine.json".to_owned(), Keep::Mine)]);
    let staged = stage(&s.root, &s.world, second(&s), &shown.fingerprint, &choices).unwrap();
    let backup = safety::backup(&s.root, s.world.id, Reason::BeforeUpdate)
        .unwrap()
        .unwrap()
        .id;
    put(
        &s.server().join("world/level.dat"),
        b"level after the backup",
    );
    let trial = make_trial(&s.root, s.world.id, &staged, Some(&backup)).unwrap();
    assert!(
        world::list(&s.root).iter().all(|w| w.id != trial),
        "a copy is never listed"
    );
    let copy = world::load(&s.root, trial).unwrap();
    assert_eq!(copy.trial_of, Some(s.world.id));
    assert_eq!(copy.pack.as_ref().unwrap().generation, 2);
    let server = s.root.world_server_dir(trial);
    assert_eq!(read(&server.join("world/level.dat")), b"level before");
    assert_eq!(read(&server.join("config/mine.json")), b"{\"spawn\":7}");
    assert!(!server.join("config/theirs.json").exists(), "taken anew");
    // Its mods come from the world's own files.
    pack::place_server(&s.root, &copy).unwrap();
    assert_eq!(read(&server.join("mods/fabric-api.jar")), b"api 2");
    assert!(!server.join("mods/sodium.jar").exists(), "client only");
    assert_eq!(read(&server.join("config/theirs.json")), b"{\"rate\":2}");
    assert_eq!(world::trials(&s.root).len(), 1);
    // An exit while it was tried: it goes, the world stays as it was.
    begin(&s.root, s.world.id).unwrap();
    let mut world = s.load();
    assert!(safety::recover(&s.root, &mut world).unwrap());
    assert!(world::trials(&s.root).is_empty());
    let world = s.load();
    assert_eq!(world.pack.as_ref().unwrap().generation, 1);
    assert_eq!((world.mutation, world.update), (None, None));
    assert!(
        !s.store().join("generation-2.mrpack").exists(),
        "staged files go"
    );
}

#[test]
fn a_committed_update_keeps_the_way_back_until_a_start_confirms_it() {
    let s = setup();
    let backup = updated(&s, Keep::Theirs);
    let world = s.load();
    let now = world.pack.as_ref().unwrap();
    assert_eq!(now.generation, 2);
    assert!(now.server_manifest.is_some());
    assert!(pending(&world));
    assert_eq!(world.generations.len(), 1);
    assert_eq!(world.backup_generations.get(&backup), Some(&1));
    let pending_update = world.update.clone().unwrap();
    assert_eq!(pending_update.from, 1);
    assert_eq!(pending_update.replaced.len(), 2, "both settings taken anew");
    // The new versions are written at the start.
    pack::place_server(&s.root, &world).unwrap();
    assert_eq!(read(&s.server().join("config/mine.json")), b"{\"spawn\":2}");
    assert_eq!(read(&s.server().join("mods/fabric-api.jar")), b"api 2");
    assert!(!s.server().join("mods/ledger.jar").exists());

    confirm(&s.root, s.world.id).unwrap();
    let world = s.load();
    assert!(!pending(&world) && world.update.is_none());
    // The backup still needs generation 1.
    assert_eq!(world.generations[0].generation, 1);
    assert!(s.store().join("generation-1.mrpack").is_file());
    assert!(!roll_back(&s.root, s.world.id).unwrap(), "nothing to undo");
}

/// A45: the world's migration to the new mods breaks it; the backup, the
/// earlier mods and the host's settings come back together.
#[test]
fn a_world_that_does_not_start_with_its_new_mods_gets_its_copy_and_mods_back() {
    let s = setup();
    updated(&s, Keep::Theirs);
    let world = s.load();
    pack::place_server(&s.root, &world).unwrap();
    put(&s.server().join("world/level.dat"), b"half migrated");
    put(
        &s.server().join("world/region/new.mca"),
        b"chunks of the new mods",
    );

    assert!(roll_back(&s.root, s.world.id).unwrap());
    let world = s.load();
    assert_eq!(world.pack.as_ref().unwrap().generation, 1);
    assert!(world.update.is_none() && world.mutation.is_none());
    assert!(world.generations.is_empty());
    assert_eq!(read(&s.server().join("world/level.dat")), b"level before");
    assert!(!s.server().join("world/region/new.mca").exists());
    assert_eq!(read(&s.server().join("config/mine.json")), b"{\"spawn\":7}");
    assert_eq!(
        read(&s.server().join("config/theirs.json")),
        b"{\"rate\":1}"
    );
    pack::place_server(&s.root, &world).unwrap();
    assert_eq!(read(&s.server().join("mods/fabric-api.jar")), b"api 1");
    assert_eq!(read(&s.server().join("mods/ledger.jar")), b"ledger 1");
    // Generation 2 and its files are gone; generation 1's stay.
    assert!(!s.store().join("generation-2.mrpack").exists());
    let sodium = hex::encode(sha2::Sha256::digest(b"sodium"));
    assert!(!s.store().join(sodium).exists());
    let api = hex::encode(sha2::Sha256::digest(b"api 1"));
    assert!(s.store().join(api).is_file());
}

#[test]
fn restoring_a_backup_brings_back_the_generation_it_was_made_with() {
    let s = setup();
    let before = updated(&s, Keep::Mine);
    confirm(&s.root, s.world.id).unwrap();
    put(
        &s.server().join("world/level.dat"),
        b"played with generation 2",
    );

    safety::restore(&s.root, s.world.id, &before).unwrap();
    let world = s.load();
    assert_eq!(world.pack.as_ref().unwrap().generation, 1);
    assert_eq!(read(&s.server().join("world/level.dat")), b"level before");
    // The restore's own backup holds the world of generation 2.
    let made_first = safety::list(&s.root, s.world.id)
        .into_iter()
        .find(|b| b.reason == Reason::BeforeRestore)
        .unwrap();
    assert_eq!(world.backup_generations.get(&made_first.id), Some(&2));
    assert_eq!(world.generations[0].generation, 2);

    safety::restore(&s.root, s.world.id, &made_first.id).unwrap();
    let world = s.load();
    assert_eq!(world.pack.as_ref().unwrap().generation, 2);
    assert_eq!(
        read(&s.server().join("world/level.dat")),
        b"played with generation 2"
    );
}

#[test]
fn an_exit_before_the_update_is_kept_puts_the_settings_back() {
    let s = setup();
    let shown = preview(&s.root, &s.world, &second(&s)).unwrap();
    let choices = HashMap::from([("config/mine.json".to_owned(), Keep::Theirs)]);
    let staged = stage(&s.root, &s.world, second(&s), &shown.fingerprint, &choices).unwrap();
    // As `commit` leaves it before the generation is the world's.
    let mine = s.server().join("config/mine.json");
    let (sha256, size) = sha256_of(&mine).unwrap();
    pack::copy_verified(&mine, &s.store().join(sha256.as_str()), &sha256).unwrap();
    assert_eq!(size, 11);
    let mut world = s.load();
    world.update = Some(Pending {
        from: 1,
        backup: None,
        replaced: vec![Replaced {
            path: "config/mine.json".into(),
            sha256,
        }],
        committed: false,
    });
    world::save(&s.root, &world).unwrap();
    begin(&s.root, s.world.id).unwrap();
    fs::remove_file(&mine).unwrap();
    drop(staged);

    let mut world = s.load();
    assert!(safety::recover(&s.root, &mut world).unwrap());
    let world = s.load();
    assert_eq!(read(&mine), b"{\"spawn\":7}");
    assert_eq!(world.pack.as_ref().unwrap().generation, 1);
    assert!(world.update.is_none() && world.mutation.is_none());
}

#[test]
fn a_conflict_shows_text_bounded_and_files_that_are_not_text_by_size() {
    let dir = tempfile::tempdir().unwrap();
    let long = dir.path().join("long.txt");
    fs::write(&long, "é".repeat(SHOWN)).unwrap();
    let shown_long = shown(&long, (SHOWN * 2) as u64);
    assert!(shown_long.truncated);
    assert!(shown_long.text.unwrap().len() <= SHOWN);
    let binary = dir.path().join("icon.png");
    fs::write(&binary, [0x89, b'P', b'N', b'G', 0, 1, 2]).unwrap();
    let shown_binary = shown(&binary, 7);
    assert_eq!((shown_binary.text, shown_binary.bytes), (None, 7));
}

#[test]
fn settings_are_read_only_through_real_folders() {
    let dir = tempfile::tempdir().unwrap();
    put(&dir.path().join("config/a.json"), b"{}");
    assert!(inside(dir.path(), "config/a.json").is_some());
    for bad in [
        "config/../config/a.json",
        "/config/a.json",
        "config//a.json",
        "config",
    ] {
        assert!(inside(dir.path(), bad).is_none(), "{bad}");
    }
    assert!(make_inside(dir.path(), "config/../x").is_err());
    #[cfg(unix)]
    {
        let outside = tempfile::tempdir().unwrap();
        put(&outside.path().join("a.json"), b"{}");
        std::os::unix::fs::symlink(outside.path(), dir.path().join("linked")).unwrap();
        assert!(inside(dir.path(), "linked/a.json").is_none());
        assert!(make_inside(dir.path(), "linked/b.json").is_err());
    }
}
