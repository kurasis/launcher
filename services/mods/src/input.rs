//! What a player typed into "Add to the catalog": a page of Modrinth or
//! CurseForge, a file's address there, a packwiz pack's `pack.toml`, or a
//! name to search for.

use crate::{curseforge, modrinth, packwiz};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Input {
    /// A Modrinth project by ID or slug, maybe with a version.
    Modrinth {
        project: String,
        version: Option<String>,
    },
    /// A file on Modrinth's CDN: its project and version IDs.
    ModrinthFile { project: String, version: String },
    /// A project's page on CurseForge (`/minecraft/<class>/<slug>`), maybe
    /// at one of its files.
    CurseForge {
        class: String,
        slug: String,
        file: Option<u64>,
    },
    /// A file's address on forgecdn: it does not name its project.
    CurseForgeFile,
    /// A packwiz pack: the address its `pack.toml` is fetched from.
    Packwiz(String),
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
    if url.path().ends_with("/pack.toml") {
        let mut secure = url.clone();
        secure.set_scheme("https").ok()?;
        return packwiz::pack_url(&secure).map(Input::Packwiz);
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
        "curseforge.com" | "www.curseforge.com" => {
            // /minecraft/<class>/<slug>[/files/<id> | /download/<id>]
            let ["minecraft", class, slug, rest @ ..] = parts.as_slice() else {
                return None;
            };
            let class = class.to_ascii_lowercase();
            let slug = slug.to_ascii_lowercase();
            if curseforge::kind_of_class(&class).is_none() || !curseforge::is_slug(&slug) {
                return None;
            }
            let file = match rest {
                ["files" | "download", id, ..] => Some(curseforge::id(id)?),
                _ => None,
            };
            Some(Input::CurseForge { class, slug, file })
        }
        "edge.forgecdn.net" | "mediafilez.forgecdn.net" => Some(Input::CurseForgeFile),
        _ => None,
    }
}
