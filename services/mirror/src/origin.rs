//! Which origins the mirror stands for, how a request names one, and whether
//! its content can change (D-062).

use std::path::PathBuf;

/// Hosts a request may name: the launcher's download sources
/// (`src-tauri/src/net.rs`). Keep both lists the same.
pub const HOSTS: &[&str] = &[
    "piston-meta.mojang.com",
    "piston-data.mojang.com",
    "launcher.mojang.com",
    "launchermeta.mojang.com",
    "libraries.minecraft.net",
    "resources.download.minecraft.net",
    "api.adoptium.net",
    "github.com",
    "meta.fabricmc.net",
    "maven.fabricmc.net",
    "meta.quiltmc.org",
    "maven.quiltmc.org",
    "maven.minecraftforge.net",
    "files.minecraftforge.net",
    "maven.neoforged.net",
];

/// Hosts only reached by a redirect: GitHub's release downloads.
pub const REDIRECT_HOSTS: &[&str] = &[
    "objects.githubusercontent.com",
    "release-assets.githubusercontent.com",
];

const MAX_PATH: usize = 1024;
const MAX_QUERY: usize = 1024;

/// Whether content at an address can change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Class {
    /// Never changes: kept forever at this path under the files folder.
    File(PathBuf),
    /// Changes (lists, APIs): fetched again after a while, kept by its URL's hash.
    Meta,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Origin {
    pub host: &'static str,
    /// The origin URL, `https://<host><path>[?<query>]`.
    pub url: String,
    pub class: Class,
}

/// Characters a path or query may hold as sent (RFC 3986 without the ones
/// nobody needs here).
fn plain(text: &str) -> bool {
    text.bytes().all(|b| {
        b.is_ascii_alphanumeric()
            || matches!(
                b,
                b'/' | b'.'
                    | b'-'
                    | b'_'
                    | b'~'
                    | b'%'
                    | b'+'
                    | b'='
                    | b'&'
                    | b','
                    | b'@'
                    | b':'
                    | b';'
                    | b'!'
                    | b'$'
                    | b'\''
                    | b'('
                    | b')'
                    | b'*'
            )
    })
}

/// `/<host>/<path>` and its query as an origin; `None` for anything else.
pub fn parse(path: &str, query: Option<&str>) -> Option<Origin> {
    let rest = path.strip_prefix('/')?;
    let (host, tail) = rest.split_once('/').unwrap_or((rest, ""));
    let host = *HOSTS.iter().find(|h| **h == host)?;
    let tail = format!("/{tail}");
    // GitHub hosts anyone's files: only Adoptium's Java releases are ours.
    if host == "github.com" && (query.is_some_and(|q| !q.is_empty()) || !adoptium_release(&tail)) {
        return None;
    }
    let lower = tail.to_ascii_lowercase();
    let unsafe_path = tail.len() > MAX_PATH
        || !plain(&tail)
        || tail.contains("//")
        || tail.split('/').any(|s| s == "." || s == "..")
        || ["%2f", "%5c", "%00", "%2e"]
            .iter()
            .any(|e| lower.contains(e));
    if unsafe_path {
        return None;
    }
    let query = query.filter(|q| !q.is_empty());
    if query.is_some_and(|q| q.len() > MAX_QUERY || !plain(q) || q.contains('#')) {
        return None;
    }
    let url = match query {
        Some(q) => format!("https://{host}{tail}?{q}"),
        None => format!("https://{host}{tail}"),
    };
    let class = match (query, kept_path(host, &tail)) {
        (None, Some(file)) if never_changes(host, &tail) => Class::File(file),
        _ => Class::Meta,
    };
    Some(Origin { host, url, class })
}

/// The file's place under the files folder, when every part is a plain name
/// (nginx then finds it at the same path).
fn kept_path(host: &str, tail: &str) -> Option<PathBuf> {
    let decoded = percent_encoding::percent_decode_str(tail)
        .decode_utf8()
        .ok()?;
    if decoded.ends_with('/') {
        return None;
    }
    let mut path = PathBuf::from(host);
    for part in decoded.split('/').skip(1) {
        let ok = !part.is_empty()
            && part != "."
            && part != ".."
            && part.bytes().all(|b| {
                b.is_ascii_alphanumeric()
                    || matches!(b, b'.' | b'-' | b'_' | b'+' | b'~' | b'@' | b'=' | b',')
            });
        if !ok {
            return None;
        }
        path.push(part);
    }
    Some(path)
}

/// `/adoptium/<repo>/releases/download/<tag>/<file>`: a Java build the
/// Adoptium API points to.
fn adoptium_release(tail: &str) -> bool {
    let parts: Vec<&str> = tail.split('/').collect();
    parts.len() >= 7 && parts[1] == "adoptium" && parts[3] == "releases" && parts[4] == "download"
}

/// Content addressed by hash, a fixed version or a release tag.
fn never_changes(host: &str, tail: &str) -> bool {
    let name = tail.rsplit('/').next().unwrap_or_default();
    match host {
        "piston-meta.mojang.com" | "launchermeta.mojang.com" => {
            tail.starts_with("/v1/packages/") || tail.starts_with("/v1/objects/")
        }
        "piston-data.mojang.com" | "launcher.mojang.com" => tail.starts_with("/v1/objects/"),
        "resources.download.minecraft.net" => true,
        "github.com" => adoptium_release(tail),
        "libraries.minecraft.net"
        | "maven.fabricmc.net"
        | "maven.quiltmc.org"
        | "maven.minecraftforge.net"
        | "maven.neoforged.net" => {
            !name.starts_with("maven-metadata") && !tail.contains("-SNAPSHOT") && name.contains('.')
        }
        // APIs and lists.
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn class(path: &str, query: Option<&str>) -> Option<Class> {
        parse(path, query).map(|o| o.class)
    }

    #[test]
    fn files_and_metadata_are_told_apart() {
        let file = |p: &str| Class::File(PathBuf::from(&p[1..]));
        for path in [
            "/piston-data.mojang.com/v1/objects/0123abcd/client.jar",
            "/piston-meta.mojang.com/v1/packages/0123abcd/1.21.1.json",
            "/resources.download.minecraft.net/ab/abcdef0123",
            "/libraries.minecraft.net/org/lwjgl/lwjgl/3.3.3/lwjgl-3.3.3.jar",
            "/maven.neoforged.net/releases/net/neoforged/neoforge/21.1.1/neoforge-21.1.1-installer.jar",
            "/github.com/adoptium/temurin21-binaries/releases/download/jdk-21.0.5+11/OpenJDK21U-jre_x64_linux_hotspot_21.0.5_11.tar.gz",
        ] {
            assert_eq!(class(path, None), Some(file(path)), "{path}");
        }
        for path in [
            "/piston-meta.mojang.com/mc/game/version_manifest_v2.json",
            "/maven.fabricmc.net/net/fabricmc/fabric-loader/maven-metadata.xml",
            "/maven.minecraftforge.net/net/minecraftforge/forge/1.0-SNAPSHOT/forge.jar",
            "/meta.fabricmc.net/v2/versions/loader/1.21.1",
            "/files.minecraftforge.net/net/minecraftforge/forge/promotions_slim.json",
            "/api.adoptium.net/v3/assets/latest/21/hotspot",
        ] {
            assert_eq!(class(path, None), Some(Class::Meta), "{path}");
        }
        assert_eq!(
            class("/libraries.minecraft.net/a/b.jar", Some("x=1")),
            Some(Class::Meta),
            "a query is never a file"
        );
        // A tag with an encoded `+` keeps the plain name on disk.
        assert_eq!(
            class(
                "/github.com/adoptium/t/releases/download/jdk-21%2B11/a.tar.gz",
                None
            ),
            Some(Class::File(PathBuf::from(
                "github.com/adoptium/t/releases/download/jdk-21+11/a.tar.gz"
            )))
        );
        assert_eq!(
            parse(
                "/api.adoptium.net/v3/assets/latest/21/hotspot",
                Some("os=linux&image_type=jre")
            )
            .unwrap()
            .url,
            "https://api.adoptium.net/v3/assets/latest/21/hotspot?os=linux&image_type=jre"
        );
    }

    #[test]
    fn only_listed_hosts_and_plain_paths_are_accepted() {
        for path in [
            "/evil.example/x",
            "/127.0.0.1/x",
            "/objects.githubusercontent.com/x",
            "/github.com.evil.example/x",
            // GitHub: only Adoptium's release downloads.
            "/github.com/someone/repo/releases/download/v1/page.html",
            "/github.com/adoptium/temurin21-binaries",
            "/github.com/login",
            "/libraries.minecraft.net/../etc/passwd",
            "/libraries.minecraft.net/a/%2e%2e/b",
            "/libraries.minecraft.net/a%2fb",
            "/libraries.minecraft.net//b",
            "/libraries.minecraft.net/a\\b",
            "/libraries.minecraft.net/a b",
            "/libraries.minecraft.net/a%00",
            "/",
            "",
        ] {
            assert!(parse(path, None).is_none(), "{path}");
        }
        assert!(parse("/libraries.minecraft.net/a", Some("x=<script>")).is_none());
        assert!(
            parse(
                &format!("/libraries.minecraft.net/{}", "a".repeat(2000)),
                None
            )
            .is_none()
        );
    }
}
