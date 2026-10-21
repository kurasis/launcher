//! The installer step of Forge, NeoForge and OptiFine (D-039).
//!
//! A Forge-style installer does more than download: it unpacks files it carries
//! and runs "processors", Java tools that remap and binary-patch the game into
//! the jar the loader starts. The launcher implements the installer's protocol
//! (install profile spec 0 and 1) itself, for an installation's game and for a
//! world's dedicated server (W12, [`prepare_server`]):
//!
//! - files under `maven/` in the installer are unpacked into the Maven tree after
//!   their hash is checked;
//! - each processor runs with the installation's private Java, its tool jar and
//!   classpath taken from verified downloads, its arguments filled from the
//!   install profile's `data`; outputs with a declared SHA-1 are checked, and a
//!   processor whose outputs already match is skipped;
//! - Mojang's mappings, which one processor would download itself, are taken
//!   from a verified download instead whenever the processor would copy them
//!   unchanged;
//! - every file the step produced is hashed and recorded in the install lock, so
//!   launch checks it and Repair can rebuild it.
//!
//! The step runs on a blocking thread and stops the running tool on cancel.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use sha1::Digest as _;

use super::coordinate::Coordinate;
use super::manifest::{InstallSide, Installer, LoaderManifest, installer_entry, maven_path};
use crate::download::{CancelToken, resolve_managed_path};
use crate::error::{ErrorCode, LauncherError, LauncherResult};
use crate::install::descriptor::{unsupported, validate_relative_path};
use crate::install::plan::GamePlan;
use crate::storage::atomic;
use crate::storage::models::{ArtifactRole, HashAlgorithm, LockedArtifact};
use crate::storage::paths::{DataRoot, Sha1};

/// What the step needs.
pub struct Inputs<'a> {
    pub root: &'a DataRoot,
    pub manifest: &'a LoaderManifest,
    pub plan: &'a GamePlan,
    /// The installation's private Java.
    pub java: &'a Path,
    /// Scratch directory (`{ROOT}`), removed afterwards.
    pub work_dir: PathBuf,
    pub cancel: &'a CancelToken,
    /// Receives progress and tool output for the console.
    pub log: &'a (dyn Fn(String) + Sync),
    /// How long a step may print nothing ([`QUIET_LIMIT`]).
    pub quiet_limit: Duration,
}

/// What the server side of the step needs (W12): the files go to the Maven
/// tree, where the server's start arguments name them.
pub struct ServerInputs<'a> {
    pub root: &'a DataRoot,
    pub manifest: &'a LoaderManifest,
    /// Mojang's server JAR (`{MINECRAFT_JAR}`).
    pub server_jar: PathBuf,
    /// Mojang's server mappings, verified: DATA_ROOT-relative path and SHA-1.
    pub mappings: Option<(String, Sha1)>,
    /// The world's Java.
    pub java: &'a Path,
    /// Scratch directory (`{ROOT}`), removed afterwards.
    pub work_dir: PathBuf,
    pub cancel: &'a CancelToken,
    pub log: &'a (dyn Fn(String) + Sync),
    pub quiet_limit: Duration,
}

/// One run of the step, for either side.
struct Step<'a> {
    root: &'a DataRoot,
    manifest: &'a LoaderManifest,
    java: &'a Path,
    work_dir: &'a Path,
    cancel: &'a CancelToken,
    log: &'a (dyn Fn(String) + Sync),
    quiet_limit: Duration,
    side: InstallSide,
    game_jar: PathBuf,
    /// Mojang's mappings for the side: DATA_ROOT-relative path and SHA-1.
    mappings: Option<(String, Sha1)>,
}

/// Lines of a tool's output kept for the console when it fails.
const OUTPUT_TAIL: usize = 200;
/// A step that prints nothing for this long is taken as hung (antivirus
/// software holding Java, a stalled download) and ended. The tools print a
/// line per file, so a working step is never this quiet.
pub const QUIET_LIMIT: Duration = Duration::from_secs(10 * 60);

fn storage_error(error: &std::io::Error) -> LauncherError {
    LauncherError::from_io(error, "Preparing the mod loader")
}

fn cancelled() -> LauncherError {
    LauncherError::new(
        ErrorCode::OperationCancelled,
        "The operation was cancelled.",
    )
}

fn failed(loader: &str) -> LauncherError {
    LauncherError::templated(
        ErrorCode::LoaderInstallFailed,
        "The {loader} installer step failed. The console window shows its output.",
        crate::error::params([("loader", loader.to_owned().into())]),
    )
}

fn sha1_file(path: &Path) -> std::io::Result<(String, u64)> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = sha1::Sha1::new();
    let mut buffer = vec![0u8; 1 << 16];
    let mut size = 0u64;
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        size += read as u64;
    }
    Ok((hex::encode(hasher.finalize()), size))
}

fn path_text(path: &Path) -> LauncherResult<String> {
    path.to_str().map(str::to_owned).ok_or_else(|| {
        LauncherError::new(
            ErrorCode::UnsupportedPathEncoding,
            "A game path cannot be represented as text for Java.",
        )
    })
}

fn maven_file(root: &DataRoot, coordinate: &str) -> LauncherResult<PathBuf> {
    resolve_managed_path(root, &maven_path(&Coordinate::parse(coordinate)?.path()))
}

/// Runs the installer step and returns every file it produced, hashed.
pub fn prepare(inputs: &Inputs<'_>) -> LauncherResult<Vec<LockedArtifact>> {
    let manifest = inputs.manifest;
    let installer = manifest
        .installer
        .as_ref()
        .ok_or_else(|| LauncherError::internal("This loader has no installer."))?;
    let installer_path = resolve_managed_path(inputs.root, &maven_path(&installer.path))?;
    let bytes = std::fs::read(&installer_path).map_err(|e| storage_error(&e))?;
    if hex::encode(sha1::Sha1::digest(&bytes)) != installer.sha1.as_str() {
        return Err(LauncherError::new(
            ErrorCode::RepairRequired,
            "A verified mod loader installer is damaged. Repair the installation.",
        ));
    }
    unpack(inputs.root, installer, &bytes)?;
    if !installer.processors.is_empty() {
        let step = Step {
            root: inputs.root,
            manifest,
            java: inputs.java,
            work_dir: &inputs.work_dir,
            cancel: inputs.cancel,
            log: inputs.log,
            quiet_limit: inputs.quiet_limit,
            side: InstallSide::Client,
            game_jar: resolve_managed_path(inputs.root, &inputs.plan.client.path)?,
            mappings: inputs
                .plan
                .mappings
                .as_ref()
                .map(|m| (m.path.clone(), m.sha1.clone())),
        };
        std::fs::create_dir_all(&inputs.work_dir).map_err(|e| storage_error(&e))?;
        let result = run_processors(&step, installer, &installer_path, &bytes);
        let _ = std::fs::remove_dir_all(&inputs.work_dir);
        result?;
    }
    generated(inputs, installer)
}

/// Runs the installer step for a dedicated server and returns every file it
/// produced that the server's start arguments or the install profile name,
/// hashed. Libraries the manifest pins are downloaded, not produced.
pub fn prepare_server(inputs: &ServerInputs<'_>) -> LauncherResult<Vec<LockedArtifact>> {
    let manifest = inputs.manifest;
    let (Some(installer), Some(arguments)) = (&manifest.installer, &manifest.server_arguments)
    else {
        return Err(LauncherError::internal(
            "This loader has no server installer.",
        ));
    };
    let installer_path = resolve_managed_path(inputs.root, &maven_path(&installer.path))?;
    let bytes = std::fs::read(&installer_path).map_err(|e| storage_error(&e))?;
    if hex::encode(sha1::Sha1::digest(&bytes)) != installer.sha1.as_str() {
        return Err(LauncherError::new(
            ErrorCode::RepairRequired,
            "A verified mod loader installer is damaged. Host the world again.",
        ));
    }
    unpack(inputs.root, installer, &bytes)?;
    let step = Step {
        root: inputs.root,
        manifest,
        java: inputs.java,
        work_dir: &inputs.work_dir,
        cancel: inputs.cancel,
        log: inputs.log,
        quiet_limit: inputs.quiet_limit,
        side: InstallSide::Server,
        game_jar: inputs.server_jar.clone(),
        mappings: inputs.mappings.clone(),
    };
    std::fs::create_dir_all(&inputs.work_dir).map_err(|e| storage_error(&e))?;
    let result = run_processors(&step, installer, &installer_path, &bytes);
    let _ = std::fs::remove_dir_all(&inputs.work_dir);
    result?;

    let pinned: std::collections::HashSet<String> = manifest
        .libraries
        .iter()
        .chain(&installer.libraries)
        .filter(|l| !l.url.is_empty())
        .map(|l| maven_path(&l.path))
        .collect();
    let mut paths: Vec<String> = Vec::new();
    arguments.command(&inputs.root.maven_root(), &inputs.server_jar, &mut |rest| {
        paths.push(maven_path(rest));
    })?;
    for value in installer.data.values() {
        if let Some(coordinate) = value.strip_prefix('[').and_then(|v| v.strip_suffix(']')) {
            let path = maven_path(&Coordinate::parse(coordinate)?.path());
            if resolve_managed_path(inputs.root, &path)?.is_file() {
                paths.push(path);
            }
        }
    }
    paths.sort();
    paths.dedup();
    let mut out = Vec::new();
    for path in paths.into_iter().filter(|p| !pinned.contains(p)) {
        let file = resolve_managed_path(inputs.root, &path)?;
        let Ok((sha1, size)) = sha1_file(&file) else {
            (inputs.log)(format!(
                "{} server installer did not produce {path}",
                manifest.kind.name()
            ));
            return Err(failed(manifest.kind.name()));
        };
        out.push(LockedArtifact {
            role: ArtifactRole::Library,
            path,
            size,
            algorithm: HashAlgorithm::Sha1,
            digest: sha1,
            url: String::new(),
            provenance: Some(format!("{} server installer", manifest.kind.name())),
        });
    }
    Ok(out)
}

/// Unpacks the installer's packed files that are missing or damaged.
fn unpack(root: &DataRoot, installer: &Installer, bytes: &[u8]) -> LauncherResult<()> {
    for file in &installer.embedded {
        let destination = resolve_managed_path(root, &maven_path(&file.path))?;
        if sha1_file(&destination).is_ok_and(|(sha1, _)| sha1 == file.sha1.as_str()) {
            continue;
        }
        let content = installer_entry(bytes, &file.entry_name())?;
        if hex::encode(sha1::Sha1::digest(&content)) != file.sha1.as_str() {
            return Err(unsupported("installer entry hash"));
        }
        if let Some(parent) = destination.parent() {
            std::fs::create_dir_all(parent).map_err(|e| storage_error(&e))?;
        }
        atomic::write(&destination, &content).map_err(|e| storage_error(&e))?;
    }
    Ok(())
}

/// Values for `{NAME}` in processor arguments.
fn variables(
    inputs: &Step<'_>,
    installer: &Installer,
    installer_path: &Path,
    bytes: &[u8],
) -> LauncherResult<BTreeMap<String, String>> {
    let root = inputs.root;
    let mut values = BTreeMap::new();
    for (key, value) in &installer.data {
        let resolved = if let Some(coordinate) =
            value.strip_prefix('[').and_then(|v| v.strip_suffix(']'))
        {
            path_text(&maven_file(root, coordinate)?)?
        } else if let Some(literal) = value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')) {
            literal.to_owned()
        } else if let Some(entry) = value.strip_prefix('/') {
            // A file inside the installer, such as the binary patches.
            validate_relative_path(entry)?;
            let target = inputs.work_dir.join("data").join(entry);
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent).map_err(|e| storage_error(&e))?;
            }
            std::fs::write(&target, installer_entry(bytes, entry)?)
                .map_err(|e| storage_error(&e))?;
            path_text(&target)?
        } else {
            value.clone()
        };
        values.insert(key.clone(), resolved);
    }
    values.insert("SIDE".into(), inputs.side.key().into());
    values.insert("MINECRAFT_JAR".into(), path_text(&inputs.game_jar)?);
    values.insert(
        "MINECRAFT_VERSION".into(),
        inputs.manifest.game_version.clone(),
    );
    values.insert("ROOT".into(), path_text(inputs.work_dir)?);
    values.insert("INSTALLER".into(), path_text(installer_path)?);
    values.insert("LIBRARY_DIR".into(), path_text(&root.maven_root())?);
    Ok(values)
}

/// Fills one argument: `[coordinate]` is a Maven-tree path, `{NAME}` a value.
fn fill(
    root: &DataRoot,
    template: &str,
    values: &BTreeMap<String, String>,
) -> LauncherResult<String> {
    if let Some(coordinate) = template.strip_prefix('[').and_then(|v| v.strip_suffix(']')) {
        return path_text(&maven_file(root, coordinate)?);
    }
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let end = after
            .find('}')
            .ok_or_else(|| unsupported("installer argument"))?;
        let value = values
            .get(&after[..end])
            .ok_or_else(|| unsupported("installer argument"))?;
        out.push_str(value);
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

/// The `Main-Class` of a jar's manifest.
fn main_class(jar: &Path) -> LauncherResult<String> {
    let bytes = std::fs::read(jar).map_err(|e| storage_error(&e))?;
    let manifest = installer_entry(&bytes, "META-INF/MANIFEST.MF")?;
    let text = String::from_utf8_lossy(&manifest);
    // Manifest lines continue on the next line when it starts with a space.
    let mut joined = String::new();
    for line in text.lines() {
        if let Some(continued) = line.strip_prefix(' ') {
            joined.push_str(continued);
        } else {
            joined.push('\n');
            joined.push_str(line);
        }
    }
    joined
        .lines()
        .find_map(|l| l.strip_prefix("Main-Class:"))
        .map(|c| c.trim().to_owned())
        .filter(|c| crate::install::descriptor::valid_main_class(c))
        .ok_or_else(|| unsupported("installer tool main class"))
}

fn run_processors(
    inputs: &Step<'_>,
    installer: &Installer,
    installer_path: &Path,
    bytes: &[u8],
) -> LauncherResult<()> {
    let root = inputs.root;
    let values = variables(inputs, installer, installer_path, bytes)?;
    let total = installer.processors.len();
    for (index, processor) in installer.processors.iter().enumerate() {
        if inputs.cancel.is_cancelled() {
            return Err(cancelled());
        }
        let step = index + 1;
        let mut outputs = Vec::new();
        for (file, sha1) in &processor.outputs {
            let file = PathBuf::from(fill(root, file, &values)?);
            let sha1 = fill(root, sha1, &values)?;
            let sha1 = sha1.trim_matches('\'').to_ascii_lowercase();
            outputs.push((file, sha1));
        }
        let up_to_date = !outputs.is_empty()
            && outputs
                .iter()
                .all(|(file, sha1)| sha1_file(file).is_ok_and(|(actual, _)| actual == *sha1));
        if up_to_date {
            (inputs.log)(format!(
                "{} installer step {step}/{total}: already done",
                inputs.manifest.kind.name()
            ));
            continue;
        }
        let args = processor
            .args
            .iter()
            .map(|a| fill(root, a, &values))
            .collect::<LauncherResult<Vec<_>>>()?;
        if copy_mappings(inputs, &args)? {
            (inputs.log)(format!(
                "{} installer step {step}/{total}: {} mappings from the verified download",
                inputs.manifest.kind.name(),
                inputs.side.key()
            ));
        } else {
            let jar = maven_file(root, &processor.jar)?;
            let mut classpath = vec![path_text(&jar)?];
            for entry in &processor.classpath {
                classpath.push(path_text(&maven_file(root, entry)?)?);
            }
            let main = match &processor.main_class {
                Some(main) if crate::install::descriptor::valid_main_class(main) => main.clone(),
                Some(_) => return Err(unsupported("installer tool main class")),
                None => main_class(&jar)?,
            };
            (inputs.log)(format!(
                "{} installer step {step}/{total}: {}",
                inputs.manifest.kind.name(),
                processor.jar
            ));
            run_tool(
                &Tool {
                    java: inputs.java,
                    work_dir: inputs.work_dir,
                    cancel: inputs.cancel,
                    log: inputs.log,
                    quiet_limit: inputs.quiet_limit,
                    loader: inputs.manifest.kind.name(),
                },
                &classpath.join(&crate::storage::paths::CLASSPATH_SEPARATOR.to_string()),
                &main,
                &args,
            )?;
        }
        for (file, sha1) in &outputs {
            let matches = sha1_file(file).is_ok_and(|(actual, _)| actual == *sha1);
            if !matches {
                (inputs.log)(format!(
                    "{} installer step {step}/{total}: an output did not match its checksum",
                    inputs.manifest.kind.name()
                ));
                return Err(failed(inputs.manifest.kind.name()));
            }
        }
    }
    Ok(())
}

/// `DOWNLOAD_MOJMAPS` without `--sanitize` copies Mojang's mappings of the side
/// unchanged; the verified copy replaces the tool's own download.
fn copy_mappings(inputs: &Step<'_>, args: &[String]) -> LauncherResult<bool> {
    let task = args
        .windows(2)
        .find(|w| w[0] == "--task")
        .map(|w| w[1].as_str());
    if task != Some("DOWNLOAD_MOJMAPS")
        || args.iter().any(|a| a == "--sanitize")
        || args
            .windows(2)
            .any(|w| w[0] == "--side" && w[1] != inputs.side.key())
    {
        return Ok(false);
    }
    let (Some((mappings, mappings_sha1)), Some(output)) = (
        inputs.mappings.as_ref(),
        args.windows(2)
            .find(|w| w[0] == "--output")
            .map(|w| PathBuf::from(&w[1])),
    ) else {
        return Ok(false);
    };
    let source = resolve_managed_path(inputs.root, mappings)?;
    let content = std::fs::read(&source).map_err(|e| storage_error(&e))?;
    if hex::encode(sha1::Sha1::digest(&content)) != mappings_sha1.as_str() {
        return Err(LauncherError::new(
            ErrorCode::RepairRequired,
            "Verified game files are damaged. Repair the installation.",
        ));
    }
    if !crate::storage::paths::is_within(&inputs.root.maven_root(), &output) {
        return Err(unsupported("installer output path"));
    }
    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent).map_err(|e| storage_error(&e))?;
    }
    atomic::write(&output, &content).map_err(|e| storage_error(&e))?;
    Ok(true)
}

/// What running one installer tool needs.
struct Tool<'a> {
    java: &'a Path,
    work_dir: &'a Path,
    cancel: &'a CancelToken,
    log: &'a (dyn Fn(String) + Sync),
    quiet_limit: Duration,
    /// The loader's name, for messages.
    loader: &'static str,
}

/// Runs one tool with the private Java and streams its output to the log.
fn run_tool(inputs: &Tool<'_>, classpath: &str, main: &str, args: &[String]) -> LauncherResult<()> {
    let mut command = Command::new(inputs.java);
    command
        // The one tool that downloads (sanitized mappings) uses the system proxy.
        .arg("-Djava.net.useSystemProxies=true")
        .arg("-cp")
        .arg(classpath)
        .arg(main)
        .args(args)
        .current_dir(inputs.work_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = command.spawn().map_err(|e| {
        crate::error::java_blocked(&e)
            .unwrap_or_else(|| LauncherError::from_io(&e, "Starting the mod loader installer"))
            .with_detail("step", main.to_owned())
    })?;
    // The tools print a line per file; only the tail is kept, and shown when
    // the step fails.
    let tail = std::sync::Mutex::new((std::collections::VecDeque::new(), 0usize));
    let last_line = std::sync::Mutex::new(std::time::Instant::now());
    let mut stalled = false;
    let status = std::thread::scope(|scope| {
        let readers: Vec<_> = [
            child
                .stdout
                .take()
                .map(|s| Box::new(s) as Box<dyn Read + Send>),
            child
                .stderr
                .take()
                .map(|s| Box::new(s) as Box<dyn Read + Send>),
        ]
        .into_iter()
        .flatten()
        .map(|stream| {
            let (tail, last_line) = (&tail, &last_line);
            scope.spawn(move || {
                for line in BufReader::new(stream).lines().map_while(Result::ok) {
                    *last_line
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner) =
                        std::time::Instant::now();
                    let mut tail = tail
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    tail.1 += 1;
                    if tail.0.len() == OUTPUT_TAIL {
                        tail.0.pop_front();
                    }
                    tail.0.push_back(line);
                }
            })
        })
        .collect();
        let status = loop {
            if inputs.cancel.is_cancelled() {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            let quiet = last_line
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .elapsed();
            match child.try_wait() {
                Ok(Some(status)) => break Some(status),
                Ok(None) if quiet > inputs.quiet_limit => {
                    stalled = true;
                    let _ = child.kill();
                    let _ = child.wait();
                    break None;
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(100)),
                Err(_) => {
                    let _ = child.kill();
                    break child.wait().ok();
                }
            }
        };
        for reader in readers {
            let _ = reader.join();
        }
        status
    });
    let (lines, count) = tail
        .into_inner()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if stalled || status.as_ref().is_some_and(|s| !s.success()) {
        if count > lines.len() {
            (inputs.log)(format!("  … {} earlier lines", count - lines.len()));
        }
        for line in lines {
            (inputs.log)(format!("  {line}"));
        }
    } else {
        (inputs.log)(format!("  {count} lines of output"));
    }
    match status {
        None if stalled => {
            (inputs.log)(format!(
                "{} installer step printed nothing for {} minutes and was ended",
                inputs.loader,
                inputs.quiet_limit.as_secs() / 60
            ));
            Err(LauncherError::templated(
                ErrorCode::LoaderInstallFailed,
                "The {loader} installer stopped responding and was ended. Antivirus software may hold Java: allow the launcher's data folder in it, then try again.",
                crate::error::params([("loader", inputs.loader.to_owned().into())]),
            ))
        }
        None => Err(cancelled()),
        Some(status) if status.success() => Ok(()),
        Some(status) => {
            (inputs.log)(format!(
                "{} installer step exited with {status}",
                inputs.loader
            ));
            Err(failed(inputs.loader))
        }
    }
}

/// Every file the step produced: unpacked files, the classpath files the
/// installer provides, and the processors' outputs named in `data`.
fn generated(inputs: &Inputs<'_>, installer: &Installer) -> LauncherResult<Vec<LockedArtifact>> {
    let root = inputs.root;
    let downloaded: std::collections::HashSet<&str> =
        inputs.plan.artifacts().map(|a| a.path.as_str()).collect();
    let mut paths: Vec<String> = installer
        .embedded
        .iter()
        .map(|e| maven_path(&e.path))
        .collect();
    let provided = inputs
        .plan
        .loader
        .as_ref()
        .map(|l| l.provided.clone())
        .unwrap_or_default();
    paths.extend(provided.iter().map(|p| p.path.clone()));
    for value in installer.data.values() {
        if let Some(coordinate) = value.strip_prefix('[').and_then(|v| v.strip_suffix(']')) {
            let path = maven_path(&Coordinate::parse(coordinate)?.path());
            if resolve_managed_path(root, &path)?.is_file() {
                paths.push(path);
            }
        }
    }
    paths.sort();
    paths.dedup();
    let mut out = Vec::new();
    for path in paths {
        if downloaded.contains(path.as_str()) {
            continue;
        }
        let file = resolve_managed_path(root, &path)?;
        let Ok((sha1, size)) = sha1_file(&file) else {
            (inputs.log)(format!(
                "{} installer did not produce {path}",
                inputs.manifest.kind.name()
            ));
            return Err(failed(inputs.manifest.kind.name()));
        };
        if let Some(expected) = provided
            .iter()
            .find(|p| p.path == path)
            .and_then(|p| p.sha1.as_ref())
            && expected.as_str() != sha1
        {
            (inputs.log)(format!(
                "{} installer produced {path} with an unexpected checksum",
                inputs.manifest.kind.name()
            ));
            return Err(failed(inputs.manifest.kind.name()));
        }
        out.push(LockedArtifact {
            role: ArtifactRole::Library,
            path,
            size,
            algorithm: HashAlgorithm::Sha1,
            digest: sha1,
            url: String::new(),
            provenance: Some(format!("{} installer", inputs.manifest.kind.name())),
        });
    }
    Ok(out)
}

/// Whether every generated file is present with its recorded hash.
pub fn verify_generated(root: &DataRoot, generated: &[LockedArtifact]) -> bool {
    generated.iter().all(|a| {
        resolve_managed_path(root, &a.path)
            .ok()
            .and_then(|path| sha1_file(&path).ok())
            .is_some_and(|(sha1, size)| sha1 == a.digest && size == a.size)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arguments_fill_values_and_coordinates() {
        let dir = tempfile::tempdir().unwrap();
        let root = DataRoot::new(dir.path().to_path_buf()).unwrap();
        let mut values = BTreeMap::new();
        values.insert("SIDE".to_owned(), "client".to_owned());
        values.insert("MOJMAPS".to_owned(), "/x/maps.txt".to_owned());
        assert_eq!(fill(&root, "--side", &values).unwrap(), "--side");
        assert_eq!(fill(&root, "{SIDE}", &values).unwrap(), "client");
        assert_eq!(
            fill(&root, "a{SIDE}b{MOJMAPS}", &values).unwrap(),
            "aclientb/x/maps.txt"
        );
        let path = fill(&root, "[net.minecraft:client:1.21.1:mappings@txt]", &values).unwrap();
        assert!(
            path.ends_with(
                &[
                    "shared",
                    "maven",
                    "net",
                    "minecraft",
                    "client",
                    "1.21.1",
                    "client-1.21.1-mappings.txt"
                ]
                .join(std::path::MAIN_SEPARATOR_STR)
            )
        );
        assert!(fill(&root, "{MISSING}", &values).is_err());
        assert!(fill(&root, "[../x:y:z]", &values).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn a_silent_installer_step_is_ended() {
        use std::os::unix::fs::PermissionsExt;
        // A "Java" that prints one line and then hangs, as when antivirus
        // software holds it.
        let dir = tempfile::tempdir().unwrap();
        let java = dir.path().join("java");
        std::fs::write(&java, "#!/bin/sh\necho started\nexec sleep 60\n").unwrap();
        std::fs::set_permissions(&java, std::fs::Permissions::from_mode(0o755)).unwrap();
        let lines = std::sync::Mutex::new(Vec::new());
        let log = |line: String| lines.lock().unwrap().push(line);
        let cancel = CancelToken::new();
        let started = std::time::Instant::now();
        let error = run_tool(
            &Tool {
                java: &java,
                work_dir: dir.path(),
                cancel: &cancel,
                log: &log,
                quiet_limit: Duration::from_secs(1),
                loader: "NeoForge",
            },
            "x.jar",
            "Main",
            &[],
        )
        .unwrap_err();
        assert!(started.elapsed() < Duration::from_secs(10));
        assert_eq!(error.code, ErrorCode::LoaderInstallFailed);
        assert!(
            error.message.contains("stopped responding"),
            "{}",
            error.message
        );
        assert!(lines.lock().unwrap().iter().any(|l| l.contains("started")));
    }

    #[cfg(unix)]
    #[test]
    fn a_java_without_permission_to_run_is_named_as_blocked() {
        let dir = tempfile::tempdir().unwrap();
        let java = dir.path().join("java");
        std::fs::write(&java, "#!/bin/sh\n").unwrap();
        let cancel = CancelToken::new();
        let error = run_tool(
            &Tool {
                java: &java,
                work_dir: dir.path(),
                cancel: &cancel,
                log: &|_| {},
                quiet_limit: QUIET_LIMIT,
                loader: "Forge",
            },
            "x.jar",
            "Main",
            &[],
        )
        .unwrap_err();
        assert_eq!(error.code, ErrorCode::PermissionDenied);
        assert!(error.message.contains("Java was not allowed to start"));
    }
}
