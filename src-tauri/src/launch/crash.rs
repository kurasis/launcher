//! What made the game stop, in words a player can act on (D-059).
//!
//! After a run ends, the game's output, the crash report it wrote and the
//! JVM's fatal-error file are matched against known causes: launcher and Java
//! setup, memory, graphics drivers, mod loader dependency errors, broken mod or
//! config files, crash report categories, and finally the exit code or signal.
//! The rules were collected from the official exit-code help, the crash
//! analysers of other launchers and log services, the loaders' own messages,
//! and crashes reproduced with this launcher (see `crash/fixtures`).
//!
//! Only lines that show the cause are kept as evidence. The JVM's error file
//! repeats the full command line, access token included, so only its header
//! and frame lines are read.

use std::collections::BTreeMap;
use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};

pub mod files;

/// Bumped when the rules change, so reports and statistics say which rules
/// explained a crash (D-061).
pub const ANALYZER_VERSION: u32 = 2;
/// Kept in a run's log folder after a crash (D-061).
pub const DIAGNOSIS_FILE: &str = "diagnosis.json";
pub const REPORT_COPY: &str = "crash-report.txt";
pub const JVM_ERROR_COPY: &str = "jvm-error.log";

/// Known causes, most specific first in [`diagnose`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CrashKind {
    /// A JVM option the user added is not understood by this Java.
    JvmOption,
    /// The JVM could not reserve the configured heap.
    HeapUnavailable,
    /// The game or a mod needs a newer Java.
    JavaTooOld,
    /// The game or loader needs an older Java.
    JavaTooNew,
    /// A mod needs another mod that is not installed.
    MissingDependency,
    /// A mod needs another version of a mod that is installed.
    WrongDependencyVersion,
    /// A mod is made for another Minecraft version.
    WrongMinecraftVersion,
    /// A mod needs a newer mod loader.
    LoaderTooOld,
    /// Two installed mods cannot run together.
    IncompatibleMods,
    /// The same mod is installed twice.
    DuplicateMods,
    /// A mod is made for another loader (Fabric mod on Forge, ...).
    ModForOtherLoader,
    /// A mod or game file is damaged (incomplete download, broken archive).
    CorruptModFile,
    /// A mod's config file is damaged.
    CorruptConfig,
    /// A mod could not change the game's code (Mixin).
    MixinFailed,
    /// A mod failed while the loader started it.
    ModFailedToLoad,
    /// OptiFine does not work with this loader or these mods.
    OptifineConflict,
    /// The game's memory (heap) ran out.
    OutOfMemory,
    /// A modpack's own check wants more memory than the installation gives.
    PackNeedsMemory,
    /// The computer itself ran out of memory.
    SystemMemory,
    /// Killed by the system or another program (often for lack of memory).
    Killed,
    /// The graphics card or driver cannot give the game OpenGL.
    OpenglUnsupported,
    /// The graphics driver crashed.
    GraphicsDriver,
    /// Software that draws over games (overlay, recorder) crashed the game.
    Overlay,
    /// The game crashed in native code (outside Java).
    NativeCrash,
    /// A native library of the game is missing.
    MissingNativeLibrary,
    /// Game files or Java are incomplete; a repair restores them.
    BrokenInstallation,
    /// The disk is full.
    DiskFull,
    /// A file is locked or not allowed to be read or written.
    FileAccess,
    /// A world could not be read.
    WorldLoadFailed,
    /// An entity in the world crashes the game every tick.
    TickingEntity,
    /// A block entity in the world crashes the game every tick.
    TickingBlockEntity,
    /// The game froze and the watchdog stopped it.
    ServerHang,
    /// The player held F3 + C.
    DebugCrash,
    /// The game wrote a crash report; its category is named.
    GameCrash,
    /// Stopped by another program (not by the launcher's Stop).
    Terminated,
    /// The game asked a question in a console nobody can answer; the launcher
    /// ended it.
    ConsoleQuestion,
    /// Only the exit code is known.
    Unknown,
}

/// What the player can do from the launcher.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CrashAction {
    OpenMods,
    OpenConfig,
    OpenCrashReports,
    OpenGameFolder,
    /// Settings → Installations: memory.
    MemorySettings,
    /// Settings → Installations: Java and JVM options.
    JavaSettings,
    /// Settings → Installations: mod loader version.
    LoaderSettings,
    Repair,
    Backups,
}

/// A finding besides the main cause.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CrashNote {
    pub kind: CrashKind,
    pub details: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CrashDiagnosis {
    pub kind: CrashKind,
    /// Values for the explanation: mods, versions, files, Java versions.
    pub details: BTreeMap<String, String>,
    /// The lines that show the cause, as the game printed them.
    pub evidence: Vec<String>,
    /// Mods the crash report or stack trace points at.
    pub suspects: Vec<String>,
    /// The crash report or JVM error file of this run, by file name.
    pub report: Option<String>,
    pub actions: Vec<CrashAction>,
    pub notes: Vec<CrashNote>,
}

/// How the process ended.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ExitInfo {
    pub code: Option<i32>,
    /// The Unix signal that ended it, if any.
    pub signal: Option<i32>,
}

impl ExitInfo {
    fn clean(self) -> bool {
        self.code == Some(0) && self.signal.is_none()
    }
}

/// A file the game wrote during the run.
pub struct RunFile<'a> {
    pub name: &'a str,
    pub text: &'a str,
}

pub struct CrashInput<'a> {
    pub exit: ExitInfo,
    /// The game's output, redacted.
    pub output: &'a [String],
    pub crash_report: Option<RunFile<'a>>,
    pub jvm_error: Option<RunFile<'a>>,
    pub max_heap_mib: u32,
    /// What the installation should get, to suggest after running out.
    pub recommended_heap_mib: Option<u32>,
    /// The launcher ended the game because it kept asking on the console.
    pub console_question: bool,
}

/// After this many unanswered console questions the launcher ends the game.
pub const CONSOLE_QUESTION_LIMIT: u32 = 3;

/// A question asked on the console when no dialog can be shown: NeoForge's
/// `y/n:` and tinyfd's `[O]kay/[C]ancel:` or `[Y]es/[N]o:`. The game's input
/// is empty, so the question repeats without end and the game never goes on.
pub fn is_console_question(line: &str) -> bool {
    static QUESTION: LazyLock<Regex> =
        LazyLock::new(|| re(r"(?i)(?:^|\s)(?:y/n|\[o\]kay/\[c\]ancel|\[y\]es/\[n\]o)\s*[:?]?\s*$"));
    QUESTION.is_match(line)
}

const MAX_EVIDENCE: usize = 5;
const MAX_EVIDENCE_CHARS: usize = 300;
const MAX_SUSPECTS: usize = 5;

fn re(pattern: &str) -> Regex {
    Regex::new(pattern).expect("crash pattern")
}

/// Lines of the game output without log4j's XML wrapping.
fn plain(output: &[String]) -> Vec<String> {
    static TAG: LazyLock<Regex> = LazyLock::new(|| re(r"</?log4j:[A-Za-z]+[^>]*>"));
    output
        .iter()
        .map(|line| {
            let text = TAG.replace_all(line, "");
            let text = text.replace("<![CDATA[", "").replace("]]>", "");
            text.trim_end().to_owned()
        })
        .filter(|line| !line.trim().is_empty())
        .collect()
}

/// The JVM error file's header and frame lines; never its command line.
fn jvm_error_lines(text: &str) -> Vec<String> {
    let mut lines = Vec::new();
    let mut in_frames = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("# If you would like to submit") {
            break;
        }
        if trimmed.starts_with('#') && !trimmed.contains("java_command") {
            lines.push(trimmed.to_owned());
        }
    }
    for line in text.lines() {
        if line.starts_with("Native frames:") {
            in_frames = true;
            continue;
        }
        if in_frames {
            if line.trim().is_empty() || lines.len() > 60 {
                break;
            }
            lines.push(line.trim_end().to_owned());
        }
    }
    lines
}

struct Context {
    /// Output, crash report and JVM error lines together.
    lines: Vec<String>,
    report: Vec<String>,
    report_name: Option<String>,
    jvm_error: bool,
    jvm_error_name: Option<String>,
    exit: ExitInfo,
    max_heap_mib: u32,
    recommended_heap_mib: Option<u32>,
}

impl Context {
    /// The first line matching `pattern`, with its captures.
    fn find(&self, pattern: &Regex) -> Option<(String, regex::Captures<'_>)> {
        self.lines
            .iter()
            .find_map(|line| pattern.captures(line).map(|c| (line.clone(), c)))
    }

    fn all<'a>(
        &'a self,
        pattern: &'a Regex,
    ) -> impl Iterator<Item = (&'a str, regex::Captures<'a>)> {
        self.lines
            .iter()
            .filter_map(move |line| pattern.captures(line).map(|c| (line.as_str(), c)))
    }

    fn report_file(&self) -> Option<String> {
        self.report_name
            .clone()
            .or_else(|| self.jvm_error_name.clone())
    }
}

fn evidence(line: &str) -> String {
    let line = line.trim();
    if line.chars().count() > MAX_EVIDENCE_CHARS {
        let cut: String = line.chars().take(MAX_EVIDENCE_CHARS).collect();
        format!("{cut}…")
    } else {
        line.to_owned()
    }
}

fn diagnosis(kind: CrashKind, details: &[(&str, String)], lines: Vec<String>) -> CrashDiagnosis {
    CrashDiagnosis {
        kind,
        details: details
            .iter()
            .filter(|(_, v)| !v.is_empty())
            .map(|(k, v)| ((*k).to_owned(), v.clone()))
            .collect(),
        evidence: lines
            .iter()
            .take(MAX_EVIDENCE)
            .map(|l| evidence(l))
            .collect(),
        suspects: Vec::new(),
        report: None,
        actions: Vec::new(),
        notes: Vec::new(),
    }
}

fn file_name(path: &str) -> String {
    path.rsplit(['/', '\\']).next().unwrap_or(path).to_owned()
}

/// Java's class file version to the Java release (52 → 8, 65 → 21).
fn java_of_class(version: &str) -> String {
    version
        .parse::<u32>()
        .ok()
        .filter(|v| *v >= 45)
        .map_or_else(String::new, |v| (v - 44).to_string())
}

// ---------------------------------------------------------------- rules ----

fn jvm_setup(cx: &Context) -> Option<CrashDiagnosis> {
    static OPTION: LazyLock<Regex> = LazyLock::new(|| {
        re(
            r"(?:Unrecognized VM option|Improperly specified VM option|Unrecognized option:|Invalid -XX option) ?'?([^'\s]+)'?",
        )
    });
    static HEAP: LazyLock<Regex> = LazyLock::new(|| {
        re(
            r"Could not reserve enough space for (\d+)KB object heap|Invalid (?:maximum|initial) heap size|Initial heap size set to a larger value than the maximum heap size|The specified size exceeds the maximum representable size",
        )
    });
    static MAIN: LazyLock<Regex> =
        LazyLock::new(|| re(r"Error: Could not find or load main class (\S+)"));
    if let Some((line, c)) = cx.find(&OPTION) {
        return Some(diagnosis(
            CrashKind::JvmOption,
            &[("option", c[1].to_owned())],
            vec![line],
        ));
    }
    if let Some((line, _)) = cx.find(&HEAP) {
        return Some(diagnosis(
            CrashKind::HeapUnavailable,
            &[("memory", cx.max_heap_mib.to_string())],
            vec![line],
        ));
    }
    if let Some((line, c)) = cx.find(&MAIN) {
        return Some(diagnosis(
            CrashKind::BrokenInstallation,
            &[("file", c[1].to_owned())],
            vec![line],
        ));
    }
    None
}

fn java_version(cx: &Context) -> Option<CrashDiagnosis> {
    static OLD: LazyLock<Regex> = LazyLock::new(|| {
        re(
            r"class file version (\d+)(?:\.\d+)?\), this version of the Java Runtime only recognizes class file versions up to (\d+)",
        )
    });
    static NEW: LazyLock<Regex> =
        LazyLock::new(|| re(r"Unsupported class file major version (\d+)"));
    static NEW_LOADER: LazyLock<Regex> = LazyLock::new(|| {
        re(
            r"cannot be cast to (?:class )?java\.net\.URLClassLoader|jdk/nashorn|NoSuchFieldException: ucp|module java\.base does not .?opens java\.(?:lang|util)",
        )
    });
    if let Some((line, c)) = cx.find(&OLD) {
        return Some(diagnosis(
            CrashKind::JavaTooOld,
            &[
                ("required", java_of_class(&c[1])),
                ("found", java_of_class(&c[2])),
            ],
            vec![line],
        ));
    }
    if let Some((line, c)) = cx.find(&NEW) {
        return Some(diagnosis(
            CrashKind::JavaTooNew,
            &[("found", java_of_class(&c[1]))],
            vec![line],
        ));
    }
    if let Some((line, _)) = cx.find(&NEW_LOADER) {
        return Some(diagnosis(CrashKind::JavaTooNew, &[], vec![line]));
    }
    None
}

fn corrupt_files(cx: &Context) -> Option<CrashDiagnosis> {
    static ZIP: LazyLock<Regex> = LazyLock::new(|| {
        re(
            r"(?:ZipException: |^\s*)(zip END header not found|invalid LOC header|invalid CEN header|zip file is empty|invalid END header|invalid stored block lengths|invalid code lengths set|unexpected end of ZLIB input stream)|SHA1 digest error for (\S+)|Invalid signature file digest|Error reading mod|error in opening zip file|Broken zip file: |Failed to read as a zip: |Failed to read \S+ as a zip file|Found a zero byte file|File \S+ is not a jar file|(?i:unexpected end of ZLIB input stream)",
        )
    });
    static JAR: LazyLock<Regex> = LazyLock::new(|| re(r"([^\s\[\]'\x22]+\.jar)"));
    let (line, _) = cx.find(&ZIP)?;
    let path = cx
        .lines
        .iter()
        .filter(|l| ZIP.is_match(l) || l.contains("Error analyzing") || l.contains("zero byte"))
        .find_map(|l| JAR.captures(l).map(|c| c[1].to_owned()));
    // NeoForge names no file when a jar breaks its scan of the mods folder.
    let in_mods = path.as_deref().is_some_and(|p| {
        p.contains("/mods/")
            || p.contains("\\mods\\")
            || p.starts_with("mods")
            || p.starts_with("<mods>")
    }) || (path.is_none()
        && cx
            .lines
            .iter()
            .any(|l| l.contains("error occurred trying to setup the game directory")));
    let mut d = diagnosis(
        CrashKind::CorruptModFile,
        &[
            ("file", path.as_deref().map(file_name).unwrap_or_default()),
            ("place", if in_mods { "mods" } else { "game" }.to_owned()),
        ],
        vec![line],
    );
    d.actions = if in_mods {
        vec![CrashAction::OpenMods]
    } else {
        vec![CrashAction::Repair]
    };
    Some(d)
}

/// A dependency that is a mod loader, Minecraft or Java gets its own kind.
/// Quilt names them ("Minecraft", "Quilt Loader"), the others use IDs.
fn dependency_kind(dependency: &str, missing: bool) -> CrashKind {
    let dependency = dependency.to_lowercase();
    match dependency.as_str() {
        "minecraft" => CrashKind::WrongMinecraftVersion,
        "java" => CrashKind::JavaTooOld,
        _ if dependency.starts_with("openjdk") || dependency.starts_with("java hotspot") => {
            CrashKind::JavaTooOld
        }
        "fabricloader" | "fabric-loader" | "fabric loader" | "quilt_loader" | "quilt loader"
        | "forge" | "minecraft forge" | "neoforge" | "fml" | "javafml" | "lowcodefml" => {
            CrashKind::LoaderTooOld
        }
        _ if missing => CrashKind::MissingDependency,
        _ => CrashKind::WrongDependencyVersion,
    }
}

/// "version 1.2 or later" → "1.2 or later"; "any version" says nothing.
fn version_phrase(phrase: &str) -> String {
    let phrase = phrase.trim();
    if phrase == "any version" {
        return String::new();
    }
    phrase.strip_prefix("version ").unwrap_or(phrase).to_owned()
}

/// The other mod of a Fabric requirement: its name and ID.
fn fabric_dependency(c: &regex::Captures<'_>) -> (String, String) {
    let id = c.get(6).or(c.get(7)).map_or("", |m| m.as_str());
    let name = c.get(5).map_or(id, |m| m.as_str());
    (name.to_owned(), id.to_owned())
}

fn loader_dependencies(cx: &Context) -> Option<CrashDiagnosis> {
    // Fabric's `resolution.depends.*` messages. The other mod is "'Fabric API'
    // (fabric-api)" when some version of it is present, and only its ID
    // ("fabric-api") when it is missing; the version is a phrase such as
    // "version 1.2 or later", "any version before 2" or "any 1.x version".
    static FABRIC: LazyLock<Regex> = LazyLock::new(|| {
        re(
            r"Mod '([^']+)' \(([^)]+)\) (\S+) requires (.+?) of (?:(?:mod )?'([^']+)' \(([^)]+)\)|([^\s,']+)), (?:which is missing|but only the wrong versions? (?:is|are) present: (.+?)|(which is disabled for this environment|which can't be loaded due to other constraints)[^!]*)!",
        )
    });
    static FABRIC_BREAKS: LazyLock<Regex> = LazyLock::new(|| {
        re(
            r"Mod '([^']+)' \(([^)]+)\) (\S+) is incompatible with (.+?) of (?:(?:mod )?'([^']+)' \(([^)]+)\)|([^\s,']+))",
        )
    });
    // Quilt's `error.dep.*` and `error.break.*` titles: "<mod> requires
    // <version phrase> of <mod>, which is missing!" without IDs.
    static QUILT: LazyLock<Regex> = LazyLock::new(|| {
        re(
            r"^\s*(?:- )?(.+?) (?:transitively )?requires (.+?) of (.+?), (?:which is missing!|but only (?:a )?different versions? (?:is|are) present(?:: (.+))?)",
        )
    });
    static QUILT_BREAKS: LazyLock<Regex> = LazyLock::new(|| {
        re(
            r"^\s*(?:- )?(.+?) breaks(?: with)? (?:.+?) of (.+?), but (?:an incompatible version is|only incompatible versions are) present",
        )
    });
    static FORGE: LazyLock<Regex> = LazyLock::new(|| {
        re(
            r"Mod ID: '([^']+)', Requested by: '([^']+)', Expected range: '([^']*)', Actual version: '([^']*)'",
        )
    });
    // Forge's and NeoForge's screen texts (`missingdependency`,
    // `missingdependency.optional`, `incompatiblemod`) as logged, colour codes
    // removed; the next line says "Currently, <mod> is <version or not
    // installed>".
    static FORGE_MESSAGE: LazyLock<Regex> = LazyLock::new(|| {
        re(r"\bMod (\S+) (requires|only supports|is incompatible with) (\S+)(?: (.+?))?\s*$")
    });
    static CURRENTLY: LazyLock<Regex> = LazyLock::new(|| re(r"Currently, (\S+) is (.+?)\s*$"));
    // Forge for 1.12.2 and older: "MissingModsException: Mod tconstruct
    // (Tinkers' Construct) requires [mantle@[1.12-1.3.3.49,)]", then
    // "unknown : need [1.12-1.3.3.49,): have missing" (or the version found).
    static LEGACY: LazyLock<Regex> = LazyLock::new(|| {
        re(
            r"MissingModsException: Mod (\S+) \(([^)]+)\) requires \[([^@,\]\s]+)(?:@([\[(][^\])]*[\])]))?",
        )
    });
    static LEGACY_HAVE: LazyLock<Regex> = LazyLock::new(|| re(r": need \S+: have (.+?)\s*$"));
    static LEGACY_DUPLICATE: LazyLock<Regex> =
        LazyLock::new(|| re(r"Found a duplicate mod (\S+) at \[(.+?)\]?\s*$"));
    static LANGUAGE: LazyLock<Regex> = LazyLock::new(|| {
        re(r"(?:Mod file (\S+) )?needs language provider (\w+):(\S+) or above to load")
    });
    static DUPLICATE: LazyLock<Regex> = LazyLock::new(|| {
        re(
            r"Mod ID: '([^']+)' from mod files: (.+)|Mod (\S+) is present in multiple files: (.+)|Duplicate mod: (\S+)|Duplicate mandatory mod ids \[([^,\]]+)|(?i)(?:found|detected) duplicate mods?|duplicate mods? (?:found|detected)|is duplicated",
        )
    });

    // Fabric and Quilt list every unmet requirement; the first explains it.
    let fabric: Vec<_> = cx.all(&FABRIC).collect();
    if let Some((_, c)) = fabric.first() {
        let (name, did) = fabric_dependency(c);
        let missing = c.get(8).is_none() && c.get(9).is_none();
        let mut d = diagnosis(
            dependency_kind(&did, missing),
            &[
                ("mod", c[1].to_owned()),
                ("modId", c[2].to_owned()),
                ("dependency", name),
                ("dependencyId", did),
                ("required", version_phrase(&c[4])),
                (
                    "found",
                    c.get(8).map_or_else(String::new, |m| m.as_str().to_owned()),
                ),
                ("count", fabric.len().to_string()),
            ],
            fabric.iter().map(|(l, _)| (*l).to_owned()).collect(),
        );
        d.actions = vec![CrashAction::OpenMods];
        if d.kind == CrashKind::LoaderTooOld {
            d.actions.push(CrashAction::LoaderSettings);
        }
        if d.kind == CrashKind::JavaTooOld {
            d.actions = vec![CrashAction::JavaSettings];
        }
        return Some(d);
    }
    if let Some((line, c)) = cx.find(&FABRIC_BREAKS) {
        let (name, did) = fabric_dependency(&c);
        let mut d = diagnosis(
            CrashKind::IncompatibleMods,
            &[
                ("mod", c[1].to_owned()),
                ("modId", c[2].to_owned()),
                ("dependency", name),
                ("dependencyId", did),
            ],
            vec![line],
        );
        d.actions = vec![CrashAction::OpenMods];
        return Some(d);
    }
    let quilt: Vec<_> = cx.all(&QUILT).collect();
    if let Some((_, c)) = quilt.first() {
        let dependency = c[3].to_owned();
        let missing = c[0].contains("which is missing");
        let mut d = diagnosis(
            dependency_kind(&dependency, missing),
            &[
                ("mod", c[1].to_owned()),
                ("dependency", dependency.clone()),
                ("dependencyId", dependency),
                ("required", version_phrase(&c[2])),
                (
                    "found",
                    c.get(4).map_or_else(String::new, |m| m.as_str().to_owned()),
                ),
                ("count", quilt.len().to_string()),
            ],
            quilt.iter().map(|(l, _)| (*l).to_owned()).collect(),
        );
        d.actions = vec![CrashAction::OpenMods];
        if d.kind == CrashKind::LoaderTooOld {
            d.actions.push(CrashAction::LoaderSettings);
        }
        if d.kind == CrashKind::JavaTooOld {
            d.actions = vec![CrashAction::JavaSettings];
        }
        return Some(d);
    }
    if let Some((line, c)) = cx.find(&QUILT_BREAKS) {
        let mut d = diagnosis(
            CrashKind::IncompatibleMods,
            &[("mod", c[1].to_owned()), ("dependency", c[2].to_owned())],
            vec![line],
        );
        d.actions = vec![CrashAction::OpenMods];
        return Some(d);
    }
    let forge: Vec<_> = cx.all(&FORGE).collect();
    if let Some((_, c)) = forge.first() {
        let did = c[1].to_owned();
        let missing = &c[4] == "[MISSING]";
        let mut d = diagnosis(
            dependency_kind(&did, missing),
            &[
                ("mod", c[2].to_owned()),
                ("modId", c[2].to_owned()),
                ("dependency", did.clone()),
                ("dependencyId", did),
                ("required", c[3].to_owned()),
                (
                    "found",
                    if missing {
                        String::new()
                    } else {
                        c[4].to_owned()
                    },
                ),
                ("count", forge.len().to_string()),
            ],
            forge.iter().map(|(l, _)| (*l).to_owned()).collect(),
        );
        d.actions = vec![CrashAction::OpenMods];
        if d.kind == CrashKind::LoaderTooOld {
            d.actions.push(CrashAction::LoaderSettings);
        }
        return Some(d);
    }
    if let Some((line, c)) = cx.find(&FORGE_MESSAGE) {
        let did = c[3].to_owned();
        let found = cx
            .all(&CURRENTLY)
            .find(|(_, f)| f[1] == did)
            .map(|(_, f)| f[2].trim().to_owned())
            .filter(|v| !matches!(v.as_str(), "not installed" | "none"));
        let kind = if &c[2] == "is incompatible with" {
            CrashKind::IncompatibleMods
        } else {
            dependency_kind(&did, found.is_none() && &c[2] == "requires")
        };
        let mut d = diagnosis(
            kind,
            &[
                ("mod", c[1].to_owned()),
                ("modId", c[1].to_owned()),
                ("dependency", did.clone()),
                ("dependencyId", did),
                (
                    "required",
                    c.get(4)
                        .map(|m| m.as_str())
                        .filter(|r| *r != "any")
                        .unwrap_or_default()
                        .to_owned(),
                ),
                ("found", found.unwrap_or_default()),
            ],
            vec![line],
        );
        d.actions = vec![CrashAction::OpenMods];
        if d.kind == CrashKind::LoaderTooOld {
            d.actions.push(CrashAction::LoaderSettings);
        }
        return Some(d);
    }
    if let Some((line, c)) = cx.find(&LEGACY) {
        let did = c[3].to_owned();
        let found = cx
            .find(&LEGACY_HAVE)
            .map(|(_, h)| h[1].to_owned())
            .filter(|h| h != "missing");
        let mut d = diagnosis(
            dependency_kind(&did, found.is_none()),
            &[
                ("mod", c[2].to_owned()),
                ("modId", c[1].to_owned()),
                ("dependency", did.clone()),
                ("dependencyId", did),
                (
                    "required",
                    c.get(4).map_or_else(String::new, |m| m.as_str().to_owned()),
                ),
                ("found", found.unwrap_or_default()),
            ],
            vec![line],
        );
        d.actions = vec![CrashAction::OpenMods];
        if d.kind == CrashKind::LoaderTooOld {
            d.actions.push(CrashAction::LoaderSettings);
        }
        return Some(d);
    }
    if let Some((line, c)) = cx.find(&LEGACY_DUPLICATE) {
        let files = c[2]
            .split(", ")
            .map(file_name)
            .collect::<Vec<_>>()
            .join(", ");
        let mut d = diagnosis(
            CrashKind::DuplicateMods,
            &[("modId", c[1].to_owned()), ("files", files)],
            vec![line],
        );
        d.actions = vec![CrashAction::OpenMods];
        return Some(d);
    }
    if let Some((line, c)) = cx.find(&LANGUAGE) {
        let mut d = diagnosis(
            CrashKind::LoaderTooOld,
            &[
                (
                    "file",
                    c.get(1).map_or_else(String::new, |m| file_name(m.as_str())),
                ),
                ("required", format!("{}:{}", &c[2], &c[3])),
            ],
            vec![line],
        );
        d.actions = vec![CrashAction::LoaderSettings, CrashAction::OpenMods];
        return Some(d);
    }
    let duplicate = cx
        .all(&DUPLICATE)
        .find(|(_, c)| c.get(1).or(c.get(3)).or(c.get(5)).or(c.get(6)).is_some())
        .or_else(|| cx.all(&DUPLICATE).next())
        .map(|(l, c)| (l.to_owned(), c));
    if let Some((line, c)) = duplicate {
        let mut d = diagnosis(
            CrashKind::DuplicateMods,
            &[
                (
                    "modId",
                    c.get(1)
                        .or(c.get(3))
                        .or(c.get(5))
                        .or(c.get(6))
                        .map_or_else(String::new, |m| m.as_str().to_owned()),
                ),
                (
                    "files",
                    c.get(2)
                        .or(c.get(4))
                        .map(|m| m.as_str().to_owned())
                        .unwrap_or_else(|| listed_files(cx, &line)),
                ),
            ],
            vec![line],
        );
        d.actions = vec![CrashAction::OpenMods];
        return Some(d);
    }
    None
}

/// Quilt lists the files below its line, as "- <mods>/a.jar".
fn listed_files(cx: &Context, after: &str) -> String {
    let Some(start) = cx.lines.iter().position(|l| l == after) else {
        return String::new();
    };
    cx.lines[start + 1..]
        .iter()
        .map(|l| l.trim())
        .take_while(|l| l.starts_with("- ") && l.ends_with(".jar"))
        .map(file_name)
        .collect::<Vec<_>>()
        .join(", ")
}

fn other_loader(cx: &Context) -> Option<CrashNote> {
    static FABRIC_ON_FORGE: LazyLock<Regex> = LazyLock::new(|| {
        re(
            r"File (\S+) is (?:a|an) (Fabric|Quilt|Forge|NeoForge|LiteLoader) mod and cannot be loaded|File (\S+) is for (?:Minecraft Forge or an older version of NeoForge|an old(?:er)? version of (?:Minecraft )?Forge)",
        )
    });
    // Quilt: "Found 2 Forge mods." and "Found 1 Neoforge mods."
    static QUILT_SKIPPED: LazyLock<Regex> =
        LazyLock::new(|| re(r"Found \d+ (Forge|Neoforge) mods\."));
    static NO_TOML: LazyLock<Regex> = LazyLock::new(|| {
        re(r"Mod file (\S*[/\\]mods[/\\]\S+) is missing (?:neoforge\.)?mods\.toml file")
    });
    if let Some((_, c)) = cx.find(&FABRIC_ON_FORGE) {
        let file = c.get(1).or(c.get(3)).map_or("", |m| m.as_str());
        let loader = c.get(2).map_or("Forge", |m| m.as_str());
        return Some(CrashNote {
            kind: CrashKind::ModForOtherLoader,
            details: [
                ("file".to_owned(), file_name(file)),
                ("loader".to_owned(), loader.to_owned()),
            ]
            .into(),
        });
    }
    if let Some((_, c)) = cx.find(&QUILT_SKIPPED) {
        let loader = if &c[1] == "Forge" {
            "Forge"
        } else {
            "NeoForge"
        };
        return Some(CrashNote {
            kind: CrashKind::ModForOtherLoader,
            details: [("loader".to_owned(), loader.to_owned())].into(),
        });
    }
    cx.find(&NO_TOML).map(|(_, c)| CrashNote {
        kind: CrashKind::ModForOtherLoader,
        details: [("file".to_owned(), file_name(&c[1]))].into(),
    })
}

fn config(cx: &Context) -> Option<CrashDiagnosis> {
    static FILE: LazyLock<Regex> = LazyLock::new(|| {
        re(
            r"Failed loading config file (\S+) of type (\w+) for modid (\S+)|Error loading config file (\S+)|Failed to load config(?: file)? (\S+\.(?:toml|json5?|cfg|properties))",
        )
    });
    static PARSE: LazyLock<Regex> = LazyLock::new(|| {
        re(
            r"nightconfig\.core\.io\.ParsingException|ParsingException: Not enough data available|MalformedJsonException|JsonSyntaxException: .*(?:config|\.json)",
        )
    });
    let file = cx.find(&FILE);
    let parse = cx.find(&PARSE);
    if file.is_none() && parse.is_none() {
        return None;
    }
    let (name, mod_id) = file
        .as_ref()
        .map_or((String::new(), String::new()), |(_, c)| {
            let name = c
                .get(1)
                .or(c.get(4))
                .or(c.get(5))
                .map_or("", |m| m.as_str());
            (
                file_name(name),
                c.get(3).map_or_else(String::new, |m| m.as_str().to_owned()),
            )
        });
    let lines = [file.map(|(l, _)| l), parse.map(|(l, _)| l)]
        .into_iter()
        .flatten()
        .collect();
    let mut d = diagnosis(
        CrashKind::CorruptConfig,
        &[("file", name), ("modId", mod_id)],
        lines,
    );
    d.actions = vec![CrashAction::OpenConfig];
    Some(d)
}

fn mods_at_runtime(cx: &Context) -> Option<CrashDiagnosis> {
    static MIXIN: LazyLock<Regex> = LazyLock::new(|| {
        re(
            r"Mixin apply (?:for mod (\S+) )?failed|MixinApplyError|MixinTransformerError|InvalidInjectionException|InvalidMixinException|MixinPrepareError|Critical injection failure",
        )
    });
    static MIXIN_SOURCE: LazyLock<Regex> = LazyLock::new(|| {
        re(
            r"Mixin apply for mod (\S+) failed|from mod (\S+?)\]|([A-Za-z0-9_-]+)\.mixins?\.json|mixins\.([A-Za-z0-9_-]+)\.json",
        )
    });
    static ENTRYPOINT: LazyLock<Regex> = LazyLock::new(|| {
        re(
            r"Could not execute entrypoint stage '(\w+)' due to errors, provided by '([^']+)'|Failed to create mod instance\. ModID: ([^\s,]+)|(?:^|\s)(\S+) \((\S+)\) has failed to load correctly|Mod (\S+) has failed to load correctly|Caught exception from (.+?) \((\S+)\)",
        )
    });
    static OPTIFINE: LazyLock<Regex> = LazyLock::new(|| {
        re(
            r"(?i)optifine.*(reflector|NoSuchMethodError|NoSuchFieldError|invalid services|incompatible|not compatible)|(reflector|invalid services).*optifine",
        )
    });
    if let Some((line, _)) = cx.find(&OPTIFINE) {
        let mut d = diagnosis(CrashKind::OptifineConflict, &[], vec![line]);
        d.actions = vec![CrashAction::OpenMods];
        return Some(d);
    }
    if let Some((line, _)) = cx.find(&MIXIN) {
        let source = cx.all(&MIXIN_SOURCE).find_map(|(_, c)| {
            [1, 2, 3, 4]
                .iter()
                .find_map(|i| c.get(*i))
                .map(|m| m.as_str().to_owned())
        });
        let mut d = diagnosis(
            CrashKind::MixinFailed,
            &[("modId", source.unwrap_or_default())],
            vec![line],
        );
        d.actions = vec![CrashAction::OpenMods];
        return Some(d);
    }
    if let Some((line, c)) = cx.find(&ENTRYPOINT) {
        let mod_id = [2, 3, 5, 6, 8]
            .iter()
            .find_map(|i| c.get(*i))
            .map_or_else(String::new, |m| m.as_str().to_owned());
        let name = c
            .get(4)
            .or(c.get(7))
            .map_or_else(String::new, |m| m.as_str().to_owned());
        let mut d = diagnosis(
            CrashKind::ModFailedToLoad,
            &[("modId", mod_id), ("mod", name)],
            vec![line],
        );
        d.actions = vec![CrashAction::OpenMods];
        return Some(d);
    }
    None
}

/// The installation's recommendation when it is more, else 2 GiB more.
fn suggested_heap(cx: &Context) -> u32 {
    cx.recommended_heap_mib
        .filter(|r| *r > cx.max_heap_mib)
        .unwrap_or(cx.max_heap_mib + 2048)
}

fn memory(cx: &Context) -> Option<CrashDiagnosis> {
    // A modpack's memory check (Prominence II): "You have allocated less
    // memory( 1024 MB) than the recommended minimum for this pack: 4096 MB."
    static PACK: LazyLock<Regex> = LazyLock::new(|| {
        re(
            r"(?i)allocated (?:less|not enough) memory\s*\(\s*(\d+)\s*MB\s*\)\s*than the (?:recommended )?minimum[^:]*:\s*(\d+)\s*MB",
        )
    });
    static HEAP: LazyLock<Regex> = LazyLock::new(|| {
        re(
            r"OutOfMemoryError: (Java heap space|GC overhead limit exceeded|Metaspace|Compressed class space|Requested array size exceeds VM limit)",
        )
    });
    static NATIVE: LazyLock<Regex> = LazyLock::new(|| {
        re(
            r"OutOfMemoryError: (?:unable to create (?:new )?native thread|Direct buffer memory|Cannot reserve \d+ bytes of direct buffer memory)|There is insufficient memory for the Java Runtime Environment to continue|Native memory allocation \((?:mmap|malloc)\) failed|Out of system resources|The paging file is too small",
        )
    });
    if let Some((line, c)) = cx.find(&PACK) {
        let mut d = diagnosis(
            CrashKind::PackNeedsMemory,
            &[("memory", c[1].to_owned()), ("required", c[2].to_owned())],
            vec![line],
        );
        d.actions = vec![CrashAction::MemorySettings];
        return Some(d);
    }
    if let Some((line, c)) = cx.find(&HEAP) {
        let suggested = suggested_heap(cx);
        let mut d = diagnosis(
            CrashKind::OutOfMemory,
            &[
                ("area", c[1].to_owned()),
                ("memory", cx.max_heap_mib.to_string()),
                ("suggested", suggested.to_string()),
            ],
            vec![line],
        );
        d.actions = vec![CrashAction::MemorySettings];
        return Some(d);
    }
    if let Some((line, _)) = cx.find(&NATIVE) {
        let mut d = diagnosis(
            CrashKind::SystemMemory,
            &[("memory", cx.max_heap_mib.to_string())],
            vec![line],
        );
        d.actions = vec![CrashAction::MemorySettings];
        return Some(d);
    }
    None
}

fn graphics(cx: &Context) -> Option<CrashDiagnosis> {
    static OPENGL: LazyLock<Regex> = LazyLock::new(|| {
        re(
            r"(?i)GLFW error (65542|65543|65544|65545)|driver does not appear to support OpenGL|Pixel format not accelerated|Couldn't set pixel format|GLXBadFBConfig|WGL_ARB_create_context(?:_profile)? is unavailable|Failed to create the GLFW window|Could not create context|OpenGL 3\.2 (?:is )?(?:required|not supported)|NSGL: Failed to find a suitable pixel format|Unable to initialise OpenGL|GLFW error before init",
        )
    });
    static NATIVES: LazyLock<Regex> = LazyLock::new(|| {
        re(
            r"UnsatisfiedLinkError.*(lwjgl|glfw|openal|jemalloc|jinput)|Failed to locate library: (\S+)|no (lwjgl\w*) in java\.library\.path|incompatible architecture \(have '?(\w+)'?, need '?(\w+)",
        )
    });
    static RIVATUNER: LazyLock<Regex> = LazyLock::new(|| re(r"RivaTuner Statistics Server"));
    if let Some((line, c)) = cx.find(&OPENGL) {
        return Some(diagnosis(
            CrashKind::OpenglUnsupported,
            &[(
                "error",
                c.get(1).map_or_else(String::new, |m| m.as_str().to_owned()),
            )],
            vec![line],
        ));
    }
    if let Some((line, _)) = cx.find(&RIVATUNER) {
        return Some(diagnosis(
            CrashKind::Overlay,
            &[("program", "RivaTuner Statistics Server".to_owned())],
            vec![line],
        ));
    }
    if let Some((line, c)) = cx.find(&NATIVES) {
        let library = [1, 2, 3]
            .iter()
            .find_map(|i| c.get(*i))
            .map_or_else(String::new, |m| m.as_str().to_owned());
        let mut d = diagnosis(
            CrashKind::MissingNativeLibrary,
            &[("library", library)],
            vec![line],
        );
        d.actions = vec![CrashAction::Repair];
        return Some(d);
    }
    None
}

/// The vendor of a graphics driver module, by file name.
fn driver_vendor(module: &str) -> Option<&'static str> {
    static VENDORS: LazyLock<[(Regex, &'static str); 4]> = LazyLock::new(|| {
        [
            (
                re(r"(?i)^(atio6axx|atioglxx|atig6pxx|atiumd|amdxc|amdvlk|aticfx|amdocl)"),
                "amd",
            ),
            (
                re(r"(?i)^(ig\w*icd(32|64)|igxelpicd|igdumd|igc\d*|igd\w*umd)"),
                "intel",
            ),
            (
                re(r"(?i)^(nvoglv|nvwgf2um|libnvidia|nvd3dum|nvlddmkm)"),
                "nvidia",
            ),
            (
                re(
                    r"(?i)^(radeonsi|iris|i965|crocus|swrast|kms_swrast|zink|nouveau|r600|virtio_gpu)_dri|^libgallium|^libGLX_mesa",
                ),
                "mesa",
            ),
        ]
    });
    VENDORS
        .iter()
        .find(|(pattern, _)| pattern.is_match(module))
        .map(|(_, vendor)| *vendor)
}

fn native(cx: &Context) -> Option<CrashDiagnosis> {
    static FATAL: LazyLock<Regex> = LazyLock::new(|| {
        re(
            r"A fatal error has been detected by the Java Runtime Environment|EXCEPTION_ACCESS_VIOLATION|EXCEPTION_STACK_OVERFLOW|EXCEPTION_IN_PAGE_ERROR",
        )
    });
    static SIGNAL: LazyLock<Regex> =
        LazyLock::new(|| re(r"(EXCEPTION_\w+|SIG[A-Z]+) \(0x[0-9a-f]+\)"));
    static FRAME: LazyLock<Regex> = LazyLock::new(|| re(r"^#?\s*[CjJVv]\s+\[([^\]+]+)"));
    static OVERLAY: LazyLock<Regex> = LazyLock::new(|| {
        re(
            r"(?i)\b(RTSSHooks\w*|DiscordHook\w*|graphics-hook\w*|nvspcap\w*|D3DGear\w*|OWClient\w*|GameOverlayRenderer\w*|MSIAfterburner\w*|bdcamvk\w*|action_x64)\b",
        )
    });
    if !cx.jvm_error && cx.find(&FATAL).is_none() {
        return None;
    }
    let fatal = cx.find(&FATAL).map(|(l, _)| l);
    let signal = cx.find(&SIGNAL).map(|(l, c)| (l, c[1].to_owned()));
    let frames: Vec<(String, String)> = cx
        .lines
        .iter()
        .filter_map(|l| FRAME.captures(l).map(|c| (l.clone(), c[1].to_owned())))
        .collect();
    let mut lines: Vec<String> = fatal.into_iter().collect();
    if let Some((l, _)) = &signal {
        lines.push(l.clone());
    }
    if let Some((overlay_line, program)) = cx.find(&OVERLAY).map(|(l, c)| (l, c[1].to_owned())) {
        lines.push(overlay_line);
        return Some(diagnosis(
            CrashKind::Overlay,
            &[("program", program)],
            lines,
        ));
    }
    if let Some((frame_line, module, vendor)) = frames
        .iter()
        .find_map(|(l, m)| driver_vendor(m).map(|v| (l.clone(), m.clone(), v)))
    {
        lines.push(frame_line);
        return Some(diagnosis(
            CrashKind::GraphicsDriver,
            &[("module", module), ("vendor", vendor.to_owned())],
            lines,
        ));
    }
    let frame = frames.first().cloned();
    if let Some((l, _)) = &frame {
        lines.push(l.clone());
    }
    let mut d = diagnosis(
        CrashKind::NativeCrash,
        &[
            ("module", frame.map(|(_, m)| m).unwrap_or_default()),
            ("signal", signal.map(|(_, s)| s).unwrap_or_default()),
        ],
        lines,
    );
    d.actions = vec![CrashAction::OpenGameFolder];
    Some(d)
}

fn system(cx: &Context) -> Option<CrashDiagnosis> {
    static DISK: LazyLock<Regex> = LazyLock::new(|| {
        re(
            r"No space left on device|There is not enough space on the disk|not enough space on the disk",
        )
    });
    static ACCESS: LazyLock<Regex> = LazyLock::new(|| {
        re(
            r"AccessDeniedException: (\S+)|FileSystemException: (\S+?): The process cannot access the file|being used by another process|Permission denied",
        )
    });
    if let Some((line, _)) = cx.find(&DISK) {
        return Some(diagnosis(CrashKind::DiskFull, &[], vec![line]));
    }
    if let Some((line, c)) = cx.find(&ACCESS) {
        let file = c
            .get(1)
            .or(c.get(2))
            .map_or_else(String::new, |m| file_name(m.as_str()));
        return Some(diagnosis(
            CrashKind::FileAccess,
            &[("file", file)],
            vec![line],
        ));
    }
    None
}

/// The crash report's category, the exception and what it names.
fn crash_report(cx: &Context) -> Option<CrashDiagnosis> {
    static DESCRIPTION: LazyLock<Regex> = LazyLock::new(|| re(r"^Description: (.+)$"));
    static ENTITY_TYPE: LazyLock<Regex> = LazyLock::new(|| re(r"^\s*Entity Type: (\S+)"));
    static ENTITY_AT: LazyLock<Regex> = LazyLock::new(|| re(r"^\s*Entity's Exact location: (.+)$"));
    static BLOCK_NAME: LazyLock<Regex> = LazyLock::new(|| re(r"^\s*Name: (\S+)"));
    static BLOCK_AT: LazyLock<Regex> = LazyLock::new(|| re(r"^\s*Block location: (.+)$"));
    static HANG: LazyLock<Regex> =
        LazyLock::new(|| re(r"(?i)a single server tick took ([\d.]+) seconds"));
    if cx.report.is_empty() {
        return None;
    }
    let find = |pattern: &Regex| {
        cx.report.iter().find_map(|l| {
            pattern
                .captures(l)
                .map(|c| (l.clone(), c[1].trim().to_owned()))
        })
    };
    let (desc_line, description) = find(&DESCRIPTION)?;
    // The exception follows the description after one empty line.
    let exception = cx
        .report
        .iter()
        .skip_while(|l| !l.starts_with("Description:"))
        .skip(1)
        .find(|l| !l.trim().is_empty())
        .cloned()
        .unwrap_or_default();
    let lower = description.to_lowercase();
    let mut lines = vec![desc_line.clone()];
    if !exception.is_empty() {
        lines.push(exception.clone());
    }
    let base = [
        ("description", description.clone()),
        ("exception", evidence(&exception)),
    ];
    let mut d = if lower.contains("manually triggered debug crash") {
        diagnosis(CrashKind::DebugCrash, &base, lines)
    } else if lower.contains("watching server") || find(&HANG).is_some() {
        let seconds = find(&HANG).map(|(_, s)| s).unwrap_or_default();
        diagnosis(
            CrashKind::ServerHang,
            &[("seconds", seconds), ("description", description)],
            lines,
        )
    } else if lower.contains("ticking entity") || lower.contains("rendering entity") {
        let mut d = diagnosis(
            CrashKind::TickingEntity,
            &[
                (
                    "entity",
                    find(&ENTITY_TYPE).map(|(_, v)| v).unwrap_or_default(),
                ),
                (
                    "position",
                    find(&ENTITY_AT).map(|(_, v)| v).unwrap_or_default(),
                ),
                ("description", description),
            ],
            lines,
        );
        d.actions = vec![CrashAction::Backups];
        d
    } else if lower.contains("ticking block entity") {
        let mut d = diagnosis(
            CrashKind::TickingBlockEntity,
            &[
                (
                    "block",
                    find(&BLOCK_NAME).map(|(_, v)| v).unwrap_or_default(),
                ),
                (
                    "position",
                    find(&BLOCK_AT).map(|(_, v)| v).unwrap_or_default(),
                ),
                ("description", description),
            ],
            lines,
        );
        d.actions = vec![CrashAction::Backups];
        d
    } else if lower.contains("nbt")
        || lower.contains("level.dat")
        || lower.contains("loading world")
        || lower.contains("reading chunk")
    {
        let mut d = diagnosis(CrashKind::WorldLoadFailed, &base, lines);
        d.actions = vec![CrashAction::Backups];
        d
    } else {
        diagnosis(CrashKind::GameCrash, &base, lines)
    };
    d.actions.push(CrashAction::OpenCrashReports);
    Some(d)
}

/// What the exit code or signal alone says.
/// The message above the first unanswered console question.
fn console_question(cx: &Context) -> CrashDiagnosis {
    let first = cx
        .lines
        .iter()
        .position(|l| is_console_question(l))
        .unwrap_or(cx.lines.len());
    let start = first.saturating_sub(MAX_EVIDENCE);
    let lines = cx.lines[start..first]
        .iter()
        .filter(|l| !l.trim().is_empty())
        .cloned()
        .collect();
    diagnosis(CrashKind::ConsoleQuestion, &[], lines)
}

fn exit_status(cx: &Context) -> CrashDiagnosis {
    let code = cx.exit.code;
    let signal = cx.exit.signal;
    let details = [
        ("exitCode", code.map_or_else(String::new, |c| c.to_string())),
        ("signal", signal.map_or_else(String::new, |s| s.to_string())),
    ];
    let kind = match (code, signal) {
        (_, Some(9)) | (Some(137), None) => CrashKind::Killed,
        (_, Some(15)) | (Some(143), None) => CrashKind::Terminated,
        (_, Some(6 | 11 | 7 | 4)) | (Some(134 | 139), None) => CrashKind::NativeCrash,
        // Windows status codes (as signed 32-bit exit codes).
        (
            Some(
                -1_073_741_819 | -1_073_740_791 | -1_073_741_571 | -1_073_740_940 | -1_073_741_676,
            ),
            _,
        ) => CrashKind::NativeCrash,
        (Some(-1_073_740_777), _) => CrashKind::Overlay,
        (Some(-805_306_369), _) => CrashKind::OutOfMemory,
        (Some(-1_073_741_515 | -2_147_483_645), _) => CrashKind::BrokenInstallation,
        _ => CrashKind::Unknown,
    };
    let mut d = diagnosis(kind, &details, Vec::new());
    match kind {
        CrashKind::Overlay => {
            d.details.insert("program".to_owned(), "D3DGear".to_owned());
        }
        CrashKind::OutOfMemory => {
            d.details
                .insert("memory".to_owned(), cx.max_heap_mib.to_string());
            d.details
                .insert("suggested".to_owned(), suggested_heap(cx).to_string());
            d.actions = vec![CrashAction::MemorySettings];
        }
        CrashKind::Killed => d.actions = vec![CrashAction::MemorySettings],
        CrashKind::BrokenInstallation => d.actions = vec![CrashAction::Repair],
        _ => {}
    }
    d
}

/// Mods a crash report or stack trace points at.
fn suspects(cx: &Context) -> Vec<String> {
    // Forge and NeoForge: "Suspected Mods:" then "\tName (id), Version: 1.0".
    static LISTED: LazyLock<Regex> = LazyLock::new(|| {
        re(r"^\s*(?:Suspected Mods?:\s*)?(\S.*?) \(([a-z][a-z0-9_.-]*)\), Version: ")
    });
    static TRANSFORMER: LazyLock<Regex> =
        LazyLock::new(|| re(r"TRANSFORMER/([a-z][a-z0-9_.-]*)@[^/\s]+/"));
    let mut found: Vec<(String, String)> = Vec::new();
    for line in &cx.lines {
        let entry = if let Some(c) = LISTED.captures(line) {
            Some((c[2].to_owned(), format!("{} ({})", &c[1], &c[2])))
        } else {
            TRANSFORMER
                .captures(line)
                .map(|c| (c[1].to_owned(), c[1].to_owned()))
        };
        if let Some((id, shown)) = entry {
            let loader = matches!(id.as_str(), "minecraft" | "forge" | "neoforge" | "fml");
            if !loader && !found.iter().any(|(known, _)| *known == id) {
                found.push((id, shown));
            }
        }
    }
    found.truncate(MAX_SUSPECTS);
    found.into_iter().map(|(_, shown)| shown).collect()
}

/// Explains why a run ended, or `None` when it simply ended. A run that
/// exited with code 0 counts only when it wrote a crash report (Forge's mod
/// error screen ends that way when the player closes it).
pub fn diagnose(input: &CrashInput<'_>) -> Option<CrashDiagnosis> {
    let report: Vec<String> = input
        .crash_report
        .as_ref()
        .map(|r| r.text.lines().map(str::to_owned).collect())
        .unwrap_or_default();
    let clean = input.exit.clean() && report.is_empty() && input.jvm_error.is_none();
    let mut lines = plain(input.output);
    lines.extend(report.iter().cloned());
    if let Some(jvm) = &input.jvm_error {
        lines.extend(jvm_error_lines(jvm.text));
    }
    let cx = Context {
        lines,
        report,
        report_name: input.crash_report.as_ref().map(|r| r.name.to_owned()),
        jvm_error: input.jvm_error.is_some(),
        jvm_error_name: input.jvm_error.as_ref().map(|r| r.name.to_owned()),
        exit: input.exit,
        max_heap_mib: input.max_heap_mib,
        recommended_heap_mib: input.recommended_heap_mib,
    };
    let rules: [fn(&Context) -> Option<CrashDiagnosis>; 11] = [
        jvm_setup,
        java_version,
        loader_dependencies,
        corrupt_files,
        config,
        memory,
        graphics,
        native,
        mods_at_runtime,
        system,
        crash_report,
    ];
    // A clean exit without a report still failed when the loader's error
    // screen was closed: Forge for 1.12.2 writes no report for it. Only the
    // loaders' own dependency messages count then.
    let found = if clean {
        loader_dependencies(&cx)
            .filter(|d| d.kind != CrashKind::DuplicateMods || d.details.contains_key("modId"))?
            .into()
    } else {
        rules.iter().find_map(|rule| rule(&cx))
    };
    let question = input.console_question.then(|| console_question(&cx));
    let mut d = match (found, question) {
        (Some(mut d), Some(question)) => {
            d.notes.push(CrashNote {
                kind: question.kind,
                details: BTreeMap::new(),
            });
            d
        }
        (Some(d), None) => d,
        (None, Some(question)) => question,
        (None, None) => exit_status(&cx),
    };
    if input.exit.clean() && d.kind == CrashKind::Unknown {
        return None;
    }
    d.details
        .entry("exitCode".to_owned())
        .or_insert_with(|| input.exit.code.map_or_else(String::new, |c| c.to_string()));
    d.details.retain(|_, v| !v.is_empty());
    d.suspects = suspects(&cx);
    d.report = cx.report_file();
    if d.report.is_some() && !d.actions.contains(&CrashAction::OpenCrashReports) {
        d.actions.push(if cx.report_name.is_some() {
            CrashAction::OpenCrashReports
        } else {
            CrashAction::OpenGameFolder
        });
    }
    if !d.suspects.is_empty() && !d.actions.contains(&CrashAction::OpenMods) {
        d.actions.push(CrashAction::OpenMods);
    }
    d.actions.dedup();
    if let Some(note) = other_loader(&cx).filter(|_| d.kind != CrashKind::ModForOtherLoader) {
        d.notes.push(note);
    }
    Some(d)
}

impl ExitInfo {
    /// The exit code and, on Unix, the signal of a finished process.
    pub fn of(status: &std::process::ExitStatus) -> Self {
        #[cfg(unix)]
        let signal = std::os::unix::process::ExitStatusExt::signal(status);
        #[cfg(not(unix))]
        let signal = None;
        Self {
            code: status.code(),
            signal,
        }
    }
}

#[cfg(test)]
mod tests;
