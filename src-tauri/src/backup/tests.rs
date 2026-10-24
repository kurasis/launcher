//! Backups through the application state: permits, restores and automatic
//! backups after playing.

use super::*;
use crate::state::NoopSink;
use crate::state::dto::{CreateInstancePayload, MutationRequest, SettingsPatch};
use crate::storage::models::InstallationHealth;
use crate::validation::MemoryPolicy;

fn state_at(dir: &Path) -> AppState {
    AppState::initialize(
        DataRoot::new(dir.to_path_buf()),
        "test",
        MemoryPolicy::from_physical_bytes(16 << 30),
        Box::new(NoopSink),
    )
}

fn request<T>(payload: T) -> MutationRequest<T> {
    MutationRequest {
        request_id: Uuid::new_v4().to_string(),
        payload,
    }
}

fn world(state: &AppState, id: InstanceId, name: &str, text: &str) -> PathBuf {
    let dir = state
        .data_root()
        .unwrap()
        .game_dir(id)
        .join("saves")
        .join(name);
    std::fs::create_dir_all(dir.join("region")).unwrap();
    std::fs::write(dir.join("level.dat"), text).unwrap();
    std::fs::write(dir.join("region").join("r.0.0.mca"), "chunks").unwrap();
    dir
}

fn setup() -> (tempfile::TempDir, AppState, InstanceId) {
    let dir = tempfile::tempdir().unwrap();
    let state = state_at(dir.path());
    let id = state
        .create_instance(request(CreateInstancePayload {
            display_name: "Survival".into(),
            version_id: "1.21.1".into(),
            loader: None,
        }))
        .unwrap()
        .id;
    (dir, state, id)
}

#[test]
fn worlds_are_backed_up_and_restored_in_place_or_as_copies() {
    let (_dir, state, id) = setup();
    let folder = world(&state, id, "My World", "v1");
    world(&state, id, "Other", "o");
    let found = overview(&state).unwrap();
    assert_eq!(found.worlds[0].worlds, ["My World", "Other"]);

    let scope = Scope::World {
        name: "My World".into(),
    };
    let backup = create(&state, id, scope.clone(), Reason::Manual).unwrap();
    assert_eq!(backup.version_label, "Minecraft 1.21.1");
    assert_eq!(backup.file_count, 2);
    assert!(state.activity().current().is_none(), "permit released");

    // A later modification time, so the next backup reads the file again.
    std::thread::sleep(std::time::Duration::from_millis(20));
    std::fs::write(folder.join("level.dat"), "v2").unwrap();
    restore(&state, &backup.id, false).unwrap();
    assert_eq!(
        std::fs::read_to_string(folder.join("level.dat")).unwrap(),
        "v1"
    );
    // The replaced state was backed up first.
    let all = overview(&state).unwrap().backups;
    assert_eq!(all.len(), 2);
    assert_eq!(all[0].reason, Reason::BeforeRestore);

    let copy = restore(&state, &backup.id, true).unwrap().world.unwrap();
    assert!(copy.starts_with("My World (backup "), "{copy}");
    let copied = folder.parent().unwrap().join(&copy);
    assert_eq!(
        std::fs::read_to_string(copied.join("level.dat")).unwrap(),
        "v1"
    );

    // Deleting frees what only that backup used.
    let before_restore = all[0].id.clone();
    assert_eq!(delete(&state, &before_restore).unwrap(), 2, "\"v2\" only");
    assert_eq!(overview(&state).unwrap().backups.len(), 1);
}

#[test]
fn nothing_runs_beside_the_game_or_another_backup() {
    let (_dir, state, id) = setup();
    world(&state, id, "W", "v1");
    let _game = state
        .activity()
        .try_acquire(OperationKind::Launch, Some(id), "run-1")
        .unwrap();
    let error = create(
        &state,
        id,
        Scope::World { name: "W".into() },
        Reason::Manual,
    )
    .unwrap_err();
    assert!(matches!(
        error.code,
        ErrorCode::AppBusy | ErrorCode::InstanceBusy | ErrorCode::GameAlreadyRunning
    ));
}

#[test]
fn a_backup_is_not_deleted_beside_the_game_or_another_backup() {
    let (_dir, state, id) = setup();
    world(&state, id, "W", "v1");
    let backup = create(
        &state,
        id,
        Scope::World { name: "W".into() },
        Reason::Manual,
    )
    .unwrap();
    for kind in [OperationKind::Launch, OperationKind::Backup] {
        let _held = state
            .activity()
            .try_acquire(kind, Some(id), "op-1")
            .unwrap();
        let error = delete(&state, &backup.id).unwrap_err();
        assert!(
            matches!(
                error.code,
                ErrorCode::AppBusy | ErrorCode::InstanceBusy | ErrorCode::GameAlreadyRunning
            ),
            "{kind:?}: {error:?}"
        );
    }
    assert_eq!(overview(&state).unwrap().backups.len(), 1);
}

#[test]
fn backups_are_made_restored_and_deleted_beside_a_hosted_world() {
    let (_dir, state, id) = setup();
    let folder = world(&state, id, "W", "v1");
    let host = state
        .activity()
        .try_acquire_hosting(Uuid::new_v4(), "host-1")
        .unwrap();
    let alone = |state: &AppState| {
        assert!(state.activity().current().is_none(), "permit released");
        assert_eq!(state.activity().hosting().unwrap().operation_id, "host-1");
    };
    let scope = Scope::World { name: "W".into() };
    let backup = create(&state, id, scope, Reason::Manual).unwrap();
    alone(&state);
    // A later modification time, so the next backup reads the file again.
    std::thread::sleep(std::time::Duration::from_millis(20));
    std::fs::write(folder.join("level.dat"), "v2").unwrap();
    restore(&state, &backup.id, false).unwrap();
    restore(&state, &backup.id, true).unwrap();
    alone(&state);
    // The newest first: the state replaced by the restore ("v2"), then the manual one.
    let all = overview(&state).unwrap().backups;
    assert_eq!(all.len(), 2);
    assert_eq!(all[0].reason, Reason::BeforeRestore);
    assert_eq!(delete(&state, &all[0].id).unwrap(), 2, "\"v2\" only");
    alone(&state);
    assert_eq!(delete(&state, &backup.id).unwrap(), 2 + 6);
    alone(&state);
    assert!(overview(&state).unwrap().backups.is_empty());
    drop(host);
}

#[test]
fn a_deleted_installations_backup_is_deleted_beside_a_hosted_world() {
    let (_dir, state, _id) = setup();
    let other = state
        .create_instance(request(CreateInstancePayload {
            display_name: "Gone".into(),
            version_id: "1.21.1".into(),
            loader: None,
        }))
        .unwrap()
        .id;
    world(&state, other, "M", "m1");
    let saved = delete_instance(&state, other, true).unwrap().unwrap();
    assert!(state.instance(other).is_none());
    let _host = state
        .activity()
        .try_acquire_hosting(Uuid::new_v4(), "host-1")
        .unwrap();
    assert!(delete(&state, &saved.id).unwrap() > 0);
    assert!(state.activity().current().is_none(), "permit released");
    assert!(overview(&state).unwrap().backups.is_empty());
}

#[test]
fn junk_backup_ids_are_refused_before_any_permit() {
    let (_dir, state, id) = setup();
    world(&state, id, "W", "v1");
    let backup = create(
        &state,
        id,
        Scope::World { name: "W".into() },
        Reason::Manual,
    )
    .unwrap();
    let uuid = Uuid::parse_str(&backup.id).unwrap();
    let _host = state
        .activity()
        .try_acquire_hosting(Uuid::new_v4(), "host-1")
        .unwrap();
    let long = "a".repeat(10_000);
    for junk in [
        String::new(),
        "../snapshots/x".into(),
        format!("../snapshots/{}", backup.id),
        "not-a-uuid".into(),
        long,
        "a\0b".into(),
        Uuid::new_v4().to_string(),
        // Other spellings of the real one: never a path, so never another file.
        backup.id.to_uppercase(),
        format!("{{{}}}", backup.id),
        format!("urn:uuid:{}", backup.id),
        uuid.simple().to_string(),
    ] {
        let error = delete(&state, &junk).unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidInput, "{junk:.40}");
        assert!(state.activity().current().is_none(), "{junk:.40}");
        assert_eq!(
            restore(&state, &junk, true).unwrap_err().code,
            ErrorCode::InvalidInput,
            "{junk:.40}"
        );
    }
    // Refused by their spelling, not because no file has that name: where names ignore
    // case (Windows, macOS) the uppercase one is the real manifest, which the loop above
    // proves untouched. Linux keeps each spelling as a file of its own.
    if cfg!(target_os = "linux") {
        let snapshots = state.data_root().unwrap().path().join("backups/snapshots");
        let manifest = snapshots.join(format!("{}.json", backup.id));
        for other in [
            backup.id.to_uppercase(),
            format!("{{{}}}", backup.id),
            format!("urn:uuid:{}", backup.id),
            uuid.simple().to_string(),
        ] {
            let copy = snapshots.join(format!("{other}.json"));
            std::fs::copy(&manifest, &copy).unwrap();
            assert_eq!(
                delete(&state, &other).unwrap_err().code,
                ErrorCode::InvalidInput,
                "{other}"
            );
            assert!(copy.exists(), "{other}");
            std::fs::remove_file(copy).unwrap();
        }
    }
    assert_eq!(overview(&state).unwrap().backups.len(), 1);
}

#[test]
fn whole_installations_are_restored_in_place_only() {
    let (_dir, state, id) = setup();
    let root = state.data_root().unwrap();
    let game = root.game_dir(id);
    world(&state, id, "W", "v1");
    std::fs::create_dir_all(game.join("mods")).unwrap();
    std::fs::write(game.join("mods").join("a.jar"), "mod").unwrap();
    let backup = create(&state, id, Scope::Installation, Reason::Manual).unwrap();
    std::fs::remove_dir_all(game.join("mods")).unwrap();
    assert_eq!(
        restore(&state, &backup.id, true).unwrap_err().code,
        ErrorCode::InvalidInput
    );
    restore(&state, &backup.id, false).unwrap();
    assert_eq!(
        std::fs::read_to_string(game.join("mods").join("a.jar")).unwrap(),
        "mod"
    );
}

#[test]
fn after_play_backs_up_changed_worlds_and_keeps_the_newest() {
    let (_dir, state, id) = setup();
    let folder = world(&state, id, "W", "v1");
    world(&state, id, "Untouched", "u");
    after_play(&state, id);
    assert!(
        overview(&state).unwrap().backups.is_empty(),
        "off by default"
    );

    state
        .set_app_settings(request(SettingsPatch {
            backup_after_play: Some(true),
            backup_keep: Some(2),
            ..SettingsPatch::default()
        }))
        .unwrap();
    after_play(&state, id);
    assert_eq!(
        overview(&state).unwrap().backups.len(),
        2,
        "both worlds once"
    );
    after_play(&state, id);
    assert_eq!(
        overview(&state).unwrap().backups.len(),
        2,
        "nothing changed"
    );
    for n in 0..3 {
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(folder.join("level.dat"), format!("v{n}")).unwrap();
        after_play(&state, id);
    }
    let backups = overview(&state).unwrap().backups;
    let of_w = backups
        .iter()
        .filter(|b| b.scope == Scope::World { name: "W".into() })
        .count();
    assert_eq!(of_w, 2, "kept the newest two");
    assert_eq!(
        state
            .set_app_settings(request(SettingsPatch {
                backup_keep: Some(0),
                ..SettingsPatch::default()
            }))
            .unwrap_err()
            .code,
        ErrorCode::InvalidInput
    );
}

#[test]
fn user_data_counts_what_a_delete_would_lose() {
    let (_dir, state, id) = setup();
    assert_eq!(inspect(&state, id).unwrap(), UserData::default());
    assert!(!inspect(&state, id).unwrap().any());
    let game = state.data_root().unwrap().game_dir(id);
    std::fs::write(game.join("options.txt"), "fov:0").unwrap();
    assert!(!inspect(&state, id).unwrap().any(), "settings alone");
    world(&state, id, "My World", "v1");
    for (dir, file) in [
        ("mods", "sodium.jar"),
        ("mods", "lithium.jar"),
        ("resourcepacks", "faithful.zip"),
        ("shaderpacks", "bsl"),
        ("screenshots", "2026-10-18.png"),
    ] {
        std::fs::create_dir_all(game.join(dir)).unwrap();
        std::fs::write(game.join(dir).join(file), "x").unwrap();
    }
    std::fs::write(game.join("mods").join(".index"), "hidden").unwrap();
    let found = inspect(&state, id).unwrap();
    assert!(found.any());
    assert_eq!(found.worlds, ["My World"]);
    assert_eq!(
        (
            found.mods,
            found.resource_packs,
            found.shader_packs,
            found.screenshots
        ),
        (2, 1, 1, 1)
    );
    // Five one-byte files, the hidden one, options and the world's two files.
    assert_eq!(found.bytes, 5 + 6 + 5 + 2 + 6);
}

#[test]
fn deleted_installations_can_come_back_from_their_backup() {
    let (dir, state, id) = setup();
    let other = state
        .create_instance(request(CreateInstancePayload {
            display_name: "Fabric".into(),
            version_id: "1.21.1".into(),
            loader: Some(crate::loader::LoaderChoice {
                kind: crate::loader::LoaderKind::Fabric,
                version: "0.16.14".into(),
            }),
        }))
        .unwrap()
        .id;
    world(&state, other, "Modded", "m1");
    state
        .set_app_settings(request(SettingsPatch {
            selected_instance_id: Some(other),
            ..SettingsPatch::default()
        }))
        .unwrap();
    let root = state.data_root().unwrap();

    // Not while its game runs.
    {
        let _game = state
            .activity()
            .try_acquire(OperationKind::Launch, Some(other), "run-1")
            .unwrap();
        assert!(delete_instance(&state, other, true).is_err());
        assert!(state.instance(other).is_some());
    }

    let saved = delete_instance(&state, other, true).unwrap().unwrap();
    assert_eq!(saved.reason, Reason::BeforeDelete);
    assert_eq!(saved.scope, Scope::Installation);
    assert_eq!(saved.version_id.as_deref(), Some("1.21.1"));
    assert_eq!(saved.loader.as_ref().unwrap().version, "0.16.14");
    assert!(state.instance(other).is_none());
    assert!(!root.instance_dir(other).exists());
    assert!(state.activity().current().is_none(), "permit released");
    // The other installation is selected instead.
    assert_eq!(state.settings().selected_instance_id, Some(id));

    // Deleting without a backup leaves no backup.
    let world_backup = create(
        &state,
        id,
        Scope::World {
            name: "none".into(),
        },
        Reason::Manual,
    );
    assert!(world_backup.is_err(), "no such world");
    world(&state, id, "Plain", "p");
    let plain_world = create(
        &state,
        id,
        Scope::World {
            name: "Plain".into(),
        },
        Reason::Manual,
    )
    .unwrap();
    assert_eq!(delete_instance(&state, id, false).unwrap(), None);
    assert!(state.instance_ids().is_empty());
    assert_eq!(state.settings().selected_instance_id, None);
    // A world backup alone cannot bring an installation back.
    assert_eq!(
        restore(&state, &plain_world.id, false).unwrap_err().code,
        ErrorCode::InstanceNotFound
    );

    // The whole-installation backup brings it back with its world, not installed.
    restore(&state, &saved.id, false).unwrap();
    let back = state.instance(other).unwrap();
    assert_eq!(back.display_name, "Fabric");
    assert_eq!(back.loader.unwrap().version, "0.16.14");
    assert_eq!(back.health, InstallationHealth::NotInstalled);
    assert_eq!(worlds(&root.game_dir(other)), ["Modded"]);

    // A delete interrupted after moving the folder aside is finished at start.
    let trash = root.instances_dir().join(format!(".deleted-{other}-x"));
    std::fs::create_dir_all(trash.join("game")).unwrap();
    drop(state);
    let state = state_at(dir.path());
    assert!(!trash.exists());
    assert!(state.instance(other).is_some());
}

#[test]
fn the_overview_says_what_deleted_installations_backups_alone_hold() {
    let (_dir, state, id) = setup();
    let make = |name: &str| {
        state
            .create_instance(request(CreateInstancePayload {
                display_name: name.into(),
                version_id: "1.21.1".into(),
                loader: None,
            }))
            .unwrap()
            .id
    };
    // Every world's region holds "chunks": stored once, used by the live one too.
    world(&state, id, "W", "v1");
    create(
        &state,
        id,
        Scope::World { name: "W".into() },
        Reason::Manual,
    )
    .unwrap();
    let gone = make("Gone");
    world(&state, gone, "M", "m1");
    let saved = delete_instance(&state, gone, true).unwrap().unwrap();

    let found = overview(&state).unwrap();
    assert_eq!(
        found.deleted_bytes,
        [DeletedBytes {
            instance_id: gone,
            bytes: 2
        }],
        "\"m1\" only"
    );
    assert_eq!(found.deleted_total_bytes, 2);
    let json = serde_json::to_value(&found).unwrap();
    assert_eq!(
        json["deletedBytes"],
        serde_json::json!([{ "instanceId": gone.to_string(), "bytes": 2 }])
    );
    assert_eq!(json["deletedTotalBytes"], 2);

    // A copy deleted too: what the two share counts for neither alone, but for both
    // together. The newest deletion comes first.
    let copy = make("Gone copy");
    world(&state, copy, "M", "m1");
    let copied = delete_instance(&state, copy, true).unwrap().unwrap();
    let found = overview(&state).unwrap();
    assert_eq!(
        found.deleted_bytes,
        [
            DeletedBytes {
                instance_id: copy,
                bytes: 0
            },
            DeletedBytes {
                instance_id: gone,
                bytes: 0
            }
        ]
    );
    assert_eq!(found.deleted_total_bytes, 2);
    assert_eq!(delete(&state, &copied.id).unwrap(), 0);
    assert_eq!(delete(&state, &saved.id).unwrap(), 2);
    let found = overview(&state).unwrap();
    assert!(found.deleted_bytes.is_empty());
    assert_eq!(found.deleted_total_bytes, 0);
}
