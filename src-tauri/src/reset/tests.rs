use super::*;
use crate::state::NoopSink;
use crate::state::dto::{CreateInstancePayload, MutationRequest, SettingsPatch};
use crate::storage::models::{InstallationHealth, Theme};
use crate::validation::MemoryPolicy;

fn request<T>(payload: T) -> MutationRequest<T> {
    MutationRequest {
        request_id: Uuid::new_v4().to_string(),
        payload,
    }
}

fn setup() -> (tempfile::TempDir, AppState, InstanceId, InstanceId) {
    let dir = tempfile::tempdir().unwrap();
    let state = AppState::initialize(
        DataRoot::new(dir.path().to_path_buf()),
        "test",
        MemoryPolicy::from_physical_bytes(16 << 30),
        Box::new(NoopSink),
    );
    let mut ids = Vec::new();
    for name in ["Survival", "Empty"] {
        ids.push(
            state
                .create_instance(request(CreateInstancePayload {
                    display_name: name.into(),
                    version_id: "1.21.1".into(),
                    loader: None,
                }))
                .unwrap()
                .id,
        );
    }
    let root = state.data_root().unwrap();
    let world = root.game_dir(ids[0]).join("saves").join("Home");
    std::fs::create_dir_all(&world).unwrap();
    std::fs::write(world.join("level.dat"), "home").unwrap();
    std::fs::write(root.game_dir(ids[1]).join("options.txt"), "fov:0").unwrap();
    for (dir, file) in [
        ("shared/libraries/ab", "artifact.jar"),
        ("metadata/versions", "1.21.1.json"),
        ("runtimes/mojang", "java"),
        ("downloads/job", "x.part"),
        ("cache/thumbnails", "t.png"),
        ("logs/runs/run", "latest.log"),
    ] {
        std::fs::create_dir_all(root.path().join(dir)).unwrap();
        std::fs::write(root.path().join(dir).join(file), "0123456789").unwrap();
    }
    (dir, state, ids[0], ids[1])
}

#[test]
fn the_preview_lists_what_each_part_holds() {
    let (_dir, state, survival, _) = setup();
    state
        .add_notoken_profile(request(crate::state::dto::AddNoTokenProfilePayload {
            name: "Alex".into(),
        }))
        .unwrap();
    let found = preview(&state).unwrap();
    assert_eq!(
        found
            .installations
            .iter()
            .map(|i| i.display_name.as_str())
            .collect::<Vec<_>>(),
        ["Empty", "Survival"]
    );
    let with_data = found
        .installations
        .iter()
        .find(|i| i.instance_id == survival)
        .unwrap();
    assert_eq!(with_data.data.worlds, ["Home"]);
    assert!(!found.installations[0].data.any());
    assert_eq!(found.game_files_bytes, 30);
    assert_eq!(found.temporary_bytes, 30);
    assert_eq!((found.backup_count, found.notoken_profiles), (0, 1));
}

#[test]
fn a_reset_backs_up_player_data_then_removes_the_chosen_parts() {
    let (_dir, state, survival, empty) = setup();
    let root = state.data_root().unwrap();
    backup::create(&state, survival, Scope::Installation, Reason::Manual).unwrap();
    state
        .set_app_settings(request(SettingsPatch {
            theme: Some(Theme::Dark),
            language: Some("ru".into()),
            ..SettingsPatch::default()
        }))
        .unwrap();
    state
        .add_notoken_profile(request(crate::state::dto::AddNoTokenProfilePayload {
            name: "Alex".into(),
        }))
        .unwrap();

    // Refused while a game runs.
    {
        let _game = state
            .activity()
            .try_acquire(OperationKind::Launch, Some(survival), "run-1")
            .unwrap();
        assert!(
            run(
                &state,
                &ResetChoice {
                    installations: true,
                    ..ResetChoice::default()
                }
            )
            .is_err()
        );
    }

    let report = run(
        &state,
        &ResetChoice {
            settings: true,
            installations: true,
            back_up_installations: true,
            game_files: false,
            accounts: true,
            backups: true,
            traffic: true,
            temporary: true,
        },
    )
    .unwrap();
    assert_eq!(report.deleted_backups, 1, "the older backup");
    assert_eq!(report.backed_up, 1, "only the installation with a world");
    assert_eq!(report.deleted_installations, 2);
    assert!(report.freed_bytes > 0);
    assert!(state.instance(empty).is_none());
    // The new backup survives the reset and can bring the installation back.
    let left = Store::new(root.path()).list();
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].info.reason, Reason::BeforeDelete);
    assert_eq!(left[0].info.instance_id, survival);
    // Game files stay; temporary files go.
    assert!(
        root.path()
            .join("shared/libraries/ab/artifact.jar")
            .exists()
    );
    assert!(!root.downloads_dir().exists());
    assert!(!root.path().join("logs/runs").exists());
    // Settings are back to defaults except the language.
    let settings = state.settings();
    assert_eq!(settings.theme, Theme::default());
    assert_eq!(settings.language.as_str(), "ru");
    assert!(state.accounts_file().notoken_profiles.is_empty());
    assert!(state.activity().current().is_none(), "permit released");
}

#[test]
fn removing_game_files_asks_installed_versions_for_a_repair() {
    let (_dir, state, survival, empty) = setup();
    let root = state.data_root().unwrap();
    state
        .update_instance_health_for_test(survival, InstallationHealth::Ready)
        .unwrap();
    let report = run(
        &state,
        &ResetChoice {
            game_files: true,
            ..ResetChoice::default()
        },
    )
    .unwrap();
    assert_eq!(report.freed_bytes, 30);
    assert!(!root.path().join("shared").exists());
    assert!(!root.runtimes_dir().exists());
    assert_eq!(
        state.instance(survival).unwrap().health,
        InstallationHealth::RepairRequired
    );
    // Never installed stays so; worlds stay.
    assert_eq!(
        state.instance(empty).unwrap().health,
        InstallationHealth::NotInstalled
    );
    assert_eq!(backup::worlds(&root.game_dir(survival)), ["Home"]);
}
