//! Deterministic account-flow tests against a local fixture server. These prove
//! decision and concurrency behaviour only; they are not live service evidence.

use std::collections::HashMap;
use std::sync::atomic::AtomicBool;

use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::*;
use crate::auth::test_server::{Recorded, TestServer};
use crate::auth::vault::test_support::FlakyStore;
use crate::state::AppState;
use crate::storage::paths::DataRoot;
use crate::validation::MemoryPolicy;

const CLIENT_ID: &str = "0f1e2d3c-4b5a-6978-8796-a5b4c3d2e1f0";
const PROFILE_ID: &str = "0f1e2d3c4b5a69788796a5b4c3d2e1f0";

struct Host {
    app: AppState,
    launch_owned: AtomicBool,
}

impl AccountHost for Host {
    fn auth_status(&self) -> AuthStatus {
        self.app.auth_status()
    }
    fn set_auth_status(&self, status: AuthStatus) {
        self.app.set_auth_status(status);
    }
    fn accounts_file(&self) -> AccountsFile {
        self.app.accounts_file()
    }
    fn update_accounts_file(
        &self,
        change: Box<dyn FnOnce(&mut AccountsFile) + Send + '_>,
    ) -> LauncherResult<()> {
        self.app.update_accounts_file(change)
    }
    fn launch_owns_account(&self) -> bool {
        self.launch_owned.load(Ordering::SeqCst)
    }
}

type Routes = Arc<Mutex<HashMap<String, (u16, String)>>>;

fn happy_routes() -> HashMap<String, (u16, String)> {
    let xbox = |token: &str, xid: bool| {
        let mut claim = json!({"uhs": "uhs-1"});
        if xid {
            claim["xid"] = json!("2535400000000001");
        }
        json!({"IssueInstant": "2026-10-16T00:00:00Z", "NotAfter": "2099-01-01T00:00:00Z",
               "Token": token, "DisplayClaims": {"xui": [claim]}})
        .to_string()
    };
    HashMap::from([
        (
            "/token".to_owned(),
            (
                200,
                json!({"token_type": "Bearer", "expires_in": 3600,
                         "access_token": "msa-access", "refresh_token": "refresh-1"})
                .to_string(),
            ),
        ),
        ("/xbox/user".to_owned(), (200, xbox("user-token", false))),
        ("/xsts".to_owned(), (200, xbox("xsts-token", true))),
        (
            "/launcher/login".to_owned(),
            (
                200,
                json!({"username": "u", "access_token": "mc-token-1", "expires_in": 86400,
                         "token_type": "Bearer"})
                .to_string(),
            ),
        ),
        (
            "/entitlements/license".to_owned(),
            (
                200,
                json!({"items": [{"name": "product_minecraft"}, {"name": "game_minecraft"}]})
                    .to_string(),
            ),
        ),
        (
            "/minecraft/profile".to_owned(),
            (
                200,
                json!({"id": PROFILE_ID, "name": "Steve_Test", "skins": []}).to_string(),
            ),
        ),
    ])
}

struct Fixture {
    _dir: tempfile::TempDir,
    root: DataRoot,
    server: TestServer,
    routes: Routes,
    host: Arc<Host>,
    store: Arc<FlakyStore>,
    service: Arc<AuthService>,
    opened: Arc<Mutex<Vec<String>>>,
}

async fn fixture_with(
    client_id: Option<&str>,
    store: Arc<FlakyStore>,
    dir: Option<tempfile::TempDir>,
) -> Fixture {
    let dir = dir.unwrap_or_else(|| tempfile::tempdir().unwrap());
    let root = DataRoot::new(dir.path().to_path_buf()).unwrap();
    let routes: Routes = Arc::new(Mutex::new(happy_routes()));
    let r = Arc::clone(&routes);
    let server = TestServer::start(Arc::new(move |req: &Recorded| {
        r.lock()
            .unwrap()
            .get(&req.path)
            .cloned()
            .unwrap_or((404, "{}".into()))
    }))
    .await;
    let host = Arc::new(Host {
        app: AppState::initialize(
            Ok(root.clone()),
            "0.1.0",
            MemoryPolicy::from_physical_bytes(16 << 30),
            Box::new(crate::state::NoopSink),
        ),
        launch_owned: AtomicBool::new(false),
    });
    let opened = Arc::new(Mutex::new(Vec::new()));
    let o = Arc::clone(&opened);
    let service = Arc::new(
        AuthService::new(
            AuthHttp::local(&server.base),
            client_id_status(client_id),
            Arc::clone(&host) as Arc<dyn AccountHost>,
            Box::new(move |url: &str| {
                o.lock().unwrap().push(url.to_owned());
                Ok(())
            }),
            Arc::clone(&store) as Arc<dyn SecretStore>,
        )
        .with_login_timeout(Duration::from_secs(5)),
    );
    Fixture {
        _dir: dir,
        root,
        server,
        routes,
        host,
        store,
        service,
        opened,
    }
}

fn client_id_status(raw: Option<&str>) -> Result<Option<ClientId>, client_id::InvalidClientId> {
    client_id::classify(raw)
}

async fn fixture() -> Fixture {
    fixture_with(Some(CLIENT_ID), Arc::default(), None).await
}

impl Fixture {
    /// Ends this "process": drops its state (releasing the DATA_ROOT lock) and
    /// returns the data directory for a restarted fixture.
    fn into_dir(self) -> tempfile::TempDir {
        let Fixture { _dir, .. } = self;
        _dir
    }

    fn route(&self, path: &str, status: u16, body: serde_json::Value) {
        self.routes
            .lock()
            .unwrap()
            .insert(path.to_owned(), (status, body.to_string()));
    }

    /// The single account these tests use, or the sign-in result.
    fn view(&self) -> View {
        View::of(&self.host.app.auth_status())
    }

    fn state(&self) -> AccountState {
        self.view().state
    }

    async fn wait_for(&self, predicate: impl Fn(&View) -> bool) -> View {
        for _ in 0..200 {
            let view = self.view();
            if predicate(&view) {
                return view;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        panic!("timed out; status = {:?}", self.host.app.auth_status());
    }

    /// Acts as the browser: follows the opened authorization URL's redirect.
    async fn browser_callback(&self, query: impl Fn(&str) -> String) -> String {
        let url = self
            .opened
            .lock()
            .unwrap()
            .last()
            .cloned()
            .expect("browser opened");
        let url = Url::parse(&url).unwrap();
        let params: HashMap<_, _> = url.query_pairs().into_owned().collect();
        let redirect = Url::parse(&params["redirect_uri"]).unwrap();
        let path = format!("{}?{}", redirect.path(), query(&params["state"]));
        let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", redirect.port().unwrap()))
            .await
            .unwrap();
        stream
            .write_all(format!("GET {path} HTTP/1.1\r\nHost: localhost\r\n\r\n").as_bytes())
            .await
            .unwrap();
        let mut response = String::new();
        let _ = stream.read_to_string(&mut response).await;
        response
    }

    async fn sign_in(&self) -> View {
        self.service.begin_login().await.unwrap();
        self.browser_callback(|state| format!("code=auth-code&state={state}"))
            .await;
        self.wait_for(|a| a.state != AccountState::SigningIn).await
    }
}

/// One account's state as the interface shows it: a pending sign-in or sign-out
/// first, then the account, then the last sign-in result.
#[derive(Debug)]
struct View {
    state: AccountState,
    profile_name: Option<String>,
    error: Option<LauncherError>,
    session_only: bool,
}

impl View {
    fn of(status: &AuthStatus) -> Self {
        let (state, profile_name, error) = if status.login_operation_id.is_some() {
            (AccountState::SigningIn, None, None)
        } else if !status.pending_sign_outs.is_empty() {
            (
                AccountState::SignOutPending,
                None,
                status.login_error.clone(),
            )
        } else if let Some(account) = status.accounts.first() {
            (
                account.state,
                Some(account.profile_name.clone()),
                account.error.clone(),
            )
        } else {
            (status.login_state, None, status.login_error.clone())
        };
        Self {
            state,
            profile_name,
            error,
            session_only: status.session_only,
        }
    }
}

#[tokio::test]
async fn successful_login_verifies_access_and_stores_only_in_vault() {
    let f = fixture().await;
    let account = f.sign_in().await;
    assert_eq!(account.state, AccountState::VerifiedAccess, "{account:?}");
    assert_eq!(account.profile_name.as_deref(), Some("Steve_Test"));
    assert!(!account.session_only);

    // Authorization URL: own client ID, S256 PKCE, state, loopback redirect.
    let url = Url::parse(f.opened.lock().unwrap().last().unwrap()).unwrap();
    let params: HashMap<_, _> = url.query_pairs().into_owned().collect();
    assert_eq!(params["client_id"], CLIENT_ID);
    assert_eq!(params["code_challenge_method"], "S256");
    assert_eq!(params["scope"], contract::SCOPES);
    assert!(params["redirect_uri"].starts_with("http://localhost:"));

    // The token request carried the matching verifier and the same redirect URI.
    let token = f.server.last("/token").unwrap();
    let form: HashMap<_, _> = url::form_urlencoded::parse(token.body.as_bytes())
        .into_owned()
        .collect();
    assert_eq!(
        pkce::challenge_for(&form["code_verifier"]),
        params["code_challenge"]
    );
    assert_eq!(form["redirect_uri"], params["redirect_uri"]);
    assert_eq!(form["code"], "auth-code");
    assert!(!form.contains_key("client_secret"));

    // XSTS used the Xbox contract header; Minecraft services got the Bearer token.
    assert_eq!(
        f.server.last("/xsts").unwrap().headers["x-xbl-contract-version"],
        "1"
    );
    assert_eq!(
        f.server.last("/entitlements/license").unwrap().headers["authorization"],
        "Bearer mc-token-1"
    );
    assert!(
        f.server
            .last("/entitlements/license")
            .unwrap()
            .query
            .starts_with("requestId=")
    );

    // Refresh token only in the vault; accounts.json has no secret.
    let key = vault_key(PROFILE_ID);
    assert_eq!(f.store.get(&key).unwrap().unwrap().expose(), "refresh-1");
    let saved = std::fs::read_to_string(f.root.accounts()).unwrap();
    assert!(saved.contains("Steve_Test"));
    for secret in ["refresh-1", "mc-token-1", "msa-access", "auth-code"] {
        assert!(!saved.contains(secret));
        assert!(
            !serde_json::to_string(&f.host.app.snapshot())
                .unwrap()
                .contains(secret)
        );
    }
}

#[tokio::test]
async fn subscription_without_ownership_is_accepted() {
    let f = fixture().await;
    f.route(
        "/entitlements/license",
        200,
        json!({"items": [{"name": "game_minecraft", "source": "GAMEPASS"}]}),
    );
    assert_eq!(f.sign_in().await.state, AccountState::VerifiedAccess);
}

#[tokio::test]
async fn negative_outcomes_get_distinct_states() {
    let cases: Vec<(&str, u16, serde_json::Value, AccountState, ErrorCode)> = vec![
        (
            "/entitlements/license",
            200,
            json!({"items": []}),
            AccountState::NoAccess,
            ErrorCode::NoGameAccess,
        ),
        (
            "/entitlements/license",
            200,
            json!({"items": [{"name": "product_minecraft"}]}),
            AccountState::NoAccess,
            ErrorCode::NoGameAccess,
        ),
        (
            "/entitlements/license",
            200,
            json!({}),
            AccountState::ProtocolError,
            ErrorCode::AuthProtocolError,
        ),
        (
            "/minecraft/profile",
            404,
            json!({"errorType": "NOT_FOUND"}),
            AccountState::ProfileSetupRequired,
            ErrorCode::ProfileSetupRequired,
        ),
        (
            "/launcher/login",
            403,
            json!({"errorMessage": "Invalid app registration, see https://aka.ms/AppRegInfo"}),
            AccountState::AppNotApproved,
            ErrorCode::AuthAppNotApproved,
        ),
        (
            "/launcher/login",
            403,
            json!({"errorMessage": "other"}),
            AccountState::Restricted,
            ErrorCode::AuthForbidden,
        ),
        (
            "/xsts",
            401,
            json!({"XErr": 2_148_916_238_i64}),
            AccountState::Restricted,
            ErrorCode::AuthAccountRestricted,
        ),
        (
            "/xsts",
            401,
            json!({"XErr": 2_148_916_233_i64}),
            AccountState::ProfileSetupRequired,
            ErrorCode::ProfileSetupRequired,
        ),
        (
            "/entitlements/license",
            503,
            json!({}),
            AccountState::ServiceUnavailable,
            ErrorCode::ServiceUnavailable,
        ),
        (
            "/entitlements/license",
            429,
            json!({}),
            AccountState::ServiceUnavailable,
            ErrorCode::RateLimited,
        ),
    ];
    for (path, status, body, state, code) in cases {
        let f = fixture().await;
        f.route(path, status, body);
        let account = f.sign_in().await;
        assert_eq!(account.state, state, "{path} {status}");
        assert_eq!(
            account.error.as_ref().map(|e| e.code),
            Some(code),
            "{path} {status}"
        );
        // Nothing is saved for a failed verification.
        assert!(f.store.get(&vault_key(PROFILE_ID)).unwrap().is_none());
        assert!(f.host.accounts_file().accounts.is_empty());
    }
}

#[tokio::test]
async fn mismatched_xsts_identity_is_a_protocol_error() {
    let f = fixture().await;
    f.route(
        "/xsts",
        200,
        json!({"NotAfter": "2099-01-01T00:00:00Z", "Token": "x", "DisplayClaims": {"xui": [{"uhs": "someone-else"}]}}),
    );
    assert_eq!(f.sign_in().await.state, AccountState::ProtocolError);
}

#[tokio::test]
async fn declined_consent_cancel_and_timeout() {
    let f = fixture().await;
    f.service.begin_login().await.unwrap();
    f.browser_callback(|state| format!("error=access_denied&state={state}"))
        .await;
    let account = f.wait_for(|a| a.state != AccountState::SigningIn).await;
    assert_eq!(account.error.unwrap().code, ErrorCode::AuthCancelled);

    // Cancel: the listener closes and a late callback reaches nothing.
    let op = f.service.begin_login().await.unwrap();
    let url = Url::parse(f.opened.lock().unwrap().last().unwrap()).unwrap();
    let port = Url::parse(
        &url.query_pairs()
            .find(|(k, _)| k == "redirect_uri")
            .unwrap()
            .1,
    )
    .unwrap()
    .port()
    .unwrap();
    f.service.cancel_login(&op).unwrap();
    assert_eq!(f.state(), AccountState::SignedOut);
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(
        tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .is_err()
    );
    assert_eq!(f.server.count("/token"), 0);
    assert_eq!(
        f.service.cancel_login(&op).unwrap_err().code,
        ErrorCode::OperationNotFound
    );

    let f = fixture().await;
    let service = Arc::new(
        AuthService::new(
            AuthHttp::local(&f.server.base),
            client_id_status(Some(CLIENT_ID)),
            Arc::clone(&f.host) as Arc<dyn AccountHost>,
            Box::new(|_: &str| Ok(())),
            Arc::new(MemoryStore::default()),
        )
        .with_login_timeout(Duration::from_millis(200)),
    );
    service.begin_login().await.unwrap();
    let account = f.wait_for(|a| a.state != AccountState::SigningIn).await;
    assert_eq!(account.error.unwrap().code, ErrorCode::AuthTimeout);
}

#[tokio::test]
async fn wrong_state_does_not_consume_the_attempt() {
    let f = fixture().await;
    f.service.begin_login().await.unwrap();
    let response = f
        .browser_callback(|_| "code=evil&state=forged".into())
        .await;
    assert!(response.starts_with("HTTP/1.1 400"));
    assert_eq!(f.state(), AccountState::SigningIn);
    f.browser_callback(|state| format!("code=auth-code&state={state}"))
        .await;
    let account = f.wait_for(|a| a.state != AccountState::SigningIn).await;
    assert_eq!(account.state, AccountState::VerifiedAccess);
    assert_eq!(f.server.count("/token"), 1);
}

#[tokio::test]
async fn repeated_begin_returns_the_same_attempt() {
    let f = fixture().await;
    let a = f.service.begin_login().await.unwrap();
    let b = f.service.begin_login().await.unwrap();
    assert_eq!(a, b);
    assert_eq!(f.opened.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn not_configured_client_id_is_explicit() {
    let f = fixture_with(None, Arc::default(), None).await;
    assert_eq!(
        f.service.begin_login().await.unwrap_err().code,
        ErrorCode::AuthNotConfigured
    );
    assert!(f.opened.lock().unwrap().is_empty());
}

#[tokio::test]
async fn sign_out_removes_credentials_and_launch_authorization() {
    let f = fixture().await;
    f.sign_in().await;
    assert!(f.service.verify_for_launch(PROFILE_ID).await.is_ok());
    f.service.sign_out(PROFILE_ID).await.unwrap();
    assert_eq!(f.state(), AccountState::SignedOut);
    assert!(f.store.get(&vault_key(PROFILE_ID)).unwrap().is_none());
    assert!(f.host.accounts_file().accounts.is_empty());
    assert_eq!(
        f.service
            .verify_for_launch(PROFILE_ID)
            .await
            .err()
            .map(|e| e.code),
        Some(ErrorCode::AuthReauthRequired)
    );
}

#[tokio::test]
async fn account_changes_are_refused_while_a_launch_owns_it() {
    let f = fixture().await;
    f.sign_in().await;
    f.host.launch_owned.store(true, Ordering::SeqCst);
    assert_eq!(
        f.service.sign_out(PROFILE_ID).await.unwrap_err().code,
        ErrorCode::AppBusy
    );
    assert_eq!(
        f.service.begin_login().await.unwrap_err().code,
        ErrorCode::AppBusy
    );
    assert_eq!(f.state(), AccountState::VerifiedAccess);
}

#[tokio::test]
async fn vault_deletion_failure_is_reported_and_retried_after_restart() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().to_path_buf();
    let store = Arc::new(FlakyStore::default());
    let f = fixture_with(Some(CLIENT_ID), Arc::clone(&store), Some(dir)).await;
    f.sign_in().await;
    store.fail_delete.store(true, Ordering::SeqCst);
    let error = f.service.sign_out(PROFILE_ID).await.unwrap_err();
    assert_eq!(error.code, ErrorCode::SecretStoreDeleteFailed);
    assert_eq!(f.state(), AccountState::SignOutPending);
    assert_eq!(f.host.accounts_file().pending_sign_outs.len(), 1);
    // Launch authorization is cleared even though deletion failed.
    assert!(f.service.verify_for_launch(PROFILE_ID).await.is_err());
    let dir = f.into_dir();

    // "Restart": a new app state and service over the same data root and vault.
    store.fail_delete.store(false, Ordering::SeqCst);
    let f = fixture_with(Some(CLIENT_ID), Arc::clone(&store), Some(dir)).await;
    assert_eq!(f.root.path(), path);
    assert_eq!(f.state(), AccountState::SignOutPending);
    f.service.restore().await;
    assert_eq!(f.state(), AccountState::SignedOut);
    assert!(store.get(&vault_key(PROFILE_ID)).unwrap().is_none());
    assert!(f.host.accounts_file().pending_sign_outs.is_empty());
    assert_eq!(f.server.count("/token"), 0, "never restored while pending");
}

#[tokio::test]
async fn restore_refreshes_and_rotates_the_refresh_token() {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(FlakyStore::default());
    let f = fixture_with(Some(CLIENT_ID), Arc::clone(&store), Some(dir)).await;
    f.sign_in().await;
    let dir = f.into_dir();

    let f = fixture_with(Some(CLIENT_ID), Arc::clone(&store), Some(dir)).await;
    assert_eq!(f.state(), AccountState::Refreshing);
    f.route(
        "/token",
        200,
        json!({"token_type": "Bearer", "expires_in": 3600, "access_token": "msa-2", "refresh_token": "refresh-2"}),
    );
    f.service.restore().await;
    assert_eq!(f.state(), AccountState::VerifiedAccess);
    let form = f.server.last("/token").unwrap().body;
    assert!(form.contains("grant_type=refresh_token"));
    assert!(form.contains("refresh_token=refresh-1"));
    assert_eq!(
        store.get(&vault_key(PROFILE_ID)).unwrap().unwrap().expose(),
        "refresh-2"
    );
    let dir = f.into_dir();

    // A response without rotation keeps the existing refresh token.
    let f = fixture_with(Some(CLIENT_ID), Arc::clone(&store), Some(dir)).await;
    f.route(
        "/token",
        200,
        json!({"token_type": "Bearer", "expires_in": 3600, "access_token": "msa-3"}),
    );
    f.service.restore().await;
    assert_eq!(f.state(), AccountState::VerifiedAccess);
    assert_eq!(
        store.get(&vault_key(PROFILE_ID)).unwrap().unwrap().expose(),
        "refresh-2"
    );
}

#[tokio::test]
async fn invalid_grant_clears_but_outage_keeps_the_account() {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(FlakyStore::default());
    let f = fixture_with(Some(CLIENT_ID), Arc::clone(&store), Some(dir)).await;
    f.sign_in().await;
    let dir = f.into_dir();

    let f = fixture_with(Some(CLIENT_ID), Arc::clone(&store), Some(dir)).await;
    f.route("/token", 503, json!({"error": "temporarily_unavailable"}));
    f.service.restore().await;
    assert_eq!(f.state(), AccountState::ServiceUnavailable);
    assert!(
        store.get(&vault_key(PROFILE_ID)).unwrap().is_some(),
        "outage keeps the login"
    );
    assert!(!f.host.accounts_file().accounts.is_empty());
    let dir = f.into_dir();

    let f = fixture_with(Some(CLIENT_ID), Arc::clone(&store), Some(dir)).await;
    f.route("/token", 400, json!({"error": "invalid_grant"}));
    f.service.restore().await;
    assert_eq!(f.state(), AccountState::ReauthRequired);
    assert!(store.get(&vault_key(PROFILE_ID)).unwrap().is_none());
    assert!(f.host.accounts_file().accounts.is_empty());
}

#[tokio::test]
async fn launch_verification_is_fresh_and_refreshes_once_on_401() {
    let f = fixture().await;
    f.sign_in().await;
    let before = f.server.count("/entitlements/license");
    let auth = f.service.verify_for_launch(PROFILE_ID).await.unwrap();
    assert_eq!(auth.profile.name, "Steve_Test");
    assert_eq!(auth.access_token.expose(), "mc-token-1");
    assert_eq!(auth.xuid.as_deref(), Some("2535400000000001"));
    assert_eq!(auth.client_id, CLIENT_ID);
    // Each launch asks for fresh entitlements; the cached token is reused.
    f.service.verify_for_launch(PROFILE_ID).await.unwrap();
    assert_eq!(f.server.count("/entitlements/license"), before + 2);
    assert_eq!(f.server.count("/token"), 1);

    // A 401 from Minecraft services triggers exactly one refresh, then succeeds.
    let entitlement_calls = Arc::new(Mutex::new(0));
    let calls = Arc::clone(&entitlement_calls);
    let routes = Arc::clone(&f.routes);
    f.server.set_handler(Arc::new(move |req: &Recorded| {
        if req.path == "/entitlements/license" {
            let mut n = calls.lock().unwrap();
            *n += 1;
            if *n == 1 {
                return (401, "{}".into());
            }
        }
        routes
            .lock()
            .unwrap()
            .get(&req.path)
            .cloned()
            .unwrap_or((404, "{}".into()))
    }));
    f.service.verify_for_launch(PROFILE_ID).await.unwrap();
    assert_eq!(f.server.count("/token"), 2, "one refresh");
    assert_eq!(f.state(), AccountState::VerifiedAccess);

    // Revoked access between launches is caught by the fresh check.
    f.route("/entitlements/license", 200, json!({"items": []}));
    let error = f.service.verify_for_launch(PROFILE_ID).await.err().unwrap();
    assert_eq!(error.code, ErrorCode::NoGameAccess);
    assert_eq!(f.state(), AccountState::NoAccess);
}

#[tokio::test]
async fn a_late_refresh_cannot_restore_a_signed_out_account() {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(FlakyStore::default());
    let f = fixture_with(Some(CLIENT_ID), Arc::clone(&store), Some(dir)).await;
    f.sign_in().await;
    let dir = f.into_dir();

    let f = fixture_with(Some(CLIENT_ID), Arc::clone(&store), Some(dir)).await;
    f.server
        .delays
        .lock()
        .unwrap()
        .insert("/token".into(), Duration::from_millis(400));
    f.route(
        "/token",
        200,
        json!({"token_type": "Bearer", "expires_in": 3600, "access_token": "late", "refresh_token": "late-refresh"}),
    );
    let service = Arc::clone(&f.service);
    let restore = tokio::spawn(async move { service.restore().await });
    tokio::time::sleep(Duration::from_millis(100)).await;
    f.service.sign_out(PROFILE_ID).await.unwrap();
    restore.await.unwrap();
    assert_eq!(f.state(), AccountState::SignedOut);
    assert!(store.get(&vault_key(PROFILE_ID)).unwrap().is_none());
    assert!(f.host.accounts_file().accounts.is_empty());
    assert!(f.service.verify_for_launch(PROFILE_ID).await.is_err());
}

#[tokio::test]
async fn unavailable_vault_switches_to_explicit_session_only_mode() {
    let store = Arc::new(FlakyStore::default());
    store.fail_put.store(true, Ordering::SeqCst);
    let f = fixture_with(Some(CLIENT_ID), Arc::clone(&store), None).await;
    let account = f.sign_in().await;
    assert_eq!(account.state, AccountState::VerifiedAccess);
    assert!(account.session_only);
    // Nothing persisted: no vault entry, no saved account record.
    assert!(store.get(&vault_key(PROFILE_ID)).unwrap().is_none());
    assert!(f.host.accounts_file().accounts.is_empty());
    assert!(f.service.verify_for_launch(PROFILE_ID).await.is_ok());
    f.service.sign_out(PROFILE_ID).await.unwrap();
    assert_eq!(f.state(), AccountState::SignedOut);
}

#[tokio::test]
async fn several_accounts_are_kept_verified_and_removed_independently() {
    const OTHER_ID: &str = "1f1e2d3c4b5a69788796a5b4c3d2e1f0";
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(FlakyStore::default());
    let f = fixture_with(Some(CLIENT_ID), Arc::clone(&store), Some(dir)).await;
    f.sign_in().await;
    // A second Microsoft account signs in through the browser.
    f.route(
        "/token",
        200,
        json!({"token_type": "Bearer", "expires_in": 3600, "access_token": "msa-b", "refresh_token": "refresh-b"}),
    );
    f.route(
        "/minecraft/profile",
        200,
        json!({"id": OTHER_ID, "name": "Alex_Test", "skins": []}),
    );
    f.sign_in().await;
    let status = f.host.app.auth_status();
    let names: Vec<_> = status
        .accounts
        .iter()
        .map(|a| a.profile_name.as_str())
        .collect();
    assert_eq!(names, ["Steve_Test", "Alex_Test"]);
    assert!(
        status
            .accounts
            .iter()
            .all(|a| a.state == AccountState::VerifiedAccess)
    );
    let file = f.host.accounts_file();
    assert_eq!(file.accounts.len(), 2);
    assert_eq!(
        file.default_profile,
        Some(ProfileRef::Microsoft {
            id: PROFILE_ID.into()
        }),
        "the first account became the default"
    );
    assert_eq!(
        store.get(&vault_key(OTHER_ID)).unwrap().unwrap().expose(),
        "refresh-b"
    );
    assert!(store.get(&vault_key(PROFILE_ID)).unwrap().is_some());

    // Each launch verifies the account it names.
    let alex = f.service.verify_for_launch(OTHER_ID).await.unwrap();
    assert_eq!(alex.profile.name, "Alex_Test");
    assert_eq!(
        f.service
            .verify_for_launch("unknown")
            .await
            .err()
            .map(|e| e.code),
        Some(ErrorCode::AuthReauthRequired)
    );

    // Removing the default account keeps the other one and clears the default.
    f.service.sign_out(PROFILE_ID).await.unwrap();
    assert!(store.get(&vault_key(PROFILE_ID)).unwrap().is_none());
    assert!(store.get(&vault_key(OTHER_ID)).unwrap().is_some());
    let file = f.host.accounts_file();
    assert_eq!(file.accounts.len(), 1);
    assert_eq!(file.default_profile, None);
    assert!(f.service.verify_for_launch(OTHER_ID).await.is_ok());
    assert!(f.service.verify_for_launch(PROFILE_ID).await.is_err());
    let dir = f.into_dir();

    // After a restart the remaining account is restored on its own.
    let f = fixture_with(Some(CLIENT_ID), Arc::clone(&store), Some(dir)).await;
    f.route(
        "/minecraft/profile",
        200,
        json!({"id": OTHER_ID, "name": "Alex_Test", "skins": []}),
    );
    f.service.restore().await;
    let status = f.host.app.auth_status();
    assert_eq!(status.accounts.len(), 1);
    assert_eq!(status.accounts[0].profile_name, "Alex_Test");
    assert_eq!(status.accounts[0].state, AccountState::VerifiedAccess);
}

#[tokio::test]
async fn the_single_account_of_an_earlier_version_is_restored() {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(FlakyStore::default());
    store
        .put(&vault_key(PROFILE_ID), &SecretString::new("refresh-1"))
        .unwrap();
    std::fs::write(
        dir.path().join("accounts.json"),
        json!({"schemaVersion": 1, "active": {"profileId": PROFILE_ID,
               "profileName": "Steve_Test", "vaultKey": vault_key(PROFILE_ID)},
               "signOutPending": false})
        .to_string(),
    )
    .unwrap();
    let f = fixture_with(Some(CLIENT_ID), Arc::clone(&store), Some(dir)).await;
    assert_eq!(f.state(), AccountState::Refreshing);
    f.service.restore().await;
    assert_eq!(f.state(), AccountState::VerifiedAccess);
    let saved = std::fs::read_to_string(f.root.accounts()).unwrap();
    assert!(
        saved.contains("\"accounts\"") && !saved.contains("\"active\""),
        "{saved}"
    );
}
