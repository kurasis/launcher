//! Account transactions (docs/spec/03 "Timing and refresh", "OAuth callback and
//! account concurrency details", "Sign-out transaction entry").
//!
//! - One login or refresh transaction at a time (`transaction`).
//! - Every async result is bound to an account generation; cancel and sign-out
//!   advance it, so a late callback or refresh can never restore a signed-out account.
//! - The generation check and every vault write happen under `commit`, the same lock
//!   sign-out holds while deleting credentials.
//! - Launch authorization is always re-verified with a fresh entitlement response.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use chrono::{DateTime, Utc};
use tokio::sync::watch;
use url::Url;

use super::client_id::{self, ClientId};
use super::contract;
use super::decision::state_for_error;
use super::flow::{self, MinecraftSession};
use super::http::AuthHttp;
use super::loopback::{Callback, LoopbackServer};
use super::pkce::{self, Pkce};
use super::protocol::Profile;
use super::vault::{MemoryStore, SecretStore};
use crate::error::{ErrorCode, LauncherError, LauncherResult};
use crate::secrets::SecretString;
use crate::state::dto::{AccountState, AccountSummary};
use crate::storage::models::{AccountRecord, AccountsFile};

/// What the auth service needs from application state. Implemented for `AppState`.
pub trait AccountHost: Send + Sync {
    fn account(&self) -> AccountSummary;
    fn set_account(&self, account: AccountSummary);
    fn accounts_file(&self) -> AccountsFile;
    fn save_accounts_file(&self, accounts: AccountsFile) -> LauncherResult<()>;
    /// True while a launch preflight, game or test run owns the account.
    fn launch_owns_account(&self) -> bool;
}

/// Opens the system browser. Production uses the opener plugin with an allowlist.
pub type BrowserOpener = Box<dyn Fn(&str) -> LauncherResult<()> + Send + Sync>;

/// Everything a launch needs from the account. Never serialized.
pub struct LaunchAuthorization {
    pub profile: Profile,
    pub access_token: SecretString,
    pub xuid: Option<String>,
    pub client_id: String,
}

struct Session {
    profile: Profile,
    vault_key: String,
    minecraft: MinecraftSession,
}

struct Attempt {
    operation_id: String,
    cancel: watch::Sender<bool>,
}

pub struct AuthService {
    http: AuthHttp,
    client_id: Result<Option<ClientId>, client_id::InvalidClientId>,
    host: Arc<dyn AccountHost>,
    opener: BrowserOpener,
    store: Mutex<Arc<dyn SecretStore>>,
    generation: AtomicU64,
    transaction: tokio::sync::Mutex<()>,
    commit: Mutex<()>,
    session: Mutex<Option<Session>>,
    attempt: Mutex<Option<Attempt>>,
    login_timeout: Duration,
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn vault_key(profile_id: &str) -> String {
    format!("msa-refresh:{profile_id}")
}

fn superseded() -> LauncherError {
    LauncherError::new(
        ErrorCode::AuthCancelled,
        "This sign-in was replaced by a newer account action.",
    )
}

impl AuthService {
    pub fn new(
        http: AuthHttp,
        client_id: Result<Option<ClientId>, client_id::InvalidClientId>,
        host: Arc<dyn AccountHost>,
        opener: BrowserOpener,
        store: Arc<dyn SecretStore>,
    ) -> Self {
        Self {
            http,
            client_id,
            host,
            opener,
            store: Mutex::new(store),
            generation: AtomicU64::new(0),
            transaction: tokio::sync::Mutex::new(()),
            commit: Mutex::new(()),
            session: Mutex::new(None),
            attempt: Mutex::new(None),
            login_timeout: Duration::from_secs(contract::LOGIN_TIMEOUT_SECONDS),
        }
    }

    #[cfg(test)]
    pub fn with_login_timeout(mut self, timeout: Duration) -> Self {
        self.login_timeout = timeout;
        self
    }

    fn store(&self) -> Arc<dyn SecretStore> {
        Arc::clone(&lock(&self.store))
    }

    fn client_id(&self) -> LauncherResult<ClientId> {
        match &self.client_id {
            Ok(Some(id)) => Ok(id.clone()),
            Ok(None) => Err(LauncherError::new(
                ErrorCode::AuthNotConfigured,
                "Microsoft sign-in is not configured in this build. The project owner must \
                 supply this launcher's own Microsoft application ID (see docs/AUTH-SETUP.md).",
            )),
            Err(_) => Err(LauncherError::new(
                ErrorCode::AuthNotConfigured,
                "The Microsoft application ID in this build is invalid.",
            )),
        }
    }

    fn publish(
        &self,
        state: AccountState,
        profile: Option<&Profile>,
        error: Option<LauncherError>,
        login_operation_id: Option<String>,
    ) {
        let session_only = !self.store().persistent();
        let fallback = self.host.account();
        let (profile_name, profile_id) = match profile {
            Some(p) => (Some(p.name.clone()), Some(p.id.clone())),
            None => (fallback.profile_name, fallback.profile_id),
        };
        let keep_identity = matches!(
            state,
            AccountState::VerifiedAccess
                | AccountState::Refreshing
                | AccountState::ServiceUnavailable
                | AccountState::ProtocolError
                | AccountState::NoAccess
                | AccountState::Restricted
        );
        self.host.set_account(AccountSummary {
            state,
            profile_name: if keep_identity { profile_name } else { None },
            profile_id: if keep_identity { profile_id } else { None },
            error,
            session_only,
            login_operation_id,
        });
    }

    fn publish_error(&self, error: &LauncherError) {
        self.publish(state_for_error(error.code), None, Some(error.clone()), None);
    }

    /// Starts a browser sign-in and returns its operation ID. A second call while an
    /// attempt is pending returns the same ID.
    pub async fn begin_login(self: &Arc<Self>) -> LauncherResult<String> {
        let client_id = self.client_id()?;
        if self.host.launch_owns_account() {
            return Err(LauncherError::new(
                ErrorCode::AppBusy,
                "The account cannot change while a game is starting or running.",
            ));
        }
        if let Some(attempt) = lock(&self.attempt).as_ref() {
            return Ok(attempt.operation_id.clone());
        }
        let server = LoopbackServer::bind().await?;
        let pkce = Pkce::generate()?;
        let state = pkce::new_state()?;
        let redirect_uri = server.redirect_uri();
        let mut url = Url::parse(&self.http.endpoints.authorize)
            .map_err(|_| LauncherError::internal("Invalid authorization endpoint."))?;
        url.query_pairs_mut()
            .append_pair("client_id", client_id.as_str())
            .append_pair("response_type", "code")
            .append_pair("redirect_uri", &redirect_uri)
            .append_pair("response_mode", "query")
            .append_pair("scope", contract::SCOPES)
            .append_pair("state", state.expose())
            .append_pair("code_challenge", &pkce.challenge)
            .append_pair("code_challenge_method", "S256")
            .append_pair("prompt", "select_account");

        let operation_id = uuid::Uuid::new_v4().to_string();
        let (cancel_tx, cancel_rx) = watch::channel(false);
        {
            let mut attempt = lock(&self.attempt);
            if let Some(existing) = attempt.as_ref() {
                return Ok(existing.operation_id.clone());
            }
            *attempt = Some(Attempt {
                operation_id: operation_id.clone(),
                cancel: cancel_tx,
            });
        }
        let generation = self.generation.load(Ordering::SeqCst);
        self.publish(
            AccountState::SigningIn,
            None,
            None,
            Some(operation_id.clone()),
        );
        if let Err(error) = (self.opener)(url.as_str()) {
            self.finish_attempt(&operation_id);
            self.publish_error(&error);
            return Err(error);
        }

        let this = Arc::clone(self);
        let op = operation_id.clone();
        // The loopback listener belongs to the current runtime; finish there.
        tokio::spawn(async move {
            let result = this
                .complete_login(
                    server,
                    pkce,
                    state,
                    redirect_uri,
                    cancel_rx,
                    client_id,
                    generation,
                )
                .await;
            this.finish_attempt(&op);
            if let Err(error) = result {
                // A superseded attempt must not overwrite the newer state.
                if this.generation.load(Ordering::SeqCst) == generation {
                    this.publish_error(&error);
                }
            }
        });
        Ok(operation_id)
    }

    #[allow(clippy::too_many_arguments)]
    async fn complete_login(
        &self,
        server: LoopbackServer,
        pkce: Pkce,
        state: SecretString,
        redirect_uri: String,
        cancel: watch::Receiver<bool>,
        client_id: ClientId,
        generation: u64,
    ) -> LauncherResult<()> {
        let callback = server.wait(&state, self.login_timeout, cancel).await?;
        let code = match callback {
            Callback::Code(code) => code,
            Callback::ProviderError(error) if error == "access_denied" => {
                return Err(LauncherError::new(
                    ErrorCode::AuthCancelled,
                    "Sign-in was declined in the browser.",
                ));
            }
            Callback::ProviderError(error) => {
                return Err(LauncherError::new(
                    ErrorCode::AuthForbidden,
                    "Microsoft did not complete the sign-in.",
                )
                .with_detail("oauthError", error));
            }
        };
        let _transaction = self.transaction.lock().await;
        if self.generation.load(Ordering::SeqCst) != generation {
            return Err(superseded());
        }
        let tokens = flow::exchange_code(
            &self.http,
            client_id.as_str(),
            &code,
            &redirect_uri,
            &pkce.verifier,
        )
        .await?;
        let Some(refresh_token) = tokens.refresh_token else {
            return Err(LauncherError::new(
                ErrorCode::AuthProtocolError,
                "Microsoft did not return a refresh credential.",
            ));
        };
        let minecraft = flow::minecraft_session(&self.http, &tokens.access_token).await?;
        let profile = flow::check_access(&self.http, &minecraft.access_token).await?;
        self.commit_session(generation, Some(refresh_token), profile, minecraft)
    }

    /// Stores the session if `generation` is still current. Holds `commit` across the
    /// generation check and the vault write.
    fn commit_session(
        &self,
        generation: u64,
        refresh_token: Option<SecretString>,
        profile: Profile,
        minecraft: MinecraftSession,
    ) -> LauncherResult<()> {
        let _commit = lock(&self.commit);
        if self.generation.load(Ordering::SeqCst) != generation {
            return Err(superseded());
        }
        let key = vault_key(&profile.id);
        let previous = self.host.accounts_file().active;
        if let Some(refresh_token) = &refresh_token {
            let store = self.store();
            if let Err(error) = store.put(&key, refresh_token) {
                if error.code != ErrorCode::SecretStoreUnavailable || !store.persistent() {
                    return Err(error);
                }
                // Explicit session-only mode: the secret stays in memory only.
                let memory: Arc<dyn SecretStore> = Arc::new(MemoryStore::default());
                memory.put(&key, refresh_token)?;
                *lock(&self.store) = memory;
            }
        }
        let persistent = self.store().persistent();
        // Replacing a different saved account removes its stored credential.
        if let Some(previous) = previous.as_ref().filter(|p| p.vault_key != key) {
            let _ = self.store().delete(&previous.vault_key);
        }
        self.host.save_accounts_file(AccountsFile {
            active: persistent.then(|| AccountRecord {
                profile_id: profile.id.clone(),
                profile_name: profile.name.clone(),
                vault_key: key.clone(),
            }),
            ..AccountsFile::default()
        })?;
        *lock(&self.session) = Some(Session {
            profile: profile.clone(),
            vault_key: key,
            minecraft,
        });
        self.publish(AccountState::VerifiedAccess, Some(&profile), None, None);
        Ok(())
    }

    fn finish_attempt(&self, operation_id: &str) {
        let mut attempt = lock(&self.attempt);
        if attempt
            .as_ref()
            .is_some_and(|a| a.operation_id == operation_id)
        {
            *attempt = None;
        }
    }

    /// Cancels the pending browser attempt. Late callbacks then find no listener.
    pub fn cancel_login(&self, operation_id: &str) -> LauncherResult<()> {
        let attempt = lock(&self.attempt).take();
        match attempt {
            Some(attempt) if attempt.operation_id == operation_id => {
                self.generation.fetch_add(1, Ordering::SeqCst);
                let _ = attempt.cancel.send(true);
                self.publish(
                    AccountState::SignedOut,
                    None,
                    Some(LauncherError::new(
                        ErrorCode::AuthCancelled,
                        "Sign-in was cancelled.",
                    )),
                    None,
                );
                Ok(())
            }
            other => {
                *lock(&self.attempt) = other;
                Err(LauncherError::new(
                    ErrorCode::OperationNotFound,
                    "There is no sign-in with that identifier.",
                ))
            }
        }
    }

    /// Restores a saved account at startup: finishes a pending sign-out first, then
    /// refreshes and re-verifies access. Network failures keep the account.
    pub async fn restore(&self) {
        let file = self.host.accounts_file();
        if file.sign_out_pending {
            let _ = self.complete_sign_out(file.active);
            return;
        }
        let Some(record) = file.active else {
            return;
        };
        if let Err(error) = self.client_id() {
            self.publish(AccountState::ReauthRequired, None, Some(error), None);
            return;
        }
        let _transaction = self.transaction.lock().await;
        let generation = self.generation.load(Ordering::SeqCst);
        let profile = Profile {
            id: record.profile_id.clone(),
            name: record.profile_name.clone(),
        };
        self.publish(AccountState::Refreshing, Some(&profile), None, None);
        if let Err(error) = self.refresh_locked(generation, &record.vault_key).await
            && self.generation.load(Ordering::SeqCst) == generation
        {
            self.publish(
                state_for_error(error.code),
                Some(&profile),
                Some(error),
                None,
            );
        }
    }

    /// Full refresh chain; the caller holds `transaction`.
    async fn refresh_locked(&self, generation: u64, vault_key: &str) -> LauncherResult<()> {
        let client_id = self.client_id()?;
        let refresh_token = self.store().get(vault_key)?.ok_or_else(|| {
            LauncherError::new(
                ErrorCode::AuthReauthRequired,
                "No saved sign-in was found. Sign in again.",
            )
        })?;
        let tokens = match flow::refresh(&self.http, client_id.as_str(), &refresh_token).await {
            Ok(tokens) => tokens,
            Err(error) if error.code == ErrorCode::AuthReauthRequired => {
                // Confirmed revocation or invalid grant: clear the saved login.
                self.forget_revoked(generation, vault_key);
                return Err(error);
            }
            Err(error) => return Err(error),
        };
        let minecraft = flow::minecraft_session(&self.http, &tokens.access_token).await?;
        let profile = flow::check_access(&self.http, &minecraft.access_token).await?;
        // Persist a rotated refresh token before the old one is discarded; when the
        // response does not rotate it, the existing vault entry stays.
        self.commit_session(generation, tokens.refresh_token, profile, minecraft)
    }

    fn forget_revoked(&self, generation: u64, vault_key: &str) {
        let _commit = lock(&self.commit);
        if self.generation.load(Ordering::SeqCst) != generation {
            return;
        }
        if self.store().delete(vault_key).is_ok() {
            let _ = self.host.save_accounts_file(AccountsFile::default());
        }
        *lock(&self.session) = None;
    }

    /// Re-verifies current access for one launch attempt (doc 03 "Timing and
    /// refresh"). Uses the cached Minecraft token only while it is valid with a skew
    /// margin, and always asks for fresh entitlements and profile.
    pub async fn verify_for_launch(&self) -> LauncherResult<LaunchAuthorization> {
        let client_id = self.client_id()?;
        let _transaction = self.transaction.lock().await;
        let generation = self.generation.load(Ordering::SeqCst);
        let vault_key = {
            let session = lock(&self.session);
            match session.as_ref() {
                Some(s) => s.vault_key.clone(),
                None => {
                    let error =
                        LauncherError::new(ErrorCode::AuthReauthRequired, "Sign in to play.");
                    return Err(error);
                }
            }
        };
        let token_valid = |expires_at: DateTime<Utc>| {
            expires_at - chrono::Duration::seconds(contract::EXPIRY_SKEW_SECONDS) > Utc::now()
        };
        let needs_refresh = lock(&self.session)
            .as_ref()
            .is_none_or(|s| !token_valid(s.minecraft.expires_at));
        if needs_refresh {
            self.refresh_locked(generation, &vault_key)
                .await
                .inspect_err(|e| self.publish_error(e))?;
        } else {
            let token = lock(&self.session)
                .as_ref()
                .map(|s| SecretString::new(s.minecraft.access_token.expose()))
                .ok_or_else(superseded)?;
            match flow::check_access(&self.http, &token).await {
                Ok(profile) => {
                    let _commit = lock(&self.commit);
                    if self.generation.load(Ordering::SeqCst) != generation {
                        return Err(superseded());
                    }
                    if let Some(session) = lock(&self.session).as_mut() {
                        session.profile = profile.clone();
                    }
                    self.publish(AccountState::VerifiedAccess, Some(&profile), None, None);
                }
                // An expired or revoked Minecraft token: refresh once, then retry.
                Err(error) if error.code == ErrorCode::AuthReauthRequired => {
                    self.refresh_locked(generation, &vault_key)
                        .await
                        .inspect_err(|e| self.publish_error(e))?;
                }
                Err(error) => {
                    self.publish_error(&error);
                    return Err(error);
                }
            }
        }
        let session = lock(&self.session);
        let session = session.as_ref().ok_or_else(superseded)?;
        Ok(LaunchAuthorization {
            profile: session.profile.clone(),
            access_token: SecretString::new(session.minecraft.access_token.expose()),
            xuid: session.minecraft.xuid.clone(),
            client_id: client_id.as_str().to_owned(),
        })
    }

    /// Local sign-out. Refused while a launch or game owns the account.
    pub async fn sign_out(&self) -> LauncherResult<()> {
        if self.host.launch_owns_account() {
            return Err(LauncherError::new(
                ErrorCode::AppBusy,
                "Sign-out is not possible while a game is starting or running.",
            ));
        }
        // Invalidate pending work first so no late result can repopulate the vault.
        self.generation.fetch_add(1, Ordering::SeqCst);
        if let Some(attempt) = lock(&self.attempt).take() {
            let _ = attempt.cancel.send(true);
        }
        let record = self.host.accounts_file().active.or_else(|| {
            lock(&self.session).as_ref().map(|s| AccountRecord {
                profile_id: s.profile.id.clone(),
                profile_name: s.profile.name.clone(),
                vault_key: s.vault_key.clone(),
            })
        });
        self.complete_sign_out(record)
    }

    fn complete_sign_out(&self, record: Option<AccountRecord>) -> LauncherResult<()> {
        let _commit = lock(&self.commit);
        *lock(&self.session) = None;
        let Some(record) = record else {
            let _ = self.host.save_accounts_file(AccountsFile::default());
            self.publish(AccountState::SignedOut, None, None, None);
            return Ok(());
        };
        match self.store().delete(&record.vault_key) {
            Ok(()) => {
                self.host.save_accounts_file(AccountsFile::default())?;
                self.publish(AccountState::SignedOut, None, None, None);
                Ok(())
            }
            Err(error) => {
                // Keep a non-secret marker so restart retries deletion and never
                // restores the credential.
                let _ = self.host.save_accounts_file(AccountsFile {
                    active: Some(record),
                    sign_out_pending: true,
                    ..AccountsFile::default()
                });
                self.publish(
                    AccountState::SignOutPending,
                    None,
                    Some(error.clone()),
                    None,
                );
                Err(error)
            }
        }
    }
}

#[cfg(test)]
mod tests;
