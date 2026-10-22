use super::*;
use crate::storage::document::SCHEMA_VERSION;
use crate::storage::models::{InstallationHealth, Resolution};

fn root() -> (tempfile::TempDir, DataRoot) {
    let dir = tempfile::tempdir().unwrap();
    let root = DataRoot::new(dir.path().to_path_buf()).unwrap();
    (dir, root)
}

/// A world whose server has run once: its save holds a few files.
fn played(root: &DataRoot) -> WorldFile {
    let world = world::create(root, "Our base", "1.21.1", WorldSettings::default()).unwrap();
    let save = save_dir(root, world.id);
    fs::create_dir_all(save.join("region")).unwrap();
    fs::write(save.join("level.dat"), b"level one").unwrap();
    fs::write(save.join("region/r.0.0.mca"), vec![7u8; 10_000]).unwrap();
    world
}

fn read(path: &Path) -> Vec<u8> {
    fs::read(path).unwrap()
}

fn instance(id: InstanceId, loader: Option<crate::loader::LoaderChoice>) -> InstanceFile {
    InstanceFile {
        schema_version: SCHEMA_VERSION,
        id,
        display_name: "Survival".into(),
        version_id: "1.21.1".into(),
        manifest_sha1: None,
        created_at: Utc::now(),
        last_played_at: None,
        max_heap_mib: 4096,
        resolution: Resolution::default(),
        health: InstallationHealth::Ready,
        last_result: None,
        java: None,
        loader,
        notes: String::new(),
        java_provider: None,
        custom_java: None,
        jvm_arguments: String::new(),
        profile: None,
        game_arguments: String::new(),
        fullscreen: false,
        launcher_skins: true,
        world_room: None,
    }
}

#[test]
fn space_is_checked_with_a_margin() {
    assert!(enough_space(None, u64::MAX));
    assert!(enough_space(Some(SPACE_MARGIN + 10), 10));
    assert!(!enough_space(Some(SPACE_MARGIN + 9), 10));
    assert!(!enough_space(Some(0), u64::MAX));
}

#[test]
fn a_restore_backs_up_the_current_save_first() {
    let (_dir, root) = root();
    let world = played(&root);
    let save = save_dir(&root, world.id);
    let first = backup(&root, world.id, Reason::Manual).unwrap().unwrap();
    assert_eq!(first.file_count, 2);

    fs::write(save.join("level.dat"), b"level two").unwrap();
    fs::remove_file(save.join("region/r.0.0.mca")).unwrap();
    restore(&root, world.id, &first.id).unwrap();
    assert_eq!(read(&save.join("level.dat")), b"level one");
    assert_eq!(read(&save.join("region/r.0.0.mca")).len(), 10_000);

    let reasons: Vec<Reason> = list(&root, world.id).iter().map(|b| b.reason).collect();
    assert_eq!(reasons, [Reason::BeforeRestore, Reason::Manual]);
    // What the restore replaced can come back too.
    let before = &list(&root, world.id)[0];
    restore(&root, world.id, &before.id).unwrap();
    assert_eq!(read(&save.join("level.dat")), b"level two");
    assert!(!save.join("region/r.0.0.mca").exists());
    assert!(world::load(&root, world.id).unwrap().mutation.is_none());
    // Nothing staged is left beside the save.
    let names: Vec<String> = fs::read_dir(root.world_server_dir(world.id))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, [SAVE]);
}

#[test]
fn backups_after_play_skip_an_unchanged_world_and_keep_ten() {
    let (_dir, root) = root();
    let world = played(&root);
    let save = save_dir(&root, world.id);
    assert!(
        backup(&root, world.id, Reason::AfterPlay)
            .unwrap()
            .is_some()
    );
    assert!(
        backup(&root, world.id, Reason::AfterPlay)
            .unwrap()
            .is_none()
    );
    for round in 0..12u8 {
        fs::write(save.join("level.dat"), [round; 3]).unwrap();
        // A different size, so the change shows without waiting for the clock.
        fs::write(save.join("stats.json"), vec![b'x'; usize::from(round) + 1]).unwrap();
        backup(&root, world.id, Reason::AfterPlay).unwrap().unwrap();
    }
    let manual = backup(&root, world.id, Reason::Manual).unwrap().unwrap();
    let all = list(&root, world.id);
    assert_eq!(
        all.iter().filter(|b| b.reason == Reason::AfterPlay).count(),
        KEEP_AFTER_PLAY
    );
    assert!(all.iter().any(|b| b.id == manual.id));
}

#[test]
fn a_world_never_played_has_nothing_to_back_up() {
    let (_dir, root) = root();
    let world = world::create(&root, "New", "1.21.1", WorldSettings::default()).unwrap();
    assert!(backup(&root, world.id, Reason::Manual).is_err());
    assert!(world::load(&root, world.id).unwrap().mutation.is_none());
}

#[test]
fn a_damaged_backup_leaves_the_world_as_it_was() {
    let (_dir, root) = root();
    let world = played(&root);
    let save = save_dir(&root, world.id);
    let first = backup(&root, world.id, Reason::Manual).unwrap().unwrap();
    fs::write(save.join("level.dat"), b"level now").unwrap();
    for prefix in fs::read_dir(root.world_backups_dir().join("objects")).unwrap() {
        for object in fs::read_dir(prefix.unwrap().path()).unwrap() {
            fs::write(object.unwrap().path(), b"junk").unwrap();
        }
    }
    let error = restore(&root, world.id, &first.id).unwrap_err();
    assert_eq!(error.code, ErrorCode::HashMismatch);
    assert_eq!(read(&save.join("level.dat")), b"level now");
    assert!(world::load(&root, world.id).unwrap().mutation.is_none());
}

#[test]
fn a_backup_of_another_world_or_junk_is_refused() {
    let (_dir, root) = root();
    let ours = played(&root);
    let theirs = played(&root);
    let their_backup = backup(&root, theirs.id, Reason::Manual).unwrap().unwrap();
    assert!(restore(&root, ours.id, &their_backup.id).is_err());
    for junk in [
        "",
        "../../objects",
        "not-a-uuid",
        "7f1d3b8e-2c4a-4f7e-9b1a-0c2d3e4f5a6b",
    ] {
        assert!(restore(&root, ours.id, junk).is_err(), "{junk}");
    }
}

#[test]
fn an_interrupted_restore_puts_the_world_back() {
    let (_dir, root) = root();
    let mut world = played(&root);
    let server = root.world_server_dir(world.id);
    // Cut between moving the save aside and moving the restored one in.
    fs::rename(
        server.join(SAVE),
        server.join(format!("{SAVE}.replaced-abc")),
    )
    .unwrap();
    fs::create_dir_all(server.join(format!("{SAVE}.restoring-abc"))).unwrap();
    world.mutation = Some(Mutation {
        kind: MutationKind::Restore,
        operation_id: "op".into(),
        started_at: Utc::now(),
    });
    world::save(&root, &world).unwrap();

    assert!(recover(&root, &mut world).unwrap());
    assert_eq!(read(&server.join(SAVE).join("level.dat")), b"level one");
    assert!(!server.join(format!("{SAVE}.restoring-abc")).exists());
    assert!(!server.join(format!("{SAVE}.replaced-abc")).exists());
    assert!(world::load(&root, world.id).unwrap().mutation.is_none());

    // Cut after the swap: the restored save stays, the old one goes.
    fs::create_dir_all(server.join(format!("{SAVE}.replaced-def"))).unwrap();
    world.mutation = Some(Mutation {
        kind: MutationKind::Restore,
        operation_id: "op".into(),
        started_at: Utc::now(),
    });
    assert!(recover(&root, &mut world).unwrap());
    assert_eq!(read(&server.join(SAVE).join("level.dat")), b"level one");
    assert!(!server.join(format!("{SAVE}.replaced-def")).exists());
}

#[test]
fn an_interrupted_import_leaves_no_world_and_hosting_marks_stay() {
    let (_dir, root) = root();
    let mut world = world::create(&root, "Half", "1.21.1", WorldSettings::default()).unwrap();
    fs::create_dir_all(
        root.world_server_dir(world.id)
            .join(format!("{SAVE}.importing-x")),
    )
    .unwrap();
    world.mutation = Some(Mutation {
        kind: MutationKind::Import,
        operation_id: "op".into(),
        started_at: Utc::now(),
    });
    world::save(&root, &world).unwrap();
    assert!(!recover(&root, &mut world).unwrap());
    assert!(!root.world_dir(world.id).exists());

    // A hosting mark is the host's to read, not this module's.
    let mut hosted = played(&root);
    hosted.mutation = Some(Mutation {
        kind: MutationKind::Host,
        operation_id: "op".into(),
        started_at: Utc::now(),
    });
    world::save(&root, &hosted).unwrap();
    assert!(recover(&root, &mut hosted).unwrap());
    assert!(world::load(&root, hosted.id).unwrap().mutation.is_some());
}

#[test]
fn an_import_copies_the_world_and_leaves_the_original() {
    let (_dir, root) = root();
    let id = InstanceId::generate();
    let source = root.game_dir(id).join("saves").join("Home");
    fs::create_dir_all(source.join("region")).unwrap();
    fs::write(source.join("level.dat"), b"home").unwrap();
    fs::write(source.join("region/r.0.0.mca"), vec![1u8; 4096]).unwrap();
    fs::write(source.join("session.lock"), b"lock").unwrap();
    fs::create_dir_all(root.game_dir(id).join("saves").join("Notes")).unwrap();
    let survival = instance(id, None);

    let found = importable(&root, std::slice::from_ref(&survival));
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].saves, ["Home"]);

    let world = import(&root, &survival, "Home", "Home together").unwrap();
    assert_eq!(world.name, "Home together");
    assert_eq!(world.minecraft, "1.21.1");
    assert!(world.mutation.is_none());
    let save = save_dir(&root, world.id);
    assert_eq!(read(&save.join("level.dat")), b"home");
    assert_eq!(read(&save.join("region/r.0.0.mca")).len(), 4096);
    assert!(!save.join("session.lock").exists());
    assert_eq!(read(&source.join("level.dat")), b"home");
    assert!(source.join("session.lock").exists());

    // Not a world, a path out of saves.
    assert!(import(&root, &survival, "Notes", "x").is_err());
    for junk in ["../Home", "..", "", "a/b", " Home"] {
        assert!(import(&root, &survival, junk, "x").is_err(), "{junk:?}");
    }
    // Only the one world was made.
    assert_eq!(world::list(&root).len(), 1);

    // An installation with a loader worlds with mods run is listed with it;
    // another loader, or a release not tested with it, is not (W12).
    let loader = |kind, version: &str| {
        Some(crate::loader::LoaderChoice {
            kind,
            version: version.into(),
        })
    };
    let fabric = instance(id, loader(crate::loader::LoaderKind::Fabric, "0.17.2"));
    let found = importable(&root, std::slice::from_ref(&fabric));
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].loader, fabric.loader);
    let optifine = instance(
        id,
        loader(crate::loader::LoaderKind::OptiFine, "1.20.1_HD_U_I6"),
    );
    assert!(importable(&root, &[optifine]).is_empty());
    let mut untested = fabric.clone();
    untested.version_id = "1.13.2".into();
    assert!(importable(&root, &[untested]).is_empty());
}

#[test]
fn an_export_holds_the_manifest_and_the_save_only() {
    let (dir, root) = root();
    let world = played(&root);
    let server = root.world_server_dir(world.id);
    fs::write(server.join("server.properties"), b"rcon.password=secret").unwrap();
    fs::write(save_dir(&root, world.id).join("session.lock"), b"lock").unwrap();
    let out = dir.path().join("out");
    fs::create_dir_all(&out).unwrap();
    let destination = out.join("Our base.zip");
    assert!(export(&root, world.id, &destination).unwrap() > 0);

    let mut zip = zip::ZipArchive::new(fs::File::open(&destination).unwrap()).unwrap();
    let mut names: Vec<String> = (0..zip.len())
        .map(|i| zip.by_index(i).unwrap().name().to_owned())
        .collect();
    names.sort();
    assert_eq!(
        names,
        ["manifest.json", "world/level.dat", "world/region/r.0.0.mca"]
    );
    let mut manifest = String::new();
    zip.by_name("manifest.json")
        .unwrap()
        .read_to_string(&mut manifest)
        .unwrap();
    let manifest: serde_json::Value = serde_json::from_str(&manifest).unwrap();
    assert_eq!(manifest["format"], FORMAT);
    assert_eq!(manifest["name"], "Our base");
    assert!(
        !manifest
            .to_string()
            .contains(dir.path().to_string_lossy().as_ref())
    );
    // Only the finished zip is in the folder.
    assert_eq!(fs::read_dir(&out).unwrap().count(), 1);
    // No folder to write to: nothing is written.
    assert!(export(&root, world.id, &dir.path().join("missing/x.zip")).is_err());
}
