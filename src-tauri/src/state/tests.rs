use std::sync::{Arc, Mutex};

use super::*;
use crate::storage::models::{DuringGameMode, OperationKind, Resolution, StartView, Theme};

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
                launcher_skins: None,
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
                launcher_skins: None,
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
            launcher_skins: None,
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
fn start_view_is_validated_and_persists() {
    let dir = tempfile::tempdir().unwrap();
    let state = state_at(dir.path(), RecordingSink::default());
    assert_eq!(state.snapshot().settings.start_view, StartView::Feed);
    let view = state
        .set_app_settings(request(SettingsPatch {
            start_view: Some("last".into()),
            last_view: Some("account".into()),
            ..SettingsPatch::default()
        }))
        .unwrap();
    assert_eq!(view.start_view, StartView::Last);
    assert_eq!(view.last_view, StartView::Account);
    for junk in [
        "",
        "Home",
        "settings",
        "about",
        "feed\u{0}",
        "../home",
        "x".repeat(10_000).as_str(),
    ] {
        let error = state
            .set_app_settings(request(SettingsPatch {
                start_view: Some(junk.into()),
                ..SettingsPatch::default()
            }))
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidInput, "{junk}");
    }
    for junk in ["last", "diagnostics", "backups", ""] {
        let error = state
            .set_app_settings(request(SettingsPatch {
                last_view: Some(junk.into()),
                ..SettingsPatch::default()
            }))
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidInput, "{junk}");
    }
    drop(state);

    let state = state_at(dir.path(), RecordingSink::default());
    let settings = state.snapshot().settings;
    assert_eq!(settings.start_view, StartView::Last);
    assert_eq!(settings.last_view, StartView::Account);
    drop(state);

    // A damaged value on disk reads as the feed; the other settings stay.
    let path = dir.path().join("settings.json");
    let mut stored: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    stored["startView"] = serde_json::json!("<script>");
    stored["lastView"] = serde_json::json!(7);
    std::fs::write(&path, serde_json::to_vec(&stored).unwrap()).unwrap();
    let state = state_at(dir.path(), RecordingSink::default());
    let snapshot = state.snapshot();
    assert_eq!(snapshot.settings.start_view, StartView::Feed);
    assert_eq!(snapshot.settings.last_view, StartView::Feed);
    assert!(snapshot.settings.writable);
    assert!(snapshot.storage_issues.is_empty());
}

#[test]
fn the_relay_only_mode_is_off_until_turned_on_and_persists() {
    let dir = tempfile::tempdir().unwrap();
    let state = state_at(dir.path(), RecordingSink::default());
    assert!(!state.snapshot().settings.worlds_relay_only);
    let view = state
        .set_app_settings(request(SettingsPatch {
            worlds_relay_only: Some(true),
            ..SettingsPatch::default()
        }))
        .unwrap();
    assert!(view.worlds_relay_only);
    drop(state);
    let state = state_at(dir.path(), RecordingSink::default());
    assert!(state.settings().worlds_relay_only);
    // Junk is refused whole.
    let junk: Result<SettingsPatch, _> =
        serde_json::from_value(serde_json::json!({ "worldsRelayOnly": "yes" }));
    assert!(junk.is_err());
}

#[test]
fn feed_settings_are_validated_and_persist() {
    use crate::storage::models::{FeedBlock, PlayerMode, VideoLanguage};
    let dir = tempfile::tempdir().unwrap();
    let state = state_at(dir.path(), RecordingSink::default());
    assert!(state.snapshot().settings.feed.enabled);
    let view = state
        .set_app_settings(request(SettingsPatch {
            feed: Some(FeedSettingsPatch {
                snapshots: Some(false),
                video_language: Some(VideoLanguage::Ru),
                shorts: Some(true),
                player: Some(PlayerMode::Browser),
                hidden: Some(vec![FeedBlock::Streams, FeedBlock::News]),
                live_notifications: Some(true),
                ..FeedSettingsPatch::default()
            }),
            ..SettingsPatch::default()
        }))
        .unwrap();
    assert!(!view.feed.snapshots && view.feed.shorts && view.feed.machine_translation);
    assert!(view.feed.live_notifications);
    assert_eq!(view.feed.video_language, VideoLanguage::Ru);
    assert_eq!(view.feed.player, PlayerMode::Browser);
    assert_eq!(view.feed.hidden, [FeedBlock::News, FeedBlock::Streams]);
    // A block twice, or more than there are, is refused and changes nothing.
    for hidden in [
        vec![FeedBlock::News, FeedBlock::News],
        vec![FeedBlock::Videos; 50],
    ] {
        let error = state
            .set_app_settings(request(SettingsPatch {
                feed: Some(FeedSettingsPatch {
                    enabled: Some(false),
                    hidden: Some(hidden),
                    ..FeedSettingsPatch::default()
                }),
                ..SettingsPatch::default()
            }))
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidInput);
    }
    assert!(state.snapshot().settings.feed.enabled);
    // Unknown fields and values are refused when the patch is read.
    for junk in [
        r#"{"feed":{"enabled":true,"evil":1}}"#,
        r#"{"feed":{"player":"popup"}}"#,
        r#"{"feed":{"hidden":["videos","<img>"]}}"#,
        r#"{"feed":{"videoLanguage":"de"}}"#,
        r#"{"feed":{"liveNotifications":"yes"}}"#,
    ] {
        assert!(
            serde_json::from_str::<SettingsPatch>(junk).is_err(),
            "{junk}"
        );
    }
    state
        .set_app_settings(request(SettingsPatch {
            feed: Some(FeedSettingsPatch {
                enabled: Some(false),
                hidden: Some(Vec::new()),
                ..FeedSettingsPatch::default()
            }),
            ..SettingsPatch::default()
        }))
        .unwrap();
    drop(state);
    let state = state_at(dir.path(), RecordingSink::default());
    let feed = state.snapshot().settings.feed;
    assert!(!feed.enabled && !feed.snapshots && feed.shorts && feed.live_notifications);
    assert!(feed.hidden.is_empty());
    assert_eq!(feed.video_language, VideoLanguage::Ru);
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
fn installations_are_renamed_also_while_busy() {
    let dir = tempfile::tempdir().unwrap();
    let id = {
        let state = state_at(dir.path(), RecordingSink::default());
        let created = create(&state, "Old name", "1.21.1").unwrap();
        let _permit = state
            .activity()
            .try_acquire(OperationKind::Launch, Some(created.id), "run-1")
            .unwrap();
        let renamed = state
            .rename_instance(request(RenameInstancePayload {
                instance_id: created.id,
                display_name: "  Survival with friends ".into(),
            }))
            .unwrap();
        assert_eq!(renamed.display_name, "Survival with friends");
        assert!(renamed.busy);
        for bad in ["   ", "a\u{0007}b", &"x".repeat(200)] {
            let error = state
                .rename_instance(request(RenameInstancePayload {
                    instance_id: created.id,
                    display_name: bad.into(),
                }))
                .unwrap_err();
            assert_eq!(error.code, ErrorCode::InvalidInput, "{bad:?}");
        }
        created.id
    };
    let state = state_at(dir.path(), RecordingSink::default());
    assert_eq!(
        state.instance(id).unwrap().display_name,
        "Survival with friends"
    );
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
            launcher_skins: None,
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
            launcher_skins: None,
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
fn launcher_skins_is_on_for_new_installations_and_saved_when_changed() {
    let dir = tempfile::tempdir().unwrap();
    let state = state_at(dir.path(), RecordingSink::default());
    let created = create(&state, "Skins", "1.21.1").unwrap();
    assert!(created.launcher_skins);
    let update = |skins| {
        state.update_instance_settings(request(UpdateInstanceSettingsPayload {
            instance_id: created.id,
            max_heap_mib: 4096,
            resolution: Resolution::default(),
            fullscreen: None,
            game_arguments: None,
            launcher_skins: skins,
        }))
    };
    assert!(!update(Some(false)).unwrap().launcher_skins);
    // Omitted, it stays as it was.
    assert!(!update(None).unwrap().launcher_skins);
    let stored =
        std::fs::read_to_string(state.data_root().unwrap().instance_file(created.id)).unwrap();
    assert!(stored.contains("\"launcherSkins\": false"), "{stored}");
    assert!(update(Some(true)).unwrap().launcher_skins);
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

#[test]
fn offline_in_the_full_game_needs_an_owner_while_the_check_is_on() {
    assert!(!super::offline_allowed(true, false));
    assert!(super::offline_allowed(true, true));
    assert!(super::offline_allowed(false, false));
}

#[test]
fn a_room_gets_one_installation_of_its_own() {
    let dir = tempfile::tempdir().unwrap();
    let state = state_at(dir.path(), RecordingSink::default());
    let mine = create(&state, "Mine", "1.21.1").unwrap();
    let room = "6f1d3b8e-2c4a-4f7e-9b1a-0c2d3e4f5a6b";
    let first = state
        .room_instance(room, "Our world", "1.21.1", None, 0)
        .unwrap();
    assert_ne!(first.id, mine.id, "the player's own is not used");
    assert_eq!(first.world_room.as_deref(), Some(room));
    assert_eq!(first.display_name, "Our world");
    let again = state
        .room_instance(room, "Renamed", "1.21.1", None, 0)
        .unwrap();
    assert_eq!(again.id, first.id, "joining twice reuses it");
    assert!(state.room_instance(room, "x", "1.20.4", None, 0).is_err());
    let other = state
        .room_instance(
            "7f1d3b8e-2c4a-4f7e-9b1a-0c2d3e4f5a6b",
            "\n",
            "1.21.1",
            None,
            0,
        )
        .unwrap();
    assert_ne!(other.id, first.id);
    assert_eq!(other.display_name, "World with friends");
    // A world with mods: the installation runs its loader, and only it.
    let fabric = crate::loader::LoaderChoice {
        kind: crate::loader::LoaderKind::Fabric,
        version: "0.17.2".into(),
    };
    let modded_room = "8f1d3b8e-2c4a-4f7e-9b1a-0c2d3e4f5a6b";
    let modded = state
        .room_instance(modded_room, "Mods", "1.21.1", Some(fabric.clone()), 12)
        .unwrap();
    assert_eq!(modded.loader.as_ref(), Some(&fabric));
    assert!(
        state
            .room_instance(modded_room, "Mods", "1.21.1", None, 0)
            .is_err()
    );
    assert!(
        state
            .room_instance(room, "x", "1.21.1", Some(fabric), 0)
            .is_err()
    );
    for junk in ["", "../x", "6F1D3B8E-2C4A-4F7E-9B1A-0C2D3E4F5A6B"] {
        assert!(state.room_instance(junk, "x", "1.21.1", None, 0).is_err());
    }
    // It survives a restart as a room installation.
    drop(state);
    let state = state_at(dir.path(), RecordingSink::default());
    assert_eq!(
        state.instance(first.id).unwrap().world_room.as_deref(),
        Some(room)
    );
}

fn game_run(run_id: &str, started_at: chrono::DateTime<Utc>) -> RunSummary {
    RunSummary {
        run_id: run_id.into(),
        kind: RunKind::Game,
        instance_id: Some(InstanceId::generate()),
        state: RunState::Preflight,
        started_at,
        ended_at: None,
        exit_code: None,
        duration_ms: None,
        drain_timed_out: false,
        output_tail: vec!["[Render thread/INFO]: Connecting to play.example, 25565".into()],
        output_lines_dropped: 0,
        error: None,
        logging_degraded: false,
        signal: None,
        diagnosis: None,
        server: Some("play.example:25565".into()),
    }
}

#[test]
fn a_game_run_counts_from_when_the_game_first_ran() {
    use std::time::{Duration, Instant};
    const MINUTE: Duration = Duration::from_secs(60);
    // Far enough from the steady clock's start that a survivor's age fits before it.
    let launch = Instant::now() + 60 * MINUTE;
    let wall = Utc::now();
    let mut run = game_run("1b2c3d4e-0000-4000-8000-000000000001", wall);
    let mark = GameMark::begin(&run, launch, wall).unwrap();
    // Starting: no time yet, and no time key in what the window gets.
    assert_eq!(
        serde_json::to_value(mark.at(launch + MINUTE)).unwrap(),
        serde_json::json!({ "state": "preflight" })
    );
    // The time counts from the spawn, not from the checks before it.
    let spawn = launch + 2 * MINUTE;
    run.state = RunState::Running;
    let mark = GameMark::update(Some(mark), &run, spawn).unwrap();
    let at = mark.at(spawn + 42 * MINUTE);
    assert_eq!(
        at,
        GameRun {
            state: RunState::Running,
            running_ms: Some(2_520_000)
        }
    );
    // Only the state and the time: no server, ids or output.
    assert_eq!(
        serde_json::to_value(at).unwrap(),
        serde_json::json!({ "state": "running", "runningMs": 2_520_000 })
    );
    // Later changes keep the start; stopping still counts.
    let mark = GameMark::update(Some(mark), &run, spawn + 5 * MINUTE).unwrap();
    run.state = RunState::Stopping;
    let mark = GameMark::update(Some(mark), &run, spawn + 6 * MINUTE).unwrap();
    assert_eq!(
        mark.at(spawn + 10 * MINUTE),
        GameRun {
            state: RunState::Stopping,
            running_ms: Some(600_000)
        }
    );
    // A steady clock: asked before its start, no time has passed.
    assert_eq!(mark.at(spawn - MINUTE).running_ms, Some(0));
    for state in [
        RunState::Exited,
        RunState::Failed,
        RunState::Cancelled,
        RunState::Stopped,
    ] {
        run.state = state;
        assert_eq!(GameMark::update(Some(mark), &run, spawn), None, "{state:?}");
        assert_eq!(GameMark::begin(&run, spawn, wall), None, "{state:?}");
    }
    // The lifecycle check's child is not a game.
    run.state = RunState::Running;
    run.kind = RunKind::LifecycleTest;
    assert_eq!(GameMark::begin(&run, spawn, wall), None);
    assert_eq!(GameMark::update(Some(mark), &run, spawn), None);
}

#[test]
fn a_game_found_after_a_restart_counts_from_its_start_on_record() {
    use std::time::{Duration, Instant};
    let now = Instant::now() + Duration::from_secs(3600);
    let wall = Utc::now();
    let mut run = game_run(
        "1b2c3d4e-0000-4000-8000-000000000004",
        wall - chrono::Duration::minutes(10),
    );
    run.state = RunState::Running;
    let mark = GameMark::begin(&run, now, wall).unwrap();
    assert_eq!(mark.at(now).running_ms, Some(600_000));
    // A start on record after the clock's now says nothing true: no time is shown,
    // and none appears later.
    run.started_at = wall + chrono::Duration::minutes(10);
    let mark = GameMark::begin(&run, now, wall).unwrap();
    assert_eq!(mark.at(now).running_ms, None);
    let mark = GameMark::update(Some(mark), &run, now).unwrap();
    assert_eq!(mark.at(now + Duration::from_secs(60)).running_ms, None);
}

#[test]
fn the_console_sees_the_current_game_until_it_ends() {
    let dir = tempfile::tempdir().unwrap();
    let state = state_at(dir.path(), RecordingSink::default());
    let page = state.read_console(0);
    assert_eq!(page.run, None);
    let json = serde_json::to_value(&page).unwrap();
    assert!(json.get("run").is_none(), "{json}");
    assert!(json.get("entries").is_some() && json.get("lastSeq").is_some());

    let id = "1b2c3d4e-0000-4000-8000-000000000002";
    state.begin_run(game_run(id, Utc::now() - chrono::Duration::minutes(5)));
    let page = state.read_console(0);
    assert_eq!(
        page.run,
        Some(GameRun {
            state: RunState::Preflight,
            running_ms: None
        })
    );
    // The journal still comes with it, and nothing of the run but its state.
    assert!(
        page.page
            .entries
            .iter()
            .any(|e| e.text.starts_with("Run 1b2c3d4e"))
    );
    let json = serde_json::to_value(&page).unwrap();
    assert_eq!(json["run"], serde_json::json!({ "state": "preflight" }));
    assert!(!json.to_string().contains("play.example"), "{json}");

    // Another run's update changes nothing.
    let other = "1b2c3d4e-0000-4000-8000-00000000ffff";
    state.update_run(other, |run| run.state = RunState::Running);
    assert_eq!(
        state.read_console(0).run.map(|r| r.state),
        Some(RunState::Preflight)
    );
    // The time counts from the spawn: the five minutes of checks are not in it.
    state.update_run(id, |run| run.state = RunState::Running);
    let run = state.read_console(0).run.unwrap();
    assert_eq!(run.state, RunState::Running);
    assert!(run.running_ms.unwrap() < 60_000, "{run:?}");
    // A later read starts where the last one stopped and still carries the run.
    let after = state.read_console(0).page.last_seq;
    let page = state.read_console(after);
    assert!(page.page.entries.is_empty());
    assert_eq!(page.run.map(|r| r.state), Some(RunState::Running));

    // Another run's end leaves this game shown.
    state.finish_run(other, |run| run.state = RunState::Exited);
    assert_eq!(
        state.read_console(0).run.map(|r| r.state),
        Some(RunState::Running)
    );
    state.finish_run(id, |run| {
        run.state = RunState::Exited;
        run.ended_at = Some(Utc::now());
    });
    assert_eq!(state.read_console(0).run, None);
    assert_eq!(state.current_run(), None);
}

#[test]
fn the_console_never_waits_on_a_store_write() {
    let dir = tempfile::tempdir().unwrap();
    let state = state_at(dir.path(), RecordingSink::default());
    state.begin_run(game_run("1b2c3d4e-0000-4000-8000-000000000003", Utc::now()));
    std::thread::scope(|scope| {
        // What a slow store write looks like from outside: the state held.
        let held = state.lock();
        let (sent, read) = std::sync::mpsc::channel();
        let state = &state;
        scope.spawn(move || sent.send(state.read_console(0)).unwrap());
        let page = read.recv_timeout(std::time::Duration::from_secs(10));
        drop(held);
        assert_eq!(
            page.expect("the console read waited").run.map(|r| r.state),
            Some(RunState::Preflight)
        );
    });
}
