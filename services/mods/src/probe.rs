//! `lapi-mods probe`: how CurseForge answers each exit from the server (the
//! first step of D-066 stage 4). Every exit, direct and each proxy, asks the
//! site's API, its file links and its pages for one public project once.
//!
//! What it prints goes into a public workflow log: our own words and numbers
//! only. Nothing from an answer and nothing about an exit (proxies are
//! numbered) is printed.

use std::path::Path;

use crate::fetch::{Client, FetchError, Reply};

/// Just Enough Items: a large, long-lived public project.
const PROJECT: u64 = 238_222;
const SLUG: &str = "jei";
const MAX_BYTES: u64 = 4 * 1024 * 1024;

/// What one check sees in the answer besides its status.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Look {
    Nothing,
    /// The first file's id in a list of files (for the checks after it).
    FirstFile,
    /// A Next.js page: whether it carries its data (`__NEXT_DATA__`).
    PageData,
    /// A redirect: whether it goes to forgecdn.
    ToCdn,
}

struct Check {
    name: &'static str,
    url: String,
    look: Look,
}

fn checks(file: Option<u64>) -> Vec<Check> {
    let api = format!("https://www.curseforge.com/api/v1/mods/{PROJECT}");
    let mut list = vec![
        Check {
            name: "modrinth_api",
            url: "https://api.modrinth.com/v2/project/sodium".into(),
            look: Look::Nothing,
        },
        Check {
            name: "cf_files",
            url: format!("{api}/files?pageIndex=0&pageSize=1"),
            look: Look::FirstFile,
        },
    ];
    if let Some(file) = file {
        list.extend([
            Check {
                name: "cf_file",
                url: format!("{api}/files/{file}"),
                look: Look::Nothing,
            },
            Check {
                name: "cf_changelog",
                url: format!("{api}/files/{file}/change-log"),
                look: Look::Nothing,
            },
            Check {
                name: "cf_download",
                url: format!("{api}/files/{file}/download"),
                look: Look::ToCdn,
            },
        ]);
    }
    list.extend([
        Check {
            name: "cf_dependencies",
            url: format!("{api}/dependencies"),
            look: Look::Nothing,
        },
        Check {
            name: "cf_project_api",
            url: api.clone(),
            look: Look::Nothing,
        },
        Check {
            name: "cf_search_api",
            url: "https://www.curseforge.com/api/v1/mods/search?gameId=432&index=0&pageSize=1&filterText=jei".into(),
            look: Look::Nothing,
        },
        Check {
            name: "cf_project_page",
            url: format!("https://www.curseforge.com/minecraft/mc-mods/{SLUG}"),
            look: Look::PageData,
        },
        Check {
            name: "cf_search_page",
            url: "https://www.curseforge.com/minecraft/search?search=jei".into(),
            look: Look::PageData,
        },
    ]);
    list
}

/// The id of the first file in `{"data":[{"id":…}]}`.
fn first_file(body: &[u8]) -> Option<u64> {
    let value: serde_json::Value = serde_json::from_slice(body).ok()?;
    value
        .get("data")?
        .as_array()?
        .first()?
        .get("id")?
        .as_u64()
        .filter(|id| *id > 0)
}

/// One line: our words and numbers.
fn line(exit: &str, check: &str, result: &Result<Reply, FetchError>, seen: Option<&str>) -> String {
    let mut out = format!("{exit:<8} {check:<16} ");
    match result {
        Ok(reply) => {
            out.push_str(&reply.status.to_string());
            if reply.challenge {
                out.push_str(" challenge");
            }
            out.push_str(&format!(" {}B", reply.size));
        }
        Err(e) => out.push_str(&e.code()),
    }
    if let Some(seen) = seen {
        out.push(' ');
        out.push_str(seen);
    }
    out
}

/// Asks every check through every exit; returns the lines to print.
pub async fn run(client: &Client, tmp: &Path) -> Vec<String> {
    let mut lines = vec![format!("exits {}", client.exits())];
    let body = tmp.join(format!("probe-{}", uuid::Uuid::new_v4()));
    let mut proxies = 0;
    for exit in 0..client.exits() {
        let name = if client.is_proxy(exit) {
            proxies += 1;
            format!("proxy{proxies}")
        } else {
            "direct".to_owned()
        };
        let mut file = None;
        let mut index = 0;
        loop {
            let list = checks(file);
            let Some(check) = list.get(index) else { break };
            index += 1;
            let _ = std::fs::remove_file(&body);
            let result = client.once(exit, &check.url, &body, MAX_BYTES).await;
            let read = || std::fs::read(&body).unwrap_or_default();
            let ok = matches!(&result, Ok(r) if r.status == 200);
            let seen = match check.look {
                Look::Nothing => None,
                Look::FirstFile => {
                    file = if ok { first_file(&read()) } else { None };
                    Some(if file.is_some() {
                        "file_found"
                    } else {
                        "no_file"
                    })
                }
                Look::PageData => ok.then(|| {
                    let page = read();
                    if page.windows(13).any(|w| w == b"__NEXT_DATA__") {
                        "page_data"
                    } else {
                        "no_page_data"
                    }
                }),
                Look::ToCdn => match &result {
                    Ok(r) if (300..400).contains(&r.status) => {
                        Some(if r.location.as_deref().is_some_and(to_cdn) {
                            "to_forgecdn"
                        } else {
                            "elsewhere"
                        })
                    }
                    _ => None,
                },
            };
            lines.push(line(&name, check.name, &result, seen));
        }
    }
    let _ = std::fs::remove_file(&body);
    lines
}

/// A redirect to one of forgecdn's file hosts.
fn to_cdn(url: &str) -> bool {
    reqwest::Url::parse(url).is_ok_and(|u| {
        u.scheme() == "https"
            && matches!(
                u.host_str(),
                Some("edge.forgecdn.net" | "mediafilez.forgecdn.net")
            )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_file_is_read_only_from_its_place() {
        assert_eq!(first_file(br#"{"data":[{"id":7},{"id":8}]}"#), Some(7));
        assert_eq!(first_file(br#"{"data":[]}"#), None);
        assert_eq!(first_file(br#"{"data":[{"id":"7"}]}"#), None);
        assert_eq!(first_file(br#"{"data":[{"id":0}]}"#), None);
        assert_eq!(first_file(b"<html>Just a moment...</html>"), None);
    }

    #[test]
    fn a_line_holds_only_our_words_and_numbers() {
        let reply = Reply {
            status: 403,
            size: 512,
            location: Some("https://evil.example/secret".into()),
            challenge: true,
        };
        let text = line("proxy2", "cf_project_page", &Ok(reply), None);
        assert_eq!(text, "proxy2   cf_project_page  403 challenge 512B");
        let text = line("direct", "cf_files", &Err(FetchError::Network(28)), None);
        assert_eq!(text, "direct   cf_files         network_28");
    }

    #[test]
    fn only_forgecdn_counts_as_the_cdn() {
        assert!(to_cdn("https://edge.forgecdn.net/files/1/2/a.jar"));
        assert!(to_cdn("https://mediafilez.forgecdn.net/files/1/2/a.jar"));
        assert!(!to_cdn("http://edge.forgecdn.net/files/a.jar"));
        assert!(!to_cdn("https://edge.forgecdn.net.evil.example/a.jar"));
        assert!(!to_cdn("not a url"));
    }

    #[test]
    fn every_check_is_a_url_the_service_may_fetch() {
        for check in checks(Some(4_000_000)) {
            assert!(crate::fetch::site(&check.url).is_some(), "{}", check.url);
        }
        assert_eq!(checks(None).len() + 3, checks(Some(1)).len());
    }
}
