//! What a player typed into "Add to the catalog": a page of Modrinth or
//! CurseForge, a file's address there, or a name to search for.

use crate::modrinth;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Input {
    /// A Modrinth project by ID or slug, maybe with a version.
    Modrinth {
        project: String,
        version: Option<String>,
    },
    /// A file on Modrinth's CDN: its project and version IDs.
    ModrinthFile { project: String, version: String },
    /// A CurseForge page or file (stage 4 of the plan).
    CurseForge,
    /// Words to search for.
    Name(String),
}

pub const MAX_INPUT: usize = 512;

/// Reads the player's text; `None` for an address of another site, a
/// malformed one, or empty text.
pub fn parse(text: &str) -> Option<Input> {
    let text = text.trim();
    if text.is_empty() || text.chars().count() > MAX_INPUT || text.chars().any(char::is_control) {
        return None;
    }
    let looks_like_url = text.contains("://") || text.starts_with("www.") || text.contains(".com/");
    if !looks_like_url {
        return Some(Input::Name(text.to_owned()));
    }
    let with_scheme = if text.contains("://") {
        text.to_owned()
    } else {
        format!("https://{text}")
    };
    let url = reqwest::Url::parse(&with_scheme).ok()?;
    if !matches!(url.scheme(), "https" | "http")
        || url.port().is_some()
        || !url.username().is_empty()
    {
        return None;
    }
    let host = url.host_str()?.to_ascii_lowercase();
    let parts: Vec<&str> = url.path_segments()?.filter(|s| !s.is_empty()).collect();
    match host.as_str() {
        "modrinth.com" | "www.modrinth.com" => {
            // /<type>/<slug>[/version/<id>]
            let [kind, project, rest @ ..] = parts.as_slice() else {
                return None;
            };
            if !matches!(
                *kind,
                "mod" | "modpack" | "resourcepack" | "shader" | "datapack" | "plugin" | "project"
            ) || !modrinth::is_ref(project)
            {
                return None;
            }
            let version = match rest {
                ["version", v, ..] if modrinth::is_ref(v) => Some((*v).to_owned()),
                _ => None,
            };
            Some(Input::Modrinth {
                project: (*project).to_owned(),
                version,
            })
        }
        "cdn.modrinth.com" => match parts.as_slice() {
            ["data", project, "versions", version, ..]
                if modrinth::is_ref(project) && modrinth::is_ref(version) =>
            {
                Some(Input::ModrinthFile {
                    project: (*project).to_owned(),
                    version: (*version).to_owned(),
                })
            }
            _ => None,
        },
        "curseforge.com"
        | "www.curseforge.com"
        | "edge.forgecdn.net"
        | "mediafilez.forgecdn.net" => Some(Input::CurseForge),
        _ => None,
    }
}
