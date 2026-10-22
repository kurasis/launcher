//! Running one world server (PLAN W3, ADAPTATION A1, A3).
//!
//! The server is Mojang's JAR on the pinned Java, with the world's server
//! directory as working directory, `server.properties` rewritten before every
//! start, loopback ports chosen at start and a fresh RCON password. It is
//! supervised like a game (`lifecycle::child`): output is classified and then
//! dropped, readiness is the `Done (` line followed by a successful local
//! status ping, `stop` goes through stdin with a grace period, and only an
//! explicit force stop kills it, which marks the run unclean. The hosting
//! permit is held until the process has exited.

use std::collections::BTreeMap;
use std::io::Write;
use std::net::{Ipv4Addr, TcpListener};
use std::ops::RangeInclusive;
use std::path::{Path, PathBuf};
use std::process::ChildStdin;
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant, SystemTime};

use chrono::Utc;
use serde::Serialize;

use super::failure::{self, Problem};
use super::identity::Nick;
use super::memory::{self, Budget};
use super::output::{self, Event};
use super::properties::RunBinding;
use super::rcon;
use super::receipt;
use super::tuple;
use super::world::{self, LastRun, Mutation, MutationKind, WorldFile};
use super::{install, pack};
use crate::error::{ErrorCode, LauncherError, LauncherResult};
use crate::lifecycle::child::{self, ChildOutcome, OutputLine, Supervised};
use crate::loader::LoaderKind;
use crate::runtime::probe;
use crate::secrets::SecretString;
use crate::state::activity::HostingPermit;
use crate::storage::paths::{DataRoot, WorldId};

/// A starting server that writes nothing for this long is stopped (spec 06's
/// startup deadline, counted from its last line instead of from spawn).
pub const STARTUP_SILENCE: Duration = Duration::from_secs(180);
/// A server still writing is stopped when it is not ready after this long: a
/// big pack on a slow computer gets time, a server that loops does not run on.
pub const STARTUP_LIMIT: Duration = Duration::from_secs(20 * 60);
/// Spec 06: time a server gets to save and exit after `stop`.
pub const STOP_GRACE: Duration = Duration::from_secs(60);
/// Time for the status ping to succeed after the `Done (` line.
const PING_WINDOW: Duration = Duration::from_secs(15);
const PING_TIMEOUT: Duration = Duration::from_secs(3);
/// A status ping may name any protocol; the server answers with its own.
const ANY_PROTOCOL: i32 = -1;
/// The protocols of releases from 1.7.6 on and of snapshots (bit 30 set from
/// 1.16.4's).
const GAME_PROTOCOLS: std::ops::RangeInclusive<i64> = 5..=0x7FFF_FFFF;

/// Everything one start needs.
pub struct Launch<'a> {
    pub root: &'a DataRoot,
    pub world_id: WorldId,
    pub java: &'a Path,
    pub server_heap_mib: u32,
    /// The host's own game heap when they play on this computer, else zero.
    pub game_heap_mib: u32,
    pub physical_memory_bytes: u64,
    pub members: &'a [Nick],
    pub operation_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Starting,
    Ready,
    Stopping,
    Exited,
}

/// What the hosting panel and tests see. Player names stay in memory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostStatus {
    pub world_id: WorldId,
    pub operation_id: String,
    pub phase: Phase,
    pub players: Vec<String>,
    /// Set once exited: whether the server saved and stopped on request.
    pub clean: Option<bool>,
    pub error: Option<ErrorCode>,
}

#[derive(Default)]
struct Shared {
    phase: Option<Phase>,
    done_line: bool,
    bind_failed: bool,
    eula_required: bool,
    stop_requested: bool,
    killed: bool,
    exited: Option<bool>,
    error: Option<ErrorCode>,
    /// Bridge-side local port to the player who logged in through it.
    players: BTreeMap<u16, Nick>,
    /// When the server last wrote a line.
    last_output: Option<Instant>,
    /// What its loader said about mods while it started.
    problems: Vec<Problem>,
}

type Signal = Arc<(Mutex<Shared>, Condvar)>;

fn lock(signal: &Signal) -> MutexGuard<'_, Shared> {
    signal.0.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A running (or just exited) world server.
pub struct HostedServer {
    world_id: WorldId,
    operation_id: String,
    port: u16,
    /// The server's game protocol, as its status ping gave it; 0 until ready.
    protocol: AtomicI32,
    supervised: Supervised,
    stdin: Mutex<Option<ChildStdin>>,
    signal: Signal,
    spawned: Instant,
    spawned_at: SystemTime,
    server_dir: PathBuf,
}

/// World ports come from below the ephemeral ranges (Linux from 32768, Windows
/// and macOS from 49152), so no outgoing connection, of the launcher or of any
/// other program, can take one in the gap between choosing it and the server
/// binding it. The gap is long: a server before 1.13 binds RCON only once it is
/// ready, and a port taken by then leaves the world without its console.
const WORLD_PORTS: RangeInclusive<u16> = 20000..=32767;
/// The usual Minecraft server ports, left to a server of the player's own.
const USUAL_SERVER_PORTS: RangeInclusive<u16> = 25565..=25575;
/// Random ports tried before giving up.
const PORT_TRIES: usize = 256;

/// Two distinct free loopback ports from [`WORLD_PORTS`]. Each is bound and
/// released, so it was free a moment ago; a race lost to another program's
/// listener shows up as a bind failure.
pub fn pick_ports() -> LauncherResult<(u16, u16)> {
    let mut held: Vec<(u16, TcpListener)> = Vec::with_capacity(2);
    let mut last_error = std::io::Error::from(std::io::ErrorKind::AddrInUse);
    for _ in 0..PORT_TRIES {
        let mut random = [0u8; 2];
        getrandom::fill(&mut random).map_err(|_| {
            LauncherError::internal("The operating system did not provide random bytes.")
        })?;
        let span = WORLD_PORTS.end() - WORLD_PORTS.start() + 1;
        let port = WORLD_PORTS.start() + u16::from_le_bytes(random) % span;
        if USUAL_SERVER_PORTS.contains(&port) || held.iter().any(|(p, _)| *p == port) {
            continue;
        }
        match TcpListener::bind((Ipv4Addr::LOCALHOST, port)) {
            Ok(listener) => held.push((port, listener)),
            Err(error) => last_error = error,
        }
        if let [(first, _), (second, _)] = held.as_slice() {
            return Ok((*first, *second));
        }
    }
    Err(LauncherError::from_io(
        &last_error,
        "Choosing a port for the world",
    ))
}

/// What the JVM starts: Mojang's server JAR, or a mod loader's server class
/// with its libraries, which finds the JAR by a system property (W12), or a
/// Forge-style loader's own start arguments, which name the server its
/// processors built.
pub enum Main<'a> {
    Jar,
    Loader {
        kind: LoaderKind,
        main_class: &'a str,
        classpath: &'a [std::path::PathBuf],
    },
    Arguments(&'a [String]),
}

/// The JVM and server arguments, in order.
pub fn arguments(jar: &Path, main: &Main<'_>, heap_mib: u32) -> Vec<String> {
    let mut arguments = vec![
        format!("-Xms{}M", heap_mib.min(1024)),
        format!("-Xmx{heap_mib}M"),
        "-Dfile.encoding=UTF-8".to_owned(),
        // Relative to the server directory, where prepare_run writes it. The
        // old name is the one every Log4j 2 reads, back to 1.7's 2.0-beta9.
        format!("-Dlog4j.configurationFile={}", world::LOG_CONFIG),
        // No lookups in messages (Log4Shell) where Log4j reads this (2.10 on);
        // the configuration's filter covers the older ones too.
        "-Dlog4j2.formatMsgNoLookups=true".to_owned(),
    ];
    match main {
        Main::Jar => {
            arguments.push("-jar".to_owned());
            arguments.push(jar.to_string_lossy().into_owned());
        }
        Main::Loader {
            kind,
            main_class,
            classpath,
        } => {
            let separator = if cfg!(windows) { ";" } else { ":" };
            // Quilt Loader renamed Fabric's properties.
            let property = match kind {
                LoaderKind::Quilt => "loader.gameJarPath",
                _ => "fabric.gameJarPath",
            };
            arguments.push(format!("-D{property}={}", jar.to_string_lossy()));
            arguments.push("-cp".to_owned());
            arguments.push(
                classpath
                    .iter()
                    .map(|p| p.to_string_lossy())
                    .collect::<Vec<_>>()
                    .join(separator),
            );
            arguments.push((*main_class).to_owned());
        }
        Main::Arguments(tokens) => {
            // FML puts its own logging configuration in place of the one
            // above: these keep its log files empty, so chat and nicks are
            // not stored (ADAPTATION A7). Its console still names the thread.
            arguments.push("-Dforge.logging.file.level=off".to_owned());
            arguments.push("-Dforge.logging.debugFile.level=off".to_owned());
            arguments.extend(tokens.iter().cloned());
        }
    }
    arguments.push("--nogui".to_owned());
    arguments
}

fn set_mutation(root: &DataRoot, world_id: WorldId, mutation: Option<Mutation>) {
    if let Ok(mut world) = world::load(root, world_id) {
        world.mutation = mutation;
        let _ = world::save(root, &world);
    }
}

fn finish_world(root: &DataRoot, world_id: WorldId, clean: bool) {
    if let Ok(mut world) = world::load(root, world_id) {
        world.mutation = None;
        world.last_run = Some(LastRun {
            ended_at: Utc::now(),
            clean,
        });
        let _ = world::save(root, &world);
    }
}

fn server_error(code: ErrorCode, message: &str) -> LauncherError {
    LauncherError::new(code, message)
}

/// Starts the server of a world whose JAR is pinned and present. Returns once
/// the process runs; [`HostedServer::wait_ready`] waits for it to accept
/// players. The permit is released when the process exits.
pub fn start(
    launch: &Launch<'_>,
    permit: HostingPermit,
    on_change: Arc<dyn Fn() + Send + Sync>,
) -> LauncherResult<HostedServer> {
    let root = launch.root;
    let world: WorldFile = world::load(root, launch.world_id)?;
    if tuple::version(&world.minecraft).is_none() {
        return Err(LauncherError::invalid_input(
            "Worlds are not available for this Minecraft version yet.",
        ));
    }
    let pin = world.server.clone().ok_or_else(|| {
        server_error(
            ErrorCode::RepairRequired,
            "The world's server is not installed yet.",
        )
    })?;
    if !install::verify(root, &pin) {
        return Err(server_error(
            ErrorCode::RepairRequired,
            "The world's server file is missing or changed. Install it again.",
        ));
    }
    // A world with mods: its loader's server, and exactly its server mods.
    let loader = match &world.pack {
        Some(generation) => {
            let manifest = generation.server_manifest.as_ref().ok_or_else(|| {
                server_error(
                    ErrorCode::RepairRequired,
                    "The world's server is not installed yet.",
                )
            })?;
            let launch = install::loader_launch(root, manifest, &root.server_jar(&pin.jar_sha1))?;
            pack::place_server(root, &world)?;
            Some((generation.loader.kind, launch))
        }
        None => None,
    };
    memory::validate_server_heap(launch.server_heap_mib)?;
    let budget: Budget = memory::budget(
        launch.physical_memory_bytes,
        launch.game_heap_mib,
        launch.server_heap_mib,
    );
    memory::check(&budget)?;

    let (port, rcon_port) = pick_ports()?;
    let rcon_password = rcon::generate_password().ok_or_else(|| {
        LauncherError::internal("The operating system did not provide random bytes.")
    })?;
    let binding = RunBinding {
        port,
        rcon_port,
        rcon_password,
    };
    world::prepare_run(root, &world, &binding, launch.members)?;
    set_mutation(
        root,
        world.id,
        Some(Mutation {
            kind: MutationKind::Host,
            operation_id: launch.operation_id.clone(),
            started_at: Utc::now(),
        }),
    );

    let jar = root.server_jar(&pin.jar_sha1);
    let main = match &loader {
        Some((
            kind,
            install::LoaderLaunch::Classpath {
                main_class,
                classpath,
            },
        )) => Main::Loader {
            kind: *kind,
            main_class,
            classpath,
        },
        Some((_, install::LoaderLaunch::Arguments(tokens))) => Main::Arguments(tokens),
        None => Main::Jar,
    };
    let mut command = probe::sanitized_command(launch.java);
    command
        .args(arguments(&jar, &main, launch.server_heap_mib))
        .current_dir(root.world_server_dir(world.id));

    let signal: Signal = Arc::new((Mutex::new(Shared::default()), Condvar::new()));
    lock(&signal).phase = Some(Phase::Starting);
    let sink_signal = Arc::clone(&signal);
    let sink_change = Arc::clone(&on_change);
    let sink: child::LineSink = Box::new(move |line: OutputLine| {
        let event = output::classify(&line.text);
        {
            let mut shared = lock(&sink_signal);
            shared.last_output = Some(Instant::now());
            // Before anyone can join, so no player wrote the line.
            if shared.phase == Some(Phase::Starting) {
                failure::note(&mut shared.problems, &line.text);
            }
            let Some(event) = event else {
                return;
            };
            match event {
                Event::Ready => shared.done_line = true,
                Event::BindFailed => shared.bind_failed = true,
                Event::EulaRequired => shared.eula_required = true,
                Event::Joined { nick, local_port } => {
                    shared.players.insert(local_port, nick);
                }
                Event::Left { nick } => shared.players.retain(|_, n| *n != nick),
            }
        }
        sink_signal.1.notify_all();
        sink_change();
    });

    let exit_signal = Arc::clone(&signal);
    let exit_root = root.clone();
    let (world_id, operation_id) = (world.id, launch.operation_id.clone());
    let exit_operation = operation_id.clone();
    let on_exit = move |outcome: ChildOutcome| {
        let clean = {
            let mut shared = lock(&exit_signal);
            let clean = shared.stop_requested
                && !shared.killed
                && outcome.status.as_ref().is_ok_and(|s| s.success());
            if !clean && shared.error.is_none() && !shared.stop_requested {
                shared.error = Some(ErrorCode::GameExitedWithError);
            }
            clean
        };
        receipt::clear(&exit_root, &exit_operation);
        // Written before the exit is told, so whoever waited for it (a
        // backup after play) finds world.json final.
        finish_world(&exit_root, world_id, clean);
        {
            let mut shared = lock(&exit_signal);
            shared.exited = Some(clean);
            shared.phase = Some(Phase::Exited);
            shared.players.clear();
        }
        drop(permit);
        exit_signal.1.notify_all();
        on_change();
    };

    let (spawned, spawned_at) = (Instant::now(), SystemTime::now());
    let (supervised, stdin) =
        child::spawn_with_stdin(command, sink, || {}, on_exit).map_err(|e| {
            set_mutation(root, world.id, None);
            LauncherError::from_io(&e, "Starting the world server")
        })?;
    receipt::record(
        root,
        &receipt::Spawned {
            world_id: world.id,
            operation_id: &operation_id,
            pid: supervised.pid(),
            executable: launch.java,
            port,
            rcon_port,
            rcon_password: &binding.rcon_password,
        },
    );
    Ok(HostedServer {
        world_id,
        operation_id,
        port,
        protocol: AtomicI32::new(0),
        supervised,
        stdin: Mutex::new(Some(stdin)),
        signal,
        spawned,
        spawned_at,
        server_dir: root.world_server_dir(world_id),
    })
}

/// How waiting for a start ended.
#[derive(Debug, PartialEq, Eq)]
enum Waited {
    Done,
    BindFailed,
    EulaRequired,
    Exited,
    /// Nothing written for the whole silence.
    Silent,
    /// Still not ready at the limit.
    TooLong,
}

/// Waits for the `Done (` line or the start's end: a server is given time as
/// long as it writes, up to `limit` from `spawned`.
fn wait_start(signal: &Signal, spawned: Instant, silence: Duration, limit: Duration) -> Waited {
    let mut shared = lock(signal);
    loop {
        if shared.bind_failed {
            return Waited::BindFailed;
        }
        if shared.eula_required {
            return Waited::EulaRequired;
        }
        if shared.exited.is_some() {
            return Waited::Exited;
        }
        if shared.done_line {
            return Waited::Done;
        }
        let now = Instant::now();
        let quiet_until = shared.last_output.unwrap_or(spawned).max(spawned) + silence;
        let limit_at = spawned + limit;
        if now >= limit_at {
            return Waited::TooLong;
        }
        if now >= quiet_until {
            return Waited::Silent;
        }
        // Lines do not wake the wait: it looks again when the silence would end.
        shared = signal
            .1
            .wait_timeout(shared, quiet_until.min(limit_at) - now)
            .unwrap_or_else(PoisonError::into_inner)
            .0;
    }
}

impl HostedServer {
    pub fn port(&self) -> u16 {
        self.port
    }

    /// The game protocol of the server's version, known once it is ready.
    pub fn protocol(&self) -> i32 {
        self.protocol.load(Ordering::Acquire)
    }

    pub fn world_id(&self) -> WorldId {
        self.world_id
    }

    pub fn status(&self) -> HostStatus {
        let shared = lock(&self.signal);
        HostStatus {
            world_id: self.world_id,
            operation_id: self.operation_id.clone(),
            phase: shared.phase.unwrap_or(Phase::Starting),
            players: shared
                .players
                .values()
                .map(|n| n.as_str().to_owned())
                .collect(),
            clean: shared.exited,
            error: shared.error,
        }
    }

    /// The player who logged in through a bridge connection from `local_port`.
    pub fn player_on(&self, local_port: u16) -> Option<Nick> {
        lock(&self.signal).players.get(&local_port).cloned()
    }

    /// Waits until the server accepts players, or fails. A server that writes
    /// nothing for `silence`, or is not ready after [`STARTUP_LIMIT`], is
    /// stopped. A failed start says which mod it failed on where the loader or
    /// a crash report of this start names one.
    pub fn wait_ready(&self, silence: Duration) -> LauncherResult<()> {
        match wait_start(&self.signal, self.spawned, silence, STARTUP_LIMIT) {
            Waited::Done => {}
            Waited::BindFailed => {
                self.force_stop();
                return Err(server_error(
                    ErrorCode::AppBusy,
                    "Another program took the world's port. Start the world again.",
                ));
            }
            Waited::EulaRequired => {
                return Err(server_error(
                    ErrorCode::InvalidInput,
                    "Accept the Minecraft EULA to host this world.",
                ));
            }
            Waited::Exited => {
                return Err(self.why_not_started(server_error(
                    ErrorCode::GameExitedWithError,
                    "The world server stopped while starting.",
                )));
            }
            Waited::Silent => {
                self.stop(STOP_GRACE);
                return Err(self.why_not_started(server_error(
                    ErrorCode::OperationInterrupted,
                    "The world server stopped responding while it started.",
                )));
            }
            Waited::TooLong => {
                self.stop(STOP_GRACE);
                return Err(self.why_not_started(server_error(
                    ErrorCode::OperationInterrupted,
                    "The world server did not start in 20 minutes.",
                )));
            }
        }
        let ping_until = Instant::now() + PING_WINDOW;
        while Instant::now() < ping_until && !self.supervised.has_exited() {
            if let Some(protocol) = super::ping::status(self.port, ANY_PROTOCOL, PING_TIMEOUT)
                .filter(|s| GAME_PROTOCOLS.contains(&s.protocol))
                .and_then(|s| i32::try_from(s.protocol).ok())
            {
                self.protocol.store(protocol, Ordering::Release);
                let mut shared = lock(&self.signal);
                if shared.phase == Some(Phase::Starting) {
                    shared.phase = Some(Phase::Ready);
                }
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(250));
        }
        self.stop(STOP_GRACE);
        Err(server_error(
            ErrorCode::GameExitedWithError,
            "The world server started but does not answer.",
        ))
    }

    /// What the loader and this start's crash reports say, in place of
    /// `otherwise` when they name a mod.
    fn why_not_started(&self, otherwise: LauncherError) -> LauncherError {
        let mut problems = lock(&self.signal).problems.clone();
        for problem in failure::crash_reports(&self.server_dir, self.spawned_at) {
            if !problems.contains(&problem) {
                problems.push(problem);
            }
        }
        let mod_files: Vec<String> = std::fs::read_dir(self.server_dir.join("mods"))
            .map(|entries| {
                entries
                    .filter_map(Result::ok)
                    .take(4096)
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();
        failure::chief(&problems, &mod_files).map_or(otherwise, failure::explain)
    }

    /// Sends one console command through stdin.
    pub fn command(&self, command: &str) -> LauncherResult<()> {
        if !rcon::valid_command(command) {
            return Err(LauncherError::invalid_input("Invalid server command."));
        }
        let mut stdin = self.stdin.lock().unwrap_or_else(PoisonError::into_inner);
        let writer = stdin.as_mut().ok_or_else(|| {
            server_error(
                ErrorCode::GameExitedWithError,
                "The world server is stopping.",
            )
        })?;
        writer
            .write_all(format!("{command}\n").as_bytes())
            .and_then(|()| writer.flush())
            .map_err(|_| {
                server_error(
                    ErrorCode::GameExitedWithError,
                    "The world server is not running.",
                )
            })
    }

    /// Asks the server to save and stop, waits up to `grace`, then force stops.
    /// Returns whether it stopped cleanly.
    pub fn stop(&self, grace: Duration) -> bool {
        {
            let mut shared = lock(&self.signal);
            if shared.exited.is_none() {
                shared.stop_requested = true;
                shared.phase = Some(Phase::Stopping);
            }
        }
        let _ = self.command("stop");
        // Closing stdin is also a stop request for the server's console reader.
        self.stdin
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if let Some(clean) = self.wait_exit(grace) {
            return clean;
        }
        self.force_stop();
        self.wait_exit(Duration::from_secs(10)).unwrap_or(false)
    }

    /// Kills the process; the run is recorded as unclean.
    pub fn force_stop(&self) {
        {
            let mut shared = lock(&self.signal);
            if shared.exited.is_some() {
                return;
            }
            shared.killed = true;
            shared.phase = Some(Phase::Stopping);
        }
        self.supervised.kill();
    }

    /// Waits for the process to exit and its exit to be recorded.
    pub fn wait_exit(&self, timeout: Duration) -> Option<bool> {
        let until = Instant::now() + timeout;
        let mut shared = lock(&self.signal);
        loop {
            if let Some(clean) = shared.exited {
                return Some(clean);
            }
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return None;
            }
            shared = self
                .signal
                .1
                .wait_timeout(shared, left)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }
}

/// Stops a server that outlived a core restart through its recorded RCON
/// port and password, after re-checking the process identity. Never kills.
/// A server before 1.16 can lose its RCON listener at start (a race in the
/// server); such a survivor is only watched until it exits.
pub fn stop_survivor(survivor: &receipt::ServerReceipt) -> bool {
    if !survivor.is_alive() {
        return false;
    }
    let password: SecretString = survivor.rcon_password();
    rcon::Rcon::connect(survivor.rcon_port, &password, PING_TIMEOUT)
        .and_then(|mut r| r.command("stop"))
        .is_ok()
}

/// Poll interval while waiting for a surviving server to exit.
pub const SURVIVOR_POLL: Duration = Duration::from_secs(2);

/// At startup: a server that outlived the previous core keeps the hosting
/// permit until it exits, so nothing replaces files its Java reads. Returns its
/// receipt, for a later stop through [`stop_survivor`].
pub fn watch_survivor(
    root: &DataRoot,
    activity: &crate::state::activity::ActivityRegistry,
) -> Option<receipt::ServerReceipt> {
    let survivor = receipt::survivor(root)?;
    let permit = activity
        .try_acquire_hosting(survivor.world_id.uuid(), survivor.operation_id.clone())
        .ok()?;
    let (root, watched) = (root.clone(), survivor.clone());
    std::thread::Builder::new()
        .name("world-survivor".into())
        .spawn(move || {
            while watched.is_alive() {
                std::thread::sleep(SURVIVOR_POLL);
            }
            receipt::clear(&root, &watched.operation_id);
            // Its exit code is not known; the save is checked as after a crash.
            finish_world(&root, watched.world_id, false);
            drop(permit);
        })
        .ok()?;
    Some(survivor)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ports_are_free_distinct_and_below_the_ephemeral_ranges() {
        for _ in 0..32 {
            let (a, b) = pick_ports().unwrap();
            assert_ne!(a, b);
            for port in [a, b] {
                assert!(WORLD_PORTS.contains(&port) && !USUAL_SERVER_PORTS.contains(&port));
                assert!(port < 32768, "below every ephemeral range");
            }
            TcpListener::bind((Ipv4Addr::LOCALHOST, a)).unwrap();
            TcpListener::bind((Ipv4Addr::LOCALHOST, b)).unwrap();
        }
    }

    #[test]
    fn an_occupied_port_is_never_chosen() {
        // Every 16th port of the range, held by another listener.
        let held: Vec<TcpListener> = WORLD_PORTS
            .step_by(16)
            .filter_map(|port| TcpListener::bind((Ipv4Addr::LOCALHOST, port)).ok())
            .collect();
        let taken: Vec<u16> = held
            .iter()
            .map(|l| l.local_addr().unwrap().port())
            .collect();
        assert!(taken.len() > 100);
        for _ in 0..64 {
            let (a, b) = pick_ports().unwrap();
            assert!(!taken.contains(&a) && !taken.contains(&b));
        }
    }

    fn signal() -> Signal {
        Arc::new((Mutex::new(Shared::default()), Condvar::new()))
    }

    #[test]
    fn a_start_has_time_while_the_server_writes() {
        let silence = Duration::from_millis(300);
        // Writing for a second, then quiet: stopped one silence after the
        // last line, well past a fixed deadline of one silence.
        let writing = signal();
        let spawned = Instant::now();
        let writer = {
            let writing = Arc::clone(&writing);
            std::thread::spawn(move || {
                while spawned.elapsed() < Duration::from_secs(1) {
                    lock(&writing).last_output = Some(Instant::now());
                    std::thread::sleep(Duration::from_millis(50));
                }
            })
        };
        assert_eq!(
            wait_start(&writing, spawned, silence, Duration::from_secs(10)),
            Waited::Silent
        );
        let waited = spawned.elapsed();
        assert!(waited >= Duration::from_secs(1) + silence / 2, "{waited:?}");
        assert!(waited < Duration::from_secs(5), "{waited:?}");
        writer.join().unwrap();

        // Writing without end: stopped at the limit.
        let busy = signal();
        let spawned = Instant::now();
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let writer = {
            let (busy, stop) = (Arc::clone(&busy), Arc::clone(&stop));
            std::thread::spawn(move || {
                while !stop.load(Ordering::Relaxed) {
                    lock(&busy).last_output = Some(Instant::now());
                    std::thread::sleep(Duration::from_millis(20));
                }
            })
        };
        assert_eq!(
            wait_start(&busy, spawned, silence, Duration::from_millis(900)),
            Waited::TooLong
        );
        assert!(spawned.elapsed() >= Duration::from_millis(900));
        stop.store(true, Ordering::Relaxed);
        writer.join().unwrap();

        // Never a line: one silence from spawn.
        let quiet = signal();
        let spawned = Instant::now();
        assert_eq!(
            wait_start(&quiet, spawned, silence, Duration::from_secs(10)),
            Waited::Silent
        );
        assert!(spawned.elapsed() >= silence);
    }

    #[test]
    fn a_start_ends_on_what_the_server_said() {
        for (set, expected) in [
            (
                (|s: &mut Shared| s.done_line = true) as fn(&mut Shared),
                Waited::Done,
            ),
            (|s| s.bind_failed = true, Waited::BindFailed),
            (|s| s.eula_required = true, Waited::EulaRequired),
            (|s| s.exited = Some(false), Waited::Exited),
        ] {
            let shared = signal();
            let spawned = Instant::now();
            let later = {
                let shared = Arc::clone(&shared);
                std::thread::spawn(move || {
                    std::thread::sleep(Duration::from_millis(100));
                    set(&mut lock(&shared));
                    shared.1.notify_all();
                })
            };
            assert_eq!(
                wait_start(&shared, spawned, Duration::from_secs(30), STARTUP_LIMIT),
                expected
            );
            assert!(spawned.elapsed() < Duration::from_secs(10));
            later.join().unwrap();
        }
    }

    #[test]
    fn arguments_bound_the_heap_and_take_no_input_from_the_world() {
        let args = arguments(
            Path::new("/data/shared/servers/x/server.jar"),
            &Main::Jar,
            2048,
        );
        assert_eq!(
            args,
            [
                "-Xms1024M",
                "-Xmx2048M",
                "-Dfile.encoding=UTF-8",
                "-Dlog4j.configurationFile=launcher-log4j2.xml",
                "-Dlog4j2.formatMsgNoLookups=true",
                "-jar",
                "/data/shared/servers/x/server.jar",
                "--nogui"
            ]
        );
    }

    #[test]
    fn a_loader_server_starts_its_class_with_its_libraries() {
        let classpath = [
            std::path::PathBuf::from("/data/shared/maven/a.jar"),
            std::path::PathBuf::from("/data/shared/maven/b.jar"),
        ];
        let args = arguments(
            Path::new("/data/shared/servers/x/server.jar"),
            &Main::Loader {
                kind: LoaderKind::Fabric,
                main_class: "net.fabricmc.loader.impl.launch.knot.KnotServer",
                classpath: &classpath,
            },
            3072,
        );
        let separator = if cfg!(windows) { ";" } else { ":" };
        assert_eq!(
            args[5..],
            [
                "-Dfabric.gameJarPath=/data/shared/servers/x/server.jar".to_owned(),
                "-cp".to_owned(),
                format!("/data/shared/maven/a.jar{separator}/data/shared/maven/b.jar"),
                "net.fabricmc.loader.impl.launch.knot.KnotServer".to_owned(),
                "--nogui".to_owned(),
            ]
        );
        assert!(!args.iter().any(|a| a == "-jar"));

        let quilt = arguments(
            Path::new("/data/shared/servers/x/server.jar"),
            &Main::Loader {
                kind: LoaderKind::Quilt,
                main_class: "org.quiltmc.loader.impl.launch.knot.KnotServer",
                classpath: &classpath,
            },
            3072,
        );
        assert_eq!(
            quilt[5],
            "-Dloader.gameJarPath=/data/shared/servers/x/server.jar"
        );
        assert_eq!(quilt[8], "org.quiltmc.loader.impl.launch.knot.KnotServer");

        // NeoForge: its own start arguments after the launcher's, its log
        // files kept empty, no game JAR property.
        let tokens = [
            "-p".to_owned(),
            "/data/shared/maven/a.jar".to_owned(),
            "cpw.mods.bootstraplauncher.BootstrapLauncher".to_owned(),
        ];
        let neoforge = arguments(
            Path::new("/data/shared/servers/x/server.jar"),
            &Main::Arguments(&tokens),
            3072,
        );
        assert_eq!(&neoforge[..2], ["-Xms1024M", "-Xmx3072M"]);
        assert_eq!(
            &neoforge[5..],
            [
                "-Dforge.logging.file.level=off",
                "-Dforge.logging.debugFile.level=off",
                "-p",
                "/data/shared/maven/a.jar",
                "cpw.mods.bootstraplauncher.BootstrapLauncher",
                "--nogui",
            ]
        );
    }
}
