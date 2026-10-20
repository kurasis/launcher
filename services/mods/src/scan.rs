//! The jar scanner (D-066, stage 6): every archive the catalog keeps is read
//! for signs of the tricks mod malware has used (fractureiser in 2023 and the
//! stealers after it) before anyone can download it.
//!
//! Each class file is parsed for the classes and methods it names, the
//! strings in its constant pool and the strings its code builds byte by byte
//! (`new String(new byte[]{104, 116, ...})`, how fractureiser hid its
//! address). Nested jars (Jar-in-Jar) are read the same way, a few levels
//! deep.
//!
//! One weak sign proves nothing: many honest mods start programs, open
//! connections or bundle native libraries. Those are only noted. A file goes
//! into quarantine for a *combination* of strong signs, mostly within one
//! class (loading code together with the network, a hidden address together
//! with running it), or for a marker of a known family. The owner decides
//! what stays (`lapi-mods review`).
//!
//! A scan is bounded like every archive the catalog reads: entries, bytes
//! read, class size, nesting.

use std::collections::BTreeSet;
use std::io::{Read, Seek};
use std::path::Path;

/// Raised whenever the rules change: kept files scanned under older rules
/// are scanned again in the background.
pub const RULES: i64 = 1;

const MAX_ENTRIES: usize = 100_000;
/// All the bytes one scan reads out of an archive and its nested jars.
const MAX_READ: u64 = 1024 * 1024 * 1024;
const MAX_CLASS: u64 = 8 * 1024 * 1024;
const MAX_NESTED: u64 = 64 * 1024 * 1024;
const MAX_DEPTH: usize = 3;
/// A string a class builds byte by byte, kept for matching.
const MAX_BUILT: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Clean,
    /// Weak signs only: served, and the signs are kept for the owner.
    Note,
    /// Strong signs together: quarantined until the owner decides.
    Suspicious,
    /// A marker of known malware: quarantined.
    Malicious,
}

impl Level {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Clean => "clean",
            Self::Note => "note",
            Self::Suspicious => "suspicious",
            Self::Malicious => "malicious",
        }
    }

    pub fn held(self) -> bool {
        self >= Self::Suspicious
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verdict {
    pub level: Level,
    /// The signs seen, in order, as fixed words.
    pub signs: Vec<&'static str>,
}

impl Verdict {
    pub fn clean() -> Self {
        Self {
            level: Level::Clean,
            signs: Vec::new(),
        }
    }

    pub fn signs_text(&self) -> String {
        self.signs.join(",")
    }
}

/// The signs, as kept in the database and shown to the owner.
pub mod sign {
    /// Opens connections (URL streams, sockets, HTTP clients).
    pub const NETWORK: &str = "network";
    /// Defines or loads classes from bytes or addresses (URLClassLoader, defineClass).
    pub const LOADS_CODE: &str = "loads_code";
    /// Starts programs (Runtime.exec, ProcessBuilder).
    pub const RUNS_PROGRAMS: &str = "runs_programs";
    /// Builds a web address byte by byte instead of writing it.
    pub const HIDDEN_ADDRESS: &str = "hidden_address";
    /// Builds the name of a code-loading or program-running call byte by byte.
    pub const HIDDEN_CALL: &str = "hidden_call";
    /// Names the files where launchers, browsers and Discord keep sign-ins.
    pub const SIGN_IN_FILES: &str = "sign_in_files";
    /// Names places programs start from with the computer.
    pub const AUTOSTART: &str = "autostart";
    /// Carries programs or scripts for the computer.
    pub const PROGRAMS_INSIDE: &str = "programs_inside";
    /// A marker of a known malware family.
    pub const KNOWN_MALWARE: &str = "known_malware";
    /// Part of the archive could not be read within the limits.
    pub const UNREAD: &str = "unread";
}

/// Markers published for known families: fractureiser's addresses.
const KNOWN: &[&str] = &["85.217.144.130", "107.189.3.101", "files-8ie.pages.dev"];

/// Where sign-ins are kept: launchers, Discord, browsers.
const SIGN_IN_FILES: &[&str] = &[
    "launcher_accounts",
    "launcher_accounts_microsoft_store",
    "microsoft_accounts.json",
    "lunarclient/settings/game/accounts.json",
    "feather/accounts.json",
    "\\local storage\\leveldb",
    "/local storage/leveldb",
    "\\user data\\default\\login data",
    "/user data/default/login data",
    "\\user data\\local state",
];

const AUTOSTART: &[&str] = &[
    "currentversion\\run",
    "\\start menu\\programs\\startup",
    "/library/launchagents",
    ".config/autostart",
    ".config/systemd/user",
];

/// Calls a built-up string may name.
const HIDDEN_CALLS: &[&str] = &[
    "java.net.urlclassloader",
    "java/net/urlclassloader",
    "defineclass",
    "loadclass",
    "java.lang.runtime",
    "java/lang/runtime",
    "getruntime",
    "exec",
    "java.lang.processbuilder",
];

const PROGRAM_ENDINGS: &[&str] = &[".exe", ".bat", ".cmd", ".ps1", ".vbs", ".scr", ".msi"];

/// What one class shows.
#[derive(Default)]
struct ClassSigns {
    network: bool,
    loads_code: bool,
    runs_programs: bool,
    hidden_address: bool,
    hidden_call: bool,
}

/// What the whole archive shows.
#[derive(Default)]
struct Seen {
    signs: BTreeSet<&'static str>,
    held: bool,
    known: bool,
    read: u64,
}

impl Seen {
    fn class(&mut self, c: &ClassSigns) {
        for (on, name) in [
            (c.network, sign::NETWORK),
            (c.loads_code, sign::LOADS_CODE),
            (c.runs_programs, sign::RUNS_PROGRAMS),
            (c.hidden_address, sign::HIDDEN_ADDRESS),
            (c.hidden_call, sign::HIDDEN_CALL),
        ] {
            if on {
                self.signs.insert(name);
            }
        }
        // Together in one class, the way a dropper looks.
        if (c.loads_code && (c.network || c.hidden_address))
            || (c.hidden_address && (c.runs_programs || c.hidden_call))
            || (c.hidden_call && c.network)
        {
            self.held = true;
        }
    }

    fn verdict(self) -> Verdict {
        let has = |s| self.signs.contains(s);
        let level = if self.known {
            Level::Malicious
        } else if self.held
            || (has(sign::SIGN_IN_FILES) && has(sign::NETWORK))
            || (has(sign::AUTOSTART) && (has(sign::RUNS_PROGRAMS) || has(sign::NETWORK)))
        {
            Level::Suspicious
        } else if self.signs.is_empty() {
            Level::Clean
        } else {
            Level::Note
        };
        Verdict {
            level,
            signs: self.signs.into_iter().collect(),
        }
    }
}

/// Scans a kept file. A file that is not a zip archive has nothing to scan.
pub fn scan_file(path: &Path) -> std::io::Result<Verdict> {
    let file = std::fs::File::open(path)?;
    let Ok(mut zip) = zip::ZipArchive::new(std::io::BufReader::new(file)) else {
        return Ok(Verdict::clean());
    };
    let mut seen = Seen::default();
    archive(&mut zip, 0, &mut seen);
    Ok(seen.verdict())
}

/// Scans an archive held in memory (tests, nested jars).
#[cfg(test)]
pub fn scan_bytes(bytes: &[u8]) -> Verdict {
    let Ok(mut zip) = zip::ZipArchive::new(std::io::Cursor::new(bytes)) else {
        return Verdict::clean();
    };
    let mut seen = Seen::default();
    archive(&mut zip, 0, &mut seen);
    seen.verdict()
}

fn archive<R: Read + Seek>(zip: &mut zip::ZipArchive<R>, depth: usize, seen: &mut Seen) {
    if zip.len() > MAX_ENTRIES {
        seen.signs.insert(sign::UNREAD);
        return;
    }
    for i in 0..zip.len() {
        let Ok(mut entry) = zip.by_index(i) else {
            seen.signs.insert(sign::UNREAD);
            continue;
        };
        if entry.is_dir() {
            continue;
        }
        let name = entry.name().to_ascii_lowercase();
        if PROGRAM_ENDINGS.iter().any(|e| name.ends_with(e)) {
            seen.signs.insert(sign::PROGRAMS_INSIDE);
        }
        let nested = name.ends_with(".jar") || name.ends_with(".zip");
        let limit = if name.ends_with(".class") {
            MAX_CLASS
        } else if nested && depth < MAX_DEPTH {
            MAX_NESTED
        } else {
            continue;
        };
        if entry.size() > limit || seen.read.saturating_add(entry.size()) > MAX_READ {
            seen.signs.insert(sign::UNREAD);
            continue;
        }
        let mut bytes = Vec::new();
        match (&mut entry).take(limit + 1).read_to_end(&mut bytes) {
            Ok(n) if n as u64 <= limit => {}
            _ => {
                seen.signs.insert(sign::UNREAD);
                continue;
            }
        }
        seen.read += bytes.len() as u64;
        if seen.read > MAX_READ {
            seen.signs.insert(sign::UNREAD);
            return;
        }
        if nested {
            if let Ok(mut inner) = zip::ZipArchive::new(std::io::Cursor::new(bytes.as_slice())) {
                archive(&mut inner, depth + 1, seen);
            }
        } else if let Some(class) = Class::parse(&bytes) {
            look_at(&class, seen);
        }
    }
}

fn look_at(class: &Class, seen: &mut Seen) {
    let mut signs = ClassSigns::default();
    for (owner, name) in &class.calls {
        match (owner.as_str(), name.as_str()) {
            ("java/net/URL", "openConnection" | "openStream")
            | ("java/net/Socket", "<init>")
            | ("java/net/http/HttpClient", "newHttpClient" | "newBuilder" | "send" | "sendAsync")
            | (
                "java/nio/channels/SocketChannel" | "java/nio/channels/AsynchronousSocketChannel",
                "open",
            ) => signs.network = true,
            ("java/net/URLClassLoader", _)
            | (_, "defineClass" | "defineAnonymousClass" | "defineHiddenClass") => {
                signs.loads_code = true;
            }
            ("java/lang/Runtime", "exec") | ("java/lang/ProcessBuilder", "<init>" | "start") => {
                signs.runs_programs = true;
            }
            _ => {}
        }
    }
    for text in &class.strings {
        words(text, seen);
    }
    for text in &class.built {
        let lower = text.to_ascii_lowercase();
        if lower.starts_with("http://") || lower.starts_with("https://") {
            signs.hidden_address = true;
        }
        if HIDDEN_CALLS.iter().any(|c| lower == *c) {
            signs.hidden_call = true;
        }
        words(text, seen);
    }
    seen.class(&signs);
}

/// Matches the fixed lists against one string.
fn words(text: &str, seen: &mut Seen) {
    if text.len() < 6 {
        return;
    }
    let lower = text.to_ascii_lowercase();
    if KNOWN.iter().any(|k| lower.contains(k)) {
        seen.known = true;
        seen.signs.insert(sign::KNOWN_MALWARE);
    }
    if SIGN_IN_FILES.iter().any(|k| lower.contains(k)) {
        seen.signs.insert(sign::SIGN_IN_FILES);
    }
    if AUTOSTART.iter().any(|k| lower.contains(k)) {
        seen.signs.insert(sign::AUTOSTART);
    }
}

/// What a class file names: the methods it calls (owner and name), the
/// strings of its constant pool and the strings its code builds from bytes.
#[derive(Debug, Default)]
pub struct Class {
    pub calls: Vec<(String, String)>,
    pub strings: Vec<String>,
    pub built: Vec<String>,
}

#[derive(Clone, Copy)]
enum Constant {
    Utf8(usize),
    Class(u16),
    Ref { class: u16, name_and_type: u16 },
    NameAndType { name: u16 },
    Other,
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let end = self.at.checked_add(n)?;
        let slice = self.bytes.get(self.at..end)?;
        self.at = end;
        Some(slice)
    }

    fn u1(&mut self) -> Option<u8> {
        self.take(1).map(|b| b[0])
    }

    fn u2(&mut self) -> Option<u16> {
        self.take(2).map(|b| u16::from_be_bytes([b[0], b[1]]))
    }

    fn u4(&mut self) -> Option<u32> {
        self.take(4)
            .map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }
}

impl Class {
    /// Parses a class file; `None` when it is not one or is cut short.
    pub fn parse(bytes: &[u8]) -> Option<Self> {
        let mut r = Reader { bytes, at: 0 };
        if r.u4()? != 0xCAFE_BABE {
            return None;
        }
        r.take(4)?;
        let count = usize::from(r.u2()?);
        let mut pool = vec![Constant::Other; count.max(1)];
        let mut utf8: Vec<String> = Vec::new();
        let mut i = 1;
        while i < count {
            let tag = r.u1()?;
            pool[i] = match tag {
                1 => {
                    let len = usize::from(r.u2()?);
                    // Modified UTF-8 reads as UTF-8 for every ordinary character.
                    utf8.push(String::from_utf8_lossy(r.take(len)?).into_owned());
                    Constant::Utf8(utf8.len() - 1)
                }
                7 => Constant::Class(r.u2()?),
                9..=11 => Constant::Ref {
                    class: r.u2()?,
                    name_and_type: r.u2()?,
                },
                12 => {
                    let name = r.u2()?;
                    r.u2()?;
                    Constant::NameAndType { name }
                }
                3 | 4 => {
                    r.take(4)?;
                    Constant::Other
                }
                5 | 6 => {
                    r.take(8)?;
                    // Eight-byte constants take two slots.
                    i += 1;
                    Constant::Other
                }
                8 | 16 | 19 | 20 => {
                    r.u2()?;
                    Constant::Other
                }
                15 => {
                    r.take(3)?;
                    Constant::Other
                }
                17 | 18 => {
                    r.take(4)?;
                    Constant::Other
                }
                _ => return None,
            };
            i += 1;
        }
        let text = |index: u16| -> Option<&str> {
            match pool.get(usize::from(index))? {
                Constant::Utf8(at) => utf8.get(*at).map(String::as_str),
                _ => None,
            }
        };
        let mut calls = Vec::new();
        for constant in &pool {
            if let Constant::Ref {
                class,
                name_and_type,
            } = *constant
                && let Some(Constant::Class(owner)) = pool.get(usize::from(class))
                && let Some(Constant::NameAndType { name }) = pool.get(usize::from(name_and_type))
                && let (Some(owner), Some(name)) = (text(*owner), text(*name))
            {
                calls.push((owner.to_owned(), name.to_owned()));
            }
        }
        // Class names count as calls with no method, for the ones named only.
        for constant in &pool {
            if let Constant::Class(name) = *constant
                && let Some(name) = text(name)
            {
                calls.push((name.to_owned(), String::new()));
            }
        }

        let code_name = utf8.iter().position(|s| s == "Code");
        let mut built = Vec::new();
        // access, this, super, interfaces
        r.take(6)?;
        let interfaces = usize::from(r.u2()?);
        r.take(interfaces.checked_mul(2)?)?;
        // fields, then methods
        for members in 0..2 {
            let n = r.u2()?;
            for _ in 0..n {
                r.take(6)?;
                let attributes = r.u2()?;
                for _ in 0..attributes {
                    let name = r.u2()?;
                    let len = r.u4()? as usize;
                    let body = r.take(len)?;
                    let is_code = members == 1
                        && matches!(pool.get(usize::from(name)), Some(Constant::Utf8(at)) if Some(*at) == code_name);
                    if is_code && body.len() >= 8 {
                        let code_len =
                            u32::from_be_bytes([body[4], body[5], body[6], body[7]]) as usize;
                        if let Some(code) = body.get(8..8usize.saturating_add(code_len)) {
                            built_strings(code, &mut built);
                        }
                    }
                }
            }
        }
        Some(Self {
            calls,
            strings: utf8,
            built,
        })
    }
}

/// The length of the instruction at `pc`, or `None` past the end or on a
/// byte that is no instruction.
fn op_len(code: &[u8], pc: usize) -> Option<usize> {
    let op = *code.get(pc)?;
    let word = |at: usize| -> Option<i32> {
        let b = code.get(at..at + 4)?;
        Some(i32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    };
    Some(match op {
        0x10 | 0x12 | 0x15..=0x19 | 0x36..=0x3a | 0xa9 | 0xbc => 2,
        0x11
        | 0x13
        | 0x14
        | 0x84
        | 0x99..=0xa8
        | 0xb2..=0xb8
        | 0xbb
        | 0xbd
        | 0xc0
        | 0xc1
        | 0xc6
        | 0xc7 => 3,
        0xc5 => 4,
        0xb9 | 0xba | 0xc8 | 0xc9 => 5,
        0xaa => {
            let start = (pc + 4) & !3;
            let low = word(start + 4)?;
            let high = word(start + 8)?;
            let n = usize::try_from(i64::from(high) - i64::from(low) + 1).ok()?;
            start + 12 + n.checked_mul(4)? - pc
        }
        0xab => {
            let start = (pc + 4) & !3;
            let pairs = usize::try_from(word(start + 4)?).ok()?;
            start + 8 + pairs.checked_mul(8)? - pc
        }
        0xc4 => {
            if *code.get(pc + 1)? == 0x84 {
                6
            } else {
                4
            }
        }
        0x00..=0x0f
        | 0x1a..=0x35
        | 0x3b..=0x83
        | 0x85..=0x98
        | 0xac..=0xb1
        | 0xbe
        | 0xbf
        | 0xc2
        | 0xc3
        | 0xca => 1,
        _ => return None,
    })
}

/// The int an instruction pushes, when it pushes a constant.
fn pushed(code: &[u8], pc: usize) -> Option<i32> {
    let op = *code.get(pc)?;
    match op {
        0x02..=0x08 => Some(i32::from(op) - 3),
        0x10 => code.get(pc + 1).map(|b| i32::from(*b as i8)),
        0x11 => code
            .get(pc + 1..pc + 3)
            .map(|b| i32::from(i16::from_be_bytes([b[0], b[1]]))),
        _ => None,
    }
}

/// Collects the byte and char arrays a method fills element by element:
/// `newarray byte`, then `dup, index, value, bastore` again and again.
fn built_strings(code: &[u8], out: &mut Vec<String>) {
    let mut pc = 0;
    let mut current: Option<Vec<u16>> = None;
    let mut consts: Vec<i32> = Vec::new();
    let finish = |current: &mut Option<Vec<u16>>, out: &mut Vec<String>| {
        if let Some(units) = current.take()
            && units.len() >= 4
        {
            let text: String = char::decode_utf16(units.iter().copied())
                .map(|c| c.unwrap_or('\u{fffd}'))
                .collect();
            if text.chars().filter(|c| !c.is_control()).count() * 10 >= text.chars().count() * 9 {
                out.push(text);
            }
        }
    };
    while let Some(len) = op_len(code, pc) {
        let op = code[pc];
        if let Some(value) = pushed(code, pc) {
            consts.push(value);
            if consts.len() > 2 {
                consts.remove(0);
            }
        } else {
            match op {
                // newarray byte (8) or char (5)
                0xbc if matches!(code.get(pc + 1), Some(8 | 5)) => {
                    finish(&mut current, out);
                    current = Some(Vec::new());
                }
                // dup
                0x59 => {}
                // bastore, castore
                0x54 | 0x55 => {
                    if let (Some(units), [index, value]) = (current.as_mut(), consts.as_slice())
                        && let Ok(index) = usize::try_from(*index)
                        && index < MAX_BUILT
                    {
                        if units.len() <= index {
                            units.resize(index + 1, 0);
                        }
                        units[index] = if op == 0x54 {
                            u16::from(*value as u8)
                        } else {
                            *value as u16
                        };
                    }
                    consts.clear();
                }
                _ => {
                    finish(&mut current, out);
                    consts.clear();
                }
            }
        }
        pc += len;
    }
    finish(&mut current, out);
}

/// Whether a stored file is an archive worth scanning, by its type.
pub fn scanned_type(media_type: &str) -> bool {
    matches!(media_type, "application/java-archive" | "application/zip")
}
