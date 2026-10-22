//! Turning one run into what the workflow shows: the bot's report, what the
//! game's log says, and a summary row. Everything copied out of the game folder
//! is cleaned of the computer's user name, home and data folder first: the
//! workflow's artifacts and summary are public.

use std::path::{Path, PathBuf};

use serde_json::Value;

/// Replaces the computer's own names in text that leaves the machine.
pub struct Redactor {
    pairs: Vec<(String, &'static str)>,
}

impl Redactor {
    pub fn new(data_root: &Path) -> Self {
        let mut pairs: Vec<(String, &'static str)> = Vec::new();
        let mut add = |value: Option<String>, label: &'static str| {
            if let Some(v) = value.filter(|v| v.len() >= 3) {
                // Paths appear with either slash, and JSON doubles backslashes.
                pairs.push((v.replace('\\', "\\\\"), label));
                pairs.push((v.replace('\\', "/"), label));
                pairs.push((v, label));
            }
        };
        add(Some(data_root.display().to_string()), "<data>");
        for var in ["USERPROFILE", "HOME"] {
            add(std::env::var(var).ok(), "<home>");
        }
        for var in ["USERNAME", "USER"] {
            add(std::env::var(var).ok(), "<user>");
        }
        add(std::env::var("COMPUTERNAME").ok(), "<computer>");
        add(std::env::var("USERDOMAIN").ok(), "<computer>");
        // Longest first, so a home folder goes before the user name inside it.
        pairs.sort_by_key(|(v, _)| std::cmp::Reverse(v.len()));
        pairs.dedup_by(|a, b| a.0 == b.0);
        Self { pairs }
    }

    pub fn clean(&self, text: &str) -> String {
        let mut out = text.to_owned();
        for (value, label) in &self.pairs {
            out = replace_ignore_case(&out, value, label);
        }
        out
    }
}

fn replace_ignore_case(text: &str, needle: &str, with: &str) -> String {
    let lower = text.to_ascii_lowercase();
    let needle_lower = needle.to_ascii_lowercase();
    if needle_lower.is_empty() || !lower.contains(&needle_lower) {
        return text.to_owned();
    }
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while let Some(found) = lower[i..].find(&needle_lower) {
        let start = i + found;
        out.push_str(&text[i..start]);
        out.push_str(with);
        i = start + needle_lower.len();
    }
    out.push_str(&text[i..]);
    out
}

/// What the game's log says about errors.
#[derive(Debug, Default)]
pub struct LogScan {
    pub errors: usize,
    pub warnings: usize,
    pub exceptions: usize,
    /// The first distinct error lines, cleaned and cut short.
    pub samples: Vec<String>,
}

pub fn scan_log(text: &str, redactor: &Redactor) -> LogScan {
    let mut scan = LogScan::default();
    for line in text.lines() {
        let upper =
            line.contains("/ERROR]") || line.contains(" ERROR ") || line.contains("[ERROR]");
        let warn = line.contains("/WARN]") || line.contains(" WARN ") || line.contains("[WARN]");
        if upper {
            scan.errors += 1;
        } else if warn {
            scan.warnings += 1;
        }
        if line.contains("Exception") && !line.trim_start().starts_with("at ") {
            scan.exceptions += 1;
        }
        if upper && scan.samples.len() < 15 {
            let clean: String = redactor.clean(line).chars().take(300).collect();
            if !scan.samples.contains(&clean) {
                scan.samples.push(clean);
            }
        }
    }
    scan
}

/// Copies a text file, cleaned, into the results.
pub fn copy_clean(from: &Path, to: &Path, redactor: &Redactor) -> bool {
    let Ok(bytes) = std::fs::read(from) else {
        return false;
    };
    let text = String::from_utf8_lossy(&bytes);
    if let Some(dir) = to.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    std::fs::write(to, redactor.clean(&text)).is_ok()
}

/// Files directly in `dir` whose names start with `prefix`.
pub fn files_starting(dir: &Path, prefix: &str) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with(prefix))
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .collect();
    found.sort();
    found
}

fn num(value: &Value) -> String {
    value
        .as_f64()
        .map_or_else(|| "–".to_owned(), |v| format!("{v:.0}"))
}

/// One Markdown section for the workflow summary.
/// `status` is `passed`, `warned` (passed with a mod of the set replaced) or `failed`.
pub fn summary(scenario: &str, status: &str, report: Option<&Value>, lines: &[String]) -> String {
    let icon = match status {
        "passed" => "✅",
        // Passed with a mod of the set replaced; another mod's or a download's trouble.
        "warned" | "theirs" => "⚠️",
        // The newest mods are the locked ones: not played again.
        "unchanged" => "⏭️",
        _ => "❌",
    };
    let mut out = format!("### {icon} {scenario}\n\n");
    if let Some(r) = report {
        out.push_str("| Check | Result | Detail |\n|---|---|---|\n");
        for check in r["checks"].as_array().into_iter().flatten() {
            out.push_str(&format!(
                "| {} | {} | {} |\n",
                check["name"].as_str().unwrap_or("?"),
                if check["ok"] == true {
                    "ok"
                } else {
                    "**failed**"
                },
                check["detail"].as_str().unwrap_or("").replace('|', "/")
            ));
        }
        out.push_str(&format!(
            "\nFPS min/avg/p5: {} / {} / {} · memory max {} MB · title after {} ms · world loaded in {} ms\n\n",
            num(&r["fps"]["min"]),
            num(&r["fps"]["avg"]),
            num(&r["fps"]["p5"]),
            num(&r["memoryMb"]["max"]),
            num(&r["titleAfterMs"]),
            num(&r["worldLoadMs"]),
        ));
        out.push_str("| Phase | FPS avg | FPS min | chunks | entities | tick ms |\n|---|---|---|---|---|---|\n");
        for p in r["phases"].as_array().into_iter().flatten().take(30) {
            out.push_str(&format!(
                "| {} | {} | {} | {} | {} | {} |\n",
                p["name"].as_str().unwrap_or("?"),
                num(&p["fps"]["avg"]),
                num(&p["fps"]["min"]),
                num(&p["chunks"]["max"]),
                num(&p["entities"]["max"]),
                p["tickMs"]["avg"]
                    .as_f64()
                    .map_or("–".to_owned(), |v| format!("{v:.1}")),
            ));
        }
        out.push('\n');
        let problems: Vec<&str> = r["problems"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect();
        if !problems.is_empty() {
            out.push_str("Bot problems:\n");
            for p in problems {
                out.push_str(&format!("- {}\n", p.replace('|', "/")));
            }
            out.push('\n');
        }
    } else {
        out.push_str("No report from the bot.\n\n");
    }
    for line in lines {
        out.push_str(line);
        out.push('\n');
    }
    out.push('\n');
    out
}

#[test]
fn the_machines_names_are_replaced_in_any_case() {
    let r = Redactor {
        pairs: vec![
            ("C:\\Users\\Ai".to_owned(), "<home>"),
            ("C:/Users/Ai".to_owned(), "<home>"),
            ("TESTBOX".to_owned(), "<computer>"),
        ],
    };
    assert_eq!(
        r.clean("at c:\\users\\ai\\x and C:/Users/Ai/y on testbox"),
        "at <home>\\x and <home>/y on <computer>"
    );
}

#[test]
fn error_lines_are_counted_and_cleaned() {
    let r = Redactor {
        pairs: vec![("/home/tester".to_owned(), "<home>")],
    };
    let scan = scan_log(
        "[a] [Render thread/ERROR]: bad /home/tester/x\n[b] [Worker/WARN]: meh\njava.lang.IllegalStateException: no\n\tat x.y(z)\n",
        &r,
    );
    assert_eq!((scan.errors, scan.warnings, scan.exceptions), (1, 1, 1));
    assert_eq!(
        scan.samples,
        vec!["[a] [Render thread/ERROR]: bad <home>/x"]
    );
}
