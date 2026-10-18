//! Fixture HTTP suite for the download executor: corruption, short reads,
//! stalls, retries, cancellation, redirects, duplicates and bounds.

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use sha1::Digest as _;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use super::receipt::{JobReceipt, JobState, reconcile_jobs};
use super::*;
use crate::storage::models::OperationKind;

/// What the fixture server sends for one request.
#[derive(Clone)]
pub(crate) enum Reply {
    Body(Vec<u8>),
    Status(u16, Vec<(&'static str, String)>),
    /// Declares `declared` bytes, sends `body`, then closes.
    Short {
        declared: usize,
        body: Vec<u8>,
    },
    /// Sends `first`, then waits before sending the rest.
    Stall {
        body: Vec<u8>,
        first: usize,
        wait: Duration,
    },
    Redirect(String),
}

type Route = Arc<dyn Fn(usize) -> Reply + Send + Sync>;

pub(crate) struct Server {
    base: String,
    routes: Arc<Mutex<HashMap<String, Route>>>,
    hits: Arc<Mutex<HashMap<String, usize>>>,
    active: Arc<AtomicUsize>,
    peak: Arc<AtomicUsize>,
}

impl Server {
    pub(crate) async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server = Self {
            base,
            routes: Arc::default(),
            hits: Arc::default(),
            active: Arc::default(),
            peak: Arc::default(),
        };
        let (routes, hits, active, peak) = (
            Arc::clone(&server.routes),
            Arc::clone(&server.hits),
            Arc::clone(&server.active),
            Arc::clone(&server.peak),
        );
        tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                let (routes, hits, active, peak) = (
                    Arc::clone(&routes),
                    Arc::clone(&hits),
                    Arc::clone(&active),
                    Arc::clone(&peak),
                );
                tokio::spawn(async move {
                    let mut head = Vec::new();
                    let mut byte = [0u8; 1];
                    while !head.ends_with(b"\r\n\r\n") {
                        if stream.read(&mut byte).await.unwrap_or(0) == 0 {
                            return;
                        }
                        head.push(byte[0]);
                    }
                    let line = String::from_utf8_lossy(&head).into_owned();
                    let path = line.split(' ').nth(1).unwrap_or("/").to_owned();
                    let nth = {
                        let mut hits = hits.lock().unwrap();
                        let n = hits.entry(path.clone()).or_default();
                        *n += 1;
                        *n - 1
                    };
                    let now = active.fetch_add(1, Ordering::SeqCst) + 1;
                    peak.fetch_max(now, Ordering::SeqCst);
                    let route = routes.lock().unwrap().get(&path).cloned();
                    let reply = route.map_or(Reply::Status(404, vec![]), |r| r(nth));
                    let _ = send(&mut stream, reply).await;
                    active.fetch_sub(1, Ordering::SeqCst);
                });
            }
        });
        server
    }

    pub(crate) fn route(&self, path: &str, route: impl Fn(usize) -> Reply + Send + Sync + 'static) {
        self.routes
            .lock()
            .unwrap()
            .insert(path.to_owned(), Arc::new(route));
    }

    pub(crate) fn serve(&self, path: &str, body: &[u8]) {
        let body = body.to_vec();
        self.route(path, move |_| Reply::Body(body.clone()));
    }

    pub(crate) fn hits(&self, path: &str) -> usize {
        self.hits.lock().unwrap().get(path).copied().unwrap_or(0)
    }

    pub(crate) fn total_hits(&self) -> usize {
        self.hits.lock().unwrap().values().sum()
    }

    pub(crate) fn url(&self, path: &str) -> Url {
        Url::parse(&format!("{}{path}", self.base)).unwrap()
    }
}

async fn send(stream: &mut tokio::net::TcpStream, reply: Reply) -> std::io::Result<()> {
    let head = |status: u16, length: usize, extra: &str| {
        format!(
            "HTTP/1.1 {status} X\r\nContent-Length: {length}\r\nConnection: close\r\n{extra}\r\n"
        )
    };
    match reply {
        Reply::Body(body) => {
            stream
                .write_all(head(200, body.len(), "").as_bytes())
                .await?;
            stream.write_all(&body).await?;
        }
        Reply::Status(status, headers) => {
            let extra: String = headers
                .iter()
                .map(|(k, v)| format!("{k}: {v}\r\n"))
                .collect();
            stream.write_all(head(status, 0, &extra).as_bytes()).await?;
        }
        Reply::Short { declared, body } => {
            stream.write_all(head(200, declared, "").as_bytes()).await?;
            stream.write_all(&body).await?;
        }
        Reply::Stall { body, first, wait } => {
            stream
                .write_all(head(200, body.len(), "").as_bytes())
                .await?;
            stream.write_all(&body[..first]).await?;
            stream.flush().await?;
            tokio::time::sleep(wait).await;
            stream.write_all(&body[first..]).await?;
        }
        Reply::Redirect(location) => {
            let extra = format!("Location: {location}\r\n");
            stream.write_all(head(302, 0, &extra).as_bytes()).await?;
        }
    }
    stream.shutdown().await
}

fn sha1_of(bytes: &[u8]) -> Sha1 {
    Sha1::parse(&hex::encode(sha1::Sha1::digest(bytes))).unwrap()
}

pub(crate) fn content(seed: u8, len: usize) -> Vec<u8> {
    (0..len)
        .map(|i| seed.wrapping_add((i % 251) as u8))
        .collect()
}

fn request(server: &Server, path: &str, body: &[u8]) -> DownloadRequest {
    let sha = sha1_of(body);
    DownloadRequest {
        source: SourcePolicy::MojangArtifacts,
        url: server.url(path),
        path: format!("shared/libraries/{sha}/artifact.jar"),
        size: body.len() as u64,
        digest: Digest::Sha1(sha),
        label: None,
    }
}

pub(crate) fn fast_config() -> ExecutorConfig {
    ExecutorConfig {
        backoff_base: Duration::from_millis(5),
        stall_timeout: Duration::from_secs(5),
        progress_interval: Duration::from_millis(20),
        space_probe: |_| None,
        ..ExecutorConfig::default()
    }
}

struct Fixture {
    _dir: tempfile::TempDir,
    root: DataRoot,
    server: Server,
}

impl Fixture {
    async fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = DataRoot::new(dir.path().to_path_buf()).unwrap();
        Self {
            _dir: dir,
            root,
            server: Server::start().await,
        }
    }

    fn executor(&self, config: ExecutorConfig) -> Executor {
        Executor::new(HttpTransport::loopback(), self.root.clone(), config)
    }

    async fn run(
        &self,
        config: ExecutorConfig,
        requests: Vec<DownloadRequest>,
    ) -> LauncherResult<DownloadReport> {
        self.executor(config)
            .run(Uuid::new_v4(), requests, &CancelToken::new(), &NoProgress)
            .await
    }

    fn file(&self, request: &DownloadRequest) -> PathBuf {
        resolve_managed_path(&self.root, &request.path).unwrap()
    }

    fn no_staging_left(&self) -> bool {
        std::fs::read_dir(self.root.downloads_dir()).map_or(true, |mut d| d.next().is_none())
    }
}

#[tokio::test]
async fn downloads_are_verified_committed_and_then_reused() {
    let f = Fixture::new().await;
    let (a, b) = (content(1, 200_000), content(2, 5));
    f.server.serve("/a", &a);
    f.server.serve("/b", &b);
    let ra = request(&f.server, "/a", &a);
    let rb = request(&f.server, "/b", &b);
    let report = f
        .run(fast_config(), vec![ra.clone(), rb.clone(), ra.clone()])
        .await
        .unwrap();
    assert_eq!(report.unique_files, 2, "duplicate request deduplicated");
    assert_eq!(report.downloaded_files, 2);
    assert_eq!(report.committed_bytes, 200_005);
    assert_eq!(report.network_bytes, 200_005);
    assert_eq!((f.server.hits("/a"), f.server.hits("/b")), (1, 1));
    assert_eq!(std::fs::read(f.file(&ra)).unwrap(), a);
    assert!(f.no_staging_left());

    // A second job (e.g. another instance) transfers nothing.
    let report = f.run(fast_config(), vec![ra, rb]).await.unwrap();
    assert_eq!((report.reused_files, report.downloaded_files), (2, 0));
    assert_eq!(report.network_bytes, 0);
    assert_eq!(f.server.total_hits(), 2);
}

#[tokio::test]
async fn identical_content_from_another_url_is_not_fetched_again() {
    let f = Fixture::new().await;
    let a = content(3, 1000);
    f.server.serve("/old/a.jar", &a);
    f.server.serve("/new/a.jar", &a);
    f.run(fast_config(), vec![request(&f.server, "/old/a.jar", &a)])
        .await
        .unwrap();
    let report = f
        .run(fast_config(), vec![request(&f.server, "/new/a.jar", &a)])
        .await
        .unwrap();
    assert_eq!(report.reused_files, 1);
    assert_eq!(f.server.hits("/new/a.jar"), 0);
}

#[tokio::test]
async fn checksum_mismatch_is_retried_once_then_rejected() {
    let f = Fixture::new().await;
    let good = content(4, 4096);
    let mut bad = good.clone();
    bad[100] ^= 0xff;
    let r = request(&f.server, "/x", &good);
    f.server.serve("/x", &bad);
    let error = f.run(fast_config(), vec![r.clone()]).await.unwrap_err();
    assert_eq!(error.code, ErrorCode::HashMismatch);
    assert_eq!(f.server.hits("/x"), 2, "one fresh retry, no more");
    assert!(
        !f.file(&r).exists(),
        "corrupt bytes never reach the final path"
    );
    assert!(f.no_staging_left());

    // Corrupt once, then correct: succeeds after the single retry.
    let (bad, good2) = (bad.clone(), good.clone());
    f.server.route("/y", move |n| {
        Reply::Body(if n == 0 { bad.clone() } else { good2.clone() })
    });
    let r = request(&f.server, "/y", &good);
    let report = f.run(fast_config(), vec![r.clone()]).await.unwrap();
    assert_eq!(report.retries, 1);
    assert_eq!(std::fs::read(f.file(&r)).unwrap(), good);
}

#[tokio::test]
async fn wrong_declared_or_actual_size_is_an_integrity_failure() {
    let f = Fixture::new().await;
    let body = content(5, 100);
    let mut r = request(&f.server, "/len", &body);
    r.size = 99;
    f.server.serve("/len", &body);
    let error = f.run(fast_config(), vec![r]).await.unwrap_err();
    assert_eq!(error.code, ErrorCode::HashMismatch);
    assert_eq!(f.server.hits("/len"), 2);
}

#[tokio::test]
async fn short_reads_and_server_errors_are_retried_with_a_limit() {
    let f = Fixture::new().await;
    let body = content(6, 10_000);
    let (b1, b2) = (body.clone(), body.clone());
    f.server.route("/short", move |n| match n {
        0 => Reply::Short {
            declared: b1.len(),
            body: b1[..4000].to_vec(),
        },
        1 => Reply::Status(503, vec![]),
        _ => Reply::Body(b2.clone()),
    });
    let r = request(&f.server, "/short", &body);
    let report = f.run(fast_config(), vec![r.clone()]).await.unwrap();
    assert_eq!(report.attempts, 3);
    assert_eq!(
        report.network_bytes, 14_000,
        "failed attempt bytes are counted"
    );
    assert_eq!(report.committed_bytes, 10_000);

    f.server.route("/down", |_| Reply::Status(503, vec![]));
    let error = f
        .run(
            fast_config(),
            vec![request(&f.server, "/down", &content(60, 10))],
        )
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::ServiceUnavailable);
    assert_eq!(
        f.server.hits("/down"),
        4,
        "initial attempt plus three retries"
    );
}

#[tokio::test]
async fn permanent_errors_are_not_retried() {
    let f = Fixture::new().await;
    let body = content(7, 10);
    let error = f
        .run(fast_config(), vec![request(&f.server, "/missing", &body)])
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::NetworkError);
    assert_eq!(f.server.hits("/missing"), 1);
}

#[tokio::test]
async fn retry_after_is_honoured_and_bounded() {
    let f = Fixture::new().await;
    let body = content(8, 10);
    let b = body.clone();
    f.server.route("/limited", move |n| {
        if n == 0 {
            Reply::Status(429, vec![("Retry-After", "1".into())])
        } else {
            Reply::Body(b.clone())
        }
    });
    let started = Instant::now();
    f.run(fast_config(), vec![request(&f.server, "/limited", &body)])
        .await
        .unwrap();
    assert!(started.elapsed() >= Duration::from_millis(950));

    f.server.route("/later", |_| {
        Reply::Status(429, vec![("Retry-After", "3600".into())])
    });
    let started = Instant::now();
    let error = f
        .run(
            fast_config(),
            vec![request(&f.server, "/later", &content(80, 10))],
        )
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::RateLimited);
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "does not wait an hour"
    );
    assert_eq!(f.server.hits("/later"), 1);
}

#[tokio::test]
async fn stalled_transfers_time_out() {
    let f = Fixture::new().await;
    let body = content(9, 1000);
    let b = body.clone();
    f.server.route("/stall", move |_| Reply::Stall {
        body: b.clone(),
        first: 10,
        wait: Duration::from_secs(30),
    });
    let config = ExecutorConfig {
        stall_timeout: Duration::from_millis(200),
        max_retries: 1,
        ..fast_config()
    };
    let started = Instant::now();
    let error = f
        .run(config, vec![request(&f.server, "/stall", &body)])
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::NetworkError);
    assert_eq!(
        error.safe_details.unwrap().get("transport"),
        Some(&crate::error::SafeValue::Text("stall".into()))
    );
    assert_eq!(f.server.hits("/stall"), 2);
    assert!(started.elapsed() < Duration::from_secs(5));
}

#[tokio::test]
async fn cancel_keeps_verified_files_and_retry_reuses_them() {
    let f = Fixture::new().await;
    let small = content(10, 500);
    let big = content(11, 64 * 1024);
    f.server.serve("/small", &small);
    let b = big.clone();
    f.server.route("/big", move |n| {
        if n == 0 {
            Reply::Stall {
                body: b.clone(),
                first: 1024,
                wait: Duration::from_secs(30),
            }
        } else {
            Reply::Body(b.clone())
        }
    });
    let rs = request(&f.server, "/small", &small);
    let rb = request(&f.server, "/big", &big);
    let executor = Arc::new(f.executor(ExecutorConfig {
        concurrency: 1,
        ..fast_config()
    }));
    let cancel = CancelToken::new();
    let job = Uuid::new_v4();
    let task = tokio::spawn({
        let (executor, cancel, requests) = (
            Arc::clone(&executor),
            cancel.clone(),
            vec![rs.clone(), rb.clone()],
        );
        async move { executor.run(job, requests, &cancel, &NoProgress).await }
    });
    // Wait until the big transfer has started streaming.
    while f.server.hits("/big") == 0 {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    tokio::time::sleep(Duration::from_millis(100)).await;
    cancel.cancel();
    let error = tokio::time::timeout(Duration::from_secs(2), task)
        .await
        .expect("cancellation is prompt")
        .unwrap()
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::OperationCancelled);
    assert!(f.file(&rs).exists(), "verified file retained");
    assert!(!f.file(&rb).exists(), "partial content never marked ready");
    assert!(!f.root.download_staging(job).exists());

    let report = f.run(fast_config(), vec![rs, rb.clone()]).await.unwrap();
    assert_eq!((report.reused_files, report.downloaded_files), (1, 1));
    assert_eq!(f.server.hits("/small"), 1);
    assert_eq!(std::fs::read(f.file(&rb)).unwrap(), big);
}

#[tokio::test]
async fn redirects_cannot_leave_the_source() {
    let f = Fixture::new().await;
    let body = content(12, 50);
    f.server.serve("/target", &body);
    let port = f.server.base.rsplit(':').next().unwrap().to_owned();
    let inside = format!("{}/target", f.server.base);
    f.server
        .route("/inside", move |_| Reply::Redirect(inside.clone()));
    let outside = format!("http://localhost:{port}/target");
    f.server
        .route("/outside", move |_| Reply::Redirect(outside.clone()));
    f.server.route("/loop", |_| Reply::Redirect("/loop".into()));

    f.run(fast_config(), vec![request(&f.server, "/inside", &body)])
        .await
        .unwrap();
    for path in ["/outside", "/loop"] {
        let error = f
            .run(
                fast_config(),
                vec![request(&f.server, path, &content(13, 50))],
            )
            .await
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::NetworkError, "{path}");
        assert!(error.message.contains("redirected"), "{path}");
    }
    assert_eq!(
        f.server.hits("/outside"),
        1,
        "policy violations are not retried"
    );
    assert_eq!(f.server.hits("/loop"), 1 + crate::net::MAX_REDIRECTS);
    assert_eq!(
        f.server.hits("/target"),
        1,
        "the escaping redirect was never followed"
    );
}

#[tokio::test]
async fn conflicting_metadata_and_unsafe_paths_fail_before_any_request() {
    let f = Fixture::new().await;
    let a = content(14, 10);
    let r = request(&f.server, "/a", &a);
    let mut other_size = r.clone();
    other_size.size += 1;
    other_size.path = "shared/libraries/other/artifact.jar".into();
    let error = f
        .run(fast_config(), vec![r.clone(), other_size])
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::MetadataConflict);

    let mut other_digest = request(&f.server, "/b", &content(15, 10));
    other_digest.path = r.path.clone();
    let error = f
        .run(fast_config(), vec![r.clone(), other_digest])
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::MetadataConflict);

    for path in [
        "../escape.jar",
        "shared/../../x",
        "/abs",
        "a//b",
        "a\\b",
        "c:x",
    ] {
        let mut bad = r.clone();
        bad.path = path.into();
        assert!(f.run(fast_config(), vec![bad]).await.is_err(), "{path}");
    }
    assert_eq!(f.server.total_hits(), 0);
}

#[tokio::test]
async fn insufficient_disk_space_is_reported_before_downloading() {
    let f = Fixture::new().await;
    let a = content(16, 1000);
    f.server.serve("/a", &a);
    let config = ExecutorConfig {
        space_probe: |_| Some(10),
        disk_reserve: 0,
        ..fast_config()
    };
    let error = f
        .run(config, vec![request(&f.server, "/a", &a)])
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::DiskFull);
    assert_eq!(f.server.total_hits(), 0);
}

#[tokio::test]
async fn existing_corrupt_files_are_replaced_and_sha256_is_supported() {
    let f = Fixture::new().await;
    let body = content(17, 3000);
    f.server.serve("/rt", &body);
    let sha256 = Sha256::parse(&hex::encode(sha2::Sha256::digest(&body))).unwrap();
    let r = DownloadRequest {
        source: SourcePolicy::Adoptium,
        url: f.server.url("/rt"),
        path: format!("downloads-cache/{sha256}.tar.gz"),
        size: body.len() as u64,
        digest: Digest::Sha256(sha256),
        label: None,
    };
    let path = f.file(&r);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, content(18, 3000)).unwrap();
    let report = f.run(fast_config(), vec![r]).await.unwrap();
    assert_eq!(report.downloaded_files, 1);
    assert_eq!(std::fs::read(path).unwrap(), body);
}

#[derive(Default)]
struct Recorder(Mutex<Vec<(Instant, DownloadProgress)>>);

impl ProgressSink for Recorder {
    fn progress(&self, progress: &DownloadProgress) {
        self.0
            .lock()
            .unwrap()
            .push((Instant::now(), progress.clone()));
    }
}

#[tokio::test]
async fn concurrency_and_progress_stay_bounded() {
    let f = Fixture::new().await;
    let mut requests = Vec::new();
    for i in 0..24u8 {
        let body = content(i, 20_000);
        let path = format!("/f{i}");
        let b = body.clone();
        f.server.route(&path, move |n| {
            if n == 0 && i % 3 == 0 {
                // A failed partial attempt must not push progress past the total.
                Reply::Short {
                    declared: b.len(),
                    body: b[..15_000].to_vec(),
                }
            } else {
                Reply::Stall {
                    body: b.clone(),
                    first: 1,
                    wait: Duration::from_millis(30),
                }
            }
        });
        requests.push(request(&f.server, &path, &body));
    }
    let recorder = Recorder::default();
    let config = ExecutorConfig {
        concurrency: 3,
        ..fast_config()
    };
    let started = Instant::now();
    let report = f
        .executor(config)
        .run(Uuid::new_v4(), requests, &CancelToken::new(), &recorder)
        .await
        .unwrap();
    let elapsed = started.elapsed();
    assert!(
        f.server.peak.load(Ordering::SeqCst) <= 3,
        "never more than 3 transfers"
    );
    assert_eq!(report.retries, 8);
    let events = recorder.0.lock().unwrap();
    // One event per 20 ms interval across both phases, plus each phase ticker's
    // immediate first tick and the three explicit phase/final events.
    let max_events = (elapsed.as_millis() / 20) as usize + 6;
    assert!(
        events.len() <= max_events,
        "{} > {max_events}",
        events.len()
    );
    let downloading: Vec<_> = events
        .iter()
        .filter(|(_, p)| p.phase == DownloadPhase::Downloading)
        .map(|(_, p)| p)
        .collect();
    assert!(
        downloading
            .windows(2)
            .all(|w| w[0].downloaded_bytes <= w[1].downloaded_bytes)
    );
    let last = downloading.last().unwrap();
    assert_eq!(last.downloaded_bytes, 24 * 20_000);
    assert_eq!(last.total_download_bytes, 24 * 20_000);
    assert_eq!(last.verified_files, 24);
    // Progress names the files in flight: never more than the transfers, and
    // nothing once every file is done.
    assert!(
        downloading.iter().any(|p| !p.current_files.is_empty()),
        "some progress names the files being downloaded"
    );
    assert!(downloading.iter().all(|p| p.current_files.len() <= 3));
    // The stored file name, not its path.
    assert!(
        downloading
            .iter()
            .flat_map(|p| &p.current_files)
            .all(|name| name == "artifact.jar")
    );
    assert!(last.current_files.is_empty());
}

#[test]
fn receipts_record_one_terminal_outcome_and_interruptions() {
    let dir = tempfile::tempdir().unwrap();
    let root = DataRoot::new(dir.path().to_path_buf()).unwrap();
    let done = Uuid::new_v4();
    let mut receipt = JobReceipt::start(&root, done, OperationKind::Install, None, None).unwrap();
    assert!(
        receipt
            .finish(&root, &Ok(DownloadReport::default()))
            .unwrap()
    );
    let cancelled = Err(cancelled_error());
    assert!(
        !receipt.finish(&root, &cancelled).unwrap(),
        "first outcome wins"
    );
    assert_eq!(
        JobReceipt::load(&root, done).unwrap().unwrap().state,
        JobState::Succeeded
    );

    let crashed = Uuid::new_v4();
    JobReceipt::start(&root, crashed, OperationKind::Repair, None, None).unwrap();
    let staging = root.download_part(crashed, "sha1", "ab");
    std::fs::create_dir_all(staging.parent().unwrap()).unwrap();
    std::fs::write(&staging, b"partial").unwrap();
    assert_eq!(
        reconcile_jobs(&root)
            .unwrap()
            .iter()
            .map(|r| r.job_id)
            .collect::<Vec<_>>(),
        vec![crashed]
    );
    let reloaded = JobReceipt::load(&root, crashed).unwrap().unwrap();
    assert_eq!(reloaded.state, JobState::Interrupted);
    assert_eq!(reloaded.error_code, Some(ErrorCode::OperationInterrupted));
    assert!(!root.downloads_dir().exists());
    assert!(reconcile_jobs(&root).unwrap().is_empty(), "idempotent");
    drop(dir);
}

#[test]
fn free_space_is_read_from_the_directory_itself() {
    let dir = tempfile::tempdir().unwrap();
    let available = super::available_space(dir.path()).expect("free space is known");
    assert!(available > 0);
    assert!(super::available_space(&dir.path().join("missing")).is_none());
}

#[test]
fn assets_show_their_logical_name_in_progress() {
    assert_eq!(
        label(
            ArtifactRole::AssetObject,
            Some("minecraft/sounds/ambient/cave/cave1.ogg")
        ),
        Some("cave1.ogg".to_owned())
    );
    assert_eq!(label(ArtifactRole::AssetObject, None), None);
    // Libraries are named by their stored file; the coordinate is provenance.
    assert_eq!(
        label(ArtifactRole::Library, Some("org.lwjgl:lwjgl:3.3.3")),
        None
    );
}
