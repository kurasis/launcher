//! User-requested diagnostic export (docs/spec/07 "Diagnostics and privacy").
//!
//! The export is a ZIP built from an explicit file list: a summary of launcher
//! state, job outcomes and the launcher-captured output of recent runs. Account
//! files, tokens, install locks, argument files, worlds, screenshots and game
//! folders are never read. Every text is sanitized again on the way out: the home
//! and data directories are shortened, and anything shaped like a token or a
//! token argument is replaced, even though run logs were redacted when written.

use std::io::{Cursor, Write};
use std::path::Path;
use std::sync::LazyLock;

use chrono::Utc;
use regex::Regex;
use serde_json::{Value, json};

use crate::download::receipt;
use crate::error::{LauncherError, LauncherResult};
use crate::state::AppState;
use crate::storage::paths::DataRoot;

/// Most recent runs whose output is included.
pub const MAX_RUNS: usize = 3;
/// Newest bytes kept from one log file.
pub const MAX_LOG_BYTES: usize = 2 * 1024 * 1024;

pub const README: &str = "Launcher diagnostic export\n\
\n\
Included:\n\
- summary.json: launcher version, OS, settings, installation versions and health,\n\
  recent operation and run results, storage notices, account state (no profile data).\n\
- jobs.json: install and repair outcomes (no plans or file lists).\n\
- runs/<run>/output*.log: launcher-captured game output of the most recent runs.\n\
\n\
Never included: account records, sign-in tokens, launch argument files, install\n\
records, worlds, screenshots, options or any other file from game folders.\n\
Home and launcher data paths are shortened; token-like values are replaced by\n\
<redacted>. Review the files before sharing them.\n";

static TOKEN_PATTERNS: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    [
        // JWT-shaped values (Minecraft and Xbox tokens).
        r"eyJ[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}(\.[A-Za-z0-9_-]*)?",
        // A value following a token-bearing argument or key.
        r"(?i)(--accessToken|access_token|refresh_token|authorization:\s*bearer)([\s=:]+)\S+",
    ]
    .iter()
    .filter_map(|p| Regex::new(p).ok())
    .collect()
});

/// Sanitizes free text for export.
pub fn sanitize(text: &str, home: Option<&Path>, data_root: &Path) -> String {
    let mut out = text.to_owned();
    if let Some(root) = data_root.to_str().filter(|r| r.len() > 1) {
        out = out.replace(root, "<data>");
    }
    if let Some(home) = home.and_then(Path::to_str).filter(|h| h.len() > 1) {
        out = out.replace(home, "~");
    }
    for (index, pattern) in TOKEN_PATTERNS.iter().enumerate() {
        out = if index == 0 {
            pattern.replace_all(&out, "<redacted>").into_owned()
        } else {
            pattern.replace_all(&out, "$1$2<redacted>").into_owned()
        };
    }
    out
}

fn sanitize_value(value: &Value, home: Option<&Path>, root: &Path) -> Value {
    match value {
        Value::String(s) => Value::String(sanitize(s, home, root)),
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(|v| sanitize_value(v, home, root))
                .collect(),
        ),
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(k, v)| (k.clone(), sanitize_value(v, home, root)))
                .collect(),
        ),
        other => other.clone(),
    }
}

fn summary(state: &AppState) -> Value {
    let snapshot = state.snapshot();
    let instances: Vec<Value> = snapshot
        .instances
        .iter()
        .map(|i| {
            json!({
                "id": i.id,
                "versionId": i.version_id,
                "health": i.health,
                "lastResult": i.last_result,
                "maxHeapMib": i.max_heap_mib,
                "resolution": i.resolution,
            })
        })
        .collect();
    json!({
        "generatedAt": Utc::now(),
        "appVersion": snapshot.app_version,
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "osVersion": sysinfo::System::long_os_version(),
        "settings": {
            "theme": snapshot.settings.theme,
            "duringGameMode": snapshot.settings.during_game_mode,
            "defaultMaxHeapMib": snapshot.settings.default_max_heap_mib,
            "defaultResolution": snapshot.settings.default_resolution,
        },
        "dataRoot": snapshot.data_root.status,
        "account": { "state": snapshot.account.state, "sessionOnly": snapshot.account.session_only },
        "instances": instances,
        "activeOperation": snapshot.active_operation,
        "currentRun": snapshot.current_run,
        "lastRun": snapshot.last_run,
        "recentOperations": snapshot.recent_operations,
        "storageIssues": snapshot.storage_issues,
        "memory": snapshot.memory.policy,
    })
}

fn jobs(root: &DataRoot) -> Value {
    let receipts = receipt::list(root).unwrap_or_default();
    Value::Array(
        receipts
            .iter()
            .rev()
            .take(20)
            .map(|r| {
                json!({
                    "jobId": r.job_id,
                    "kind": r.kind,
                    "instanceId": r.instance_id,
                    "state": r.state,
                    "errorCode": r.error_code,
                    "createdAt": r.created_at,
                    "finishedAt": r.finished_at,
                    "downloads": r.downloads,
                })
            })
            .collect(),
    )
}

/// Run log directories, newest first; `selected` first if given.
fn run_dirs(root: &DataRoot, selected: Option<&str>) -> Vec<std::path::PathBuf> {
    let runs = root.path().join("logs").join("runs");
    let mut dirs: Vec<(std::time::SystemTime, std::path::PathBuf)> = std::fs::read_dir(&runs)
        .map(|entries| {
            entries
                .flatten()
                .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
                .filter(|e| uuid::Uuid::parse_str(&e.file_name().to_string_lossy()).is_ok())
                .map(|e| {
                    let modified = e
                        .metadata()
                        .and_then(|m| m.modified())
                        .unwrap_or(std::time::UNIX_EPOCH);
                    (modified, e.path())
                })
                .collect()
        })
        .unwrap_or_default();
    dirs.sort_by_key(|d| std::cmp::Reverse(d.0));
    let mut ordered: Vec<std::path::PathBuf> = Vec::new();
    if let Some(run) = selected.filter(|r| uuid::Uuid::parse_str(r).is_ok()) {
        let dir = runs.join(run);
        if dir.is_dir() {
            ordered.push(dir);
        }
    }
    for (_, dir) in dirs {
        if !ordered.contains(&dir) {
            ordered.push(dir);
        }
    }
    ordered.truncate(MAX_RUNS);
    ordered
}

/// Builds the export archive in memory.
pub fn build(
    state: &AppState,
    root: &DataRoot,
    run: Option<&str>,
    home: Option<&Path>,
) -> LauncherResult<Vec<u8>> {
    let failed = |_| LauncherError::internal("Building the diagnostic export failed.");
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    let mut add = |name: &str, bytes: &[u8]| -> LauncherResult<()> {
        zip.start_file(name, options).map_err(failed)?;
        zip.write_all(bytes)
            .map_err(|_| LauncherError::internal("Building the diagnostic export failed."))
    };
    add("README.txt", README.as_bytes())?;
    let data_root = root.path();
    let pretty = |value: &Value| {
        serde_json::to_vec_pretty(&sanitize_value(value, home, data_root)).unwrap_or_default()
    };
    add("summary.json", &pretty(&summary(state)))?;
    add("jobs.json", &pretty(&jobs(root)))?;
    for dir in run_dirs(root, run) {
        let run_name = dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut files: Vec<_> = entries
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.starts_with("output") && n.ends_with(".log"))
            .collect();
        files.sort();
        for name in files {
            let Ok(bytes) = std::fs::read(dir.join(&name)) else {
                continue;
            };
            let tail = &bytes[bytes.len().saturating_sub(MAX_LOG_BYTES)..];
            let text = sanitize(&String::from_utf8_lossy(tail), home, data_root);
            add(&format!("runs/{run_name}/{name}"), text.as_bytes())?;
        }
    }
    let cursor = zip.finish().map_err(failed)?;
    Ok(cursor.into_inner())
}

#[cfg(test)]
mod tests {
    use std::io::Read;

    use super::*;
    use crate::validation::MemoryPolicy;

    #[test]
    fn tokens_and_private_paths_are_removed() {
        let home = Path::new("/home/jurgen");
        let root = Path::new("/home/jurgen/.local/share/launcher");
        let text = "at /home/jurgen/.local/share/launcher/shared/x.jar user /home/jurgen/Desktop \
                    --accessToken abc.def-123 token eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0In0.sig \
                    refresh_token=0.AAAA-secret Authorization: Bearer xyz";
        let out = sanitize(text, Some(home), root);
        for leaked in [
            "jurgen",
            "abc.def-123",
            "eyJhbGciOi",
            "0.AAAA-secret",
            "xyz",
        ] {
            assert!(!out.contains(leaked), "{leaked} in {out}");
        }
        assert!(out.contains("<data>/shared/x.jar"));
        assert!(out.contains("~/Desktop"));
        assert!(out.contains("--accessToken <redacted>"));
    }

    #[test]
    fn the_archive_contains_only_the_listed_files() {
        let dir = tempfile::tempdir().unwrap();
        let root = DataRoot::new(dir.path().join("data")).unwrap();
        let state = AppState::initialize(
            Ok(root.clone()),
            "0.1.0",
            MemoryPolicy::from_physical_bytes(16 * 1024 * 1024 * 1024),
            Box::new(crate::state::NoopSink),
        );
        // Sensitive and private files that must never be exported.
        std::fs::write(root.accounts(), b"{\"secret\":\"vault-key\"}").unwrap();
        let run = uuid::Uuid::new_v4();
        let logs = root.run_logs(run);
        std::fs::create_dir_all(&logs).unwrap();
        std::fs::write(
            logs.join("output.log"),
            b"[main] eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0In0.sig\n",
        )
        .unwrap();
        std::fs::write(logs.join("jvm.args"), b"--secret").unwrap();
        let bytes = build(&state, &root, Some(&run.to_string()), None).unwrap();
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        let mut names: Vec<String> = (0..archive.len())
            .map(|i| archive.by_index(i).unwrap().name().to_owned())
            .collect();
        names.sort();
        assert_eq!(
            names,
            [
                "README.txt".to_owned(),
                "jobs.json".to_owned(),
                format!("runs/{run}/output.log"),
                "summary.json".to_owned(),
            ]
        );
        let mut log = String::new();
        archive
            .by_name(&format!("runs/{run}/output.log"))
            .unwrap()
            .read_to_string(&mut log)
            .unwrap();
        assert_eq!(log, "[main] <redacted>\n");
        let mut summary = String::new();
        archive
            .by_name("summary.json")
            .unwrap()
            .read_to_string(&mut summary)
            .unwrap();
        assert!(!summary.contains("vault-key") && !summary.contains(root.path().to_str().unwrap()));
        assert!(summary.contains("\"appVersion\": \"0.1.0\""));
    }
}
