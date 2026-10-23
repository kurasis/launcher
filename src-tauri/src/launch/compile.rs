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
    pub notoken: bool,
    /// An account started without the online check (D-052).
    pub offline: bool,
}

pub struct LaunchInputs<'a> {
    pub plan: &'a GamePlan,
    pub root: &'a DataRoot,
    pub game_dir: &'a Path,
    /// `${game_assets}`: the by-name asset directory of legacy versions.
    pub game_assets: &'a Path,
    pub natives_dir: &'a Path,
    pub max_heap_mib: u32,
    pub resolution: Resolution,
    pub identity: &'a LaunchIdentity,
    pub launcher_version: &'a str,
    /// The user's JVM options (D-044); later options win in the JVM.
    pub extra_jvm_args: &'a [String],
    /// The user's Minecraft arguments, after the version's own (D-056).
    pub extra_game_args: &'a [String],
    pub fullscreen: bool,
    /// Launcher Skins (D-065): the verified Java agent for a game without a
    /// loader, passed as `-javaagent` before the user's options.
    pub skins_agent: Option<&'a Path>,
    /// The skins API address of a test build (`-Dlauncherskins.api`); `None`
    /// leaves the mod's own default.
    pub skins_api: Option<&'a str>,
    /// The launcher's tweaker jar for a game before 1.6 that takes one
    /// ([`super::legacy::legacy_tweaker`]): added to the classpath, its class named
    /// with `--tweakClass` or in place of Mojang's `AlphaVanillaTweaker`.
    pub legacy_tweaker: Option<&'a Path>,
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
    if let Some(jar) = inputs.legacy_tweaker {
        let text = path_text(jar)?;
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
    values.insert("version_name", plan.version_name().to_owned());
    // Mod loaders (D-039); unused by vanilla arguments.
    let library_directory = path_text(&inputs.root.maven_root())?;
    if library_directory.contains(CLASSPATH_SEPARATOR) {
        return Err(LauncherError::new(
            ErrorCode::UnsupportedDataPath,
            "A game file path contains the Java classpath separator.",
        ));
    }
    values.insert("library_directory", library_directory);
    values.insert("version_type", plan.version_type.clone());
    values.insert("game_directory", path_text(inputs.game_dir)?);
    values.insert("assets_root", path_text(&inputs.root.assets_root())?);
    values.insert("assets_index_name", plan.asset_index.id.as_str().to_owned());
    values.insert("game_assets", path_text(inputs.game_assets)?);
    // Legacy arguments: no Twitch or skin properties are passed.
    values.insert("user_properties", "{}".into());
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
    // The pre-1.7 session format; it contains the token, so game arguments only.
    let session = format!("token:{token}:{}", identity.uuid);
    values.insert("auth_session", session.clone());

    // The initial heap is min(512 MiB, maximum) (spec 01/06); options the
    // player added come later and win.
    let mut jvm_args = vec![
        format!("-Xmx{}M", inputs.max_heap_mib),
        format!(
            "-Xms{}M",
            crate::validation::MemoryPolicy::initial_heap_mib(inputs.max_heap_mib)
        ),
    ];
    let mut native_dirs = Vec::new();
    for template in &plan.jvm_arguments {
        if template.contains("${auth_access_token}") || template.contains("${auth_session}") {
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
    // Launcher Skins (D-065), before the user's options.
    if let Some(api) = inputs.skins_api {
        jvm_args.push(format!("-Dlauncherskins.api={api}"));
    }
    if let Some(agent) = inputs.skins_agent {
        jvm_args.push(format!("-javaagent:{}", path_text(agent)?));
    }
    jvm_args.extend(inputs.extra_jvm_args.iter().cloned());
    let mut game_args = plan
        .game_arguments
        .iter()
        .map(|template| substitute(template, &values))
        .collect::<LauncherResult<Vec<_>>>()?;
    if inputs.legacy_tweaker.is_some() {
        use super::legacy::{ALPHA_TWEAKER_CLASS, ALPHA_VANILLA_TWEAKER, LegacyTweaker};
        match super::legacy::legacy_tweaker(plan) {
            Some(LegacyTweaker::GameFolder) => {
                game_args.push("--tweakClass".to_owned());
                game_args.push(super::legacy::LEGACY_TWEAKER_CLASS.to_owned());
            }
            Some(LegacyTweaker::WindowClose) => {
                if let Some(at) = game_args
                    .windows(2)
                    .position(|pair| pair[0] == "--tweakClass" && pair[1] == ALPHA_VANILLA_TWEAKER)
                {
                    ALPHA_TWEAKER_CLASS.clone_into(&mut game_args[at + 1]);
                }
            }
            None => {}
        }
    }
    // Plans are evaluated with `is_demo_user` false; both qualified descriptors
    // map that feature to exactly this argument. Mod loaders pass it on to the
    // game's own main class.
    if identity.notoken {
        game_args.push("--demo".to_owned());
    }
    if inputs.fullscreen {
        game_args.push("--fullscreen".to_owned());
    }
    game_args.extend(inputs.extra_game_args.iter().cloned());
    Ok(CompiledLaunch {
        working_dir: inputs.game_dir.to_path_buf(),
        jvm_args,
        main_class: plan.main_class.clone(),
        game_args,
        native_dirs,
        secrets: vec![session, token],
    })
}

#[cfg(test)]
mod tests;
