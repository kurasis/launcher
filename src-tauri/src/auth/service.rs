//! Account transactions (docs/spec/03 "Timing and refresh", "OAuth callback and
//! account concurrency details", "Sign-out transaction entry"), for any number of
//! Microsoft accounts (D-045).
//!
//! - One login or refresh transaction at a time (`transaction`).
//! - Every async result is bound to a generation: a sign-in to the login
//!   generation (advanced by cancel), a refresh to its account's generation
//!   (advanced by that account's sign-out), so a late callback or refresh can never
//!   restore a signed-out account.
//! - The generation check and every vault write happen under `commit`, the same lock
//!   sign-out holds while deleting credentials.
//! - Launch authorization is always re-verified with a fresh entitlement response.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
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
use crate::state::dto::{AccountState, AuthStatus, MicrosoftAccount, PendingSignOut};
use crate::storage::models::{AccountRecord, AccountsFile, ProfileRef};
use crate::sync::lock;

/// What the auth service needs from application state. Implemented for `AppState`.
pub trait AccountHost: Send + Sync {
    fn auth_status(&self) -> AuthStatus;
    fn set_auth_status(&self, status: AuthStatus);
    fn accounts_file(&self) -> AccountsFile;
    /// Changes `accounts.json` in one step under the state lock.
    fn update_accounts_file(
        &self,
        change: Box<dyn FnOnce(&mut AccountsFile) + Send + '_>,
    ) -> LauncherResult<()>;
    /// True while a launch preflight, game or test run owns the accounts.
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
    /// Started without the online check because the services could not be
    /// reached (D-052).
    pub offline: bool,
}

/// The game's access token when an account plays offline without a token from
/// this run; Minecraft accepts any value for single-player.
const OFFLINE_TOKEN: &str = "0";

struct Session {
    profile: Profile,
    vault_key: String,
    minecraft: MinecraftSession,
}

struct Attempt {
    operation_id: String,
    cancel: watch::Sender<bool>,
}

/// Which work a result belongs to; it commits only while still current.
enum Ticket {
    Login(u64),
    Account(String, u64),
}

pub struct AuthService {
    http: AuthHttp,
    client_id: Result<Option<ClientId>, client_id::InvalidClientId>,
    host: Arc<dyn AccountHost>,
    opener: BrowserOpener,
    store: Mutex<Arc<dyn SecretStore>>,
    login_generation: AtomicU64,
    generations: Mutex<HashMap<String, u64>>,
    transaction: tokio::sync::Mutex<()>,
    commit: Mutex<()>,
    sessions: Mutex<HashMap<String, Session>>,
    attempt: Mutex<Option<Attempt>>,
    status: Mutex<AuthStatus>,
    login_timeout: Duration,
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

fn sign_in_to_play() -> LauncherError {
    LauncherError::new(ErrorCode::AuthReauthRequired, "Sign in to play.")
}

impl AuthService {
    pub fn new(
        http: AuthHttp,
        client_id: Result<Option<ClientId>, client_id::InvalidClientId>,
        host: Arc<dyn AccountHost>,
        opener: BrowserOpener,
        store: Arc<dyn SecretStore>,
    ) -> Self {
        let status = host.auth_status();
        Self {
            http,
            client_id,
            host,
            opener,
            store: Mutex::new(store),
            login_generation: AtomicU64::new(0),
            generations: Mutex::default(),
            transaction: tokio::sync::Mutex::new(()),
            commit: Mutex::new(()),
            sessions: Mutex::default(),
            attempt: Mutex::new(None),
            status: Mutex::new(status),
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

    fn generation(&self, profile_id: &str) -> u64 {
        lock(&self.generations)
            .get(profile_id)
            .copied()
            .unwrap_or(0)
    }

    fn current(&self, ticket: &Ticket) -> bool {
        match ticket {
            Ticket::Login(g) => self.login_generation.load(Ordering::SeqCst) == *g,
            Ticket::Account(id, g) => self.generation(id) == *g,
        }
    }

    /// Changes the reported status and hands it to the host, in order.
    fn update_status(&self, change: impl FnOnce(&mut AuthStatus)) {
        let mut status = lock(&self.status);
        change(&mut status);
        status.session_only = !self.store().persistent();
        self.host.set_auth_status(status.clone());
    }

    fn publish_account(
        &self,
        profile: &Profile,
        state: AccountState,
        error: Option<LauncherError>,
    ) {
        self.update_status(|status| {
            status.upsert(MicrosoftAccount {
                profile_id: profile.id.clone(),
                profile_name: profile.name.clone(),
                state,
                error,
            });
        });
    }

    /// Reports an error for a known account, keeping its name.
    fn publish_account_error(&self, profile_id: &str, error: &LauncherError) {
        self.update_status(|status| {
            if let Some(account) = status
                .accounts
                .iter_mut()
                .find(|a| a.profile_id == profile_id)
            {
                account.state = state_for_error(error.code);
                account.error = Some(error.clone());
            }
        });
    }

    fn publish_login(&self, operation_id: Option<String>, error: Option<LauncherError>) {
        self.update_status(|status| {
            status.login_state = match (&operation_id, &error) {
                (Some(_), _) => AccountState::SigningIn,
                (None, Some(error)) => state_for_error(error.code),
                (None, None) => AccountState::SignedOut,
            };
            status.login_operation_id = operation_id;
            status.login_error = error;
        });
    }

    /// Starts a browser sign-in that adds an account (or renews a saved one) and
    /// returns its operation ID. A second call while an attempt is pending returns
    /// the same ID.
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
        let generation = self.login_generation.load(Ordering::SeqCst);
        self.publish_login(Some(operation_id.clone()), None);
        if let Err(error) = (self.opener)(url.as_str()) {
            self.finish_attempt(&operation_id);
            self.publish_login(None, Some(error.clone()));
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
            // A superseded attempt must not overwrite the newer state.
            if this.login_generation.load(Ordering::SeqCst) == generation {
                this.publish_login(None, result.err());
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
        if self.login_generation.load(Ordering::SeqCst) != generation {
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
        self.commit_session(
            &Ticket::Login(generation),
            Some(refresh_token),
            profile,
            minecraft,
        )
    }

    /// Stores the session if `ticket` is still current. Holds `commit` across the
    /// generation check and the vault write.
    fn commit_session(
        &self,
        ticket: &Ticket,
        refresh_token: Option<SecretString>,
        profile: Profile,
        minecraft: MinecraftSession,
    ) -> LauncherResult<()> {
        let _commit = lock(&self.commit);
        if !self.current(ticket) {
            return Err(superseded());
        }
        if let Ticket::Account(id, _) = ticket
            && *id != profile.id
        {
            return Err(LauncherError::new(
                ErrorCode::AuthProtocolError,
                "The account answered with a different Minecraft profile.",
            ));
        }
        let key = vault_key(&profile.id);
        if let Some(refresh_token) = &refresh_token {
            let store = self.store();
            if let Err(error) = store.put(&key, refresh_token) {
                if error.code != ErrorCode::SecretStoreUnavailable || !store.persistent() {
                    return Err(error);
                }
                // Explicit session-only mode: secrets stay in memory only. Saved
                // accounts cannot be restored from it, so they are not kept either.
                let memory: Arc<dyn SecretStore> = Arc::new(MemoryStore::default());
                memory.put(&key, refresh_token)?;
                *lock(&self.store) = memory;
            }
        }
        let persistent = self.store().persistent();
        let record = AccountRecord {
            profile_id: profile.id.clone(),
            profile_name: profile.name.clone(),
            vault_key: key.clone(),
        };
        let first = self.host.accounts_file().default_profile.is_none();
        self.host.update_accounts_file(Box::new(move |file| {
            if persistent {
                match file
                    .accounts
                    .iter_mut()
                    .find(|a| a.profile_id == record.profile_id)
                {
                    Some(existing) => *existing = record.clone(),
                    None => file.accounts.push(record.clone()),
                }
            } else {
                file.accounts.clear();
            }
            if first {
                file.default_profile = Some(ProfileRef::Microsoft {
                    id: record.profile_id,
                });
            }
        }))?;
        lock(&self.sessions).insert(
            profile.id.clone(),
            Session {
                profile: profile.clone(),
                vault_key: key,
                minecraft,
            },
        );
        self.publish_account(&profile, AccountState::VerifiedAccess, None);
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
                self.login_generation.fetch_add(1, Ordering::SeqCst);
                let _ = attempt.cancel.send(true);
                self.publish_login(
                    None,
                    Some(LauncherError::new(
                        ErrorCode::AuthCancelled,
                        "Sign-in was cancelled.",
                    )),
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

    /// Restores saved accounts at startup: finishes pending sign-outs first, then
    /// refreshes and re-verifies each account. Network failures keep the account.
    pub async fn restore(&self) {
        let file = self.host.accounts_file();
        for record in file.pending_sign_outs.clone() {
            let _ = self.complete_sign_out(&record.profile_id, Some(record.clone()));
        }
        if let Err(error) = self.client_id() {
            for record in &file.accounts {
                self.publish_account(
                    &Profile {
                        id: record.profile_id.clone(),
                        name: record.profile_name.clone(),
                    },
                    AccountState::ReauthRequired,
                    Some(error.clone()),
                );
            }
            return;
        }
        let _transaction = self.transaction.lock().await;
        for record in &file.accounts {
            let generation = self.generation(&record.profile_id);
            let profile = Profile {
                id: record.profile_id.clone(),
                name: record.profile_name.clone(),
            };
            self.publish_account(&profile, AccountState::Refreshing, None);
            let ticket = Ticket::Account(record.profile_id.clone(), generation);
            if let Err(error) = self.refresh_locked(&ticket, &record.vault_key).await
                && self.current(&ticket)
            {
                self.publish_account(&profile, state_for_error(error.code), Some(error));
            }
        }
    }

    /// Full refresh chain; the caller holds `transaction`.
    async fn refresh_locked(&self, ticket: &Ticket, vault_key: &str) -> LauncherResult<()> {
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
                self.forget_revoked(ticket, vault_key);
                return Err(error);
            }
            Err(error) => return Err(error),
        };
        let minecraft = flow::minecraft_session(&self.http, &tokens.access_token).await?;
        let profile = flow::check_access(&self.http, &minecraft.access_token).await?;
        // Persist a rotated refresh token before the old one is discarded; when the
        // response does not rotate it, the existing vault entry stays.
        self.commit_session(ticket, tokens.refresh_token, profile, minecraft)
    }

    /// Forgets a revoked login; the account stays listed so it can sign in again.
    fn forget_revoked(&self, ticket: &Ticket, vault_key: &str) {
        let _commit = lock(&self.commit);
        let Ticket::Account(id, _) = ticket else {
            return;
        };
        if !self.current(ticket) {
            return;
        }
        if self.store().delete(vault_key).is_ok() {
            let id = id.clone();
            let _ = self.host.update_accounts_file(Box::new(move |file| {
                file.accounts.retain(|a| a.profile_id != id);
            }));
        }
        lock(&self.sessions).remove(id);
    }

    /// Re-verifies current access of one account for one launch attempt (doc 03
    /// "Timing and refresh"). Uses the cached Minecraft token only while it is
    /// valid with a skew margin, and always asks for fresh entitlements and profile.
    /// Without a connection, an account that proved it owns Minecraft when it was
    /// signed in still starts, offline ([`Self::offline_authorization`]).
    pub async fn verify_for_launch(&self, profile_id: &str) -> LauncherResult<LaunchAuthorization> {
        let client_id = self.client_id()?;
        match self.verify_online(profile_id, &client_id).await {
            Err(error) if error.code == ErrorCode::NetworkError => self
                .offline_authorization(profile_id, &client_id)
                .ok_or(error),
            other => other,
        }
    }

    /// Offline play (D-052): a saved account starts without the online check
    /// while the sign-in and Minecraft services cannot be reached. It was
    /// checked for a Minecraft licence when it was signed in; the game gets the
    /// last Minecraft token of this run, which may have expired, or a placeholder.
    /// Single-player works; multiplayer and Realms need the connection.
    fn offline_authorization(
        &self,
        profile_id: &str,
        client_id: &crate::auth::client_id::ClientId,
    ) -> Option<LaunchAuthorization> {
        let record = self
            .host
            .accounts_file()
            .accounts
            .into_iter()
            .find(|a| a.profile_id == profile_id)?;
        let sessions = lock(&self.sessions);
        let session = sessions.get(profile_id);
        Some(LaunchAuthorization {
            profile: session.map_or_else(
                || Profile {
                    id: record.profile_id.clone(),
                    name: record.profile_name.clone(),
                },
                |s| s.profile.clone(),
            ),
            access_token: SecretString::new(
                session.map_or(OFFLINE_TOKEN, |s| s.minecraft.access_token.expose()),
            ),
            xuid: session.and_then(|s| s.minecraft.xuid.clone()),
            client_id: client_id.as_str().to_owned(),
            offline: true,
        })
    }

    async fn verify_online(
        &self,
        profile_id: &str,
        client_id: &crate::auth::client_id::ClientId,
    ) -> LauncherResult<LaunchAuthorization> {
        let _transaction = self.transaction.lock().await;
        let ticket = Ticket::Account(profile_id.to_owned(), self.generation(profile_id));
        // A saved account whose restore failed (for example offline) refreshes now.
        let vault_key = lock(&self.sessions)
            .get(profile_id)
            .map(|s| s.vault_key.clone())
            .or_else(|| {
                self.host
                    .accounts_file()
                    .accounts
                    .into_iter()
                    .find(|a| a.profile_id == profile_id)
                    .map(|a| a.vault_key)
            })
            .ok_or_else(sign_in_to_play)?;
        let token_valid = |expires_at: DateTime<Utc>| {
            expires_at - chrono::Duration::seconds(contract::EXPIRY_SKEW_SECONDS) > Utc::now()
        };
        let cached = lock(&self.sessions).get(profile_id).and_then(|s| {
            token_valid(s.minecraft.expires_at)
                .then(|| SecretString::new(s.minecraft.access_token.expose()))
        });
        let fail = |e: LauncherError| {
            self.publish_account_error(profile_id, &e);
            e
        };
        match cached {
            None => self
                .refresh_locked(&ticket, &vault_key)
                .await
                .map_err(fail)?,
            Some(token) => match flow::check_access(&self.http, &token).await {
                Ok(profile) => {
                    let _commit = lock(&self.commit);
                    if !self.current(&ticket) {
                        return Err(superseded());
                    }
                    if let Some(session) = lock(&self.sessions).get_mut(profile_id) {
                        session.profile = profile.clone();
                    }
                    self.publish_account(&profile, AccountState::VerifiedAccess, None);
                }
                // An expired or revoked Minecraft token: refresh once, then retry.
                Err(error) if error.code == ErrorCode::AuthReauthRequired => {
                    self.refresh_locked(&ticket, &vault_key)
                        .await
                        .map_err(fail)?;
                }
                Err(error) => return Err(fail(error)),
            },
        }
        let sessions = lock(&self.sessions);
        let session = sessions.get(profile_id).ok_or_else(superseded)?;
        Ok(LaunchAuthorization {
            profile: session.profile.clone(),
            access_token: SecretString::new(session.minecraft.access_token.expose()),
            xuid: session.minecraft.xuid.clone(),
            client_id: client_id.as_str().to_owned(),
            offline: false,
        })
    }

    /// Local sign-out of one account, or a retry of a pending one. Refused while a
    /// launch or game owns the accounts.
    pub async fn sign_out(&self, profile_id: &str) -> LauncherResult<()> {
        if self.host.launch_owns_account() {
            return Err(LauncherError::new(
                ErrorCode::AppBusy,
                "Sign-out is not possible while a game is starting or running.",
            ));
        }
        // Invalidate pending work first so no late result can repopulate the vault.
        *lock(&self.generations)
            .entry(profile_id.to_owned())
            .or_default() += 1;
        let file = self.host.accounts_file();
        let record = file
            .accounts
            .iter()
            .chain(&file.pending_sign_outs)
            .find(|a| a.profile_id == profile_id)
            .cloned()
            .or_else(|| {
                lock(&self.sessions).get(profile_id).map(|s| AccountRecord {
                    profile_id: s.profile.id.clone(),
                    profile_name: s.profile.name.clone(),
                    vault_key: s.vault_key.clone(),
                })
            });
        self.complete_sign_out(profile_id, record)
    }

    fn complete_sign_out(
        &self,
        profile_id: &str,
        record: Option<AccountRecord>,
    ) -> LauncherResult<()> {
        let _commit = lock(&self.commit);
        lock(&self.sessions).remove(profile_id);
        let forget = |status: &mut AuthStatus| {
            status.accounts.retain(|a| a.profile_id != profile_id);
            status
                .pending_sign_outs
                .retain(|a| a.profile_id != profile_id);
        };
        let id = profile_id.to_owned();
        let Some(record) = record else {
            let _ = self.host.update_accounts_file(Box::new(move |file| {
                file.remove(&ProfileRef::Microsoft { id });
            }));
            self.update_status(forget);
            return Ok(());
        };
        match self.store().delete(&record.vault_key) {
            Ok(()) => {
                self.host.update_accounts_file(Box::new(move |file| {
                    file.remove(&ProfileRef::Microsoft { id: id.clone() });
                    file.pending_sign_outs.retain(|a| a.profile_id != id);
                }))?;
                self.update_status(forget);
                Ok(())
            }
            Err(error) => {
                // Keep a non-secret marker so restart retries deletion and never
                // restores the credential.
                let pending = record.clone();
                let _ = self.host.update_accounts_file(Box::new(move |file| {
                    file.remove(&ProfileRef::Microsoft { id: id.clone() });
                    if !file.pending_sign_outs.iter().any(|a| a.profile_id == id) {
                        file.pending_sign_outs.push(pending);
                    }
                }));
                self.update_status(|status| {
                    forget(status);
                    status.pending_sign_outs.push(PendingSignOut {
                        profile_id: record.profile_id.clone(),
                        profile_name: record.profile_name.clone(),
                    });
                    status.login_error = Some(error.clone());
                    status.login_state = AccountState::SignOutPending;
                });
                Err(error)
            }
        }
    }
}

#[cfg(test)]
mod tests;
