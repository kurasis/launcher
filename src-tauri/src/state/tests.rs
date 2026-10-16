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
        assert_eq!(created.max_heap_mib, 4096);
        state
            .update_instance_settings(request(UpdateInstanceSettingsPayload {
                instance_id: created.id,
                max_heap_mib: 6144,
                resolution: Resolution {
                    width: 1920,
                    height: 1080,
                },
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
    for (name, version) in [("", "1.21.1"), ("ok", "1.21"), ("ok", "26.4-snapshot-4")] {
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
            }))
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidInput);
    }
    let missing = state
        .update_instance_settings(request(UpdateInstanceSettingsPayload {
            instance_id: InstanceId::generate(),
            max_heap_mib: 4096,
            resolution: Resolution::default(),
        }))
        .unwrap_err();
    assert_eq!(missing.code, ErrorCode::InstanceNotFound);
    assert_eq!(sink.events().len(), events_before);
    assert_eq!(state.snapshot().instances[0].max_heap_mib, 4096);
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
fn busy_instance_cannot_be_reconfigured() {
    let dir = tempfile::tempdir().unwrap();
    let state = state_at(dir.path(), RecordingSink::default());
    let created = create(&state, "Busy", "1.21.1").unwrap();
    let _permit = state
        .activity()
        .try_acquire(OperationKind::Install, created.id, "op-1")
        .unwrap();
    assert!(state.snapshot().instances[0].busy);
    let error = state
        .update_instance_settings(request(UpdateInstanceSettingsPayload {
            instance_id: created.id,
            max_heap_mib: 2048,
            resolution: Resolution::default(),
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
                    let lower = key.to_ascii_lowercase();
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
    assert_eq!(json["account"]["state"], "signed_out");
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
