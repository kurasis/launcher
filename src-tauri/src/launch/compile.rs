//! Launch argument compiler (docs/spec/05 "Argument compiler").
//!
//! Produces an ordered argument vector, never a shell string: memory setting,
//! metadata JVM arguments, logging argument, main class, metadata game
//! arguments. Every placeholder in an included argument must have an explicit
//! value; an unknown one is a hard error. The access token may only appear in
//! game arguments, which are passed directly to the process and never written to
//! an argument file.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::error::{ErrorCode, LauncherError, LauncherResult};
use crate::install::plan::GamePlan;
use crate::secrets::SecretString;
use crate::storage::models::Resolution;
use crate::storage::paths::{CLASSPATH_SEPARATOR, DataRoot};

pub const LAUNCHER_NAME: &str = "launcher";
pub const REDACTED: &str = "<redacted>";

/// The verified account data a launch needs. Built from a fresh launch
/// authorization; never persisted.
pub struct LaunchIdentity {
    pub player_name: String,
    /// 32 hex characters without dashes, as returned by the profile service.
    pub uuid: String,
    pub access_token: SecretString,
    /// Only from the authenticated XSTS claim; empty when the service gave none.
    pub xuid: Option<String>,
    pub client_id: String,
    /// Minecraft's demo mode (`--demo`). Only the debug-build test profile sets it;
    /// a signed-in account never does.
    pub demo: bool,
}

pub struct LaunchInputs<'a> {
    pub plan: &'a GamePlan,
    pub root: &'a DataRoot,
    pub game_dir: &'a Path,
    pub natives_dir: &'a Path,
    pub max_heap_mib: u32,
    pub resolution: Resolution,
    pub identity: &'a LaunchIdentity,
    pub launcher_version: &'a str,
}

/// A fully resolved launch, ready to spawn.
pub struct CompiledLaunch {
    pub working_dir: PathBuf,
    /// Non-secret JVM arguments, including the classpath; may go to an argfile.
    pub jvm_args: Vec<String>,
    pub main_class: String,
    /// Game arguments; contain the access token.
    pub game_args: Vec<String>,
    /// Directories named by `${natives_directory}/<suffix>` arguments.
    pub native_dirs: Vec<PathBuf>,
    secrets: Vec<String>,
}

impl CompiledLaunch {
    /// Values that must never appear in logs or previews.
    pub fn secrets(&self) -> &[String] {
        &self.secrets
    }

    /// Human-readable argument list with secrets replaced and the home directory
    /// shortened to `~`.
    pub fn preview(&self, home: Option<&Path>) -> Vec<String> {
        let home = home.and_then(Path::to_str).filter(|h| h.len() > 1);
        self.jvm_args
            .iter()
            .chain(std::iter::once(&self.main_class))
            .chain(self.game_args.iter())
            .map(|arg| {
                let mut text = arg.clone();
                for secret in &self.secrets {
                    text = text.replace(secret.as_str(), REDACTED);
                }
                match home {
                    Some(home) => text.replace(home, "~"),
                    None => text,
                }
            })
            .collect()
    }
}

fn unresolved(name: &str) -> LauncherError {
    LauncherError::new(
        ErrorCode::UnsupportedVersionMetadata,
        "The version metadata uses a launch setting this launcher does not support.",
    )
    .with_detail("placeholder", name.to_owned())
}

fn path_text(path: &Path) -> LauncherResult<String> {
    path.to_str().map(str::to_owned).ok_or_else(|| {
        LauncherError::new(
            ErrorCode::UnsupportedPathEncoding,
            "A game path cannot be represented as text for Java.",
        )
    })
}

/// Replaces every `${name}` in `template`. Unknown names are errors.
pub fn substitute(template: &str, values: &HashMap<&str, String>) -> LauncherResult<String> {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(start) = rest.find("${") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let end = after.find('}').ok_or_else(|| unresolved("unterminated"))?;
        let name = &after[..end];
        let value = values.get(name).ok_or_else(|| unresolved(name))?;
        out.push_str(value);
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

fn safe_suffix(suffix: &str) -> bool {
    !suffix.is_empty()
        && suffix.split('/').all(|part| {
            !part.is_empty()
                && part != "."
                && part != ".."
                && part
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        })
}

pub fn compile(inputs: &LaunchInputs<'_>) -> LauncherResult<CompiledLaunch> {
    let plan = inputs.plan;
    let root = inputs.root.path();
    let identity = inputs.identity;
    let mut classpath = Vec::with_capacity(plan.classpath.len());
    for entry in &plan.classpath {
        let mut path = root.to_path_buf();
        path.extend(entry.split('/'));
        let text = path_text(&path)?;
        if text.contains(CLASSPATH_SEPARATOR) {
            return Err(LauncherError::new(
                ErrorCode::UnsupportedDataPath,
                "A game file path contains the Java classpath separator.",
            ));
        }
        classpath.push(text);
    }
    let natives = path_text(inputs.natives_dir)?;
    let token = identity.access_token.expose().to_owned();
    if token.is_empty() {
        return Err(LauncherError::new(
            ErrorCode::AuthReauthRequired,
            "Sign in again to play.",
        ));
    }
    let mut values: HashMap<&str, String> = HashMap::new();
    values.insert("auth_player_name", identity.player_name.clone());
    values.insert("auth_uuid", identity.uuid.clone());
    values.insert("auth_xuid", identity.xuid.clone().unwrap_or_default());
    values.insert("clientid", identity.client_id.clone());
    values.insert("user_type", "msa".into());
    values.insert("version_name", plan.version_id.clone());
    values.insert("version_type", plan.version_type.clone());
    values.insert("game_directory", path_text(inputs.game_dir)?);
    values.insert("assets_root", path_text(&inputs.root.assets_root())?);
    values.insert("assets_index_name", plan.asset_index.id.as_str().to_owned());
    values.insert("natives_directory", natives.clone());
    values.insert("launcher_name", LAUNCHER_NAME.into());
    values.insert("launcher_version", inputs.launcher_version.to_owned());
    values.insert(
        "classpath",
        classpath.join(&CLASSPATH_SEPARATOR.to_string()),
    );
    values.insert("classpath_separator", CLASSPATH_SEPARATOR.to_string());
    values.insert("resolution_width", inputs.resolution.width.to_string());
    values.insert("resolution_height", inputs.resolution.height.to_string());

    // The token is substituted into game arguments only.
    let mut jvm_values = values.clone();
    jvm_values.remove("auth_access_token");
    values.insert("auth_access_token", token.clone());

    let mut jvm_args = vec![format!("-Xmx{}M", inputs.max_heap_mib)];
    let mut native_dirs = Vec::new();
    for template in &plan.jvm_arguments {
        if template.contains("${auth_access_token}") {
            return Err(unresolved("auth_access_token in JVM arguments"));
        }
        if let Some(index) = template.find("${natives_directory}/") {
            let suffix = &template[index + "${natives_directory}/".len()..];
            if !safe_suffix(suffix) {
                return Err(unresolved("natives_directory suffix"));
            }
            let mut dir = inputs.natives_dir.to_path_buf();
            dir.extend(suffix.split('/'));
            native_dirs.push(dir);
        }
        jvm_args.push(substitute(template, &jvm_values)?);
    }
    if let Some(logging) = &plan.logging {
        let mut path = root.to_path_buf();
        path.extend(logging.artifact.path.split('/'));
        let mut logging_values = HashMap::new();
        logging_values.insert("path", path_text(&path)?);
        jvm_args.push(substitute(&logging.argument, &logging_values)?);
    }
    let mut game_args = plan
        .game_arguments
        .iter()
        .map(|template| substitute(template, &values))
        .collect::<LauncherResult<Vec<_>>>()?;
    // Plans are evaluated with `is_demo_user` false; both qualified descriptors
    // map that feature to exactly this argument.
    if identity.demo {
        game_args.push("--demo".to_owned());
    }
    Ok(CompiledLaunch {
        working_dir: inputs.game_dir.to_path_buf(),
        jvm_args,
        main_class: plan.main_class.clone(),
        game_args,
        native_dirs,
        secrets: vec![token],
    })
}

#[cfg(test)]
mod tests;
