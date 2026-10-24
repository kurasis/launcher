//! What the launcher sends, and the checks every field passes before anything
//! is stored (D-070). Text is cleaned, not trusted: it is printed on the
//! owner's terminal later.

use std::sync::LazyLock;

use serde::Deserialize;

/// Kinds, categories and their subcategories; the launcher reads the same
/// file for its form.
static TREE: LazyLock<Tree> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../categories.json")).expect("categories.json")
});

#[derive(Deserialize)]
struct Tree {
    kinds: Vec<String>,
    categories: Vec<Category>,
}

#[derive(Deserialize)]
struct Category {
    id: String,
    sub: Vec<String>,
}

pub const MIN_MESSAGE_CHARS: usize = 20;
pub const MAX_MESSAGE_CHARS: usize = 5000;
/// A short subject the player gives the review ("Crash with shaders").
pub const MAX_SUBJECT_CHARS: usize = 80;
pub const MAX_IMAGES: usize = 5;
/// Files the player picked plus the logs the launcher found.
pub const MAX_TEXTS: usize = 12;
pub const MAX_TEXT_BYTES: usize = 512 * 1024;
pub const MAX_TEXTS_BYTES: usize = 3 * 1024 * 1024;
/// One picture as sent (the launcher scales it down before).
pub const MAX_IMAGE_BYTES: usize = 8 * 1024 * 1024;
/// The whole request body.
pub const MAX_BODY: usize = 16 * 1024 * 1024;

/// Text files by extension; the content decides the rest.
const TEXT_EXTENSIONS: [&str; 13] = [
    "log",
    "txt",
    "json",
    "json5",
    "toml",
    "cfg",
    "conf",
    "properties",
    "yml",
    "yaml",
    "xml",
    "csv",
    "ini",
];

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Incoming {
    pub schema: u32,
    pub challenge: String,
    pub nonce: String,
    pub kind: String,
    pub category: String,
    #[serde(default)]
    pub subcategory: Option<String>,
    #[serde(default)]
    pub subject: Option<String>,
    pub message: String,
    #[serde(default)]
    pub rating: Option<u8>,
    pub launcher_version: String,
    pub os: String,
    pub language: String,
    #[serde(default)]
    pub install_id: Option<String>,
    /// The diagnostic ID of the launcher error the review is about, or the
    /// ad complained about.
    #[serde(default)]
    pub diagnostic_id: Option<String>,
    #[serde(default)]
    pub ad: Option<i64>,
    #[serde(default)]
    pub files: Vec<IncomingFile>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct IncomingFile {
    pub name: String,
    /// `text` (UTF-8 in `text`) or `image` (base64 in `data`).
    pub kind: String,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub data: Option<String>,
}

/// A review that passed the checks, before its pictures are re-encoded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checked {
    pub kind: String,
    pub category: String,
    pub subcategory: String,
    /// One line, or none when the player left it empty.
    pub subject: Option<String>,
    pub message: String,
    pub rating: Option<u8>,
    pub launcher_version: String,
    pub os: String,
    pub language: String,
    pub install_id: Option<String>,
    pub diagnostic_id: Option<String>,
    pub ad: Option<i64>,
    pub texts: Vec<(String, String)>,
    pub images: Vec<Vec<u8>>,
}

/// Control characters (but newline and tab) and those that reorder text
/// become U+FFFD; carriage returns go.
pub fn clean(text: &str) -> String {
    text.chars()
        .filter(|c| *c != '\r')
        .map(|c| {
            let reorders = matches!(c, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}');
            if (c.is_control() && c != '\n' && c != '\t') || reorders {
                '\u{FFFD}'
            } else {
                c
            }
        })
        .collect()
}

/// The subject as kept: cleaned, on one line, trimmed; an empty one is none.
pub fn subject(text: Option<&str>) -> Result<Option<String>, &'static str> {
    let Some(text) = text else {
        return Ok(None);
    };
    let one_line: String = clean(text)
        .chars()
        .map(|c| if c == '\n' || c == '\t' { ' ' } else { c })
        .collect();
    let one_line = one_line
        .split(' ')
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    match one_line.chars().count() {
        0 => Ok(None),
        n if n > MAX_SUBJECT_CHARS => Err("subject"),
        _ => Ok(Some(one_line)),
    }
}

/// A short field of letters, digits and a few signs.
fn token(text: &str, max: usize, extra: &str) -> bool {
    !text.is_empty()
        && text.len() <= max
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || extra.contains(c))
}

/// An install ID as the launcher makes them: a hyphenated lower-case UUID.
pub fn install_id(text: &str) -> bool {
    text.len() == 36
        && text.char_indices().all(|(i, c)| match i {
            8 | 13 | 18 | 23 => c == '-',
            _ => c.is_ascii_digit() || ('a'..='f').contains(&c),
        })
}

/// The kind, category and subcategory as stored: an unknown subcategory
/// becomes "other", so a launcher with an older tree loses nothing.
pub fn place(kind: &str, category: &str, sub: Option<&str>) -> Option<(String, String)> {
    if !TREE.kinds.iter().any(|k| k == kind) {
        return None;
    }
    let found = TREE.categories.iter().find(|c| c.id == category);
    Some(match (found, sub) {
        (Some(c), Some(s)) if c.sub.iter().any(|x| x == s) => (c.id.clone(), s.to_owned()),
        (Some(c), _) => (c.id.clone(), "other".to_owned()),
        (None, _) => ("other".to_owned(), "other".to_owned()),
    })
}

/// A file name to show and store: the last part of a path, only harmless
/// characters, at most 80 bytes, and an extension of a text file.
pub fn text_name(name: &str) -> Option<String> {
    let base = name.rsplit(['/', '\\']).next().unwrap_or_default();
    let safe: String = base
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .take(80)
        .collect();
    let safe = safe.trim_start_matches('.').to_owned();
    let extension = safe.rsplit_once('.')?.1.to_ascii_lowercase();
    TEXT_EXTENSIONS
        .contains(&extension.as_str())
        .then_some(safe)
}

/// Text a person or a program wrote, not binary data: no NUL, and at most
/// one control character in a hundred (colour codes of logs are fine).
fn readable(text: &str) -> bool {
    if text.contains('\0') {
        return false;
    }
    let controls = text
        .chars()
        .filter(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t' | '\u{1b}'))
        .count();
    controls * 100 <= text.chars().count()
}

impl Incoming {
    /// The review as it will be stored, or why it is refused.
    pub fn check(self) -> Result<Checked, &'static str> {
        use base64::Engine as _;
        if self.schema != 1 {
            return Err("schema");
        }
        let (category, subcategory) =
            place(&self.kind, &self.category, self.subcategory.as_deref()).ok_or("kind")?;
        let subject = subject(self.subject.as_deref())?;
        let message = clean(self.message.trim());
        let chars = message.chars().count();
        if !(MIN_MESSAGE_CHARS..=MAX_MESSAGE_CHARS).contains(&chars) {
            return Err("message");
        }
        let rating = match self.rating {
            None => None,
            Some(r) if self.kind == "impression" && (1..=5).contains(&r) => Some(r),
            Some(_) => return Err("rating"),
        };
        if !token(&self.launcher_version, 32, ".+-") {
            return Err("launcherVersion");
        }
        if !token(&self.os, 64, ".-_ ") {
            return Err("os");
        }
        if !token(&self.language, 16, "-") {
            return Err("language");
        }
        if self.install_id.as_deref().is_some_and(|i| !install_id(i)) {
            return Err("installId");
        }
        if self
            .diagnostic_id
            .as_deref()
            .is_some_and(|id| !token(id, 64, "-_"))
        {
            return Err("diagnosticId");
        }
        if self.ad.is_some_and(|a| !(1..=1_000_000_000).contains(&a)) {
            return Err("ad");
        }
        let mut texts = Vec::new();
        let mut images = Vec::new();
        let mut text_bytes = 0;
        for file in self.files {
            match (file.kind.as_str(), file.text, file.data) {
                ("text", Some(text), None) => {
                    let name = text_name(&file.name).ok_or("file name")?;
                    if text.len() > MAX_TEXT_BYTES || !readable(&text) {
                        return Err("text file");
                    }
                    text_bytes += text.len();
                    texts.push((name, text));
                }
                ("image", None, Some(data)) => {
                    if data.len() > MAX_IMAGE_BYTES / 3 * 4 + 4 {
                        return Err("image size");
                    }
                    let bytes = base64::engine::general_purpose::STANDARD
                        .decode(data.as_bytes())
                        .map_err(|_| "image data")?;
                    images.push(bytes);
                }
                _ => return Err("file"),
            }
        }
        if texts.len() > MAX_TEXTS || images.len() > MAX_IMAGES || text_bytes > MAX_TEXTS_BYTES {
            return Err("files");
        }
        // Two files with one name: the later one gets a number.
        let mut seen = std::collections::HashSet::new();
        for (name, _) in &mut texts {
            let mut candidate = name.clone();
            let mut n = 2;
            while !seen.insert(candidate.to_ascii_lowercase()) {
                candidate = format!("{n}-{name}");
                n += 1;
            }
            *name = candidate;
        }
        Ok(Checked {
            kind: self.kind,
            category,
            subcategory,
            subject,
            message,
            rating,
            launcher_version: self.launcher_version,
            os: self.os,
            language: self.language,
            install_id: self.install_id,
            diagnostic_id: self.diagnostic_id,
            ad: self.ad,
            texts,
            images,
        })
    }
}

/// Signs of spam: many links, or hardly any letters. Such a review is kept,
/// marked, and left out of the usual list.
pub fn looks_like_spam(message: &str) -> bool {
    let lower = message.to_lowercase();
    let links = lower.matches("http://").count()
        + lower.matches("https://").count()
        + lower.matches("www.").count();
    let chars = message
        .chars()
        .filter(|c| !c.is_whitespace())
        .count()
        .max(1);
    let letters = message.chars().filter(|c| c.is_alphabetic()).count();
    links > 5 || letters * 10 < chars * 3
}
