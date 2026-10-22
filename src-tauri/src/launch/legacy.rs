//! Launch preparation for older versions (D-035): native jars extracted into
//! the run's natives directory, asset files laid out by name for the legacy
//! asset schemes, and the launcher's tweaker for games before 1.6. Modern
//! versions need none of them.

use std::fs;
use std::io::{self, Read as _};
use std::path::{Path, PathBuf};

use serde_json::Value;
use sha2::{Digest as _, Sha256};

use crate::error::{ErrorCode, LauncherError, LauncherResult};
use crate::install::descriptor::validate_relative_path;
use crate::install::plan::{AssetLayout, GamePlan, NativeJar};
use crate::storage::paths::{DataRoot, Sha1};

/// Bound for everything extracted from native jars of one launch.
pub const MAX_NATIVE_BYTES: u64 = 256 * 1024 * 1024;

fn storage(e: &io::Error) -> LauncherError {
    LauncherError::from_io(e, "Preparing the game files")
}

fn unsafe_jar() -> LauncherError {
    LauncherError::new(
        ErrorCode::UnsafeArchive,
        "A native library archive contains an unsafe entry. Repair the installation.",
    )
}

/// Extracts every native jar into `dest`, skipping excluded prefixes and
/// directories. Entry names must stay inside `dest`.
pub fn extract_natives(root: &DataRoot, natives: &[NativeJar], dest: &Path) -> LauncherResult<()> {
    let mut total = 0u64;
    for jar in natives {
        let mut path = root.path().to_path_buf();
        path.extend(jar.path.split('/'));
        let file = fs::File::open(&path).map_err(|e| storage(&e))?;
        let mut archive =
            zip::ZipArchive::new(io::BufReader::new(file)).map_err(|_| unsafe_jar())?;
        for index in 0..archive.len() {
            let mut entry = archive.by_index(index).map_err(|_| unsafe_jar())?;
            let name = entry.name().to_owned();
            if entry.is_dir() || jar.exclude.iter().any(|prefix| name.starts_with(prefix)) {
                continue;
            }
            validate_relative_path(&name).map_err(|_| unsafe_jar())?;
            total = total.saturating_add(entry.size());
            if total > MAX_NATIVE_BYTES {
                return Err(unsafe_jar());
            }
            let mut target = dest.to_path_buf();
            target.extend(name.split('/'));
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent).map_err(|e| storage(&e))?;
            }
            let mut out = fs::File::create(&target).map_err(|e| storage(&e))?;
            io::copy(&mut (&mut entry).take(MAX_NATIVE_BYTES), &mut out)
                .map_err(|e| storage(&e))?;
        }
    }
    Ok(())
}

/// The directory `${game_assets}` names: the assets root for modern versions,
/// otherwise the by-name layout, created from the verified objects.
pub fn prepare_assets(
    root: &DataRoot,
    plan: &GamePlan,
    game_dir: &Path,
) -> LauncherResult<PathBuf> {
    let dir = match plan.asset_layout {
        AssetLayout::Modern => return Ok(root.assets_root()),
        AssetLayout::Virtual => root
            .assets_root()
            .join("virtual")
            .join(plan.asset_index.id.as_str()),
        AssetLayout::Resources => game_dir.join("resources"),
    };
    let bytes = fs::read(root.asset_index_source(&plan.asset_index.artifact.sha1))
        .map_err(|e| storage(&e))?;
    let value: Value = serde_json::from_slice(&bytes).map_err(|_| repair())?;
    let objects = value
        .get("objects")
        .and_then(Value::as_object)
        .ok_or_else(repair)?;
    for (name, object) in objects {
        validate_relative_path(name).map_err(|_| repair())?;
        let hash = object
            .get("hash")
            .and_then(Value::as_str)
            .and_then(|h| Sha1::parse(h).ok())
            .ok_or_else(repair)?;
        let size = object
            .get("size")
            .and_then(Value::as_u64)
            .ok_or_else(repair)?;
        let mut target = dir.clone();
        target.extend(name.split('/'));
        if fs::metadata(&target).is_ok_and(|m| m.is_file() && m.len() == size) {
            continue;
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|e| storage(&e))?;
        }
        let _ = fs::remove_file(&target);
        let source = root.asset_object(&hash);
        // A hard link costs no space; copy where links are unavailable.
        if fs::hard_link(&source, &target).is_err() {
            fs::copy(&source, &target).map_err(|e| storage(&e))?;
        }
    }
    Ok(dir)
}

/// Launchwrapper 1.5, which starts every Mojang release before 1.6.
const LAUNCHWRAPPER_1_5: &str = "net.minecraft:launchwrapper:1.5";

/// The launcher's tweaker for launchwrapper 1.5 (`java/legacy-tweaker`): Mojang's
/// own tweaker sets the game folder only at the end of `Minecraft.main`, after the
/// game thread has started, and that thread may take the system user's default
/// `.minecraft` folder for its options, saves and statistics first. This one sets
/// the folder before `main` runs.
pub const LEGACY_TWEAKER_CLASS: &str = "app.launcher.legacy.LegacyTweaker";
const LEGACY_TWEAKER_JAR: &[u8] = include_bytes!("../../../java/legacy-tweaker/legacy-tweaker.jar");

/// A vanilla game started by launchwrapper 1.5 with Mojang's default tweaker.
pub fn needs_legacy_tweaker(plan: &GamePlan) -> bool {
    plan.loader.is_none()
        && plan.main_class == super::service::LAUNCHWRAPPER_MAIN_CLASS
        && plan
            .libraries
            .iter()
            .any(|l| l.provenance.as_deref() == Some(LAUNCHWRAPPER_1_5))
        && !plan.game_arguments.iter().any(|a| a == "--tweakClass")
}

/// Writes the tweaker jar into the shared files, named by its content so a game
/// still running with an older one keeps it, and returns its path.
pub fn legacy_tweaker_jar(root: &DataRoot) -> LauncherResult<PathBuf> {
    let digest = hex::encode(Sha256::digest(LEGACY_TWEAKER_JAR));
    let path = root.legacy_tweaker(&digest[..16]);
    if fs::read(&path).is_ok_and(|bytes| bytes == LEGACY_TWEAKER_JAR) {
        return Ok(path);
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| storage(&e))?;
    }
    crate::storage::atomic::write(&path, LEGACY_TWEAKER_JAR).map_err(|e| storage(&e))?;
    Ok(path)
}

fn repair() -> LauncherError {
    LauncherError::new(
        ErrorCode::RepairRequired,
        "A verified asset index is damaged. Repair the installation.",
    )
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;

    use super::*;

    fn jar(path: &Path, entries: &[(&str, &[u8])]) {
        let mut zip = zip::ZipWriter::new(fs::File::create(path).unwrap());
        for (name, bytes) in entries {
            zip.start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(bytes).unwrap();
        }
        zip.finish().unwrap();
    }

    #[test]
    fn natives_are_extracted_without_excluded_entries() {
        let dir = tempfile::tempdir().unwrap();
        let root = DataRoot::new(dir.path().to_path_buf()).unwrap();
        fs::create_dir_all(dir.path().join("shared/libraries/a")).unwrap();
        jar(
            &dir.path().join("shared/libraries/a/artifact.jar"),
            &[
                ("liblwjgl64.so", b"so"),
                ("META-INF/MANIFEST.MF", b"m"),
                ("sub/openal.so", b"al"),
            ],
        );
        let natives = [NativeJar {
            path: "shared/libraries/a/artifact.jar".into(),
            sha1: Sha1::parse("0000000000000000000000000000000000000001").unwrap(),
            exclude: vec!["META-INF/".into()],
        }];
        let dest = dir.path().join("natives");
        extract_natives(&root, &natives, &dest).unwrap();
        assert_eq!(fs::read(dest.join("liblwjgl64.so")).unwrap(), b"so");
        assert_eq!(fs::read(dest.join("sub/openal.so")).unwrap(), b"al");
        assert!(!dest.join("META-INF").exists());
    }

    #[test]
    fn the_legacy_tweaker_is_written_once_by_its_content() {
        let dir = tempfile::tempdir().unwrap();
        let root = DataRoot::new(dir.path().to_path_buf()).unwrap();
        let path = legacy_tweaker_jar(&root).unwrap();
        assert!(path.starts_with(dir.path().join("shared").join("launcher")));
        assert_eq!(fs::read(&path).unwrap(), LEGACY_TWEAKER_JAR);
        let archive = zip::ZipArchive::new(fs::File::open(&path).unwrap()).unwrap();
        let class = format!("{}.class", LEGACY_TWEAKER_CLASS.replace('.', "/"));
        assert!(archive.file_names().any(|name| name == class));
        // A damaged copy is replaced; the name stays.
        fs::write(&path, b"broken").unwrap();
        assert_eq!(legacy_tweaker_jar(&root).unwrap(), path);
        assert_eq!(fs::read(&path).unwrap(), LEGACY_TWEAKER_JAR);
    }

    #[test]
    fn entries_escaping_the_natives_directory_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let root = DataRoot::new(dir.path().to_path_buf()).unwrap();
        fs::create_dir_all(dir.path().join("shared/libraries/b")).unwrap();
        jar(
            &dir.path().join("shared/libraries/b/artifact.jar"),
            &[("../evil.so", b"x")],
        );
        let natives = [NativeJar {
            path: "shared/libraries/b/artifact.jar".into(),
            sha1: Sha1::parse("0000000000000000000000000000000000000002").unwrap(),
            exclude: Vec::new(),
        }];
        let error = extract_natives(&root, &natives, &dir.path().join("natives")).unwrap_err();
        assert_eq!(error.code, ErrorCode::UnsafeArchive);
        assert!(!dir.path().join("evil.so").exists());
    }
}
