use std::sync::{Arc, Mutex};

use super::*;
use crate::storage::models::{DuringGameMode, OperationKind, Resolution, Theme};

const GIB: u64 = 1024 * 1024 * 1024;

#[derive(Clone, Default)]
struct RecordingSink(Arc<Mutex<Vec<(String, u64, serde_json::Value)>>>);

impl EventSink for RecordingSink {
    fn emit(&self, event: &BackendEvent) {
        self.0
            .lock()
            .unwrap()
            .push((event.name().to_owned(), event.revision(), event.payload()));
    }
}

impl RecordingSink {
    fn events(&self) -> Vec<(String, u64, serde_json::Value)> {
        self.0.lock().unwrap().clone()
    }
}

fn state_at(root: &std::path::Path, sink: RecordingSink) -> AppState {
    AppState::initialize(
        DataRoot::new(root.to_path_buf()),
        "0.1.0",
        MemoryPolicy::from_physical_bytes(16 * GIB),
        Box::new(sink),
    )
}

fn request<T>(payload: T) -> MutationRequest<T> {
    MutationRequest {
        request_id: Uuid::new_v4().to_string(),
        payload,
    }
}

fn create(state: &AppState, name: &str, version: &str) -> LauncherResult<InstanceSummary> {
    state.create_instance(request(CreateInstancePayload {
        display_name: name.into(),
        version_id: version.into(),
        loader: None,
    }))
}

#[test]
fn created_instance_and_selection_survive_restart() {
    let dir = tempfile::tempdir().unwrap();
    let id = {
        let state = state_at(dir.path(), RecordingSink::default());
        let created = create(&state, "  Survival  ", "1.21.1").unwrap();
        assert_eq!(created.display_name, "Survival");
        assert_eq!(created.health, InstallationHealth::NotInstalled);
        assert!(created.manifest_sha1.is_none());
        // Recommended for vanilla 1.21.1 on 16 GiB (D-058).
        assert_eq!(created.max_heap_mib, 3072);
        state
            .update_instance_settings(request(UpdateInstanceSettingsPayload {
                instance_id: created.id,
                max_heap_mib: 6144,
                resolution: Resolution {
                    width: 1920,
                    height: 1080,
                },
                fullscreen: None,
                game_arguments: None,
            }))
            .unwrap();
        created.id
    };
    let state = state_at(dir.path(), RecordingSink::default());
    let snapshot = state.snapshot();
    assert_eq!(snapshot.settings.selected_instance_id, Some(id));
    assert_eq!(snapshot.instances.len(), 1);
    let restored = &snapshot.instances[0];
    assert_eq!(restored.max_heap_mib, 6144);
    assert_eq!(restored.resolution.width, 1920);
    assert_eq!(restored.version_id, "1.21.1");
}

#[test]
fn each_revision_emits_exactly_one_event() {
    let dir = tempfile::tempdir().unwrap();
    let sink = RecordingSink::default();
    let state = state_at(dir.path(), sink.clone());
    assert_eq!(state.snapshot().revision, 0);
    let first = create(&state, "A", "1.21.1").unwrap();
    // First instance is auto-selected: instance + settings events.
    create(&state, "B", "26.3").unwrap();
    let events = sink.events();
    let names: Vec<_> = events.iter().map(|(n, r, _)| (n.as_str(), *r)).collect();
    assert_eq!(
        names,
        [
            ("instance_changed", 1),
            ("settings_changed", 2),
            ("instance_changed", 3)
        ]
    );
    assert_eq!(state.snapshot().revision, 3);
    assert_eq!(events[0].2["instance"]["id"], first.id.to_string());
    assert_eq!(events[0].2["sessionEpoch"], state.session_epoch());
    assert_eq!(
        state.snapshot().settings.selected_instance_id,
        Some(first.id)
    );
}

#[test]
fn repeated_request_id_is_idempotent_and_conflicts_are_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let state = state_at(dir.path(), RecordingSink::default());
    let req = request(CreateInstancePayload {
        display_name: "Once".into(),
        version_id: "1.21.1".into(),
        loader: None,
    });
    let first = state.create_instance(req.clone()).unwrap();
    let second = state.create_instance(req.clone()).unwrap();
    assert_eq!(first, second);
    assert_eq!(state.snapshot().instances.len(), 1);

    let conflicting = MutationRequest {
        request_id: req.request_id.clone(),
        payload: CreateInstancePayload {
            display_name: "Other".into(),
            version_id: "1.21.1".into(),
            loader: None,
        },
    };
    assert_eq!(
        state.create_instance(conflicting).unwrap_err().code,
        ErrorCode::RequestIdConflict
    );
    let bad_id = MutationRequest {
        request_id: "../x".into(),
        payload: req.payload,
    };
    assert_eq!(
        state.create_instance(bad_id).unwrap_err().code,
        ErrorCode::InvalidInput
    );
}

#[test]
fn invalid_inputs_are_rejected_without_changes() {
    let dir = tempfile::tempdir().unwrap();
    let sink = RecordingSink::default();
    let state = state_at(dir.path(), sink.clone());
    for (name, version) in [("", "1.21.1"), ("ok", "../1.21"), ("ok", "a/b"), ("ok", "")] {
        assert_eq!(
            create(&state, name, version).unwrap_err().code,
            ErrorCode::InvalidInput
        );
    }
    let created = create(&state, "Valid", "26.3").unwrap();
    let events_before = sink.events().len();
    for (heap, width, height) in [(512, 1280, 720), (4096, 100, 720), (999_999, 1280, 720)] {
        let error = state
            .update_instance_settings(request(UpdateInstanceSettingsPayload {
                instance_id: created.id,
                max_heap_mib: heap,
                resolution: Resolution { width, height },
                fullscreen: None,
                game_arguments: None,
            }))
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidInput);
    }
    let missing = state
        .update_instance_settings(request(UpdateInstanceSettingsPayload {
            instance_id: InstanceId::generate(),
            max_heap_mib: 4096,
            resolution: Resolution::default(),
            fullscreen: None,
            game_arguments: None,
        }))
        .unwrap_err();
    assert_eq!(missing.code, ErrorCode::InstanceNotFound);
    assert_eq!(sink.events().len(), events_before);
    assert_eq!(state.snapshot().instances[0].max_heap_mib, 3072);
}

#[test]
fn settings_patch_validates_and_persists() {
    let dir = tempfile::tempdir().unwrap();
    let state = state_at(dir.path(), RecordingSink::default());
    let view = state
        .set_app_settings(request(SettingsPatch {
            theme: Some(Theme::Dark),
            during_game_mode: Some(DuringGameMode::KeepOpen),
            default_max_heap_mib: Some(3072),
            ..SettingsPatch::default()
        }))
        .unwrap();
    assert_eq!(view.theme, Theme::Dark);
    let error = state
        .set_app_settings(request(SettingsPatch {
            selected_instance_id: Some(InstanceId::generate()),
            ..SettingsPatch::default()
        }))
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::InstanceNotFound);
    drop(state);

    let state = state_at(dir.path(), RecordingSink::default());
    let settings = state.snapshot().settings;
    assert_eq!(settings.theme, Theme::Dark);
    assert_eq!(settings.during_game_mode, DuringGameMode::KeepOpen);
    assert_eq!(settings.default_max_heap_mib, Some(3072));
    assert_eq!(
        create(&state, "Uses default", "1.21.1")
            .unwrap()
            .max_heap_mib,
        3072
    );
}

#[test]
fn unsupported_settings_schema_blocks_mutation_without_data_loss() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("settings.json"), br#"{"schemaVersion":9}"#).unwrap();
    let state = state_at(dir.path(), RecordingSink::default());
    let snapshot = state.snapshot();
    assert!(!snapshot.settings.writable);
    assert_eq!(
        snapshot.storage_issues[0].code,
        ErrorCode::UnsupportedSchema
    );
    let error = state
        .set_app_settings(request(SettingsPatch {
            theme: Some(Theme::Light),
            ..SettingsPatch::default()
        }))
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::UnsupportedSchema);
    // Instances can still be created, but selection is not persisted.
    create(&state, "Still works", "1.21.1").unwrap();
    assert_eq!(
        std::fs::read(dir.path().join("settings.json")).unwrap(),
        br#"{"schemaVersion":9}"#
    );
}

#[test]
fn locked_data_root_makes_state_unavailable() {
    let dir = tempfile::tempdir().unwrap();
    let _owner = state_at(dir.path(), RecordingSink::default());
    let second = state_at(dir.path(), RecordingSink::default());
    let snapshot = second.snapshot();
    assert_eq!(snapshot.data_root.status, DataRootStatus::Unavailable);
    assert_eq!(
        snapshot.data_root.error.unwrap().code,
        ErrorCode::DataRootLocked
    );
    assert_eq!(
        create(&second, "x", "1.21.1").unwrap_err().code,
        ErrorCode::DataRootLocked
    );
}

#[test]
fn notes_are_saved_while_busy_and_survive_restart() {
    let dir = tempfile::tempdir().unwrap();
    let id = {
        let state = state_at(dir.path(), RecordingSink::default());
        let created = create(&state, "Noted", "1.21.1").unwrap();
        assert_eq!(created.notes, "");
        let _permit = state
            .activity()
            .try_acquire(OperationKind::Launch, Some(created.id), "run-1")
            .unwrap();
        let saved = state
            .set_instance_notes(request(SetInstanceNotesPayload {
                instance_id: created.id,
                notes: "Modpack: test\r\nseed 42 ".into(),
            }))
            .unwrap();
        assert_eq!(saved.notes, "Modpack: test\nseed 42");
        assert!(saved.busy);
        let error = state
            .set_instance_notes(request(SetInstanceNotesPayload {
                instance_id: created.id,
                notes: "bad\u{0000}".into(),
            }))
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidInput);
        created.id
    };
    let state = state_at(dir.path(), RecordingSink::default());
    assert_eq!(state.instance(id).unwrap().notes, "Modpack: test\nseed 42");
}

#[test]
fn busy_instance_cannot_be_reconfigured() {
    let dir = tempfile::tempdir().unwrap();
    let state = state_at(dir.path(), RecordingSink::default());
    let created = create(&state, "Busy", "1.21.1").unwrap();
    let _permit = state
        .activity()
        .try_acquire(OperationKind::Install, Some(created.id), "op-1")
        .unwrap();
    assert!(state.snapshot().instances[0].busy);
    let error = state
        .update_instance_settings(request(UpdateInstanceSettingsPayload {
            instance_id: created.id,
            max_heap_mib: 2048,
            resolution: Resolution::default(),
            fullscreen: None,
            game_arguments: None,
        }))
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::InstanceBusy);
    assert_eq!(
        state.snapshot().active_operation.unwrap().operation_id,
        "op-1"
    );
}

#[test]
fn snapshot_exposes_no_secret_fields() {
    let dir = tempfile::tempdir().unwrap();
    let state = state_at(dir.path(), RecordingSink::default());
    create(&state, "A", "1.21.1").unwrap();
    let json = serde_json::to_value(state.snapshot()).unwrap();
    fn walk(value: &serde_json::Value, path: &str) {
        match value {
            serde_json::Value::Object(map) => {
                for (key, child) in map {
                    // "notoken" names the profiles that have no token (D-040).
                    let lower = key.to_ascii_lowercase().replace("notoken", "");
                    for forbidden in [
                        "token", "secret", "password", "verifier", "refresh", "vault",
                    ] {
                        assert!(!lower.contains(forbidden), "{path}.{key}");
                    }
                    walk(child, &format!("{path}.{key}"));
                }
            }
            serde_json::Value::Array(items) => {
                for item in items {
                    walk(item, path);
                }
            }
            _ => {}
        }
    }
    walk(&json, "snapshot");
    assert_eq!(json["apiSchemaVersion"], 1);
    assert_eq!(json["account"]["loginState"], "signed_out");
    assert_eq!(json["account"]["accounts"], serde_json::json!([]));
}

#[test]
fn malformed_payloads_are_rejected_by_deserialization() {
    let unknown_field = serde_json::json!({
        "requestId": "r1",
        "payload": {"displayName": "x", "versionId": "1.21.1", "path": "/etc"}
    });
    assert!(
        serde_json::from_value::<MutationRequest<CreateInstancePayload>>(unknown_field).is_err()
    );
    let traversal_id = serde_json::json!({
        "requestId": "r2",
        "payload": {"instanceId": "../../x", "maxHeapMib": 2048,
                    "resolution": {"width": 1280, "height": 720}}
    });
    assert!(
        serde_json::from_value::<MutationRequest<UpdateInstanceSettingsPayload>>(traversal_id)
            .is_err()
    );
}

#[test]
fn notoken_profiles_defaults_and_installation_choices() {
    let dir = tempfile::tempdir().unwrap();
    let (alex, id) = {
        let state = state_at(dir.path(), RecordingSink::default());
        let instance = create(&state, "A", "1.21.1").unwrap().id;
        assert_eq!(
            state.profile_for(instance),
            None,
            "nobody: the built-in demo player"
        );
        let add = |name: &str| {
            state.add_notoken_profile(request(AddNoTokenProfilePayload { name: name.into() }))
        };
        let alex = add(" Alex ").unwrap();
        assert_eq!(alex.name, "Alex");
        for bad in ["alex", "a", "Has space", "Jürgen"] {
            assert_eq!(add(bad).unwrap_err().code, ErrorCode::InvalidInput, "{bad}");
        }
        let steve = add("Steve").unwrap();
        let account = state.account();
        assert_eq!(account.notoken_profiles.len(), 2);
        let alex_ref = ProfileRef::NoToken {
            id: alex.id.clone(),
        };
        let steve_ref = ProfileRef::NoToken {
            id: steve.id.clone(),
        };
        assert_eq!(
            account.default_profile,
            Some(alex_ref.clone()),
            "the first is the default"
        );
        assert_eq!(state.profile_for(instance), Some(alex_ref.clone()));

        // An installation can name another profile, even while it is busy.
        let _permit = state
            .activity()
            .try_acquire(OperationKind::Launch, Some(instance), "run-1")
            .unwrap();
        let chosen = state
            .set_instance_profile(request(SetInstanceProfilePayload {
                instance_id: instance,
                profile: Some(steve_ref.clone()),
            }))
            .unwrap();
        assert_eq!(chosen.profile, Some(steve_ref.clone()));
        assert_eq!(state.profile_for(instance), Some(steve_ref.clone()));
        assert_eq!(
            state
                .set_instance_profile(request(SetInstanceProfilePayload {
                    instance_id: instance,
                    profile: Some(ProfileRef::Microsoft {
                        id: "nobody".into()
                    }),
                }))
                .unwrap_err()
                .code,
            ErrorCode::InvalidInput
        );

        // Removing its profile sends the installation back to the default.
        state
            .remove_notoken_profile(request(ProfileIdPayload {
                profile_id: steve.id.clone(),
            }))
            .unwrap();
        assert_eq!(state.profile_for(instance), Some(alex_ref.clone()));
        state
            .set_default_profile(request(SetDefaultProfilePayload { profile: None }))
            .unwrap();
        assert_eq!(state.profile_for(instance), None);
        state
            .set_default_profile(request(SetDefaultProfilePayload {
                profile: Some(alex_ref),
            }))
            .unwrap();
        (alex, instance)
    };
    // Everything survives a restart; the name is what a demo launch plays as.
    let state = state_at(dir.path(), RecordingSink::default());
    assert_eq!(
        state.profile_for(id),
        Some(ProfileRef::NoToken {
            id: alex.id.clone()
        })
    );
    assert_eq!(state.notoken_name(&alex.id).as_deref(), Some("Alex"));
}

#[test]
fn loader_version_changes_and_asks_for_an_install() {
    let dir = tempfile::tempdir().unwrap();
    let state = state_at(dir.path(), RecordingSink::default());
    let fabric = state
        .create_instance(request(CreateInstancePayload {
            display_name: "Fabric".into(),
            version_id: "1.21.1".into(),
            loader: Some(crate::loader::LoaderChoice {
                kind: crate::loader::LoaderKind::Fabric,
                version: "0.16.14".into(),
            }),
        }))
        .unwrap();
    // As after an install.
    state
        .mutate("test_ready", request(()), |data, store, (), changes| {
            let mut ready = data.instances[&fabric.id].clone();
            ready.health = InstallationHealth::Ready;
            store.save_instance(&ready)?;
            data.instances.insert(fabric.id, ready);
            changes.push(Change::Instance(fabric.id));
            Ok(())
        })
        .unwrap();
    let set = |id, version: &str| {
        state.set_instance_loader(request(SetInstanceLoaderPayload {
            instance_id: id,
            version: version.into(),
        }))
    };

    // The same version changes nothing.
    assert_eq!(
        set(fabric.id, "0.16.14").unwrap().health,
        InstallationHealth::Ready
    );
    assert_eq!(
        set(fabric.id, "../evil").unwrap_err().code,
        ErrorCode::InvalidInput
    );
    {
        let _permit = state
            .activity()
            .try_acquire(OperationKind::Launch, Some(fabric.id), "run-1")
            .unwrap();
        assert_eq!(
            set(fabric.id, "0.17.2").unwrap_err().code,
            ErrorCode::InstanceBusy
        );
    }
    let changed = set(fabric.id, "0.17.2").unwrap();
    let loader = changed.loader.unwrap();
    assert_eq!(loader.kind, crate::loader::LoaderKind::Fabric);
    assert_eq!(loader.version, "0.17.2");
    assert_eq!(changed.health, InstallationHealth::NotInstalled);

    let vanilla = create(&state, "Vanilla", "1.21.1").unwrap();
    assert_eq!(
        set(vanilla.id, "0.17.2").unwrap_err().code,
        ErrorCode::InvalidInput
    );

    drop(state);
    let state = state_at(dir.path(), RecordingSink::default());
    assert_eq!(
        state.instance(fabric.id).unwrap().loader.unwrap().version,
        "0.17.2"
    );
}

#[test]
fn fullscreen_and_game_arguments_are_saved_and_new_installations_take_the_default() {
    let dir = tempfile::tempdir().unwrap();
    let state = state_at(dir.path(), RecordingSink::default());
    let first = create(&state, "Windowed", "1.21.1").unwrap();
    assert!(!first.fullscreen);
    assert_eq!(first.game_arguments, "");
    let update = |fullscreen, arguments: Option<&str>| {
        state.update_instance_settings(request(UpdateInstanceSettingsPayload {
            instance_id: first.id,
            max_heap_mib: 4096,
            resolution: Resolution::default(),
            fullscreen,
            game_arguments: arguments.map(str::to_owned),
        }))
    };
    let saved = update(Some(true), Some("  --quickPlaySingleplayer \"My World\" ")).unwrap();
    assert!(saved.fullscreen);
    assert_eq!(saved.game_arguments, "--quickPlaySingleplayer \"My World\"");
    // Omitted fields stay.
    let kept = update(None, None).unwrap();
    assert!(kept.fullscreen);
    assert_eq!(kept.game_arguments, "--quickPlaySingleplayer \"My World\"");
    assert_eq!(
        update(None, Some("--username Notch")).unwrap_err().code,
        ErrorCode::InvalidInput
    );

    state
        .set_app_settings(request(SettingsPatch {
            default_fullscreen: Some(true),
            ..SettingsPatch::default()
        }))
        .unwrap();
    assert!(state.settings().default_fullscreen);
    assert!(create(&state, "Fullscreen", "1.21.1").unwrap().fullscreen);
}

#[test]
fn new_installations_get_the_memory_their_kind_needs_unless_a_default_is_fixed() {
    let dir = tempfile::tempdir().unwrap();
    let state = state_at(dir.path(), RecordingSink::default());
    let fabric = |name: &str| {
        state
            .create_instance(request(CreateInstancePayload {
                display_name: name.into(),
                version_id: "1.20.1".into(),
                loader: Some(crate::loader::LoaderChoice {
                    kind: crate::loader::LoaderKind::Fabric,
                    version: "0.16.14".into(),
                }),
            }))
            .unwrap()
    };
    assert_eq!(create(&state, "Old", "1.8.9").unwrap().max_heap_mib, 2048);
    let modded = fabric("Modded");
    assert_eq!(modded.max_heap_mib, 4096);

    // The installation's recommendation counts its mods.
    let mods = state.data_root().unwrap().game_dir(modded.id).join("mods");
    std::fs::create_dir_all(&mods).unwrap();
    for i in 0..151 {
        std::fs::write(mods.join(format!("mod{i}.jar")), "").unwrap();
    }
    std::fs::write(mods.join("notes.txt"), "").unwrap();
    let recommended = state.recommend_memory_for(modded.id).unwrap();
    assert_eq!(recommended.mib, 8192);
    assert_eq!(
        recommended.workload,
        validation::Workload::Modded { mods: 151 }
    );
    assert_eq!(state.recommend_memory("1.21.1", false).unwrap().mib, 3072);

    // A fixed default wins; 0 goes back to the recommendation.
    let patch = |mib| {
        state.set_app_settings(request(SettingsPatch {
            default_max_heap_mib: Some(mib),
            ..SettingsPatch::default()
        }))
    };
    patch(5120).unwrap();
    assert_eq!(
        create(&state, "Fixed", "1.21.1").unwrap().max_heap_mib,
        5120
    );
    patch(0).unwrap();
    assert_eq!(state.settings().default_max_heap_mib, None);
    assert_eq!(fabric("Again").max_heap_mib, 4096);
}
