use std::sync::{Arc, Mutex};

use super::*;
use crate::validation::MemoryPolicy;

const KEY: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

/// Method, path, JSON body and headers of one request.
type Call = (String, String, Value, Vec<(&'static str, String)>);

/// A service that answers with fixed statuses and records each request.
#[derive(Clone, Default)]
struct Fake {
    calls: Arc<Mutex<Vec<Call>>>,
    review_status: Arc<Mutex<u16>>,
    statuses: Arc<Mutex<Value>>,
}

impl Transport for Fake {
    fn call<'a>(
        &'a self,
        method: &'a str,
        path: &'a str,
        body: Option<Vec<u8>>,
        headers: Vec<(&'static str, String)>,
    ) -> Boxed<'a, Result<(u16, Vec<u8>), String>> {
        Box::pin(async move {
            let body: Value = body
                .map(|b| serde_json::from_slice(&b).unwrap())
                .unwrap_or(Value::Null);
            self.calls
                .lock()
                .unwrap()
                .push((method.to_owned(), path.to_owned(), body, headers));
            let answer = match (method, path) {
                ("GET", "/v1/challenge") => (200, json!({ "challenge": "8.1.ab.cd", "bits": 8 })),
                ("POST", "/v1/reviews") => {
                    let status = *self.review_status.lock().unwrap();
                    (status, json!({ "id": 7, "key": KEY }))
                }
                ("POST", "/v1/status") => (200, self.statuses.lock().unwrap().clone()),
                ("GET", "/v1/export") => (200, json!({ "reviews": [{ "id": 7 }] })),
                ("DELETE", _) => (204, Value::Null),
                _ => (404, Value::Null),
            };
            Ok((answer.0, serde_json::to_vec(&answer.1).unwrap()))
        })
    }
}

struct NoAccount;

impl Accounts for NoAccount {
    fn token<'a>(&'a self, _: &'a AppState) -> Boxed<'a, Token> {
        Box::pin(async { Token::Token("account-token".into()) })
    }
}

fn setup(status: u16) -> (tempfile::TempDir, AppState, Reviews, Fake) {
    let dir = tempfile::tempdir().unwrap();
    let root = DataRoot::new(dir.path().join("data")).unwrap();
    let state = AppState::initialize(
        Ok(root),
        "0.1.0",
        MemoryPolicy::from_physical_bytes(16 * 1024 * 1024 * 1024),
        Box::new(crate::state::NoopSink),
    );
    let fake = Fake::default();
    *fake.review_status.lock().unwrap() = status;
    let reviews = Reviews::new(Box::new(fake.clone()));
    (dir, state, reviews, fake)
}

fn draft(files: Vec<DraftFile>) -> Draft {
    Draft {
        kind: "bug".into(),
        category: "launch".into(),
        subcategory: Some("crashes".into()),
        subject: None,
        message: "The game closes when I open a world with Sodium.".into(),
        rating: Some(5),
        link_account: false,
        diagnostic_id: None,
        ad: None,
        files,
    }
}

fn text(name: &str, text: &str) -> DraftFile {
    DraftFile {
        name: name.into(),
        kind: "text".into(),
        text: Some(text.into()),
        data: None,
    }
}

#[test]
fn the_proof_of_work_is_what_the_service_checks() {
    let nonce = solve("12.1.ab.cd", 12);
    let hash = Sha256::digest(format!("12.1.ab.cd:{nonce}").as_bytes());
    assert!(zeros(&hash) >= 12);
    assert_eq!(zeros(&[0, 0b0001_0000]), 11);
}

#[test]
fn only_text_files_by_name() {
    assert_eq!(
        text_name("C:\\Users\\a\\latest.log").as_deref(),
        Some("latest.log")
    );
    assert_eq!(
        text_name("sodium-options.JSON").as_deref(),
        Some("sodium-options.JSON")
    );
    assert_eq!(text_name("mod.jar"), None);
    assert_eq!(text_name(".env.txt"), None);
    assert_eq!(text_name("README"), None);
}

#[tokio::test]
async fn a_review_is_sent_scrubbed_and_remembered() {
    let (_dir, state, reviews, fake) = setup(201);
    let png = base64::engine::general_purpose::STANDARD.encode(b"\x89PNG\r\n\x1a\nrest");
    let delivery = reviews
        .send(
            &state,
            &NoAccount,
            draft(vec![
                text(
                    "/home/jurgen/latest.log",
                    "[Render thread/INFO]: Setting user: Steve_Secret\nConnecting to 203.0.113.5, 25565\n",
                ),
                DraftFile {
                    name: "shot.png".into(),
                    kind: "image".into(),
                    text: None,
                    data: Some(png),
                },
            ]),
            Some(Path::new("/home/jurgen")),
        )
        .await
        .unwrap();
    assert_eq!(delivery, Delivery::Sent);
    let calls = fake.calls.lock().unwrap().clone();
    let (_, _, body, headers) = &calls[1];
    assert!(headers.is_empty(), "not linked: no token");
    assert_eq!(body["challenge"], "8.1.ab.cd");
    assert_eq!(body["rating"], Value::Null, "only impressions are rated");
    assert_eq!(body["installId"], json!(state.install_id().unwrap()));
    let log = body["files"][0]["text"].as_str().unwrap();
    assert_eq!(body["files"][0]["name"], "latest.log");
    assert!(
        !log.contains("Steve_Secret") && !log.contains("203.0.113.5"),
        "{log}"
    );

    assert!(
        body.get("subject").is_none_or(Value::is_null),
        "no subject given"
    );

    let mine = reviews.mine(&state, &NoAccount, false).await.unwrap();
    assert_eq!(mine.reviews.len(), 1);
    assert_eq!(mine.reviews[0].id, 7);
    assert_eq!(mine.reviews[0].subject, None);
    assert_eq!(read_sent(&state.data_root().unwrap()).reviews[0].key, KEY);
    assert_eq!(mine.waiting, 0);
    // The key never reaches the interface.
    assert!(!serde_json::to_string(&mine).unwrap().contains(KEY));
}

#[tokio::test]
async fn answers_are_fetched_and_seen() {
    let (_dir, state, reviews, fake) = setup(201);
    reviews
        .send(&state, &NoAccount, draft(Vec::new()), None)
        .await
        .unwrap();
    *fake.statuses.lock().unwrap() = json!({ "reviews": [
        { "id": 7, "received": 1, "status": "answered", "reply": "Fixed in 0.9.2\u{1b}[2J", "replied": 1_800_000_000 }
    ]});
    let mine = reviews.mine(&state, &NoAccount, true).await.unwrap();
    assert_eq!(mine.unseen, 1);
    assert_eq!(mine.reviews[0].status, "answered");
    assert_eq!(mine.reviews[0].reply.as_deref(), Some("Fixed in 0.9.2[2J"));
    let asked = fake.calls.lock().unwrap().last().unwrap().clone();
    assert_eq!(
        asked.2["reviews"][0]["key"], KEY,
        "the key goes in the body"
    );

    // Six hours have not passed: nothing is asked without a refresh.
    let before = fake.calls.lock().unwrap().len();
    reviews.mine(&state, &NoAccount, false).await.unwrap();
    assert_eq!(fake.calls.lock().unwrap().len(), before);

    reviews.seen(&state).unwrap();
    assert_eq!(
        reviews
            .mine(&state, &NoAccount, false)
            .await
            .unwrap()
            .unseen,
        0
    );

    reviews.delete(&state, 7).await.unwrap();
    let last = fake.calls.lock().unwrap().last().unwrap().clone();
    assert_eq!(
        (last.0.as_str(), last.1.as_str()),
        ("DELETE", "/v1/reviews/7")
    );
    assert_eq!(last.3, vec![("x-review-key", KEY.to_owned())]);
    assert!(
        reviews
            .mine(&state, &NoAccount, false)
            .await
            .unwrap()
            .reviews
            .is_empty()
    );
}

#[tokio::test]
async fn a_review_waits_while_the_service_cannot_take_it() {
    let (_dir, state, reviews, fake) = setup(503);
    let mut linked = draft(Vec::new());
    linked.link_account = true;
    let delivery = reviews
        .send(&state, &NoAccount, linked, None)
        .await
        .unwrap();
    assert_eq!(delivery, Delivery::Queued);
    let calls = fake.calls.lock().unwrap().clone();
    assert_eq!(
        calls[1].3,
        vec![("authorization", "Bearer account-token".to_owned())]
    );
    assert_eq!(
        reviews
            .mine(&state, &NoAccount, false)
            .await
            .unwrap()
            .waiting,
        1
    );

    // Due later: a flush now sends nothing; once due, it goes.
    reviews.flush(&state, &NoAccount).await;
    assert_eq!(fake.calls.lock().unwrap().len(), 2);
    let root = state.data_root().unwrap();
    let path = fs::read_dir(outbox(&root))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let mut pending: Pending = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    pending.next = Utc::now();
    fs::write(&path, serde_json::to_vec(&pending).unwrap()).unwrap();
    *fake.review_status.lock().unwrap() = 201;
    reviews.flush(&state, &NoAccount).await;
    let mine = reviews.mine(&state, &NoAccount, false).await.unwrap();
    assert_eq!((mine.waiting, mine.reviews.len()), (0, 1));
    assert!(mine.reviews[0].account);
}

#[tokio::test]
async fn junk_drafts_are_refused() {
    let (_dir, state, reviews, _fake) = setup(201);
    let mut short = draft(Vec::new());
    short.message = "too short".into();
    let mut kind = draft(Vec::new());
    kind.kind = "Bug; drop".into();
    let bad_picture = DraftFile {
        name: "a.png".into(),
        kind: "image".into(),
        text: None,
        data: Some(base64::engine::general_purpose::STANDARD.encode(b"MZ\x90\x00")),
    };
    for bad in [
        short,
        kind,
        draft(vec![text("run.exe", "MZ")]),
        draft(vec![bad_picture]),
        draft((0..13).map(|n| text(&format!("{n}.log"), "x")).collect()),
    ] {
        assert!(reviews.send(&state, &NoAccount, bad, None).await.is_err());
    }
    assert!(reviews.text_file(&state, "a.log", "a\0b", None).is_err());
    let found = reviews
        .text_file(&state, "options.txt", "lastServer:play.example.net\n", None)
        .unwrap();
    assert_eq!(found.files[0].name, "options.txt");
}

#[test]
fn logs_hold_the_latest_files_scrubbed() {
    let (_dir, state, reviews, _fake) = setup(201);
    let root = state.data_root().unwrap();
    fs::create_dir_all(root.logs_dir()).unwrap();
    fs::write(
        root.logs_dir().join(crate::journal::LOG_FILE),
        "INFO Setting user: Steve_Secret\n",
    )
    .unwrap();
    let run = uuid::Uuid::new_v4();
    let logs = root.run_logs(run);
    fs::create_dir_all(&logs).unwrap();
    fs::write(logs.join("output.log"), "line\n".repeat(3000)).unwrap();
    let found = reviews
        .logs(&state, None, Some(&run.to_string()), None)
        .unwrap();
    let names: Vec<&str> = found.files.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(names, ["launcher.log", "output.log", "system.txt"]);
    assert!(
        !found.files[0]
            .text
            .as_deref()
            .unwrap()
            .contains("Steve_Secret")
    );
    assert_eq!(
        found.files[1].text.as_deref().unwrap().lines().count(),
        LOG_LINES
    );
    assert!(
        found.files[2]
            .text
            .as_deref()
            .unwrap()
            .starts_with("Launcher 0.1.0")
    );
    assert!(reviews.logs(&state, Some("../x"), None, None).is_err());
}

struct SignedOut;

impl Accounts for SignedOut {
    fn token<'a>(&'a self, _: &'a AppState) -> Boxed<'a, Token> {
        Box::pin(async { Token::SignedOut })
    }
}

#[tokio::test]
async fn the_accounts_reviews_are_exported_with_its_token() {
    let (_dir, state, reviews, fake) = setup(201);
    let data = reviews.export(&state, &NoAccount).await.unwrap();
    assert_eq!(data["reviews"][0]["id"], 7);
    let calls = fake.calls.lock().unwrap().clone();
    assert_eq!(calls.len(), 1);
    assert_eq!(
        (calls[0].0.as_str(), calls[0].1.as_str()),
        ("GET", "/v1/export")
    );
    assert!(
        calls[0]
            .3
            .contains(&("authorization", "Bearer account-token".to_owned()))
    );
    // Signed out: nothing is asked.
    assert_eq!(reviews.export(&state, &SignedOut).await, None);
    assert_eq!(fake.calls.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn the_subject_is_one_short_line() {
    let (_dir, state, reviews, fake) = setup(201);
    let mut titled = draft(Vec::new());
    titled.subject = Some("  Crash\nwith\u{202E}\t shaders \u{1b}[2J ".into());
    reviews
        .send(&state, &NoAccount, titled, None)
        .await
        .unwrap();
    let body = fake.calls.lock().unwrap()[1].2.clone();
    assert_eq!(body["subject"], "Crash with shaders [2J");
    let mine = reviews.mine(&state, &NoAccount, false).await.unwrap();
    assert_eq!(
        mine.reviews[0].subject.as_deref(),
        Some("Crash with shaders [2J")
    );

    let mut blank = draft(Vec::new());
    blank.subject = Some(" \n ".into());
    reviews.send(&state, &NoAccount, blank, None).await.unwrap();
    assert!(fake.calls.lock().unwrap()[3].2["subject"].is_null());

    let mut long = draft(Vec::new());
    long.subject = Some("ж".repeat(81));
    assert!(reviews.send(&state, &NoAccount, long, None).await.is_err());
    let mut fits = draft(Vec::new());
    fits.subject = Some("ж".repeat(80));
    assert!(reviews.send(&state, &NoAccount, fits, None).await.is_ok());
}
