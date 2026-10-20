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
/// The search as the site's own page asks it.
const SEARCH_FULL: &str = "https://www.curseforge.com/api/v1/mods/search?gameId=432&index=0&classId=6&filterText=jei&pageSize=20&sortField=1";

/// What one check sees in the answer besides its status.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Look {
    Nothing,
    /// The first file's id in a list of files (for the checks after it).
    FirstFile,
    /// A page: which kinds of readable data it carries (a redirect within
    /// the site is followed once).
    PageData,
    /// A redirect: whether it goes to forgecdn.
    ToCdn,
    /// A search answer of the API: how many hits, and whether JEI is one.
    Hits,
    /// A page's description and pictures: where they sit and how many.
    Media,
}

struct Check {
    name: &'static str,
    url: String,
    look: Look,
    headers: &'static [&'static str],
}

fn checks(file: Option<u64>) -> Vec<Check> {
    let api = format!("https://www.curseforge.com/api/v1/mods/{PROJECT}");
    let mut list = vec![
        Check {
            name: "modrinth_api",
            url: "https://api.modrinth.com/v2/project/sodium".into(),
            look: Look::Nothing,
            headers: &[],
        },
        Check {
            name: "cf_files",
            url: format!("{api}/files?pageIndex=0&pageSize=1"),
            look: Look::FirstFile,
            headers: &[],
        },
    ];
    if let Some(file) = file {
        list.extend([
            Check {
                name: "cf_file",
                url: format!("{api}/files/{file}"),
                look: Look::Nothing,
                headers: &[],
            },
            Check {
                name: "cf_changelog",
                url: format!("{api}/files/{file}/change-log"),
                look: Look::Nothing,
                headers: &[],
            },
            Check {
                name: "cf_download",
                url: format!("{api}/files/{file}/download"),
                look: Look::ToCdn,
                headers: &[],
            },
        ]);
    }
    list.extend([
        Check {
            name: "cf_dependencies",
            url: format!("{api}/dependencies"),
            look: Look::Nothing,
            headers: &[],
        },
        Check {
            name: "cf_project_api",
            url: api.clone(),
            look: Look::Nothing,
            headers: &[],
        },
        Check {
            name: "cf_search_api",
            url: "https://www.curseforge.com/api/v1/mods/search?gameId=432&index=0&pageSize=1&filterText=jei".into(),
            look: Look::Nothing,
            headers: &[],
        },
        Check {
            name: "cf_search_api_site",
            url: "https://www.curseforge.com/api/v1/mods/search?gameId=432&index=0&pageSize=1&filterText=jei".into(),
            look: Look::Hits,
            headers: crate::curseforge::SITE_FETCH,
        },
        Check {
            name: "cf_search_api_full",
            url: SEARCH_FULL.into(),
            look: Look::Hits,
            headers: &[],
        },
        Check {
            name: "cf_search_api_both",
            url: SEARCH_FULL.into(),
            look: Look::Hits,
            headers: crate::curseforge::SITE_FETCH,
        },
        Check {
            name: "cf_project_page",
            url: format!("https://www.curseforge.com/minecraft/mc-mods/{SLUG}"),
            look: Look::PageData,
            headers: &[],
        },
        Check {
            name: "cf_gallery_page",
            url: format!("https://www.curseforge.com/minecraft/mc-mods/{SLUG}/gallery"),
            look: Look::Media,
            headers: &[],
        },
        Check {
            name: "cf_gallery_create",
            url: "https://www.curseforge.com/minecraft/mc-mods/create/gallery".into(),
            look: Look::Media,
            headers: &[],
        },
        Check {
            name: "cf_images_api",
            url: format!("{api}/images"),
            look: Look::Media,
            headers: &[],
        },
        Check {
            name: "cf_description_api",
            url: format!("{api}/description"),
            look: Look::Media,
            headers: &[],
        },
        Check {
            name: "imgur_picture",
            // A picture JEI's description shows.
            url: "https://i.imgur.com/YTuxxOL.png".into(),
            look: Look::Nothing,
            headers: &[],
        },
        Check {
            name: "imgur_as_picture",
            // The same, asked as a browser asks for an <img>.
            url: "https://i.imgur.com/YTuxxOL.png".into(),
            look: Look::Nothing,
            headers: crate::fetch::PICTURE,
        },
        Check {
            name: "cf_search_page",
            url: crate::curseforge::search_url(SLUG),
            look: Look::PageData,
            headers: &[],
        },
    ]);
    list
}

/// A search answer `{"data":[{"id":…}]}`: how many hits, and whether JEI
/// is one. Our words and numbers.
fn hits(body: &[u8]) -> String {
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(body) else {
        return "not_json".to_owned();
    };
    let list = value
        .get("data")
        .and_then(serde_json::Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    format!(
        "hits:{},{}",
        list.len(),
        if list
            .iter()
            .any(|h| h.get("id").and_then(serde_json::Value::as_u64) == Some(PROJECT))
        {
            "jei_found"
        } else {
            "no_jei"
        }
    )
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

/// The kinds of data a page carries that a worker could read: the Pages
/// Router's `__NEXT_DATA__`, the App Router's `self.__next_f` stream,
/// JSON-LD and Open Graph tags. Our words only.
fn page_data(page: &[u8]) -> String {
    let has = |needle: &[u8]| page.windows(needle.len()).any(|w| w == needle);
    let kinds: Vec<&str> = [
        (&b"__NEXT_DATA__"[..], "next_data"),
        (b"self.__next_f", "next_flight"),
        (b"application/ld+json", "ld_json"),
        (b"og:description", "og_tags"),
    ]
    .into_iter()
    .filter(|(needle, _)| has(needle))
    .map(|(_, name)| name)
    .collect();
    if kinds.is_empty() {
        "no_page_data".to_owned()
    } else {
        kinds.join(",")
    }
}

/// Where a page's description and pictures sit: in the HTML itself or in
/// its scripts (the App Router's stream). Counts of picture addresses on
/// forgecdn, paragraphs, and the word "recipe" (JEI's description is about
/// recipes). Our words and numbers.
fn media_report(page: &[u8]) -> String {
    let page = String::from_utf8_lossy(page);
    let mut html = String::new();
    let mut scripts = String::new();
    let mut rest = page.as_ref();
    while let Some(start) = rest.find("<script") {
        html.push_str(&rest[..start]);
        let tail = &rest[start..];
        let end = tail
            .find("</script>")
            .map_or(tail.len(), |e| e + "</script>".len());
        scripts.push_str(&tail[..end]);
        rest = &tail[end..];
    }
    html.push_str(rest);
    let count = |text: &str| {
        let lower = text.to_ascii_lowercase();
        format!(
            "pictures:{},paragraphs:{},recipe:{}",
            lower.matches("media.forgecdn.net/attachments").count(),
            ["<p>", "<p ", "\\u003cp\\u003e", "\\u003cp "]
                .iter()
                .map(|p| lower.matches(p).count())
                .sum::<usize>(),
            lower.matches("recipe").count(),
        )
    };
    format!("html[{}] scripts[{}]", count(&html), count(&scripts))
}

/// What the CurseForge adapter reads from the project's page: our words and
/// numbers (whether the ID is the project's, which fields came, how many).
fn card_report(page: &[u8]) -> String {
    let card = crate::curseforge::page(&String::from_utf8_lossy(page), SLUG);
    let mut words = vec![match card.id {
        Some(PROJECT) => "id_ok".to_owned(),
        Some(_) => "id_other".to_owned(),
        None => "no_id".to_owned(),
    }];
    for (name, there) in [
        ("title", !card.title.is_empty()),
        ("summary", !card.summary.is_empty()),
        ("icon", card.icon.is_some()),
        ("downloads", card.downloads > 0),
        ("published", card.published > 0),
        ("updated", card.updated > 0),
    ] {
        if there {
            words.push(name.to_owned());
        }
    }
    words.push(format!("body:{}", card.body.len()));
    words.push(format!("authors:{}", card.authors.len()));
    words.push(format!("gallery:{}", card.gallery.len()));
    words.push(format!("categories:{}", card.categories.len()));
    let shapes = crate::curseforge::shapes(&String::from_utf8_lossy(page), SLUG);
    if !shapes.is_empty() {
        words.push(format!("fields[{}]", shapes.join(" ")));
    }
    words.join(",")
}

/// What the adapter finds on the search page: how many hits, whether JEI
/// (the project searched for) is among them, and how many have an author,
/// a summary and downloads. Our words and numbers.
fn search_report(page: &[u8]) -> String {
    let hits = crate::curseforge::candidates(&String::from_utf8_lossy(page));
    let count = |f: fn(&crate::modrinth::Candidate) -> bool| hits.iter().filter(|c| f(c)).count();
    format!(
        "hits:{},{},authors:{},summaries:{},downloads:{}",
        hits.len(),
        if hits.iter().any(|c| c.id == PROJECT.to_string()) {
            "jei_found"
        } else {
            "no_jei"
        },
        count(|c| !c.author.is_empty()),
        count(|c| !c.summary.is_empty()),
        count(|c| c.downloads > 0),
    )
}

/// A proxy's address with another scheme (`http`, `socks5h`), to see
/// whether the proxy speaks another protocol than its address says.
pub fn with_scheme(proxy: &str, scheme: &str) -> Option<String> {
    let (_, rest) = proxy.split_once("://")?;
    Some(format!("{scheme}://{rest}"))
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
        let mut refused = false;
        loop {
            let list = checks(file);
            let Some(check) = list.get(index) else { break };
            index += 1;
            let _ = std::fs::remove_file(&body);
            let result = client
                .once(exit, None, &check.url, check.headers, &body, MAX_BYTES)
                .await;
            if index == 1 && matches!(result, Err(FetchError::Network(_))) {
                refused = true;
            }
            let read = || std::fs::read(&body).unwrap_or_default();
            let ok = matches!(&result, Ok(r) if r.status == 200);
            let seen = match check.look {
                Look::Nothing => None,
                Look::FirstFile => {
                    file = if ok { first_file(&read()) } else { None };
                    Some(if file.is_some() {
                        "file_found".to_owned()
                    } else {
                        "no_file".to_owned()
                    })
                }
                Look::PageData => match &result {
                    Ok(r) if r.status == 200 => {
                        let page = read();
                        Some(match check.name {
                            "cf_project_page" => format!(
                                "{} {} {} {}",
                                page_data(&page),
                                card_report(&page),
                                media_report(&page),
                                crate::curseforge::media(&String::from_utf8_lossy(&page))
                            ),
                            "cf_search_page" => {
                                format!("{} {}", page_data(&page), search_report(&page))
                            }
                            _ => page_data(&page),
                        })
                    }
                    // One hop within the site, as a browser would.
                    Ok(r) if (300..400).contains(&r.status) => {
                        match r.location.as_deref().filter(|l| same_site(l)) {
                            Some(next) => {
                                let next = next.to_owned();
                                let _ = std::fs::remove_file(&body);
                                Some(
                                    match client
                                        .once(exit, None, &next, &[], &body, MAX_BYTES)
                                        .await
                                    {
                                        Ok(r) if r.status == 200 => {
                                            let page = read();
                                            let mut seen = format!(
                                                "then 200 {}B {}",
                                                r.size,
                                                page_data(&page)
                                            );
                                            if check.name == "cf_search_page" {
                                                seen.push(' ');
                                                seen.push_str(&search_report(&page));
                                            }
                                            seen
                                        }
                                        Ok(r) => format!(
                                            "then {}{}",
                                            r.status,
                                            if r.challenge { " challenge" } else { "" }
                                        ),
                                        Err(e) => format!("then {}", e.code()),
                                    },
                                )
                            }
                            None => Some("elsewhere".to_owned()),
                        }
                    }
                    _ => None,
                },
                Look::Hits => ok.then(|| hits(&read())),
                Look::Media => ok.then(|| {
                    let page = read();
                    format!(
                        "{} {}",
                        media_report(&page),
                        crate::curseforge::media(&String::from_utf8_lossy(&page))
                    )
                }),
                Look::ToCdn => match &result {
                    Ok(r) if (300..400).contains(&r.status) => Some(
                        if r.location.as_deref().is_some_and(to_cdn) {
                            "to_forgecdn"
                        } else {
                            "elsewhere"
                        }
                        .to_owned(),
                    ),
                    _ => None,
                },
            };
            lines.push(line(&name, check.name, &result, seen.as_deref()));
        }
        // A proxy that refused the first request: does it speak another
        // protocol than its address says?
        if refused && client.is_proxy(exit) {
            for scheme in ["http", "socks5h"] {
                let _ = std::fs::remove_file(&body);
                let result = client
                    .once(
                        exit,
                        Some(scheme),
                        &checks(None)[1].url,
                        &[],
                        &body,
                        MAX_BYTES,
                    )
                    .await;
                lines.push(line(&format!("{name}/{scheme}"), "cf_files", &result, None));
            }
        }
    }
    let _ = std::fs::remove_file(&body);
    lines
}

/// An address on www.curseforge.com.
fn same_site(url: &str) -> bool {
    crate::fetch::site(url).is_some()
        && reqwest::Url::parse(url).is_ok_and(|u| u.host_str() == Some("www.curseforge.com"))
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
    fn page_data_names_only_what_it_found() {
        assert_eq!(page_data(b"<html>nothing</html>"), "no_page_data");
        assert_eq!(
            page_data(
                b"<script>self.__next_f.push([1,\"x\"])</script><meta property=\"og:description\">"
            ),
            "next_flight,og_tags"
        );
        assert_eq!(page_data(b"<script id=\"__NEXT_DATA__\">"), "next_data");
    }

    #[test]
    fn media_is_counted_apart_in_html_and_scripts() {
        let page = br#"<div><p>Shows a recipe</p><img src="https://media.forgecdn.net/attachments/1/2/a.png"><path d=""/></div><script>self.__next_f.push([1,"\u003cp\u003eRecipe and recipe\u003c/p\u003e"])</script><p class="x">end"#;
        assert_eq!(
            media_report(page),
            "html[pictures:1,paragraphs:2,recipe:1] scripts[pictures:0,paragraphs:1,recipe:2]"
        );
        // An unclosed script ends the page.
        assert_eq!(
            media_report(b"<p>a<script>recipe"),
            "html[pictures:0,paragraphs:1,recipe:0] scripts[pictures:0,paragraphs:0,recipe:1]"
        );
    }

    #[test]
    fn a_scheme_is_swapped_and_nothing_else() {
        assert_eq!(
            with_scheme("socks5://u:p@203.0.113.1:8000", "http").as_deref(),
            Some("http://u:p@203.0.113.1:8000")
        );
        assert_eq!(with_scheme("no scheme", "http"), None);
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
