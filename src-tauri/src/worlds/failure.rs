//! Why a world server did not start (W12): what its loader and its crash
//! report say about a mod, as the mod, what it needs and what the world has.
//!
//! Lines are read while the server starts, before anyone can join, and only
//! these few values are kept from them: mod ids, names that pass
//! [`clean_name`], versions and a Java release. A crash report is read from
//! the world's own `crash-reports/`, written during this start, bounded in
//! size; nothing leaves the computer.

use std::path::Path;
use std::time::{Duration, SystemTime};

use crate::error::{ErrorCode, LauncherError, params};

/// Problems kept from one start.
const MAX_PROBLEMS: usize = 8;
/// Crash reports looked at, and the size of one.
const MAX_REPORTS: usize = 32;
const MAX_REPORT_BYTES: u64 = 4 * 1024 * 1024;
/// A crash report counts when written this little before the start.
const CLOCK_SLACK: Duration = Duration::from_secs(5);
/// Lines of a crash report read.
const MAX_REPORT_LINES: usize = 20_000;

/// A mod or what it depends on: its id, and its name where the loader gave one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Named {
    pub id: String,
    pub name: Option<String>,
}

impl Named {
    fn new(id: &str, name: Option<&str>) -> Option<Self> {
        Some(Self {
            id: clean_id(id)?,
            name: name.and_then(clean_name),
        })
    }

    /// What the player reads: the loader's and the game's own names, else the
    /// mod's name, else its id.
    pub fn shown(&self) -> String {
        let known = match self.id.as_str() {
            "minecraft" => Some("Minecraft"),
            "neoforge" => Some("NeoForge"),
            "forge" => Some("Forge"),
            "fabricloader" => Some("Fabric Loader"),
            "quilt_loader" => Some("Quilt Loader"),
            "fabric-api" | "fabric" => Some("Fabric API"),
            "java" => Some("Java"),
            _ => None,
        };
        known
            .map(str::to_owned)
            .or_else(|| self.name.clone())
            .unwrap_or_else(|| self.id.clone())
    }
}

/// What a mod asks of a dependency, where it is one version or a lower bound.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Needs {
    AtLeast(String),
    Exactly(String),
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Problem {
    /// `by` needs `dependency`, which the world does not have.
    Missing { by: Named, dependency: Named },
    /// `by` needs another version of `dependency` than `actual`.
    Wrong {
        by: Named,
        dependency: Named,
        needs: Needs,
        actual: String,
    },
    /// A mod is built for a newer Java than the version's own.
    Java { release: u32 },
    /// `by` failed while the server started.
    Failed { by: Named },
}

impl Problem {
    /// The mod the problem is about.
    fn by(&self) -> Option<&Named> {
        match self {
            Self::Missing { by, .. } | Self::Wrong { by, .. } | Self::Failed { by } => Some(by),
            Self::Java { .. } => None,
        }
    }

    /// Lower first: what a mod needs comes before what then failed.
    fn rank(&self) -> u8 {
        match self {
            Self::Missing { .. } | Self::Wrong { .. } => 0,
            Self::Java { .. } => 1,
            Self::Failed { .. } => 2,
        }
    }
}

/// A mod id as the loaders allow them (Forge `[a-z][a-z0-9_]`, Fabric and
/// Quilt also `-`), up to 64 characters.
fn clean_id(id: &str) -> Option<String> {
    let mut bytes = id.bytes();
    let first_ok = bytes.next().is_some_and(|b| b.is_ascii_lowercase());
    (first_ok
        && id.len() <= 64
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'_' | b'-')))
    .then(|| id.to_owned())
}

/// A mod's name as its jar gives it, kept only when it is short plain text:
/// letters, digits, spaces and a few marks, never a brace or a control.
pub fn clean_name(name: &str) -> Option<String> {
    let name = name.trim();
    (!name.is_empty()
        && name.chars().count() <= 64
        && name
            .chars()
            .all(|c| c.is_alphanumeric() || " -_.'&+!:()".contains(c)))
    .then(|| name.to_owned())
}

/// A version or a range bound: `1.21.1`, `21.0.167`, `0.92.12+1.20.1`.
fn clean_version(version: &str) -> Option<String> {
    let version = version.trim();
    (!version.is_empty()
        && version.len() <= 64
        && version
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'+' | b'-' | b'_')))
    .then(|| version.to_owned())
}

/// A Maven range as Forge and NeoForge write them: `[21.1.238,)` is a lower
/// bound, `[1.21.1]` and `[1.21.1,1.21.1]` one version.
fn maven_needs(range: &str) -> Needs {
    let Some(inner) = range.strip_prefix('[') else {
        return Needs::Other;
    };
    if let Some(low) = inner.strip_suffix(",)") {
        return clean_version(low).map_or(Needs::Other, Needs::AtLeast);
    }
    if let Some(inner) = inner.strip_suffix(']') {
        let (low, high) = inner.split_once(',').unwrap_or((inner, inner));
        if low == high {
            return clean_version(low).map_or(Needs::Other, Needs::Exactly);
        }
    }
    Needs::Other
}

/// Fabric Loader's words for a requirement: `version 0.16.0 or later`,
/// `version 1.21.1`, `any version between …`.
fn fabric_needs(words: &str) -> Needs {
    let Some(version) = words.strip_prefix("version ") else {
        return Needs::Other;
    };
    if let Some(low) = version.strip_suffix(" or later") {
        return clean_version(low).map_or(Needs::Other, Needs::AtLeast);
    }
    clean_version(version).map_or(Needs::Other, Needs::Exactly)
}

/// Forge's and NeoForge's dependency line:
/// `Mod ID: 'neoforge', Requested by: 'jei', Expected range: '[21.1.238,)', Actual version: '21.0.167'`.
fn forge_dependency(line: &str) -> Option<Problem> {
    let rest = line.trim_start().strip_prefix("Mod ID: '")?;
    let (dependency, rest) = rest.split_once("', Requested by: '")?;
    let (by, rest) = rest.split_once("', Expected range: '")?;
    let (range, rest) = rest.split_once("', Actual version: '")?;
    let actual = rest.trim_end().strip_suffix('\'')?;
    let by = Named::new(by, None)?;
    let dependency = Named::new(dependency, None)?;
    if actual == "[MISSING]" {
        return Some(Problem::Missing { by, dependency });
    }
    Some(Problem::Wrong {
        by,
        dependency,
        needs: maven_needs(range),
        actual: clean_version(actual)?,
    })
}

/// `'Name' (id)` at the start of `text`, and what follows it.
fn quoted_mod(text: &str) -> Option<(Named, &str)> {
    let rest = text.strip_prefix('\'')?;
    let (name, rest) = rest.split_once("' (")?;
    let (id, rest) = rest.split_once(')')?;
    Some((Named::new(id, Some(name))?, rest))
}

/// Fabric Loader's (0.14 on) details:
/// `- Mod 'AppleSkin' (appleskin) 3.0.6+mc1.21 requires any version of fabric-api, which is missing!`,
/// `- Mod 'Fabric API' (fabric-api) 0.92.12+1.20.1 requires any version between 1.20 (inclusive)
/// and 1.20.2- (exclusive) of 'Minecraft' (minecraft), but only the wrong version is present: 1.21.1!`.
fn fabric_dependency(line: &str) -> Option<Problem> {
    let line = line.trim_start();
    let line = line.strip_prefix("- ").unwrap_or(line);
    let (by, rest) = quoted_mod(line.strip_prefix("Mod ")?)?;
    let (_, rest) = rest.split_once(" requires ")?;
    let (words, rest, missing) = if let Some(words) = rest.strip_suffix(", which is missing!") {
        (words, "", true)
    } else {
        let (words, actual) = rest.split_once(", but only the wrong version is present: ")?;
        (words, actual.strip_suffix('!')?, false)
    };
    // The dependency follows the last " of ": `'Name' (id)` or a bare id.
    let (needs, dependency) = words.rsplit_once(" of ")?;
    let dependency = match quoted_mod(dependency) {
        Some((named, "")) => named,
        _ => Named::new(dependency, None)?,
    };
    if missing {
        return Some(Problem::Missing { by, dependency });
    }
    Some(Problem::Wrong {
        by,
        dependency,
        needs: fabric_needs(needs),
        actual: clean_version(rest)?,
    })
}

/// FML's words before 1.13, for a mod made for another Minecraft version:
/// `The mod jei does not wish to run in Minecraft version Minecraft 1.12.2. …`.
fn old_forge_version(line: &str) -> Option<Problem> {
    let (_, rest) = line.split_once("The mod ")?;
    let (id, rest) = rest.split_once(" does not wish to run in Minecraft version Minecraft ")?;
    let (actual, _) = rest.split_once(". ")?;
    Some(Problem::Wrong {
        by: Named::new(id, None)?,
        dependency: Named::new("minecraft", None)?,
        needs: Needs::Other,
        actual: clean_version(actual)?,
    })
}

/// The JVM's own words for a class built for a newer Java:
/// `… has been compiled by a more recent version of the Java Runtime (class file version 65.0), …`.
fn newer_java(line: &str) -> Option<Problem> {
    let (_, rest) = line
        .split_once("compiled by a more recent version of the Java Runtime (class file version ")?;
    let (major, _) = rest.split_once('.')?;
    let major: u32 = major.parse().ok()?;
    // Class file 52 is Java 8; each release adds one.
    (53..=200).contains(&major).then(|| Problem::Java {
        release: major - 44,
    })
}

/// A mod named as the one that failed: Fabric's entrypoint and Mixin errors,
/// Forge's and NeoForge's `Failure message:` of a crash report.
fn failed_mod(line: &str) -> Option<Problem> {
    let line = line.trim();
    if let Some((_, rest)) = line.split_once("due to errors, provided by '") {
        let (id, _) = rest.split_once('\'')?;
        return Some(Problem::Failed {
            by: Named::new(id, None)?,
        });
    }
    if let Some((_, rest)) = line.split_once("Mixin apply for mod ") {
        let (id, _) = rest.split_once(' ')?;
        return Some(Problem::Failed {
            by: Named::new(id, None)?,
        });
    }
    let rest = line
        .strip_prefix("Failure message")?
        .strip_prefix(':')?
        .trim_start();
    // `Spark (spark) has failed to load correctly`, `… has encountered an error …`
    let (name, rest) = rest.split_once(" (")?;
    let (id, rest) = rest.split_once(") ")?;
    rest.starts_with("has ").then_some(())?;
    Some(Problem::Failed {
        by: Named::new(id, Some(name))?,
    })
}

/// The problem one line names, if any.
pub fn read(line: &str) -> Option<Problem> {
    forge_dependency(line)
        .or_else(|| fabric_dependency(line))
        .or_else(|| old_forge_version(line))
        .or_else(|| newer_java(line))
        .or_else(|| failed_mod(line))
}

/// Keeps the problem `line` names, once, up to [`MAX_PROBLEMS`].
pub fn note(problems: &mut Vec<Problem>, line: &str) {
    if problems.len() >= MAX_PROBLEMS {
        return;
    }
    if let Some(problem) = read(line)
        && !problems.contains(&problem)
    {
        problems.push(problem);
    }
}

/// Problems in the crash reports the server wrote since `since`: regular files
/// `crash-*.txt` in `server_dir/crash-reports`, newest first.
pub fn crash_reports(server_dir: &Path, since: SystemTime) -> Vec<Problem> {
    let Ok(entries) = std::fs::read_dir(server_dir.join("crash-reports")) else {
        return Vec::new();
    };
    let since = since.checked_sub(CLOCK_SLACK).unwrap_or(since);
    let mut reports: Vec<(SystemTime, std::path::PathBuf)> = entries
        .filter_map(Result::ok)
        .take(MAX_REPORTS * 4)
        .filter(|e| {
            let name = e.file_name();
            let name = name.to_string_lossy();
            name.starts_with("crash-") && name.ends_with(".txt")
        })
        .filter_map(|e| {
            // Not followed: a link is not the server's own report.
            let meta = std::fs::symlink_metadata(e.path()).ok()?;
            let modified = meta.modified().ok()?;
            (meta.is_file() && meta.len() <= MAX_REPORT_BYTES && modified >= since)
                .then(|| (modified, e.path()))
        })
        .collect();
    reports.sort_by_key(|r| std::cmp::Reverse(r.0));
    let mut problems = Vec::new();
    for (_, path) in reports.into_iter().take(MAX_REPORTS) {
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        let text = String::from_utf8_lossy(&bytes);
        if let Some(by) = first_mod_frame(&text)
            && problems.len() < MAX_PROBLEMS
        {
            let problem = Problem::Failed { by };
            if !problems.contains(&problem) {
                problems.push(problem);
            }
        }
        let mut lines = text.lines().take(MAX_REPORT_LINES).peekable();
        while let Some(line) = lines.next() {
            note(&mut problems, line);
            // `Suspected Mod(s):` then `\tName (id), Version: …` lines.
            if line.trim_start().starts_with("Suspected Mod") && !line.contains("NONE") {
                while let Some(next) = lines.peek() {
                    let Some((name, rest)) = next.trim().split_once(" (") else {
                        break;
                    };
                    let Some((id, _)) = rest.split_once("), Version: ") else {
                        break;
                    };
                    if let Some(by) = Named::new(id, Some(name))
                        && problems.len() < MAX_PROBLEMS
                    {
                        let problem = Problem::Failed { by };
                        if !problems.contains(&problem) {
                            problems.push(problem);
                        }
                    }
                    lines.next();
                }
            }
        }
    }
    problems
}

/// The mod in the first frames of a crash report's own stack trace, where
/// Forge and NeoForge name each frame's module: `at TRANSFORMER/spark@1.10.66/…`.
/// The loader and the game are not a mod's; libraries' modules have dots.
fn first_mod_frame(report: &str) -> Option<Named> {
    let (_, rest) = report.split_once("\nDescription: ")?;
    rest.lines()
        .skip(1)
        .skip_while(|l| !l.trim_start().starts_with("at "))
        .take_while(|l| l.trim_start().starts_with("at "))
        .take(64)
        .filter_map(|l| {
            let module = l.trim_start().strip_prefix("at TRANSFORMER/")?;
            let (id, _) = module.split_once('@')?;
            (!matches!(id, "minecraft" | "forge" | "neoforge"))
                .then(|| Named::new(id, None))
                .flatten()
        })
        .next()
}

/// Whether a mod file of the world is the mod `id`: `jei.jar`,
/// `jei-1.21.1-neoforge-19.21.jar`, `cloth-config-15.0.jar` for `cloth_config`.
fn file_is(file: &str, id: &str) -> bool {
    let stem = file
        .strip_suffix(".jar")
        .unwrap_or(file)
        .to_ascii_lowercase()
        .replace('_', "-");
    let id = id.replace('_', "-");
    stem == id
        || stem
            .strip_prefix(&id)
            .is_some_and(|rest| rest.starts_with(['-', '+', '.']))
}

/// The problem to tell: the earliest kind, and among those one about a mod
/// file of the world (a mod inside another mod's jar names that jar's mod
/// less well), else the first.
pub fn chief<'a>(problems: &'a [Problem], mod_files: &[String]) -> Option<&'a Problem> {
    let rank = problems.iter().map(Problem::rank).min()?;
    let candidates: Vec<&Problem> = problems.iter().filter(|p| p.rank() == rank).collect();
    candidates
        .iter()
        .find(|p| {
            p.by()
                .is_some_and(|by| mod_files.iter().any(|f| file_is(f, &by.id)))
        })
        .or_else(|| candidates.first())
        .copied()
}

/// The error a start that failed with `problem` returns.
pub fn explain(problem: &Problem) -> LauncherError {
    let code = ErrorCode::GameExitedWithError;
    match problem {
        Problem::Missing { by, dependency } => LauncherError::templated(
            code,
            "{mod} needs {dependency}, which is not in the world. Add it to the installation or remove {mod}, then update the world's mods.",
            params([
                ("mod", by.shown().into()),
                ("dependency", dependency.shown().into()),
            ]),
        ),
        Problem::Wrong {
            by,
            dependency,
            needs,
            actual,
        } => {
            let values = |version: Option<&String>| {
                let mut values = params([
                    ("mod", by.shown().into()),
                    ("dependency", dependency.shown().into()),
                    ("actual", actual.clone().into()),
                ]);
                if let Some(version) = version {
                    values.insert("version".to_owned(), version.clone().into());
                }
                values
            };
            match (dependency.id.as_str(), needs) {
                ("minecraft", Needs::Exactly(version)) => LauncherError::templated(
                    code,
                    "{mod} is made for Minecraft {version}, and the world is {actual}. Take its release for {actual} or remove it.",
                    values(Some(version)),
                ),
                ("minecraft", _) => LauncherError::templated(
                    code,
                    "{mod} is made for another Minecraft version than the world's {actual}. Take its release for {actual} or remove it.",
                    values(None),
                ),
                (_, Needs::AtLeast(version)) => LauncherError::templated(
                    code,
                    "{mod} needs {dependency} {version} or newer, and the world has {actual}. Take an older release of {mod} or remove it.",
                    values(Some(version)),
                ),
                (_, Needs::Exactly(version)) => LauncherError::templated(
                    code,
                    "{mod} needs {dependency} {version}, and the world has {actual}. Take another release of {mod} or remove it.",
                    values(Some(version)),
                ),
                (_, Needs::Other) => LauncherError::templated(
                    code,
                    "{mod} does not work with {dependency} {actual}. Take another release of {mod} or remove it.",
                    values(None),
                ),
            }
        }
        Problem::Java { release } => LauncherError::templated(
            code,
            "A mod in this world needs Java {java} or newer, which this Minecraft version does not run on. Remove the mod or take its release for this version.",
            params([("java", (*release).into())]),
        ),
        Problem::Failed { by } => LauncherError::templated(
            code,
            "{mod} stopped the world server while it started. Take another release of {mod} or remove it.",
            params([("mod", by.shown().into())]),
        ),
    }
}

#[cfg(test)]
#[path = "failure_tests.rs"]
mod tests;
