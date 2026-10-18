//! The files a stopped run left in its game folder: the crash report and the
//! JVM's fatal-error file (`hs_err_pid*.log`).

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use crate::launch::redact::Redactor;

/// Only the start of a file is read; the cause is always near the top.
const MAX_READ_BYTES: u64 = 2 * 1024 * 1024;
/// Clocks and file systems round modification times.
const CLOCK_SLACK: Duration = Duration::from_secs(2);

#[derive(Debug, Default, PartialEq, Eq)]
pub struct RunFiles {
    /// File name and redacted text.
    pub crash_report: Option<(String, String)>,
    pub jvm_error: Option<(String, String)>,
}

/// The crash report the game named in its output, or the newest one written
/// since `since`, and the newest JVM error file. The JVM error file repeats
/// the command line, access token included, so it is redacted on disk too.
pub fn collect(
    game_dir: &Path,
    since: SystemTime,
    output: &[String],
    redactor: &Redactor,
) -> RunFiles {
    let since = since.checked_sub(CLOCK_SLACK).unwrap_or(since);
    let reports = game_dir.join("crash-reports");
    // Quilt names its first report again when its error window fails; that
    // report has the cause, the later one only the window's failure.
    let named = output
        .iter()
        .find_map(|line| {
            line.contains("original crash report")
                .then(|| named_report(line, &reports))
                .flatten()
        })
        .or_else(|| {
            output
                .iter()
                .rev()
                .find_map(|line| named_report(line, &reports))
        });
    let crash_report = named
        .or_else(|| newest(&reports, since, |name| name.ends_with(".txt")))
        .and_then(|path| read(&path, redactor));
    let jvm_error = newest(game_dir, since, |name| {
        name.starts_with("hs_err_pid") && name.ends_with(".log")
    })
    .and_then(|path| {
        redact_in_place(&path, redactor);
        read(&path, redactor)
    });
    RunFiles {
        crash_report,
        jvm_error,
    }
}

/// A line naming a crash report: `#@!@# Game crashed! Crash report saved to:
/// #@!@# <path>`, `This crash report has been saved to: <path>`, Quilt's
/// `The full crash report has been saved to <path>` and `please see the
/// original crash report <path>`. Only the file name is used, and only when
/// that file is in the installation's `crash-reports` folder.
fn named_report(line: &str, reports: &Path) -> Option<PathBuf> {
    if !line.to_lowercase().contains("crash report") {
        return None;
    }
    let path = line
        .split_whitespace()
        .rev()
        .map(|word| {
            word.trim_matches(|c: char| {
                matches!(
                    c,
                    '"' | '\'' | '[' | ']' | '(' | ')' | '<' | '>' | '#' | '!' | '@'
                )
            })
        })
        .find(|word| word.ends_with(".txt"))?;
    let name = Path::new(path).file_name()?;
    let candidate = reports.join(name);
    candidate.is_file().then_some(candidate)
}

fn newest(dir: &Path, since: SystemTime, wanted: impl Fn(&str) -> bool) -> Option<PathBuf> {
    fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter(|entry| entry.file_name().to_str().is_some_and(&wanted))
        .filter_map(|entry| {
            let meta = entry.metadata().ok()?;
            let modified = meta.modified().ok()?;
            (meta.is_file() && modified >= since).then(|| (modified, entry.path()))
        })
        .max_by_key(|(modified, _)| *modified)
        .map(|(_, path)| path)
}

fn read(path: &Path, redactor: &Redactor) -> Option<(String, String)> {
    let mut bytes = Vec::new();
    fs::File::open(path)
        .ok()?
        .take(MAX_READ_BYTES)
        .read_to_end(&mut bytes)
        .ok()?;
    let text = String::from_utf8_lossy(&bytes);
    let text: Vec<String> = text.lines().map(|l| redactor.redact(l)).collect();
    let name = path.file_name()?.to_string_lossy().into_owned();
    Some((name, text.join("\n")))
}

fn redact_in_place(path: &Path, redactor: &Redactor) {
    let Ok(bytes) = fs::read(path) else {
        return;
    };
    let text = String::from_utf8_lossy(&bytes);
    let redacted: String = text
        .split_inclusive('\n')
        .map(|line| {
            let end = line.len() - line.trim_end_matches(['\r', '\n']).len();
            let (body, newline) = line.split_at(line.len() - end);
            format!("{}{newline}", redactor.redact(body))
        })
        .collect();
    if redacted != text {
        let _ = fs::write(path, redacted);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn redactor() -> Redactor {
        Redactor::new(&["SECRET-TOKEN-1234".to_owned()])
    }

    #[test]
    fn finds_named_report_and_redacts_the_jvm_error_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let game = dir.path();
        fs::create_dir_all(game.join("crash-reports")).expect("dir");
        fs::write(game.join("crash-reports/crash-old.txt"), "Description: old").expect("old");
        fs::write(game.join("crash-reports/crash-new.txt"), "Description: new").expect("new");
        fs::write(
            game.join("hs_err_pid42.log"),
            "# SIGSEGV\njava_command: Main --accessToken SECRET-TOKEN-1234\n",
        )
        .expect("hs_err");
        let output = vec![
            "#@!@# Game crashed! Crash report saved to: #@!@# /elsewhere/crash-reports/crash-old.txt"
                .to_owned(),
        ];
        let files = collect(game, SystemTime::now(), &output, &redactor());
        assert_eq!(
            files.crash_report,
            Some(("crash-old.txt".to_owned(), "Description: old".to_owned()))
        );
        let (name, text) = files.jvm_error.expect("jvm error");
        assert_eq!(name, "hs_err_pid42.log");
        assert!(!text.contains("SECRET-TOKEN"));
        let on_disk = fs::read_to_string(game.join("hs_err_pid42.log")).expect("read");
        assert!(!on_disk.contains("SECRET-TOKEN"), "{on_disk}");
        assert!(on_disk.ends_with('\n'));
    }

    #[test]
    fn quilt_names_its_reports_without_a_colon() {
        let dir = tempfile::tempdir().expect("tempdir");
        let game = dir.path();
        fs::create_dir_all(game.join("crash-reports")).expect("dir");
        for name in ["crash-1-quilt_loader.txt", "crash-2-quilt_loader.txt"] {
            fs::write(game.join("crash-reports").join(name), name).expect("report");
        }
        let saved = vec![
            "Crashed! The full crash report has been saved to /x/crash-reports/crash-2-quilt_loader.txt"
                .to_owned(),
        ];
        let files = collect(game, SystemTime::now(), &saved, &redactor());
        assert_eq!(
            files.crash_report.map(|(n, _)| n).as_deref(),
            Some("crash-2-quilt_loader.txt")
        );
        // The error window failed: the first report has the cause.
        let failed = vec![
            "java.lang.Error: Failed to open the error gui - please see the original crash report /x/crash-reports/crash-1-quilt_loader.txt".to_owned(),
            "Crashed! The full crash report has been saved to /x/crash-reports/crash-2-quilt_loader.txt".to_owned(),
        ];
        let files = collect(game, SystemTime::now(), &failed, &redactor());
        assert_eq!(
            files.crash_report.map(|(n, _)| n).as_deref(),
            Some("crash-1-quilt_loader.txt")
        );
    }

    #[test]
    fn older_files_belong_to_earlier_runs() {
        let dir = tempfile::tempdir().expect("tempdir");
        let game = dir.path();
        fs::create_dir_all(game.join("crash-reports")).expect("dir");
        fs::write(game.join("crash-reports/crash-old.txt"), "Description: old").expect("old");
        fs::write(game.join("hs_err_pid1.log"), "# old").expect("hs_err");
        let later = SystemTime::now() + Duration::from_secs(60);
        assert_eq!(collect(game, later, &[], &redactor()), RunFiles::default());
        // Without a named report the newest recent one is taken.
        let files = collect(game, SystemTime::now(), &[], &redactor());
        assert_eq!(
            files.crash_report.map(|(n, _)| n).as_deref(),
            Some("crash-old.txt")
        );
    }
}
