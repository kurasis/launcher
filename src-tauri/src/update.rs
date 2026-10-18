//! Launcher updates from upd.lapi.sh (D-063).
//!
//! At start and every [`EVERY`], the launcher asks the manifest of its
//! channel, platform and install type. [`decide`] says whether this install is
//! offered the version (rollout percent, revoked versions). An offered update
//! downloads in the background (in the automatic mode, and not on a metered
//! connection without a press) and is checked against its signature; it is
//! installed when the launcher closes with nothing running, or at once with
//! "Restart now". Where the launcher cannot install itself (a `.deb`), the
//! player is told and given the link. Every step's state is an
//! [`UpdateView`] in the app snapshot. A new version that does not start goes
//! back to the one before ([`rollback`]).

pub mod rollback;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{AppHandle, Manager, Runtime};
use tauri_plugin_updater::{Update, UpdaterExt};

use crate::error::{ErrorCode, LauncherError, LauncherResult};
use crate::state::AppState;

const BASE: &str = "https://upd.lapi.sh";

/// upd.lapi.sh, or the update server a test build was made for
/// (`LAUNCHER_UPDATE_URL` at build time; release builds never set it).
fn base() -> &'static str {
    option_env!("LAUNCHER_UPDATE_URL")
        .filter(|u| !u.trim().is_empty())
        .unwrap_or(BASE)
}
/// `{channel}` and, for a return, `{version}` are ours; the updater fills in the rest.
const ENDPOINT: &str = "/v1/{channel}/{{target}}-{{arch}}-{{bundle_type}}.json";
const VERSION_ENDPOINT: &str = "/releases/{version}/{{target}}-{{arch}}-{{bundle_type}}.json";
pub const EVERY: Duration = Duration::from_secs(6 * 3600);
const WHATS_NEW_DIR: &str = "updates";

/// The update public key built in by the Package workflow; none means this
/// build does not look for updates.
pub fn public_key() -> Option<&'static str> {
    option_env!("LAUNCHER_UPDATE_PUBKEY").filter(|k| !k.trim().is_empty())
}

/// Which versions the player gets (Settings → General).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UpdateChannel {
    #[default]
    Stable,
    /// Test versions first; stable ones too, as beta is never behind.
    Beta,
}

impl UpdateChannel {
    fn path(self) -> &'static str {
        match self {
            Self::Stable => "stable",
            Self::Beta => "beta",
        }
    }
}

/// The player's choice in Settings → General.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UpdateMode {
    /// Download by itself, install when the launcher closes.
    #[default]
    Auto,
    /// Only say that a version is there.
    Notify,
    /// Never look.
    Off,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    /// No update known.
    #[default]
    Idle,
    /// Offered; downloads on a press (notify mode, metered connection).
    Available,
    Downloading,
    /// Downloaded and checked; installs on close or with "Restart now".
    Ready,
    /// The launcher cannot install itself here: the player gets a link.
    Notice,
    Failed,
}

/// Why the last step failed; the interface says it in the player's language.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UpdateError {
    /// Not reachable or not answered; the next check tries again.
    Network,
    /// The signature did not match: nothing was installed.
    Signature,
    /// Installing failed; the download link is the way.
    Install,
}

/// The notes of the version now running, shown once after an update.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WhatsNew {
    pub version: String,
    pub notes: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateView {
    /// This build has the update key.
    pub configured: bool,
    pub phase: Phase,
    pub version: Option<String>,
    /// Release notes by language tag; `en` is the fallback.
    pub notes: BTreeMap<String, String>,
    pub mandatory: bool,
    /// Where to get the version by hand (`.deb`, or when installing failed).
    pub download_url: Option<String>,
    pub downloaded: u64,
    pub total: Option<u64>,
    pub error: Option<UpdateError>,
    pub whats_new: Option<WhatsNew>,
    /// The launcher went back to this version: the newer one did not start.
    pub rolled_back: Option<rollback::RolledBack>,
}

/// What the manifest offers this install.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Offer {
    pub version: String,
    pub installable: bool,
    pub mandatory: bool,
    pub notes: BTreeMap<String, String>,
    pub download_url: Option<String>,
}

/// Whether `manifest` offers this install a version: one newer than
/// `current` when `seed` falls within the rollout percent, or any other when
/// `current` was revoked; never `skipped`, the version that did not start here.
pub fn decide(current: &str, manifest: &Value, seed: u32, skipped: Option<&str>) -> Option<Offer> {
    let current_version = semver::Version::parse(current).ok()?;
    let version = manifest.get("version")?.as_str()?.trim_start_matches('v');
    if skipped == Some(version) {
        return None;
    }
    let offered = semver::Version::parse(version).ok()?;
    let revoked = manifest
        .get("revoked")
        .and_then(Value::as_array)
        .is_some_and(|list| list.iter().any(|v| v.as_str() == Some(current)));
    let rollout = manifest
        .get("rollout")
        .and_then(Value::as_u64)
        .unwrap_or(100);
    let within = u64::from(seed % 100) < rollout;
    let wanted = (offered > current_version && within) || (revoked && offered != current_version);
    if !wanted {
        return None;
    }
    let mut notes: BTreeMap<String, String> = manifest
        .get("notesByLanguage")
        .and_then(Value::as_object)
        .map(|m| {
            m.iter()
                .filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_owned())))
                .collect()
        })
        .unwrap_or_default();
    if notes.is_empty()
        && let Some(text) = manifest
            .get("notes")
            .and_then(Value::as_str)
            .filter(|t| !t.is_empty())
    {
        notes.insert("en".into(), text.to_owned());
    }
    Some(Offer {
        version: version.to_owned(),
        installable: manifest
            .get("installable")
            .and_then(Value::as_bool)
            .unwrap_or(true),
        mandatory: revoked
            || manifest
                .get("mandatory")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        notes,
        download_url: manifest
            .get("download")
            .and_then(Value::as_str)
            .filter(|u| u.starts_with("https://"))
            .map(str::to_owned),
    })
}

fn backup_dir(root: &Path, version: &str) -> PathBuf {
    root.join("backups").join(format!("before-{version}"))
}

/// Before the first start of a new version: copies the settings and the
/// installation records to `backups/before-<version>/`, and returns the
/// notes the update left for "What's new". A copy made already (before the
/// settings were read, after an update) stays as it is.
pub fn first_start(root: &Path, version: &str) -> std::io::Result<Option<WhatsNew>> {
    back_up(root, version)?;
    let notes = root
        .join(WHATS_NEW_DIR)
        .join(format!("notes-{version}.json"));
    Ok(std::fs::read(&notes)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok()))
}

fn back_up(root: &Path, version: &str) -> std::io::Result<()> {
    let backup = backup_dir(root, version);
    if backup.exists() {
        return Ok(());
    }
    let temp = root.join("backups").join(format!(".before-{version}.tmp"));
    let _ = std::fs::remove_dir_all(&temp);
    copy_records(root, &temp)?;
    std::fs::rename(&temp, &backup)
}

/// Settings, accounts and installation records from `root` into `backup`.
fn copy_records(root: &Path, backup: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(backup.join("instances"))?;
    for name in ["settings.json", "accounts.json"] {
        let from = root.join(name);
        if from.is_file() {
            // Accounts hold no tokens (D-045); they make the copy whole.
            std::fs::copy(&from, backup.join(name))?;
        }
    }
    if let Ok(entries) = std::fs::read_dir(root.join("instances")) {
        for entry in entries.flatten() {
            let record = entry.path().join("instance.json");
            if record.is_file() {
                std::fs::copy(
                    &record,
                    backup
                        .join("instances")
                        .join(format!("{}.json", entry.file_name().to_string_lossy())),
                )?;
            }
        }
    }
    Ok(())
}

/// Going back from `failed`: the settings and installation records as they
/// were before it first started, as the older version may not read what the
/// newer one wrote.
fn restore_before(root: &Path, failed: &str) -> std::io::Result<()> {
    let backup = backup_dir(root, failed);
    if !backup.is_dir() {
        return Ok(());
    }
    for name in ["settings.json", "accounts.json"] {
        let from = backup.join(name);
        if from.is_file() {
            crate::storage::atomic::write(&root.join(name), &std::fs::read(&from)?)?;
        }
    }
    if let Ok(entries) = std::fs::read_dir(backup.join("instances")) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let Some(id) = name.strip_suffix(".json") else {
                continue;
            };
            let folder = root.join("instances").join(id);
            if folder.is_dir() {
                crate::storage::atomic::write(
                    &folder.join("instance.json"),
                    &std::fs::read(entry.path())?,
                )?;
            }
        }
    }
    Ok(())
}

/// At start, before the settings are read (D-063 stage 2): counts the start of
/// a new version and backs up what it may change; after the starts that did not
/// go well, installs the version before and starts it, so this does not return.
/// What went wrong on the way back is returned for the log.
pub fn at_start<R: Runtime>(app: &AppHandle<R>, root: &Path) -> Option<String> {
    let current = app.package_info().version.to_string();
    let dir = root.join(WHATS_NEW_DIR);
    match rollback::on_start(&dir, &current) {
        rollback::Start::Normal => None,
        rollback::Start::First => back_up(root, &current)
            .err()
            .map(|e| format!("Update: copying the settings before {current} failed: {e}")),
        rollback::Start::GoBack { to, failed } => {
            eprintln!("Version {failed} did not start; going back to {to}");
            let handle = app.clone();
            let root = root.to_path_buf();
            let back = std::thread::spawn(move || {
                tauri::async_runtime::block_on(go_back(&handle, &root, &to, &failed))
            })
            .join()
            .unwrap_or_else(|_| Err("going back stopped".to_owned()));
            match back {
                // Windows does not get here: the installer starts the version.
                Ok(()) => app.restart(),
                Err(e) => Some(format!(
                    "Update: this version did not start before; going back failed: {e}"
                )),
            }
        }
    }
}

/// Downloads the version `to` (signed, its own manifest on upd.lapi.sh),
/// puts the records back as they were before `failed` and installs it.
async fn go_back<R: Runtime>(
    app: &AppHandle<R>,
    root: &Path,
    to: &str,
    failed: &str,
) -> Result<(), String> {
    let key = public_key().ok_or("this build has no update key")?;
    let url = url::Url::parse(&format!(
        "{}{}",
        base(),
        VERSION_ENDPOINT.replace("{version}", to)
    ))
    .map_err(|e| e.to_string())?;
    let update = app
        .updater_builder()
        .pubkey(key)
        .endpoints(vec![url])
        .and_then(|b| {
            b.version_comparator(|_, _| true)
                .timeout(Duration::from_secs(300))
                .build()
        })
        .map_err(|e| e.to_string())?
        .check()
        .await
        .map_err(|e| e.to_string())?
        .ok_or("upd.lapi.sh has no such version")?;
    if update.version.trim_start_matches('v') != to {
        return Err(format!("upd.lapi.sh answered version {}", update.version));
    }
    let bytes = update
        .download(|_, _| {}, || {})
        .await
        .map_err(|e| e.to_string())?;
    restore_before(root, failed).map_err(|e| format!("restoring the settings: {e}"))?;
    rollback::record_going_back(&root.join(WHATS_NEW_DIR), failed, to)
        .map_err(|e| e.to_string())?;
    update.install(&bytes).map_err(|e| e.to_string())
}

/// The downloaded update waiting for the launcher to close.
#[derive(Default)]
pub struct Updates {
    offered: Mutex<Option<Update>>,
    ready: Mutex<Option<(Update, Vec<u8>)>>,
    /// A check or download runs.
    busy: AtomicBool,
    /// The interface has shown in this run.
    started: AtomicBool,
}

/// Holds [`Updates::busy`] until dropped.
struct Busy<'a>(&'a AtomicBool);

impl Drop for Busy<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

impl Updates {
    fn start(&self) -> Option<Busy<'_>> {
        (!self.busy.swap(true, Ordering::SeqCst)).then_some(Busy(&self.busy))
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// What the player is told about an updater error; the details go to the log.
fn failure(state: &AppState, what: &str, error: &tauri_plugin_updater::Error) -> UpdateError {
    state
        .console()
        .warn(format!("Update: {what} failed: {error}"));
    match error {
        tauri_plugin_updater::Error::Minisign(_)
        | tauri_plugin_updater::Error::SignatureUtf8(_)
        | tauri_plugin_updater::Error::Base64(_)
        | tauri_plugin_updater::Error::SignedVersionMismatch { .. }
        | tauri_plugin_updater::Error::MissingSignedVersion => UpdateError::Signature,
        _ if what == "installing" => UpdateError::Install,
        _ => UpdateError::Network,
    }
}

impl Updates {
    fn view(state: &AppState) -> UpdateView {
        let mut view = state.update();
        view.configured = public_key().is_some();
        view
    }

    fn set(state: &AppState, edit: impl FnOnce(&mut UpdateView)) {
        let mut view = Self::view(state);
        edit(&mut view);
        state.set_update(view);
    }

    /// Asks the channel's manifest; `manual` reports a failure to the player.
    pub async fn check<R: Runtime>(&self, app: &AppHandle<R>, manual: bool) {
        let state = app.state::<AppState>();
        let Some(key) = public_key() else {
            return;
        };
        let settings = state.settings();
        if !manual && settings.update_mode == UpdateMode::Off {
            return;
        }
        if state.network() == crate::state::dto::NetworkStatus::Offline && !manual {
            return;
        }
        let Some(busy) = self.start() else {
            return;
        };
        if matches!(Self::view(&state).phase, Phase::Downloading | Phase::Ready) {
            return;
        }
        let seed = match settings.update_seed {
            Some(seed) => seed,
            None => {
                let seed = rand_seed();
                let _ = state.edit_settings(|s| s.update_seed = Some(seed));
                seed
            }
        };
        let endpoint = format!(
            "{}{}",
            base(),
            ENDPOINT.replace("{channel}", settings.update_channel.path())
        );
        let built = url::Url::parse(&endpoint)
            .map_err(|_| tauri_plugin_updater::Error::UnsupportedOs)
            .and_then(|url| {
                app.updater_builder()
                    .pubkey(key)
                    .endpoints(vec![url])?
                    // Any other version: `decide` says which are offered.
                    .version_comparator(|current, release| release.version != current)
                    .timeout(Duration::from_secs(60))
                    .build()
            });
        let found = match built {
            Ok(updater) => updater.check().await,
            Err(e) => Err(e),
        };
        let current = state.app_version().to_owned();
        match found {
            Ok(Some(update)) => match decide(
                &current,
                &update.raw_json,
                seed,
                settings.skipped_version.as_deref(),
            ) {
                Some(offer) => {
                    let installable = offer.installable;
                    Self::set(&state, |v| {
                        v.phase = if installable {
                            Phase::Available
                        } else {
                            Phase::Notice
                        };
                        v.version = Some(offer.version.clone());
                        v.notes = offer.notes.clone();
                        v.mandatory = offer.mandatory;
                        v.download_url = offer.download_url.clone();
                        v.error = None;
                    });
                    state
                        .console()
                        .info(format!("Update: version {} is available", offer.version));
                    if installable {
                        *lock(&self.offered) = Some(update);
                        let metered = settings.metered;
                        if settings.update_mode == UpdateMode::Auto && !metered {
                            drop(busy);
                            self.download(app).await;
                        }
                    }
                }
                None => Self::set(&state, |v| {
                    v.phase = Phase::Idle;
                    v.error = None;
                }),
            },
            Ok(None) => Self::set(&state, |v| {
                v.phase = Phase::Idle;
                v.error = None;
            }),
            Err(e) => {
                let message = failure(&state, "checking", &e);
                if manual {
                    Self::set(&state, |v| {
                        v.phase = Phase::Failed;
                        v.error = Some(message);
                    });
                }
            }
        }
    }

    /// Downloads the offered update and checks its signature.
    pub async fn download<R: Runtime>(&self, app: &AppHandle<R>) {
        let state = app.state::<AppState>();
        let Some(update) = lock(&self.offered).clone() else {
            return;
        };
        let Some(_busy) = self.start() else {
            return;
        };
        Self::set(&state, |v| {
            v.phase = Phase::Downloading;
            v.downloaded = 0;
            v.total = None;
            v.error = None;
        });
        let mut received = 0u64;
        let mut shown = Instant::now();
        let progress_state = app.state::<AppState>();
        let result = update
            .download(
                |chunk, total| {
                    received += chunk as u64;
                    // A few announcements a second are enough for a progress bar.
                    if shown.elapsed() >= Duration::from_millis(250) {
                        shown = Instant::now();
                        Self::set(&progress_state, |v| {
                            v.downloaded = received;
                            v.total = total;
                        });
                    }
                },
                || {},
            )
            .await;
        match result {
            Ok(bytes) => {
                if let Some(root) = state.data_root() {
                    let notes = WhatsNew {
                        version: update.version.clone(),
                        notes: Self::view(&state).notes,
                    };
                    let dir = root.path().join(WHATS_NEW_DIR);
                    let _ = std::fs::create_dir_all(&dir);
                    let _ = std::fs::write(
                        dir.join(format!("notes-{}.json", update.version)),
                        serde_json::to_vec(&notes).unwrap_or_default(),
                    );
                }
                state.console().info(format!(
                    "Update: version {} downloaded and checked; it installs when the launcher closes",
                    update.version
                ));
                let size = bytes.len() as u64;
                *lock(&self.ready) = Some((update, bytes));
                Self::set(&state, |v| {
                    v.phase = Phase::Ready;
                    v.downloaded = size;
                    v.total = Some(size);
                });
            }
            Err(e) => {
                let message = failure(&state, "downloading", &e);
                Self::set(&state, |v| {
                    v.phase = Phase::Failed;
                    v.error = Some(message);
                });
            }
        }
    }

    /// Something runs that an install must not interrupt.
    fn busy_now(state: &AppState) -> bool {
        state.current_run().is_some() || state.activity().current().is_some()
    }

    /// When the launcher closes: installs a ready update unless a game or an
    /// install runs. On Windows the installer takes over after this returns.
    pub fn install_on_close(&self, state: &AppState) {
        let Some((update, bytes)) = lock(&self.ready).take() else {
            return;
        };
        if Self::busy_now(state) {
            state.console().info(
                "Update: a game or an install is running; the update waits for the next close",
            );
            return;
        }
        state
            .console()
            .info(format!("Update: installing version {}", update.version));
        Self::watch(state, &update.version);
        if let Err(e) = update.restart_after_install(false).install(&bytes) {
            state
                .console()
                .warn(format!("Update: installing failed: {e}"));
        }
    }

    /// "Restart now": installs the ready update and starts the new version.
    pub fn restart_now<R: Runtime>(&self, app: &AppHandle<R>) -> LauncherResult<()> {
        let state = app.state::<AppState>();
        if Self::busy_now(&state) {
            return Err(LauncherError::new(
                ErrorCode::AppBusy,
                "Close the game and wait for installs to finish, then restart to update.",
            ));
        }
        let Some((update, bytes)) = lock(&self.ready).take() else {
            return Err(LauncherError::invalid_input(
                "No downloaded update is waiting.",
            ));
        };
        state.console().info(format!(
            "Update: installing version {} and restarting",
            update.version
        ));
        Self::watch(&state, &update.version);
        match update.install(&bytes) {
            // Windows does not get here: its installer restarts the launcher.
            Ok(()) => app.restart(),
            Err(e) => {
                let message = failure(&state, "installing", &e);
                let download = Self::view(&state).download_url;
                Self::set(&state, |v| {
                    v.phase = Phase::Failed;
                    v.error = Some(message);
                    v.download_url = download;
                });
                Err(LauncherError::new(
                    ErrorCode::Internal,
                    "The update could not be installed. Download it from the link instead.",
                ))
            }
        }
    }

    /// The new version's starts are counted until one goes well.
    fn watch(state: &AppState, to: &str) {
        if let Some(root) = state.data_root()
            && let Err(e) =
                rollback::before_install(&root.path().join(WHATS_NEW_DIR), state.app_version(), to)
        {
            state.console().warn(format!(
                "Update: the launcher could not note the update, so it cannot go back if {to} does not start: {e}"
            ));
        }
    }

    /// The interface has shown: once the launcher has stayed up a little
    /// longer, this start went well.
    pub fn interface_shown<R: Runtime>(&self, app: &AppHandle<R>) {
        if self.started.swap(true, Ordering::SeqCst) {
            return;
        }
        let handle = app.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(rollback::HEALTHY_AFTER).await;
            Self::went_well(&handle.state::<AppState>());
        });
    }

    /// A normal close after the interface showed is a good start too.
    pub fn at_exit(&self, state: &AppState) {
        if self.started.load(Ordering::SeqCst) {
            Self::went_well(state);
        }
    }

    fn went_well(state: &AppState) {
        if let Some(root) = state.data_root() {
            rollback::healthy(&root.path().join(WHATS_NEW_DIR), state.app_version());
        }
    }

    /// "What's new", or the note about going back, was read.
    pub fn dismiss_whats_new(state: &AppState) {
        let view = Self::view(state);
        if let Some(root) = state.data_root() {
            let dir = root.path().join(WHATS_NEW_DIR);
            if let Some(seen) = view.whats_new {
                let _ = std::fs::remove_file(dir.join(format!("notes-{}.json", seen.version)));
            }
            if view.rolled_back.is_some() {
                rollback::forget(&dir);
            }
        }
        Self::set(state, |v| {
            v.whats_new = None;
            v.rolled_back = None;
        });
    }

    /// At start: notices a new version (backup, "What's new") or a return to
    /// this one, and records it.
    pub fn after_start(state: &AppState) {
        let version = state.app_version().to_owned();
        let rolled_back = state
            .data_root()
            .and_then(|root| rollback::went_back(&root.path().join(WHATS_NEW_DIR), &version));
        if let Some(back) = &rolled_back {
            state.console().warn(format!(
                "Update: version {} did not start, so the launcher went back to {version}; {} will not be offered again",
                back.failed, back.failed
            ));
            let failed = back.failed.clone();
            let _ = state.edit_settings(|s| {
                s.skipped_version = Some(failed);
                s.last_version = Some(version.clone());
            });
            Self::set(state, |v| v.rolled_back = rolled_back.clone());
            return;
        }
        let last = state.settings().last_version;
        if last.as_deref() == Some(version.as_str()) {
            Self::set(state, |_| {});
            return;
        }
        let whats_new = match state.data_root() {
            Some(root) if last.is_some() => match first_start(root.path(), &version) {
                Ok(notes) => {
                    state.console().info(format!(
                        "Update: first start of {version}; settings and installation records copied to backups/before-{version}"
                    ));
                    notes
                }
                Err(e) => {
                    state.console().warn(format!(
                        "Update: copying the settings before {version} failed: {e}"
                    ));
                    None
                }
            },
            _ => None,
        };
        let _ = state.edit_settings(|s| s.last_version = Some(version));
        Self::set(state, |v| v.whats_new = whats_new);
    }
}

fn rand_seed() -> u32 {
    let bytes = uuid::Uuid::new_v4().into_bytes();
    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn manifest(version: &str, rollout: u64, revoked: &[&str]) -> Value {
        json!({
            "version": version,
            "notes": "Fixes",
            "url": "https://upd.lapi.sh/releases/x",
            "signature": "",
            "rollout": rollout,
            "mandatory": false,
            "revoked": revoked,
            "notesByLanguage": { "en": "Fixes", "ru": "Исправления" },
            "download": "https://upd.lapi.sh/download/",
            "installable": true,
        })
    }

    #[test]
    fn newer_versions_are_offered_within_the_rollout() {
        let offer = decide("0.1.0", &manifest("0.2.0", 100, &[]), 7, None).unwrap();
        assert_eq!(offer.version, "0.2.0");
        assert_eq!(offer.notes["ru"], "Исправления");
        assert!(!offer.mandatory && offer.installable);
        // 10 %: seeds ending 0..9 are in, others wait.
        assert!(decide("0.1.0", &manifest("0.2.0", 10, &[]), 109, None).is_some());
        assert!(decide("0.1.0", &manifest("0.2.0", 10, &[]), 110, None).is_none());
        assert!(
            decide("0.1.0", &manifest("0.2.0", 0, &[]), 0, None).is_none(),
            "paused"
        );
        assert!(
            decide("0.2.0", &manifest("0.2.0", 100, &[]), 1, None).is_none(),
            "same version"
        );
        assert!(
            decide("0.3.0", &manifest("0.2.0", 100, &[]), 1, None).is_none(),
            "older version"
        );
    }

    #[test]
    fn a_revoked_version_is_left_even_for_an_older_one() {
        let offer = decide("0.3.0", &manifest("0.2.0", 0, &["0.3.0"]), 99, None).unwrap();
        assert_eq!(offer.version, "0.2.0");
        assert!(offer.mandatory);
    }

    #[test]
    fn a_version_that_did_not_start_here_is_never_offered_again() {
        let skipped = Some("0.3.0");
        assert!(decide("0.2.0", &manifest("0.3.0", 100, &[]), 1, skipped).is_none());
        // Not even when this one is revoked and it is the channel's version.
        assert!(decide("0.2.0", &manifest("0.3.0", 100, &["0.2.0"]), 1, skipped).is_none());
        // A newer fix is.
        assert!(decide("0.2.0", &manifest("0.3.1", 100, &[]), 1, skipped).is_some());
    }

    #[test]
    fn going_back_restores_the_records_from_before_the_failed_version() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("settings.json"), b"old").unwrap();
        std::fs::create_dir_all(root.join("instances/abc")).unwrap();
        std::fs::write(root.join("instances/abc/instance.json"), b"old record").unwrap();
        back_up(root, "0.3.0").unwrap();
        // The new version rewrites them; a second copy does not replace the first.
        std::fs::write(root.join("settings.json"), b"new").unwrap();
        std::fs::write(root.join("instances/abc/instance.json"), b"new record").unwrap();
        assert_eq!(first_start(root, "0.3.0").unwrap(), None);
        restore_before(root, "0.3.0").unwrap();
        assert_eq!(std::fs::read(root.join("settings.json")).unwrap(), b"old");
        assert_eq!(
            std::fs::read(root.join("instances/abc/instance.json")).unwrap(),
            b"old record"
        );
        // Without a copy nothing is touched.
        restore_before(root, "0.9.0").unwrap();
        assert_eq!(std::fs::read(root.join("settings.json")).unwrap(), b"old");
    }

    #[test]
    fn notice_only_installs_get_the_link() {
        let mut deb = manifest("0.2.0", 100, &[]);
        deb["installable"] = json!(false);
        deb["download"] = json!("https://upd.lapi.sh/releases/0.2.0/Launcher_0.2.0_amd64.deb");
        let offer = decide("0.1.0", &deb, 1, None).unwrap();
        assert!(!offer.installable);
        assert_eq!(
            offer.download_url.as_deref(),
            Some("https://upd.lapi.sh/releases/0.2.0/Launcher_0.2.0_amd64.deb")
        );
        // Only HTTPS links are passed on.
        deb["download"] = json!("javascript:alert(1)");
        assert!(
            decide("0.1.0", &deb, 1, None)
                .unwrap()
                .download_url
                .is_none()
        );
        assert!(decide("0.1.0", &json!({"version": "nonsense"}), 1, None).is_none());
    }

    #[test]
    fn the_first_start_of_a_version_backs_up_settings_and_records() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("settings.json"), b"{\"theme\":\"dark\"}").unwrap();
        std::fs::create_dir_all(root.join("instances/abc")).unwrap();
        std::fs::write(root.join("instances/abc/instance.json"), b"{}").unwrap();
        std::fs::create_dir_all(root.join(WHATS_NEW_DIR)).unwrap();
        let notes = WhatsNew {
            version: "0.2.0".into(),
            notes: [("en".into(), "New".into())].into(),
        };
        std::fs::write(
            root.join(WHATS_NEW_DIR).join("notes-0.2.0.json"),
            serde_json::to_vec(&notes).unwrap(),
        )
        .unwrap();
        let found = first_start(root, "0.2.0").unwrap();
        assert_eq!(found, Some(notes));
        let backup = root.join("backups/before-0.2.0");
        assert_eq!(
            std::fs::read(backup.join("settings.json")).unwrap(),
            b"{\"theme\":\"dark\"}"
        );
        assert!(backup.join("instances/abc.json").is_file());
        assert_eq!(
            first_start(root, "0.3.0").unwrap(),
            None,
            "no notes for 0.3.0"
        );
    }
}
