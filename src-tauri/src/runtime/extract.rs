//! Safe extraction of runtime archives (docs/spec/04 "Safe archive and path
//! handling").
//!
//! Every entry path is normalized and must stay inside the destination; absolute
//! paths, `..`, drive prefixes, alternate-stream syntax, NUL and control
//! characters are rejected, as are duplicate or case-colliding paths. Symbolic
//! links are allowed only with a relative target that resolves inside the
//! archive to an entry that exists, and nothing is ever written through a link.
//! Hard links may only name an earlier regular file and are materialized as
//! copies. Devices, FIFOs and other special entries are rejected. Entry count,
//! file size and total size are bounded while reading, independent of headers.

use std::collections::{HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use super::adoptium::ArchiveFormat;
use crate::error::{ErrorCode, LauncherError, LauncherResult};

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub max_entries: usize,
    pub max_file_bytes: u64,
    pub max_total_bytes: u64,
}

impl Default for Limits {
    /// Temurin JREs expand to roughly 130–220 MB in 200–600 entries; JDKs to
    /// about 350 MB.
    fn default() -> Self {
        Self {
            max_entries: 20_000,
            max_file_bytes: 512 * 1024 * 1024,
            max_total_bytes: 1024 * 1024 * 1024,
        }
    }
}

pub fn unsafe_archive(reason: &str) -> LauncherError {
    LauncherError::new(
        ErrorCode::UnsafeArchive,
        "The Java runtime package contains unsafe or unsupported content.",
    )
    .with_detail("reason", reason.to_owned())
}

const WINDOWS_RESERVED: &[&str] = &[
    "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8",
    "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
];

/// Splits an archive path into safe components; `.` and empty parts are dropped.
pub fn normalize(raw: &str) -> LauncherResult<Vec<String>> {
    if raw.starts_with('/') || raw.starts_with('\\') {
        return Err(unsafe_archive("absolute path"));
    }
    let mut parts = Vec::new();
    for part in raw.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            return Err(unsafe_archive("parent directory reference"));
        }
        let stem = part.split('.').next().unwrap_or("").to_ascii_lowercase();
        if part.len() > 255
            || part
                .chars()
                .any(|c| matches!(c, '\\' | ':' | '\0') || c.is_control())
            || WINDOWS_RESERVED.contains(&stem.as_str())
            || part.ends_with(' ')
        {
            return Err(unsafe_archive("unsupported path component"));
        }
        parts.push(part.to_owned());
    }
    if parts.is_empty() {
        return Err(unsafe_archive("empty path"));
    }
    Ok(parts)
}

/// Resolves a link target relative to the link's directory. `None` if it leaves
/// the archive root or is absolute.
pub fn resolve_link(link: &[String], target: &str) -> Option<Vec<String>> {
    if target.is_empty()
        || target.starts_with('/')
        || target.contains('\\')
        || target.contains(':')
        || target.contains('\0')
    {
        return None;
    }
    let mut resolved: Vec<String> = link[..link.len() - 1].to_vec();
    for part in target.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                resolved.pop()?;
            }
            other => resolved.push(other.to_owned()),
        }
    }
    (!resolved.is_empty()).then_some(resolved)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Dir,
    File,
    Link,
}

struct Extractor<'a> {
    dest: &'a Path,
    limits: Limits,
    entries: usize,
    total: u64,
    /// Lowercased path → kind, for duplicate and case-collision checks.
    seen: HashMap<String, Kind>,
    /// Exact paths of regular files, for hard links.
    files: HashSet<String>,
    links: Vec<(Vec<String>, Vec<String>)>,
}

fn key(parts: &[String]) -> String {
    parts.join("/").to_lowercase()
}

fn io_error(error: &io::Error) -> LauncherError {
    LauncherError::from_io(error, "Extracting the Java runtime failed")
}

impl<'a> Extractor<'a> {
    fn new(dest: &'a Path, limits: Limits) -> Self {
        Self {
            dest,
            limits,
            entries: 0,
            total: 0,
            seen: HashMap::new(),
            files: HashSet::new(),
            links: Vec::new(),
        }
    }

    fn path(&self, parts: &[String]) -> PathBuf {
        let mut path = self.dest.to_path_buf();
        path.extend(parts);
        path
    }

    /// Registers an entry and makes sure its parents are plain directories.
    fn admit(&mut self, parts: &[String], kind: Kind) -> LauncherResult<PathBuf> {
        self.entries += 1;
        if self.entries > self.limits.max_entries {
            return Err(unsafe_archive("too many entries"));
        }
        for depth in 1..parts.len() {
            match self.seen.get(&key(&parts[..depth])) {
                None => {
                    self.seen.insert(key(&parts[..depth]), Kind::Dir);
                    fs::create_dir_all(self.path(&parts[..depth])).map_err(|e| io_error(&e))?;
                }
                Some(Kind::Dir) => {}
                Some(_) => return Err(unsafe_archive("entry below a file or link")),
            }
        }
        match self.seen.get(&key(parts)) {
            Some(Kind::Dir) if kind == Kind::Dir => {}
            Some(_) => return Err(unsafe_archive("duplicate or case-colliding path")),
            None => {
                self.seen.insert(key(parts), kind);
            }
        }
        Ok(self.path(parts))
    }

    fn dir(&mut self, parts: &[String]) -> LauncherResult<()> {
        let path = self.admit(parts, Kind::Dir)?;
        fs::create_dir_all(path).map_err(|e| io_error(&e))
    }

    fn file(
        &mut self,
        parts: &[String],
        declared: u64,
        executable: bool,
        reader: &mut dyn Read,
    ) -> LauncherResult<()> {
        if declared > self.limits.max_file_bytes {
            return Err(unsafe_archive("file too large"));
        }
        if self.total + declared > self.limits.max_total_bytes {
            return Err(unsafe_archive("expanded size too large"));
        }
        let path = self.admit(parts, Kind::File)?;
        let mut out = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|e| io_error(&e))?;
        let written =
            io::copy(&mut reader.take(declared + 1), &mut out).map_err(|e| io_error(&e))?;
        if written != declared {
            return Err(unsafe_archive("entry size does not match its header"));
        }
        out.flush().map_err(|e| io_error(&e))?;
        drop(out);
        set_mode(&path, executable)?;
        self.total += declared;
        self.files.insert(parts.join("/"));
        Ok(())
    }

    fn hard_link(&mut self, parts: &[String], target: &str) -> LauncherResult<()> {
        let target = normalize(target)?;
        if !self.files.contains(&target.join("/")) {
            return Err(unsafe_archive("hard link to a missing or non-file entry"));
        }
        let source = self.path(&target);
        let size = fs::metadata(&source).map_err(|e| io_error(&e))?.len();
        let executable = is_executable(&source);
        let mut reader = File::open(&source).map_err(|e| io_error(&e))?;
        self.file(parts, size, executable, &mut reader)
    }

    fn symlink(&mut self, parts: &[String], target: &str) -> LauncherResult<()> {
        let resolved =
            resolve_link(parts, target).ok_or_else(|| unsafe_archive("link leaves the runtime"))?;
        let path = self.admit(parts, Kind::Link)?;
        self.links.push((parts.to_vec(), resolved));
        create_symlink(target, &path)
    }

    /// Every link must end at an existing file or directory inside the archive,
    /// following link chains a bounded number of times.
    fn finish(&self) -> LauncherResult<()> {
        let by_path: HashMap<&[String], &[String]> = self
            .links
            .iter()
            .map(|(link, target)| (link.as_slice(), target.as_slice()))
            .collect();
        for (_, target) in &self.links {
            let mut current = target.clone();
            let mut hops = 0;
            loop {
                match self.seen.get(&key(&current)) {
                    Some(Kind::File | Kind::Dir) => break,
                    Some(Kind::Link) => {
                        hops += 1;
                        if hops > 16 {
                            return Err(unsafe_archive("link loop"));
                        }
                        current = by_path
                            .get(current.as_slice())
                            .ok_or_else(|| unsafe_archive("dangling link"))?
                            .to_vec();
                    }
                    None => return Err(unsafe_archive("dangling link")),
                }
            }
        }
        Ok(())
    }
}

#[cfg(unix)]
fn set_mode(path: &Path, executable: bool) -> LauncherResult<()> {
    use std::os::unix::fs::PermissionsExt;
    let mode = if executable { 0o755 } else { 0o644 };
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).map_err(|e| io_error(&e))
}

#[cfg(not(unix))]
fn set_mode(_path: &Path, _executable: bool) -> LauncherResult<()> {
    Ok(())
}

#[cfg(unix)]
pub fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    fs::symlink_metadata(path).is_ok_and(|m| m.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
pub fn is_executable(_path: &Path) -> bool {
    false
}

#[cfg(unix)]
fn create_symlink(target: &str, path: &Path) -> LauncherResult<()> {
    std::os::unix::fs::symlink(target, path).map_err(|e| io_error(&e))
}

#[cfg(not(unix))]
fn create_symlink(_target: &str, _path: &Path) -> LauncherResult<()> {
    // Windows packages have no links; creating them would need extra privileges.
    Err(unsafe_archive("links are not supported on this platform"))
}

fn utf8(bytes: &[u8]) -> LauncherResult<&str> {
    std::str::from_utf8(bytes).map_err(|_| unsafe_archive("path is not UTF-8"))
}

/// Extracts `archive` into the empty directory `dest`.
pub fn extract(
    archive: &Path,
    format: ArchiveFormat,
    dest: &Path,
    limits: Limits,
) -> LauncherResult<()> {
    let file = File::open(archive).map_err(|e| io_error(&e))?;
    let mut extractor = Extractor::new(dest, limits);
    match format {
        ArchiveFormat::TarGz => extract_tar(file, &mut extractor)?,
        ArchiveFormat::Zip => extract_zip(file, &mut extractor)?,
        ArchiveFormat::MojangManifest => {
            return Err(LauncherError::internal("A file list is not an archive."));
        }
    }
    extractor.finish()
}

/// One entry of a package delivered as a file list instead of an archive.
pub enum Listed<'a> {
    Dir,
    /// A verified downloaded file to copy in.
    File {
        source: PathBuf,
        size: u64,
        executable: bool,
    },
    Link(&'a str),
}

/// Builds a package from a file list into the empty directory `dest`, with the
/// same path, link and size rules as archive extraction.
pub fn materialize<'a>(
    entries: impl IntoIterator<Item = (&'a str, Listed<'a>)>,
    dest: &Path,
    limits: Limits,
) -> LauncherResult<()> {
    let mut extractor = Extractor::new(dest, limits);
    for (path, entry) in entries {
        let parts = normalize(path)?;
        match entry {
            Listed::Dir => extractor.dir(&parts)?,
            Listed::File {
                source,
                size,
                executable,
            } => {
                let mut reader = File::open(source).map_err(|e| io_error(&e))?;
                extractor.file(&parts, size, executable, &mut reader)?;
            }
            Listed::Link(target) => extractor.symlink(&parts, target)?,
        }
    }
    extractor.finish()
}

fn unreadable() -> LauncherError {
    unsafe_archive("archive could not be read")
}

fn extract_tar(file: File, x: &mut Extractor<'_>) -> LauncherResult<()> {
    use tar::EntryType;
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(io::BufReader::new(file)));
    for entry in archive.entries().map_err(|_| unreadable())? {
        let mut entry = entry.map_err(|_| unreadable())?;
        let raw = entry.path_bytes().into_owned();
        let parts = normalize(utf8(&raw)?)?;
        let kind = entry.header().entry_type();
        match kind {
            EntryType::Directory => x.dir(&parts)?,
            EntryType::Regular | EntryType::Continuous => {
                let size = entry.header().size().map_err(|_| unreadable())?;
                let mode = entry.header().mode().map_err(|_| unreadable())?;
                x.file(&parts, size, mode & 0o111 != 0, &mut entry)?;
            }
            EntryType::Symlink => {
                let target = entry.link_name_bytes().ok_or_else(unreadable)?.into_owned();
                x.symlink(&parts, utf8(&target)?)?;
            }
            EntryType::Link => {
                let target = entry.link_name_bytes().ok_or_else(unreadable)?.into_owned();
                x.hard_link(&parts, utf8(&target)?)?;
            }
            _ => return Err(unsafe_archive("special file entry")),
        }
    }
    Ok(())
}

const S_IFMT: u32 = 0o170_000;
const S_IFLNK: u32 = 0o120_000;
const S_IFREG: u32 = 0o100_000;
const S_IFDIR: u32 = 0o040_000;

fn extract_zip(file: File, x: &mut Extractor<'_>) -> LauncherResult<()> {
    let mut archive = zip::ZipArchive::new(io::BufReader::new(file)).map_err(|_| unreadable())?;
    if archive.len() > x.limits.max_entries {
        return Err(unsafe_archive("too many entries"));
    }
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|_| unreadable())?;
        let raw = entry.name_raw().to_vec();
        let parts = normalize(utf8(&raw)?)?;
        let mode = entry.unix_mode();
        let file_type = mode.map(|m| m & S_IFMT).filter(|t| *t != 0);
        if entry.is_dir() || file_type == Some(S_IFDIR) {
            x.dir(&parts)?;
        } else if file_type == Some(S_IFLNK) {
            let mut target = String::new();
            entry
                .by_ref()
                .take(4096)
                .read_to_string(&mut target)
                .map_err(|_| unreadable())?;
            x.symlink(&parts, &target)?;
        } else if file_type.is_none() || file_type == Some(S_IFREG) {
            let size = entry.size();
            let executable = mode.is_some_and(|m| m & 0o111 != 0);
            x.file(&parts, size, executable, &mut entry)?;
        } else {
            return Err(unsafe_archive("special file entry"));
        }
    }
    Ok(())
}

/// The single top-level directory of an extracted package.
pub fn single_root(dest: &Path) -> LauncherResult<PathBuf> {
    let mut entries = fs::read_dir(dest).map_err(|e| io_error(&e))?;
    let first = entries
        .next()
        .ok_or_else(|| unsafe_archive("empty archive"))?
        .map_err(|e| io_error(&e))?;
    if entries.next().is_some() {
        return Err(unsafe_archive("more than one top-level entry"));
    }
    let file_type = first.file_type().map_err(|e| io_error(&e))?;
    if !file_type.is_dir() {
        return Err(unsafe_archive("top-level entry is not a directory"));
    }
    Ok(first.path())
}

#[cfg(test)]
pub(crate) mod tests;
