//! Where the launcher keeps its data (D-043).
//!
//! In order of precedence:
//!
//! 1. **Portable mode:** a file named [`PORTABLE_MARKER`] next to the launcher
//!    (next to the `.app` on macOS, next to the AppImage on Linux) keeps all data in
//!    [`PORTABLE_DATA_DIR`] beside it, so the folder can move to another computer.
//! 2. **Chosen folder:** [`POINTER_FILE`] in the per-user configuration directory
//!    names a folder the user picked in Settings.
//! 3. **Default:** the per-user application data directory.
//!
//! Moving to another folder copies the data while nothing else runs, then records
//! the new folder; the launcher uses it from its next start. The old folder is
//! kept until the user deletes it.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{ErrorCode, LauncherError, LauncherResult};
use crate::storage::atomic;
use crate::storage::document::SCHEMA_VERSION;
use crate::storage::paths::DataRoot;

/// File next to the launcher that turns on portable mode.
pub const PORTABLE_MARKER: &str = "portable.txt";
/// Data folder of portable mode, beside the marker.
pub const PORTABLE_DATA_DIR: &str = "launcher-data";
/// Names the chosen data folder; lives in the per-user configuration directory.
pub const POINTER_FILE: &str = "data-location.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LocationMode {
    Default,
    Custom,
    Portable,
}

/// The data folder in use and how it was chosen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DataLocation {
    pub mode: LocationMode,
    pub path: PathBuf,
    /// The per-user default folder.
    pub default_path: PathBuf,
    /// Where portable mode keeps its data (beside the launcher), if known.
    pub portable_path: Option<PathBuf>,
    /// A move recorded this session; the launcher uses it from its next start.
    pub pending_path: Option<PathBuf>,
    /// The pending folder already held launcher data, which is used as it is.
    #[serde(default)]
    pub pending_reused: bool,
}

/// What a new data folder holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetState {
    /// Empty or missing: the data is copied there.
    Empty,
    /// Launcher data from before: the launcher switches to it without copying.
    LauncherData,
}

/// A folder the launcher wrote: settings plus its instances or compatibility list.
fn holds_launcher_data(path: &Path) -> bool {
    path.join("settings.json").is_file()
        && (path.join("instances").is_dir() || path.join("compatibility.json").is_file())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Pointer {
    schema_version: u64,
    path: PathBuf,
}

/// The folder that holds the launcher and, in portable mode, its marker.
pub fn launcher_folder() -> Option<PathBuf> {
    // An AppImage runs from a temporary mount; the image file itself is the
    // launcher the user sees.
    if let Some(image) = std::env::var_os("APPIMAGE") {
        return PathBuf::from(image).parent().map(Path::to_path_buf);
    }
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    // macOS: Name.app/Contents/MacOS/launcher → the folder holding Name.app.
    if dir.ends_with("Contents/MacOS")
        && let Some(bundle) = dir.parent().and_then(Path::parent)
        && bundle.extension().is_some_and(|e| e == "app")
    {
        return bundle.parent().map(Path::to_path_buf);
    }
    Some(dir.to_path_buf())
}

/// Resolves the data folder. `config_dir` holds the pointer file; `default` is
/// the per-user data directory.
pub fn resolve(launcher_dir: Option<&Path>, config_dir: &Path, default: PathBuf) -> DataLocation {
    let portable_path = launcher_dir.map(|d| d.join(PORTABLE_DATA_DIR));
    if let Some(dir) = launcher_dir
        && dir.join(PORTABLE_MARKER).is_file()
    {
        return DataLocation {
            mode: LocationMode::Portable,
            path: dir.join(PORTABLE_DATA_DIR),
            default_path: default,
            portable_path,
            pending_path: None,
            pending_reused: false,
        };
    }
    let chosen = fs::read(config_dir.join(POINTER_FILE))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Pointer>(&bytes).ok())
        .map(|p| p.path)
        .filter(|p| p.is_absolute());
    match chosen {
        Some(path) => DataLocation {
            mode: LocationMode::Custom,
            path,
            default_path: default,
            portable_path,
            pending_path: None,
            pending_reused: false,
        },
        None => DataLocation {
            mode: LocationMode::Default,
            path: default.clone(),
            default_path: default,
            portable_path,
            pending_path: None,
            pending_reused: false,
        },
    }
}

/// Whether a folder can receive a copy: missing, or empty.
fn is_empty_or_missing(path: &Path) -> LauncherResult<bool> {
    match fs::read_dir(path) {
        Ok(mut entries) => Ok(entries.next().is_none()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(true),
        Err(e) => Err(LauncherError::from_io(&e, "Reading the new data folder")),
    }
}

/// Checks a requested new data folder against the current one.
pub fn check_target(current: &Path, target: &Path) -> LauncherResult<(DataRoot, TargetState)> {
    if !target.is_absolute() {
        return Err(LauncherError::invalid_input("Choose a full folder path."));
    }
    let root = DataRoot::new(target.to_path_buf())?;
    let same = |a: &Path, b: &Path| {
        fs::canonicalize(a)
            .ok()
            .zip(fs::canonicalize(b).ok())
            .is_some_and(|(a, b)| a == b)
            || a == b
    };
    if same(current, target) {
        return Err(LauncherError::invalid_input(
            "The launcher already uses this folder.",
        ));
    }
    let current_real = fs::canonicalize(current).unwrap_or_else(|_| current.to_path_buf());
    let target_real = target
        .ancestors()
        .find_map(|a| fs::canonicalize(a).ok().map(|real| (a, real)))
        .map(|(a, real)| real.join(target.strip_prefix(a).unwrap_or(Path::new(""))))
        .unwrap_or_else(|| target.to_path_buf());
    if target_real.starts_with(&current_real) || current_real.starts_with(&target_real) {
        return Err(LauncherError::invalid_input(
            "The new folder cannot be inside the current data folder, or contain it.",
        ));
    }
    if is_empty_or_missing(target)? {
        Ok((root, TargetState::Empty))
    } else if holds_launcher_data(target) {
        Ok((root, TargetState::LauncherData))
    } else {
        Err(LauncherError::invalid_input(
            "Choose an empty folder, one that does not exist yet, or one that already holds launcher data.",
        ))
    }
}

/// Top-level entries of a data folder that are not copied: the single-instance
/// lock and job-owned download staging.
const NOT_COPIED: &[&str] = &["locks", "downloads"];

/// Copies a data folder. Symbolic links are refused: the launcher never creates
/// them, so one would point outside its data.
pub fn copy_tree(from: &Path, to: &Path, progress: &dyn Fn(u64, u64)) -> LauncherResult<()> {
    let mut files = 0u64;
    let mut bytes = 0u64;
    copy_dir(from, to, true, &mut files, &mut bytes, progress)
}

fn copy_dir(
    from: &Path,
    to: &Path,
    top: bool,
    files: &mut u64,
    bytes: &mut u64,
    progress: &dyn Fn(u64, u64),
) -> LauncherResult<()> {
    fs::create_dir_all(to)
        .map_err(|e| LauncherError::from_io(&e, "Creating the new data folder"))?;
    for entry in
        fs::read_dir(from).map_err(|e| LauncherError::from_io(&e, "Reading the data folder"))?
    {
        let entry = entry.map_err(|e| LauncherError::from_io(&e, "Reading the data folder"))?;
        let name = entry.file_name();
        if top && NOT_COPIED.iter().any(|n| name == *n) {
            continue;
        }
        let kind = entry
            .file_type()
            .map_err(|e| LauncherError::from_io(&e, "Reading the data folder"))?;
        let target = to.join(&name);
        if kind.is_symlink() {
            return Err(LauncherError::new(
                ErrorCode::UnsafeArchive,
                "The data folder contains a symbolic link; it was not copied.",
            ));
        } else if kind.is_dir() {
            copy_dir(&entry.path(), &target, false, files, bytes, progress)?;
        } else {
            *bytes += fs::copy(entry.path(), &target)
                .map_err(|e| LauncherError::from_io(&e, "Copying launcher data"))?;
            *files += 1;
            if files.is_multiple_of(500) {
                progress(*files, *bytes);
            }
        }
    }
    if top {
        progress(*files, *bytes);
    }
    Ok(())
}

/// Records the chosen folder for the next start; `None` returns to the default.
pub fn write_pointer(config_dir: &Path, path: Option<&Path>) -> LauncherResult<()> {
    let file = config_dir.join(POINTER_FILE);
    match path {
        None => match fs::remove_file(&file) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                Err(LauncherError::from_io(&e, "Saving the data folder choice"))
            }
            _ => Ok(()),
        },
        Some(path) => {
            fs::create_dir_all(config_dir)
                .map_err(|e| LauncherError::from_io(&e, "Saving the data folder choice"))?;
            let bytes = serde_json::to_vec_pretty(&Pointer {
                schema_version: SCHEMA_VERSION,
                path: path.to_path_buf(),
            })
            .map_err(|_| LauncherError::internal("The data folder choice could not be saved."))?;
            atomic::write(&file, &bytes)
                .map_err(|e| LauncherError::from_io(&e, "Saving the data folder choice"))
        }
    }
}

/// Turns on portable mode next to the launcher.
pub fn write_marker(launcher_dir: &Path) -> LauncherResult<()> {
    fs::write(
        launcher_dir.join(PORTABLE_MARKER),
        "This launcher keeps its data in the launcher-data folder next to it.\n\
         Delete this file to use the per-user data folder again.\n",
    )
    .map_err(|e| LauncherError::from_io(&e, "Turning on portable mode"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn portable_marker_wins_then_the_pointer_then_the_default() {
        let dir = tempfile::tempdir().unwrap();
        let (app, config) = (dir.path().join("app"), dir.path().join("config"));
        fs::create_dir_all(&app).unwrap();
        let default = dir.path().join("default");
        let found = resolve(Some(&app), &config, default.clone());
        assert_eq!(found.mode, LocationMode::Default);
        assert_eq!(found.path, default);

        let chosen = dir.path().join("games");
        write_pointer(&config, Some(&chosen)).unwrap();
        let found = resolve(Some(&app), &config, default.clone());
        assert_eq!((found.mode, found.path), (LocationMode::Custom, chosen));

        write_marker(&app).unwrap();
        let found = resolve(Some(&app), &config, default.clone());
        assert_eq!(found.mode, LocationMode::Portable);
        assert_eq!(found.path, app.join(PORTABLE_DATA_DIR));

        fs::remove_file(app.join(PORTABLE_MARKER)).unwrap();
        write_pointer(&config, None).unwrap();
        assert_eq!(
            resolve(Some(&app), &config, default).mode,
            LocationMode::Default
        );
    }

    #[test]
    fn a_relative_or_broken_pointer_means_the_default() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join(POINTER_FILE),
            r#"{"schemaVersion":1,"path":"rel"}"#,
        )
        .unwrap();
        assert_eq!(
            resolve(None, dir.path(), dir.path().join("d")).mode,
            LocationMode::Default
        );
        fs::write(dir.path().join(POINTER_FILE), "garbage").unwrap();
        assert_eq!(
            resolve(None, dir.path(), dir.path().join("d")).mode,
            LocationMode::Default
        );
    }

    #[test]
    fn targets_must_be_new_empty_and_apart() {
        let dir = tempfile::tempdir().unwrap();
        let current = dir.path().join("data");
        fs::create_dir_all(current.join("instances")).unwrap();
        assert!(check_target(&current, &current).is_err());
        assert!(check_target(&current, &current.join("instances").join("x")).is_err());
        assert!(check_target(&current.join("instances"), &current).is_err());
        assert!(check_target(&current, Path::new("relative")).is_err());
        let busy = dir.path().join("busy");
        fs::create_dir_all(&busy).unwrap();
        fs::write(busy.join("f"), "x").unwrap();
        assert!(check_target(&current, &busy).is_err());
        let state = |p: &Path| check_target(&current, p).unwrap().1;
        assert_eq!(state(&dir.path().join("new")), TargetState::Empty);
        let empty = dir.path().join("empty");
        fs::create_dir_all(&empty).unwrap();
        assert_eq!(state(&empty), TargetState::Empty);
        let earlier = dir.path().join("earlier");
        fs::create_dir_all(earlier.join("instances")).unwrap();
        fs::write(earlier.join("settings.json"), "{}").unwrap();
        assert_eq!(state(&earlier), TargetState::LauncherData);
    }

    #[test]
    fn copies_data_but_not_locks_or_staging() {
        let dir = tempfile::tempdir().unwrap();
        let (from, to) = (dir.path().join("from"), dir.path().join("to"));
        for (path, text) in [
            ("settings.json", "{}"),
            ("instances/a/instance.json", "{}"),
            ("shared/maven/x/y.jar", "jar"),
            ("locks/supervisor.lock", "lock"),
            ("downloads/job/part", "partial"),
        ] {
            let file = from.join(path);
            fs::create_dir_all(file.parent().unwrap()).unwrap();
            fs::write(file, text).unwrap();
        }
        let seen = std::cell::Cell::new((0, 0));
        copy_tree(&from, &to, &|files, bytes| seen.set((files, bytes))).unwrap();
        assert_eq!(
            fs::read_to_string(to.join("shared/maven/x/y.jar")).unwrap(),
            "jar"
        );
        assert!(to.join("instances/a/instance.json").is_file());
        assert!(!to.join("locks").exists());
        assert!(!to.join("downloads").exists());
        assert_eq!(seen.get(), (3, 7));
        assert!(
            from.join("settings.json").is_file(),
            "the old folder is kept"
        );
    }
}
