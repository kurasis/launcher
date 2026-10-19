//! What an adapter (Modrinth now, CurseForge next) makes of its source's
//! answers: our shape of a project and its versions, before they are stored.

/// The kinds of things in the catalog.
pub const KINDS: &[&str] = &[
    "mod",
    "modpack",
    "resourcepack",
    "shader",
    "datapack",
    "world",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceImage {
    pub url: String,
    pub title: String,
    pub description: String,
}

#[derive(Debug, Clone)]
pub struct SourceProject {
    pub source: &'static str,
    pub source_id: String,
    pub slug: String,
    pub kind: &'static str,
    pub title: String,
    pub summary: String,
    pub body: String,
    pub license: String,
    pub icon_url: Option<String>,
    pub client_side: String,
    pub server_side: String,
    pub downloads: i64,
    pub followers: i64,
    pub categories: Vec<String>,
    /// Name and role, in the source's order.
    pub authors: Vec<(String, String)>,
    pub gallery: Vec<SourceImage>,
    pub url: String,
    pub published: i64,
    pub updated: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceFile {
    pub name: String,
    pub size: i64,
    pub sha1: Option<String>,
    pub sha512: Option<String>,
    pub url: String,
    pub primary: bool,
    pub role: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceDependency {
    /// required, optional, incompatible or embedded.
    pub kind: String,
    pub project: Option<String>,
    pub version: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SourceVersion {
    pub source_id: String,
    pub number: String,
    pub name: String,
    pub changelog: String,
    /// release, beta or alpha.
    pub channel: String,
    pub published: i64,
    pub downloads: i64,
    pub games: Vec<String>,
    pub loaders: Vec<String>,
    pub files: Vec<SourceFile>,
    pub dependencies: Vec<SourceDependency>,
}

/// A text from a source, bounded: control characters out (newlines and tabs
/// stay where `multiline`), at most `max` characters.
pub fn clean(text: &str, max: usize, multiline: bool) -> String {
    text.chars()
        .filter(|c| !c.is_control() || (multiline && (*c == '\n' || *c == '\t')))
        .filter(|c| !matches!(c, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}'))
        .take(max)
        .collect::<String>()
        .trim()
        .to_owned()
}

/// A word of a closed vocabulary from a source (a loader, a category, a game
/// version): short, plain ASCII.
pub fn word(text: &str) -> Option<String> {
    let ok = !text.is_empty()
        && text.len() <= 48
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_' | b'+'));
    ok.then(|| text.to_ascii_lowercase())
}

/// What the versions of a project to fetch at once are: for every pair of
/// game version and loader, the newest release, or the newest of any channel
/// when there is no release. Returns indexes into `versions`.
pub fn newest_per_pair(versions: &[SourceVersion]) -> Vec<usize> {
    use std::collections::BTreeMap;
    let mut best: BTreeMap<(String, String), usize> = BTreeMap::new();
    for (i, version) in versions.iter().enumerate() {
        let loaders: Vec<String> = if version.loaders.is_empty() {
            vec![String::new()]
        } else {
            version.loaders.clone()
        };
        for game in &version.games {
            for loader in &loaders {
                let key = (game.clone(), loader.clone());
                let better = match best.get(&key) {
                    None => true,
                    Some(&j) => {
                        let current = &versions[j];
                        let rank = |v: &SourceVersion| (v.channel == "release", v.published);
                        rank(version) > rank(current)
                    }
                };
                if better {
                    best.insert(key, i);
                }
            }
        }
    }
    let mut chosen: Vec<usize> = best.into_values().collect();
    chosen.sort_unstable();
    chosen.dedup();
    chosen
}
