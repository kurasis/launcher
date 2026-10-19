//! The launcher account at oauth.lapi.sh (D-064).
//!
//! Signing in and registering happen on oauth.lapi.sh's own pages, in a
//! window of the launcher with no address bar, no IPC and no other site: the
//! password never passes through the launcher's interface. The window runs an
//! OAuth authorization code flow with PKCE; when the pages finish, they go to
//! `/app/done?code=…&state=…`, which the window stops and hands to
//! [`LauncherAccount::finish`]. The refresh token then lives in the OS
//! credential vault, the access token (10 minutes) only in memory; who the
//! account is (ID, nick) is kept in `accounts.json`.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, Runtime, WebviewUrl, WebviewWindowBuilder};

use crate::auth::pkce::{Pkce, new_state, secrets_equal};
use crate::auth::vault::SecretStore;
use crate::secrets::SecretString;
use crate::state::AppState;
use crate::storage::models::{LauncherAccountRecord, ProfileRef};
use crate::sync::lock;

const BASE: &str = "https://oauth.lapi.sh";
/// The window's label; it gets no capability, so its pages cannot call the
/// launcher.
pub const WINDOW: &str = "launcher-account";
const VAULT_KEY: &str = "launcher-account-refresh";
const CLIENT_ID: &str = "launcher";
const MAX_RESPONSE_BYTES: usize = 64 * 1024;

/// oauth.lapi.sh, or the server a test build was made for
/// (`LAUNCHER_ACCOUNT_URL` at build time; release builds never set it).
pub fn base() -> &'static str {
    #[cfg(test)]
    if let Some(local) = tests::LOCAL_SERVER.with(std::cell::Cell::get) {
        return local;
    }
    option_env!("LAUNCHER_ACCOUNT_URL")
        .filter(|u| !u.trim().is_empty())
        .unwrap_or(BASE)
}

fn done_url() -> String {
    format!("{}/app/done", base())
}

/// What went wrong, for the interface to say in the player's language.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AccountProblem {
    /// oauth.lapi.sh could not be reached.
    Network,
    /// The sign-in ended or was refused: sign in again.
    SignInAgain,
    NickTaken,
    NickReserved,
    NickInvalid,
    TooMany,
    /// The system credential store did not keep the sign-in.
    Vault,
    Server,
}

/// What the interface shows (part of the account summary).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LauncherAccountView {
    /// Signed in on this computer.
    pub id: Option<String>,
    pub nick: Option<String>,
    pub email_verified: bool,
    /// The sign-in window is open.
    pub signing_in: bool,
    /// The saved sign-in no longer works.
    pub needs_sign_in: bool,
    pub problem: Option<AccountProblem>,
}

/// What lives only in memory, next to `accounts.json`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Status {
    pub signing_in: bool,
    pub needs_sign_in: bool,
    pub problem: Option<AccountProblem>,
}

impl LauncherAccountView {
    pub fn new(record: Option<&LauncherAccountRecord>, status: &Status) -> Self {
        Self {
            id: record.map(|r| r.id.clone()),
            nick: record.map(|r| r.nick.clone()),
            email_verified: record.is_some_and(|r| r.email_verified),
            signing_in: status.signing_in,
            needs_sign_in: status.needs_sign_in && record.is_some(),
            problem: status.problem,
        }
    }
}

struct Pending {
    state: SecretString,
    verifier: SecretString,
}

struct Access {
    token: SecretString,
    expires: std::time::Instant,
}

#[derive(Deserialize)]
struct TokenAnswer {
    access_token: String,
    refresh_token: String,
    expires_in: u64,
    account: AccountAnswer,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AccountAnswer {
    id: String,
    nick: String,
    #[serde(default)]
    email_verified: bool,
}

pub struct LauncherAccount {
    http: reqwest::Client,
    vault: Arc<dyn SecretStore>,
    /// Kept here when the vault refuses it: signed in for this session only.
    memory: Mutex<Option<SecretString>>,
    pending: Mutex<Option<Pending>>,
    /// The access token; the lock also keeps two refreshes from racing (a
    /// refresh token works once, and a second use ends the session).
    access: tokio::sync::Mutex<Option<Access>>,
}

/// An answer: its status and JSON body (bounded).
struct Answer {
    status: u16,
    body: serde_json::Value,
}

impl Answer {
    fn error(&self) -> &str {
        self.body
            .get("error")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
    }

    fn problem(&self) -> AccountProblem {
        match (self.status, self.error()) {
            (400 | 401, _) => AccountProblem::SignInAgain,
            (409, _) => AccountProblem::NickTaken,
            (422, "nick_reserved") => AccountProblem::NickReserved,
            (422, e) if e.starts_with("nick_") => AccountProblem::NickInvalid,
            (429, _) => AccountProblem::TooMany,
            _ => AccountProblem::Server,
        }
    }
}

/// The client for our account and friends servers: no redirects (a token is
/// never forwarded to another host), HTTPS unless the server is a local one.
pub(crate) fn our_client(app_version: &str, server: &str) -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .https_only(server.starts_with("https://"))
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(30))
        .user_agent(format!("Launcher/{app_version}"))
        .build()
        .map_err(|_| "The HTTPS client could not be created.".to_owned())
}

/// Why an answer's body was not read.
pub(crate) enum BodyError {
    Network,
    /// More than the caller takes: it stops reading there.
    TooLarge,
}

/// An answer's body, read a piece at a time and given up past `max` bytes.
pub(crate) async fn body_within(
    mut response: reqwest::Response,
    max: usize,
) -> Result<Vec<u8>, BodyError> {
    if response
        .content_length()
        .is_some_and(|length| length > max as u64)
    {
        return Err(BodyError::TooLarge);
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| BodyError::Network)? {
        if body.len() + chunk.len() > max {
            return Err(BodyError::TooLarge);
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

impl LauncherAccount {
    pub fn new(app_version: &str, vault: Arc<dyn SecretStore>) -> Result<Self, String> {
        let http = our_client(app_version, base())?;
        Ok(Self {
            http,
            vault,
            memory: Mutex::new(None),
            pending: Mutex::new(None),
            access: tokio::sync::Mutex::new(None),
        })
    }

    async fn send(&self, request: reqwest::RequestBuilder) -> Result<Answer, AccountProblem> {
        let response = request
            .header(reqwest::header::ACCEPT, "application/json")
            .send()
            .await
            .map_err(|_| AccountProblem::Network)?;
        let status = response.status().as_u16();
        let bytes = body_within(response, MAX_RESPONSE_BYTES)
            .await
            .map_err(|e| match e {
                BodyError::Network => AccountProblem::Network,
                BodyError::TooLarge => AccountProblem::Server,
            })?;
        Ok(Answer {
            status,
            body: serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
        })
    }

    fn refresh_token(&self) -> Option<SecretString> {
        if let Some(token) = lock(&self.memory).as_ref() {
            return Some(SecretString::new(token.expose()));
        }
        self.vault.get(VAULT_KEY).ok().flatten()
    }

    fn keep_refresh_token(&self, state: &AppState, token: &str) {
        match self.vault.put(VAULT_KEY, &SecretString::new(token)) {
            Ok(()) => *lock(&self.memory) = None,
            Err(_) => {
                state.console().warn(
                    "Launcher account: the system credential store did not keep the sign-in; it lasts until the launcher closes",
                );
                *lock(&self.memory) = Some(SecretString::new(token));
            }
        }
    }

    fn forget_tokens(&self) {
        *lock(&self.memory) = None;
        let _ = self.vault.delete(VAULT_KEY);
    }

    /// The address the window opens.
    fn authorize_url(&self, screen: &str, lang: &str) -> Result<url::Url, AccountProblem> {
        let pkce = Pkce::generate().map_err(|_| AccountProblem::Server)?;
        let state = new_state().map_err(|_| AccountProblem::Server)?;
        let mut url = url::Url::parse(&format!("{}/authorize", base()))
            .map_err(|_| AccountProblem::Server)?;
        url.query_pairs_mut()
            .append_pair("client_id", CLIENT_ID)
            .append_pair("redirect_uri", &done_url())
            .append_pair("response_type", "code")
            .append_pair("code_challenge", &pkce.challenge)
            .append_pair("code_challenge_method", "S256")
            .append_pair("state", state.expose())
            .append_pair("device", &device())
            .append_pair("lang", lang)
            .append_pair(
                "screen",
                if screen == "register" {
                    "register"
                } else {
                    "login"
                },
            );
        *lock(&self.pending) = Some(Pending {
            state,
            verifier: pkce.verifier,
        });
        Ok(url)
    }

    /// Opens (or brings forward) the sign-in window on `screen`
    /// (`login` or `register`).
    pub fn open<R: Runtime>(self: &Arc<Self>, app: &AppHandle<R>, screen: &str) {
        let state = app.state::<AppState>();
        if let Some(window) = app.get_webview_window(WINDOW) {
            let _ = window.set_focus();
            return;
        }
        let lang = state.settings().language.as_str().to_owned();
        let url = match self.authorize_url(screen, &lang) {
            Ok(url) => url,
            Err(problem) => {
                state.set_launcher_account_status(|s| s.problem = Some(problem));
                return;
            }
        };
        let account = Arc::clone(self);
        let handle = app.clone();
        let built = WebviewWindowBuilder::new(app, WINDOW, WebviewUrl::External(url))
            .title(if screen == "register" {
                "Launcher account: create"
            } else {
                "Launcher account: sign in"
            })
            .inner_size(460.0, 720.0)
            .min_inner_size(360.0, 520.0)
            // Nothing of this window stays on the computer after it closes.
            .incognito(true)
            .disable_drag_drop_handler()
            .on_navigation(move |url| {
                if let Some((code, returned_state)) = finished(url) {
                    let account = Arc::clone(&account);
                    let handle = handle.clone();
                    tauri::async_runtime::spawn(async move {
                        if let Some(window) = handle.get_webview_window(WINDOW) {
                            let _ = window.close();
                        }
                        account.finish(&handle, &code, &returned_state).await;
                    });
                    return false;
                }
                let ok = allowed(url);
                if !ok {
                    // Scheme and host only: an address may carry a code.
                    eprintln!(
                        "launcher account window: refused {}://{}",
                        url.scheme(),
                        url.host_str().unwrap_or_default()
                    );
                }
                ok
            })
            // Links that would open another window open nothing.
            .on_new_window(|_, _| tauri::webview::NewWindowResponse::Deny)
            .build();
        match built {
            Ok(window) => {
                let handle = app.clone();
                window.on_window_event(move |event| {
                    if matches!(event, tauri::WindowEvent::Destroyed) {
                        handle
                            .state::<AppState>()
                            .set_launcher_account_status(|s| s.signing_in = false);
                    }
                });
                state.set_launcher_account_status(|s| {
                    s.signing_in = true;
                    s.problem = None;
                });
            }
            Err(e) => {
                state.console().warn(format!(
                    "Launcher account: the sign-in window did not open: {e}"
                ));
                state.set_launcher_account_status(|s| s.problem = Some(AccountProblem::Server));
            }
        }
    }

    /// The pages finished: trades the code for tokens.
    pub async fn finish<R: Runtime>(&self, app: &AppHandle<R>, code: &str, returned: &str) {
        let state = app.state::<AppState>();
        let pending = lock(&self.pending).take();
        let Some(pending) = pending.filter(|p| secrets_equal(p.state.expose(), returned)) else {
            state
                .console()
                .warn("Launcher account: a sign-in answer did not match; ignored");
            return;
        };
        let answer = self
            .send(self.http.post(format!("{}/v1/token", base())).form(&[
                ("grant_type", "authorization_code"),
                ("code", code),
                ("code_verifier", pending.verifier.expose()),
                ("client_id", CLIENT_ID),
                ("redirect_uri", &done_url()),
            ]))
            .await;
        match answer {
            Ok(answer) if answer.status == 200 => match serde_json::from_value(answer.body) {
                Ok(tokens) => {
                    self.signed_in(&state, tokens).await;
                    // Friends connect at once.
                    if let Some(friends) = app.try_state::<Arc<crate::friends::Friends>>() {
                        friends.wake();
                    }
                }
                Err(_) => {
                    state.set_launcher_account_status(|s| s.problem = Some(AccountProblem::Server))
                }
            },
            Ok(answer) => {
                let problem = answer.problem();
                state.set_launcher_account_status(|s| s.problem = Some(problem));
            }
            Err(problem) => state.set_launcher_account_status(|s| s.problem = Some(problem)),
        }
    }

    async fn signed_in(&self, state: &AppState, tokens: TokenAnswer) {
        self.keep_refresh_token(state, &tokens.refresh_token);
        *self.access.lock().await = Some(Access {
            token: SecretString::new(tokens.access_token),
            expires: std::time::Instant::now() + Duration::from_secs(tokens.expires_in.min(3600)),
        });
        let record = LauncherAccountRecord {
            id: tokens.account.id,
            nick: tokens.account.nick,
            email_verified: tokens.account.email_verified,
        };
        state
            .console()
            .info(format!("Launcher account: signed in as {}", record.nick));
        let saved = state.update_accounts_file(Box::new(move |file| {
            // Another account before: installations that played as it play
            // as the default now.
            if let Some(old) = &file.launcher_account
                && old.id != record.id
            {
                file.remove(&ProfileRef::Launcher { id: old.id.clone() });
            }
            file.launcher_account = Some(record);
        }));
        state.set_launcher_account_status(|s| {
            s.needs_sign_in = false;
            s.problem = saved.is_err().then_some(AccountProblem::Server);
        });
    }

    /// A valid access token, refreshed when it is about to end.
    pub async fn access_token(&self, state: &AppState) -> Result<SecretString, AccountProblem> {
        let mut access = self.access.lock().await;
        if let Some(current) = access.as_ref()
            && current.expires > std::time::Instant::now() + Duration::from_secs(30)
        {
            return Ok(SecretString::new(current.token.expose()));
        }
        let Some(refresh) = self.refresh_token() else {
            // Nothing kept (the vault refused it, or it lived in memory only):
            // the account stays listed and offers to sign in again.
            state.set_launcher_account_status(|s| s.needs_sign_in = true);
            return Err(AccountProblem::SignInAgain);
        };
        let answer = self
            .send(self.http.post(format!("{}/v1/token", base())).form(&[
                ("grant_type", "refresh_token"),
                ("refresh_token", refresh.expose()),
                ("client_id", CLIENT_ID),
            ]))
            .await?;
        if answer.status != 200 {
            let problem = answer.problem();
            if problem == AccountProblem::SignInAgain {
                self.forget_tokens();
                state.set_launcher_account_status(|s| s.needs_sign_in = true);
            }
            return Err(problem);
        }
        let tokens: TokenAnswer =
            serde_json::from_value(answer.body).map_err(|_| AccountProblem::Server)?;
        self.keep_refresh_token(state, &tokens.refresh_token);
        let token = SecretString::new(tokens.access_token);
        *access = Some(Access {
            token: SecretString::new(token.expose()),
            expires: std::time::Instant::now() + Duration::from_secs(tokens.expires_in.min(3600)),
        });
        drop(access);
        self.remember(state, &tokens.account);
        Ok(token)
    }

    /// Keeps the nick and confirmation the server reports.
    fn remember(&self, state: &AppState, account: &AccountAnswer) {
        let (id, nick, verified) = (
            account.id.clone(),
            account.nick.clone(),
            account.email_verified,
        );
        let _ = state.update_accounts_file(Box::new(move |file| {
            if let Some(record) = file.launcher_account.as_mut()
                && record.id == id
            {
                record.nick = nick;
                record.email_verified = verified;
            }
        }));
    }

    /// At start: checks that the saved sign-in still works and refreshes the
    /// nick (it may have changed on another computer).
    pub async fn check(&self, state: &AppState) {
        if state.accounts_file().launcher_account.is_none() {
            return;
        }
        match self.access_token(state).await {
            Ok(_) | Err(AccountProblem::Network) => {}
            Err(problem) => state.set_launcher_account_status(|s| s.problem = Some(problem)),
        }
    }

    pub async fn set_nick(&self, state: &AppState, nick: &str) -> Result<(), AccountProblem> {
        let token = self.access_token(state).await?;
        let answer = self
            .send(
                self.http
                    .post(format!("{}/v1/me/nick", base()))
                    .bearer_auth(token.expose())
                    .json(&serde_json::json!({ "nick": nick })),
            )
            .await?;
        if answer.status != 200 {
            return Err(answer.problem());
        }
        if let Some(account) = answer.body.get("account")
            && let Ok(account) = serde_json::from_value::<AccountAnswer>(account.clone())
        {
            state.console().info(format!(
                "Launcher account: the nick is now {}",
                account.nick
            ));
            self.remember(state, &account);
        }
        if let Some(fresh) = answer.body.get("access_token").and_then(|t| t.as_str()) {
            *self.access.lock().await = Some(Access {
                token: SecretString::new(fresh),
                expires: std::time::Instant::now() + Duration::from_secs(9 * 60),
            });
        }
        Ok(())
    }

    pub async fn resend_confirmation(&self, state: &AppState) -> Result<(), AccountProblem> {
        let token = self.access_token(state).await?;
        let answer = self
            .send(
                self.http
                    .post(format!("{}/v1/me/verify-email", base()))
                    .bearer_auth(token.expose()),
            )
            .await?;
        if answer.status == 204 {
            Ok(())
        } else {
            Err(answer.problem())
        }
    }

    /// Signs out here: ends the session on the server (when it answers) and
    /// forgets the tokens and the account.
    pub async fn sign_out(&self, state: &AppState) {
        // Held throughout: a refresh in flight finishes first, so the token it
        // keeps is the one revoked and forgotten here, not one left behind.
        let mut access = self.access.lock().await;
        if let Some(refresh) = self.refresh_token() {
            let _ = self
                .send(
                    self.http
                        .post(format!("{}/v1/revoke", base()))
                        .form(&[("token", refresh.expose())]),
                )
                .await;
        }
        self.forget_tokens();
        *access = None;
        let _ = state.update_accounts_file(Box::new(|file| {
            if let Some(old) = file.launcher_account.clone() {
                file.remove(&ProfileRef::Launcher { id: old.id });
            }
        }));
        state.set_launcher_account_status(|s| *s = Status::default());
        state.console().info("Launcher account: signed out");
    }
}

/// `/app/done?code=…&state=…` on our server: the pages finished.
fn finished(url: &url::Url) -> Option<(String, String)> {
    let done = url::Url::parse(&done_url()).ok()?;
    if url.scheme() != done.scheme()
        || url.host_str() != done.host_str()
        || url.port_or_known_default() != done.port_or_known_default()
        || url.path() != done.path()
    {
        return None;
    }
    let mut code = None;
    let mut state = None;
    for (key, value) in url.query_pairs() {
        match key.as_ref() {
            "code" if value.len() <= 128 => code = Some(value.into_owned()),
            "state" if value.len() <= 128 => state = Some(value.into_owned()),
            _ => {}
        }
    }
    Some((code?, state?))
}

/// Where the window may go: our server's pages and Cloudflare Turnstile.
fn allowed(url: &url::Url) -> bool {
    let Ok(ours) = url::Url::parse(base()) else {
        return false;
    };
    let same_server = url.scheme() == ours.scheme()
        && url.host_str() == ours.host_str()
        && url.port_or_known_default() == ours.port_or_known_default();
    let turnstile = url.scheme() == "https" && url.host_str() == Some("challenges.cloudflare.com");
    // `about:blank` and `about:srcdoc`: frames Turnstile makes, nothing loaded.
    same_server || turnstile || url.scheme() == "about"
}

/// What the sessions list on the server calls this computer.
fn device() -> String {
    let os = match std::env::consts::OS {
        "windows" => "Windows",
        "macos" => "macOS",
        "linux" => "Linux",
        other => other,
    };
    format!("{os} ({})", std::env::consts::ARCH)
}

#[cfg(test)]
mod tests {
    use super::*;

    thread_local! {
        /// A local account server for one test thread (others keep the real
        /// address).
        pub static LOCAL_SERVER: std::cell::Cell<Option<&'static str>> =
            const { std::cell::Cell::new(None) };
    }

    /// Answers every request with `{}` and keeps the request bodies.
    async fn local_server() -> (String, Arc<Mutex<Vec<String>>>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = format!("http://{}", listener.local_addr().unwrap());
        let bodies = Arc::new(Mutex::new(Vec::new()));
        let kept = Arc::clone(&bodies);
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                let kept = Arc::clone(&kept);
                tokio::spawn(async move {
                    let mut request = Vec::new();
                    let mut buffer = [0u8; 4096];
                    loop {
                        let n = socket.read(&mut buffer).await.unwrap_or(0);
                        if n == 0 {
                            return;
                        }
                        request.extend_from_slice(&buffer[..n]);
                        let text = String::from_utf8_lossy(&request).into_owned();
                        if let Some((head, body)) = text.split_once("\r\n\r\n") {
                            let length = head
                                .lines()
                                .find_map(|l| {
                                    l.to_ascii_lowercase()
                                        .strip_prefix("content-length:")
                                        .map(|v| v.trim().parse::<usize>().unwrap_or(0))
                                })
                                .unwrap_or(0);
                            if body.len() >= length {
                                lock(&kept).push(body.to_owned());
                                break;
                            }
                        }
                    }
                    let _ = socket
                        .write_all(b"HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: 2\r\nconnection: close\r\n\r\n{}")
                        .await;
                });
            }
        });
        (address, bodies)
    }

    fn url(text: &str) -> url::Url {
        url::Url::parse(text).unwrap()
    }

    #[test]
    fn only_our_finish_address_ends_the_window() {
        assert_eq!(
            finished(&url("https://oauth.lapi.sh/app/done?code=abc&state=xyz")),
            Some(("abc".into(), "xyz".into()))
        );
        for other in [
            "https://oauth.lapi.sh/app/done?code=abc",
            "https://evil.example/app/done?code=abc&state=xyz",
            "https://oauth.lapi.sh.evil.example/app/done?code=a&state=b",
            "http://oauth.lapi.sh/app/done?code=a&state=b",
            "https://oauth.lapi.sh:8443/app/done?code=a&state=b",
            "https://oauth.lapi.sh/app/done/x?code=a&state=b",
            "https://oauth.lapi.sh/login?code=a&state=b",
        ] {
            assert_eq!(finished(&url(other)), None, "{other}");
        }
    }

    #[test]
    fn the_window_goes_nowhere_else() {
        for ok in [
            "https://oauth.lapi.sh/authorize?x=1",
            "https://oauth.lapi.sh/reset",
            "https://challenges.cloudflare.com/cdn-cgi/challenge-platform/x",
            "about:blank",
            "about:srcdoc",
        ] {
            assert!(allowed(&url(ok)), "{ok}");
        }
        for bad in [
            "https://evil.example/",
            "http://oauth.lapi.sh/authorize",
            "https://oauth.lapi.sh.evil.example/",
            "https://lapi.sh/",
            "file:///etc/passwd",
            "javascript:alert(1)",
            "data:text/html,<script>alert(1)</script>",
            "tauri://localhost/index.html",
        ] {
            assert!(!allowed(&url(bad)), "{bad}");
        }
    }

    #[test]
    fn answers_say_what_to_do() {
        let answer = |status, error: &str| Answer {
            status,
            body: serde_json::json!({ "error": error }),
        };
        assert_eq!(
            answer(400, "invalid_grant").problem(),
            AccountProblem::SignInAgain
        );
        assert_eq!(
            answer(409, "nick_taken").problem(),
            AccountProblem::NickTaken
        );
        assert_eq!(
            answer(422, "nick_length").problem(),
            AccountProblem::NickInvalid
        );
        assert_eq!(
            answer(422, "nick_reserved").problem(),
            AccountProblem::NickReserved
        );
        assert_eq!(answer(429, "too_many").problem(), AccountProblem::TooMany);
        assert_eq!(answer(500, "").problem(), AccountProblem::Server);
        let view = LauncherAccountView::new(
            None,
            &Status {
                needs_sign_in: true,
                ..Status::default()
            },
        );
        assert!(!view.needs_sign_in, "nothing to sign in again to");
    }

    /// A refresh in flight when the player signs out keeps a new token; the
    /// sign-out waits for it and revokes and forgets that one.
    #[tokio::test]
    async fn a_refresh_during_sign_out_leaves_nothing_behind() {
        let (address, bodies) = local_server().await;
        LOCAL_SERVER.with(|s| s.set(Some(Box::leak(address.into_boxed_str()))));
        let dir = tempfile::tempdir().unwrap();
        let state = AppState::initialize(
            crate::storage::paths::DataRoot::new(dir.path().to_path_buf()),
            "test",
            crate::validation::MemoryPolicy::from_physical_bytes(16 << 30),
            Box::new(crate::state::NoopSink),
        );
        let vault = Arc::new(crate::auth::vault::MemoryStore::default());
        let account = LauncherAccount::new("test", vault.clone()).unwrap();
        account.keep_refresh_token(&state, "old-refresh-token");
        let refresh = async {
            let held = account.access.lock().await;
            tokio::time::sleep(Duration::from_millis(100)).await;
            account.keep_refresh_token(&state, "new-refresh-token");
            drop(held);
        };
        let sign_out = async {
            tokio::time::sleep(Duration::from_millis(20)).await;
            account.sign_out(&state).await;
        };
        tokio::join!(refresh, sign_out);
        LOCAL_SERVER.with(|s| s.set(None));
        assert!(account.refresh_token().is_none(), "nothing kept");
        assert!(vault.get(VAULT_KEY).unwrap().is_none());
        let revoked = lock(&bodies).join(" ");
        assert!(revoked.contains("new-refresh-token"), "{revoked}");
    }

    /// A sign-in kept only in memory is gone after a restart: the account
    /// stays listed and offers to sign in again.
    #[tokio::test]
    async fn a_lost_sign_in_asks_to_sign_in_again() {
        let dir = tempfile::tempdir().unwrap();
        let state = AppState::initialize(
            crate::storage::paths::DataRoot::new(dir.path().to_path_buf()),
            "test",
            crate::validation::MemoryPolicy::from_physical_bytes(16 << 30),
            Box::new(crate::state::NoopSink),
        );
        state
            .update_accounts_file(Box::new(|file| {
                file.launcher_account = Some(LauncherAccountRecord {
                    id: "11111111-2222-4333-8444-555555555555".into(),
                    nick: "Steve_2".into(),
                    email_verified: false,
                });
            }))
            .unwrap();
        let account =
            LauncherAccount::new("test", Arc::new(crate::auth::vault::MemoryStore::default()))
                .unwrap();
        assert_eq!(
            account.access_token(&state).await.err(),
            Some(AccountProblem::SignInAgain)
        );
        let view = state.snapshot().account.launcher_account;
        assert_eq!(view.nick.as_deref(), Some("Steve_2"));
        assert!(view.needs_sign_in);
    }
}
