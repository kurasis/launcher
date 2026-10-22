use std::sync::Arc;

use super::*;
use crate::state::dto::{MutationRequest, SettingsPatch};
use crate::validation::MemoryPolicy;

/// Records what would be sent and answers with fixed statuses.
#[derive(Clone, Default)]
struct Fake {
    sent: Arc<Mutex<Vec<(String, Value)>>>,
    status: Arc<Mutex<u16>>,
}

impl Poster for Fake {
    fn post<'a>(
        &'a self,
        url: &'a str,
        gzip_json: Vec<u8>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<u16, String>> + Send + 'a>> {
        Box::pin(async move {
            let mut json = Vec::new();
            std::io::Read::read_to_end(
                &mut flate2::read::GzDecoder::new(&gzip_json[..]),
                &mut json,
            )
            .unwrap();
            self.sent
                .lock()
                .unwrap()
                .push((url.to_owned(), serde_json::from_slice(&json).unwrap()));
            Ok(*self.status.lock().unwrap())
        })
    }
}

fn setup(status: u16) -> (tempfile::TempDir, AppState, Reports, Fake) {
    let dir = tempfile::tempdir().unwrap();
    let root = DataRoot::new(dir.path().join("data")).unwrap();
    let state = AppState::initialize(
        Ok(root),
        "0.1.0",
        MemoryPolicy::from_physical_bytes(16 * 1024 * 1024 * 1024),
        Box::new(crate::state::NoopSink),
    );
    let fake = Fake::default();
    *fake.status.lock().unwrap() = status;
    let reports = Reports::new(Box::new(fake.clone()));
    (dir, state, reports, fake)
}

fn crashed_run(state: &AppState) -> Subject {
    let root = state.data_root().unwrap();
    let run = uuid::Uuid::new_v4();
    let logs = root.run_logs(run);
    fs::create_dir_all(&logs).unwrap();
    fs::write(
        logs.join("output.log"),
        "[Render thread/INFO]: Setting user: Steve_Secret\n\
         [Render thread/INFO]: [CHAT] <Steve_Secret> meet me at 203.0.113.5\n\
         java.lang.OutOfMemoryError: Java heap space\n",
    )
    .unwrap();
    let diagnosis = crate::launch::crash::CrashDiagnosis {
        kind: crate::launch::crash::CrashKind::OutOfMemory,
        details: [("memory".to_owned(), "2048".to_owned())].into(),
        evidence: vec!["java.lang.OutOfMemoryError: Java heap space".into()],
        suspects: Vec::new(),
        report: None,
        actions: Vec::new(),
        notes: Vec::new(),
        signature: String::new(),
    };
    fs::write(
        logs.join("diagnosis.json"),
        serde_json::to_vec(&json!({ "analyzerVersion": 2, "diagnosis": diagnosis })).unwrap(),
    )
    .unwrap();
    Subject::Game {
        run_id: run.to_string(),
        instance_id: uuid::Uuid::new_v4().to_string(),
    }
}

#[tokio::test]
async fn a_game_report_is_scrubbed_previewed_and_sent_with_the_chosen_files() {
    let (_dir, state, reports, fake) = setup(201);
    let preview = reports.preview(&state, &crashed_run(&state), None).unwrap();
    assert_eq!(
        (preview.source.as_str(), preview.kind.as_str()),
        ("game", "out_of_memory")
    );
    let names: Vec<&str> = preview.files.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(names, ["output.log", "diagnosis.json"]);
    let output = &preview.files[0].text;
    assert!(
        !output.contains("Steve_Secret") && !output.contains("203.0.113.5"),
        "{output}"
    );
    assert!(output.contains("OutOfMemoryError"));
    assert_eq!(preview.removed.chat_lines, 1);

    let delivery = reports
        .send(
            &state,
            &preview.id,
            &["output.log".to_owned()],
            "It crashed at night",
            None,
        )
        .await
        .unwrap();
    assert_eq!(delivery, Delivery::Sent);
    let (url, body) = fake.sent.lock().unwrap()[0].clone();
    assert_eq!(url, REPORTS_URL);
    assert_eq!(body["kind"], "out_of_memory");
    assert_eq!(
        body["files"].as_array().unwrap().len(),
        1,
        "only the chosen file"
    );
    assert_eq!(body["note"], "It crashed at night");
    assert_eq!(
        body["installId"],
        state.settings().install_id.unwrap().as_str()
    );
    assert!(
        fs::read_dir(outbox(&state.data_root().unwrap()))
            .unwrap()
            .next()
            .is_none()
    );
    // A preview is used once.
    assert!(
        reports
            .send(&state, &preview.id, &[], "", None)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn an_unreachable_service_keeps_the_report_for_later() {
    let (_dir, state, reports, fake) = setup(503);
    let preview = reports.preview(&state, &crashed_run(&state), None).unwrap();
    let names: Vec<String> = preview.files.iter().map(|f| f.name.clone()).collect();
    assert_eq!(
        reports
            .send(&state, &preview.id, &names, "", None)
            .await
            .unwrap(),
        Delivery::Queued
    );
    let root = state.data_root().unwrap();
    let meta: Pending = fs::read_dir(outbox(&root))
        .unwrap()
        .flatten()
        .find(|e| e.file_name().to_string_lossy().ends_with(".meta.json"))
        .map(|e| serde_json::from_slice(&fs::read(e.path()).unwrap()).unwrap())
        .unwrap();
    assert_eq!(meta.attempts, 1);
    assert!(meta.next > Utc::now(), "not retried at once");
    // Later the service is back and the retry is due.
    *fake.status.lock().unwrap() = 201;
    for entry in fs::read_dir(outbox(&root)).unwrap().flatten() {
        if entry.file_name().to_string_lossy().ends_with(".meta.json") {
            let due = Pending {
                next: Utc::now(),
                ..meta.clone()
            };
            fs::write(entry.path(), serde_json::to_vec(&due).unwrap()).unwrap();
        }
    }
    reports.flush(&state).await;
    assert_eq!(fake.sent.lock().unwrap().len(), 2);
    assert!(fs::read_dir(outbox(&root)).unwrap().next().is_none());
}

#[tokio::test]
async fn a_launcher_failure_report_marks_the_record_sent() {
    let (_dir, state, reports, _fake) = setup(201);
    let logs = state.data_root().unwrap().path().join("logs");
    let mut failure = crate::journal::Failure::new(
        crate::journal::FailureKind::Panic,
        "0.1.0",
        "called `Option::unwrap()` on a `None` value",
    );
    failure.place = Some("src/state/mod.rs:42".into());
    failure.log_tail = vec!["INFO Launcher 0.1.0 started".into()];
    crate::journal::record(&logs, &failure).unwrap();
    let preview = reports
        .preview(
            &state,
            &Subject::Launcher {
                failure_id: failure.id.clone(),
            },
            None,
        )
        .unwrap();
    assert_eq!(preview.kind, "panic");
    assert_eq!(preview.signature, "panic:src/state/mod.rs:42");
    let names: Vec<String> = preview.files.iter().map(|f| f.name.clone()).collect();
    assert_eq!(names, ["failure.json", "launcher.log"]);
    reports
        .send(&state, &preview.id, &names, "", None)
        .await
        .unwrap();
    assert!(crate::journal::failures(&logs)[0].reported);
}

#[tokio::test]
async fn statistics_go_unless_turned_off() {
    let (_dir, state, reports, fake) = setup(204);
    let event = Event {
        source: "game".into(),
        kind: "out_of_memory".into(),
        signature: "out_of_memory:".into(),
        launcher_version: "0.1.0".into(),
        os: "linux".into(),
        game_version: "1.21.1".into(),
        loader: "fabric".into(),
        loader_version: "0.18.2".into(),
        analyzer_version: 2,
        exit_code: Some(-1),
    };
    // On by default (D-071).
    count(&state, event.clone());
    reports.flush(&state).await;
    {
        let sent = fake.sent.lock().unwrap();
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].0, STATS_URL);
        assert_eq!(sent[0].1["events"][0]["kind"], "out_of_memory");
        assert!(
            sent[0].1.get("installId").is_none(),
            "no install id in statistics"
        );
    }

    // Counted while the service did not answer, then turned off: the
    // waiting events go, and nothing more is counted or sent.
    *fake.status.lock().unwrap() = 503;
    count(&state, event.clone());
    reports.flush(&state).await;
    let queue = stats_queue(&state.data_root().unwrap());
    assert!(queue.exists());
    state
        .set_app_settings(MutationRequest {
            request_id: uuid::Uuid::new_v4().to_string(),
            payload: SettingsPatch {
                stats: Some(false),
                ..SettingsPatch::default()
            },
        })
        .unwrap();
    forget_stats(&state);
    assert!(!queue.exists());
    *fake.status.lock().unwrap() = 204;
    count(&state, event);
    reports.flush(&state).await;
    assert_eq!(
        fake.sent.lock().unwrap().len(),
        2,
        "nothing after turning off"
    );
}
