//! Tests of the mods catalog client and content installer (D-066).

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use serde_json::{Value, json};

use super::content::{self, ContentLock, LockedItem, Misfit, Target, Wanted};
use super::markdown::{self, Node};
use super::*;
use crate::loader::LoaderKind;
use crate::storage::paths::{DataRoot, InstanceId};

const PICTURE: &str = "https://files-mods.lapi.sh/i/0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef.png";

fn id(n: u8) -> String {
    format!("00000000-0000-4000-8000-{n:012}")
}

fn hash(n: u8) -> String {
    format!("{n:02x}").repeat(32)
}

// ----- descriptions -----

fn kinds(nodes: &[Node], out: &mut Vec<&'static str>) {
    for node in nodes {
        out.push(node.k);
        kinds(&node.c, out);
    }
}

fn all_kinds(nodes: &[Node]) -> Vec<&'static str> {
    let mut out = Vec::new();
    kinds(nodes, &mut out);
    out
}

fn all_text(nodes: &[Node]) -> String {
    let mut out = String::new();
    for node in nodes {
        if let Some(t) = &node.text {
            out.push_str(t);
        }
        out.push_str(&all_text(&node.c));
    }
    out
}

fn hrefs(nodes: &[Node], out: &mut Vec<String>) {
    for node in nodes {
        if let Some(h) = &node.href {
            out.push(h.clone());
        }
        hrefs(&node.c, out);
    }
}

fn all_hrefs(nodes: &[Node]) -> Vec<String> {
    let mut out = Vec::new();
    hrefs(nodes, &mut out);
    out
}

fn md(text: &str) -> Vec<Node> {
    markdown::parse(text, &our_picture)
}

#[test]
fn markdown_keeps_structure_as_nodes() {
    let nodes = md("# Title\n\nSome *em* and **strong**, `code`.\n\n- one\n- two\n\n> quote");
    let k = all_kinds(&nodes);
    for want in [
        "h1", "p", "em", "strong", "code", "ul", "li", "quote", "text",
    ] {
        assert!(k.contains(&want), "{want} in {k:?}");
    }
    assert!(all_text(&nodes).contains("Some em and strong"));
}

#[test]
fn markdown_tables_have_heading_cells() {
    let nodes = md("| a | b |\n|---|---|\n| 1 | 2 |");
    let k = all_kinds(&nodes);
    assert!(k.contains(&"table") && k.contains(&"th") && k.contains(&"td"));
}

#[test]
fn markdown_never_carries_html_or_scripts() {
    let nodes = md("<script>alert(1)</script><style>body{}</style>\
         <div onclick=\"x()\">visible <b>bold</b></div>\n\n\
         <iframe src=\"https://evil.example\"></iframe>");
    let text = all_text(&nodes);
    assert!(!text.contains("alert"), "{text}");
    assert!(!text.contains("body{}"), "{text}");
    assert!(!text.contains('<'), "{text}");
    assert!(text.contains("visible"));
    assert!(all_hrefs(&nodes).is_empty());
    for k in all_kinds(&nodes) {
        assert!(!matches!(k, "script" | "style" | "div" | "iframe"));
    }
}

fn html(text: &str) -> Vec<Node> {
    markdown::parse_html(text, &our_picture)
}

#[test]
fn html_descriptions_keep_their_structure() {
    let nodes = html(&format!(
        "<h2><span style=\"font-size:14px\">Title</span></h2>\r\n\
         <p onclick=\"x()\" title=\"hidden words\">Some <strong>bold</strong>, <i>slanted</i>, \
         <a href=\"https://x.example/a?b=1&amp;c=2\">a link</a> and \
         <a href=\"javascript:alert(1)\">no link</a>.</p>\n\
         <ul>\n  <li>one\n  <li>two</li>\n</ul>\n\
         <p><img src=\"{PICTURE}\" alt=\"shot\" onerror=\"x()\"></p>\
         <table><tbody><tr><th>a<td>b</tr></tbody></table>\
         <pre>  keep\n  spaces</pre><p>&rsquo;&#8217;&#x2019;&#8238;&amp;lt; &unknown;</p>\
         <!-- a note --><div><p>in a block</p></div><code>x &lt; y</code>"
    ));
    let k = all_kinds(&nodes);
    for want in [
        "h2", "span", "p", "strong", "em", "a", "ul", "li", "img", "table", "tr", "th", "td",
        "pre", "block", "code",
    ] {
        assert!(k.contains(&want), "{want} in {k:?}");
    }
    // Items and paragraphs are siblings, not nested in each other.
    let list = nodes.iter().find(|n| n.k == "ul").unwrap();
    assert_eq!(list.c.iter().filter(|n| n.k == "li").count(), 2);
    assert!(nodes.iter().filter(|n| n.k == "p").count() >= 3);
    assert_eq!(all_hrefs(&nodes), ["https://x.example/a?b=1&c=2", PICTURE]);
    let text = all_text(&nodes);
    assert!(
        text.contains("Some bold, slanted, a link and no link."),
        "{text}"
    );
    assert!(text.contains("  keep\n  spaces"), "{text}");
    assert!(
        text.contains("\u{2019}\u{2019}\u{2019}&lt; &unknown;"),
        "{text}"
    );
    assert!(!text.contains('\u{202e}'));
    assert!(text.contains("x < y"), "{text}");
    for hidden in ["hidden words", "a note", "x()", "font-size"] {
        assert!(!text.contains(hidden), "{hidden} in {text}");
    }
    // The picture keeps its description.
    fn find<'a>(nodes: &'a [Node], k: &str) -> Option<&'a Node> {
        nodes
            .iter()
            .find_map(|n| if n.k == k { Some(n) } else { find(&n.c, k) })
    }
    assert_eq!(find(&nodes, "img").unwrap().text.as_deref(), Some("shot"));
}

#[test]
fn broken_html_is_read_safely() {
    for junk in [
        "</div></p></li>",
        "<b><i>x</b>y</i>z",
        "<p>a<p>b<div>c</p>d",
        "<a href=\"https://a.example\"><a>nested</a>",
        "<script>alert(1)",
        "<p/><br/><hr><img><img src=>",
        "<<p>>&#99999999;&#xFFFFFFF;&#0;",
    ] {
        let nodes = html(junk);
        let text = all_text(&nodes);
        assert!(!text.contains("alert"), "{junk}: {text}");
        assert!(!text.contains('\u{0}'), "{junk}");
    }
    let deep = format!("{}deep", "<div><ul><li>".repeat(5000));
    let nodes = html(&deep);
    fn depth(nodes: &[Node]) -> usize {
        nodes.iter().map(|n| 1 + depth(&n.c)).max().unwrap_or(0)
    }
    assert!(depth(&nodes) <= 26, "{}", depth(&nodes));
    assert!(all_text(&nodes).contains("deep"));
    assert!(all_kinds(&html(&"<b>a</b>".repeat(100_000))).len() <= 20_001);
}

#[test]
fn html_in_markdown_keeps_its_structure_and_the_markdown_around_it() {
    let nodes = md("<h2 align=\"center\">Hi</h2>\n\nSome *em* and <b>bold\n\nnext <i>one</i>");
    assert!(all_kinds(&nodes).contains(&"h2"));
    // An element left open ends with its paragraph.
    let paragraphs: Vec<&Node> = nodes.iter().filter(|n| n.k == "p").collect();
    assert_eq!(paragraphs.len(), 2, "{nodes:?}");
    assert!(all_text(&paragraphs[1].c).starts_with("next one"));
    assert!(all_kinds(&paragraphs[1].c).contains(&"em"));
}

#[test]
fn markdown_links_are_https_only() {
    let nodes = md(
        "[ok](https://modrinth.com/mod/x) [js](javascript:alert(1)) [plain](http://a.example) \
         [data](data:text/html,x) [creds](https://u:p@a.example) [file](file:///etc/passwd)",
    );
    assert_eq!(all_hrefs(&nodes), ["https://modrinth.com/mod/x"]);
    // A dropped link keeps its words.
    let text = all_text(&nodes);
    for word in ["js", "plain", "data", "creds", "file"] {
        assert!(text.contains(word), "{word} in {text}");
    }
}

#[test]
fn markdown_pictures_only_from_the_catalog() {
    let nodes = md(&format!(
        "![ours]({PICTURE}) ![theirs](https://cdn.modrinth.com/a.png) \
         <img src=\"{PICTURE}\" onerror=\"x()\"> <img src='https://tracker.example/p.gif'>"
    ));
    let imgs: Vec<String> = all_hrefs(&nodes);
    assert_eq!(imgs, [PICTURE, PICTURE]);
    // A foreign picture leaves its description.
    assert!(all_text(&nodes).contains("theirs"));
}

#[test]
fn markdown_is_bounded() {
    // Deep nesting does not overflow and is cut.
    let deep = format!("{}deep", "> ".repeat(5000));
    let nodes = md(&deep);
    fn depth(nodes: &[Node]) -> usize {
        nodes.iter().map(|n| 1 + depth(&n.c)).max().unwrap_or(0)
    }
    assert!(depth(&nodes) <= 26, "{}", depth(&nodes));
    assert!(all_text(&nodes).contains("deep"));
    // Much text is cut.
    let long = "word ".repeat(200_000);
    assert!(all_text(&md(&long)).chars().count() <= 256 * 1024);
    // Many nodes are cut.
    let many = "*a* ".repeat(100_000);
    assert!(all_kinds(&md(&many)).len() <= 20_001);
}

#[test]
fn markdown_drops_control_characters() {
    let text = all_text(&md("a\u{0}b\u{1b}[31mc\u{202e}"));
    assert!(!text.contains('\u{0}') && !text.contains('\u{1b}'));
}

#[test]
fn links_refuse_everything_but_plain_https() {
    assert!(markdown::link("https://example.com/a?b#c").is_some());
    for bad in [
        "http://example.com",
        "javascript:alert(1)",
        "JAVASCRIPT:alert(1)",
        "data:text/html,x",
        "https://user@example.com",
        "file:///c:/x",
        "//example.com",
        "",
    ] {
        assert!(markdown::link(bad).is_none(), "{bad}");
    }
    assert!(markdown::link(&format!("https://a.example/{}", "x".repeat(3000))).is_none());
}

// ----- checks of the catalog's answers -----

#[test]
fn identifiers_and_names_are_checked() {
    assert!(our_id(&id(1)));
    for bad in [
        "",
        "x",
        "00000000-0000-4000-8000-00000000000g",
        "../../etc/passwd",
    ] {
        assert!(!our_id(bad), "{bad}");
    }
    assert!(project_ref("sodium") && project_ref(&id(1)));
    for bad in ["", "a/b", "a?b", "a b", "..%2f", &"x".repeat(65)] {
        assert!(!project_ref(bad), "{bad}");
    }
    assert!(game_version("1.21.1") && game_version("24w14a") && game_version("26.1"));
    assert!(!game_version("1.21&loader=x") && !game_version(""));
    assert!(loader("fabric") && !loader("Fabric") && !loader("x"));
}

#[test]
fn pictures_and_files_only_from_the_catalog_host() {
    assert_eq!(our_picture(PICTURE).as_deref(), Some(PICTURE));
    for bad in [
        PICTURE.replace("files-mods.lapi.sh", "files-mods.lapi.sh.evil.example"),
        PICTURE.replace("https://", "http://"),
        PICTURE.replace(".png", ".svg"),
        PICTURE.replace("0123", "ABCD"),
        format!("{PICTURE}?x=1"),
        "https://files-mods.lapi.sh/i/../f/x.png".to_owned(),
        "https://cdn.modrinth.com/data/x/icon.png".to_owned(),
    ] {
        assert!(our_picture(&bad).is_none(), "{bad}");
    }
    let sha = hash(7);
    let url = format!("https://files-mods.lapi.sh/f/{sha}/sodium.jar");
    assert!(our_file(&url, &sha).is_some());
    assert!(our_file(&url, &hash(8)).is_none());
    assert!(our_file(&format!("{url}?a=b"), &sha).is_none());
    assert!(our_file(&url.replace("files-mods", "mods"), &sha).is_none());
    assert!(our_file(&url.replace("https", "http"), &sha).is_none());
}

#[test]
fn files_from_r2_are_ours_and_fall_back_to_our_server() {
    let sha = hash(7);
    let cdn = format!("https://cdn-mods.lapi.sh/f/{sha}");
    let ours = format!("https://files-mods.lapi.sh/f/{sha}");
    assert!(our_file(&cdn, &sha).is_some());
    assert!(our_file(&ours, &sha).is_some());
    for bad in [
        format!("{cdn}/sodium.jar"),
        format!("{cdn}/"),
        format!("{cdn}?a=b"),
        format!("{cdn}#x"),
        format!("https://cdn-mods.lapi.sh/f/{}", hash(8)),
        format!("https://cdn-mods.lapi.sh/i/{sha}"),
        format!("https://cdn-mods.lapi.sh/f/{sha}/a/b"),
        format!("https://cdn-mods.lapi.sh:8443/f/{sha}"),
        format!("http://cdn-mods.lapi.sh/f/{sha}"),
        format!("https://cdn-mods.lapi.sh.example.com/f/{sha}"),
        format!("https://user@cdn-mods.lapi.sh/f/{sha}"),
        format!("https://cdn.modrinth.com/f/{sha}"),
    ] {
        assert!(our_file(&bad, &sha).is_none(), "{bad}");
    }
    let reserve = reserve_url(&url::Url::parse(&cdn).unwrap()).unwrap();
    assert_eq!(reserve.as_str(), ours);
    assert!(reserve_url(&url::Url::parse(&ours).unwrap()).is_none());
    assert!(reserve_url(&url::Url::parse("https://cdn.modrinth.com/f/x").unwrap()).is_none());
}

#[test]
fn hits_and_projects_from_junk_are_dropped_or_cleaned() {
    assert!(Hit::from(&json!({"id": "x", "kind": "mod"})).is_none());
    assert!(Hit::from(&json!({"id": id(1), "kind": "plugin"})).is_none());
    assert!(Hit::from(&json!(null)).is_none());
    let hit = Hit::from(&json!({
        "id": id(1), "kind": "mod", "title": "A\u{1b}[2J\nB", "downloads": -5,
        "iconThumb": "https://evil.example/i.png", "sources": ["modrinth", "evil"],
        "status": "weird",
    }))
    .unwrap();
    assert_eq!(hit.title, "A[2J B");
    assert_eq!(hit.downloads, 0);
    assert_eq!(hit.icon, None);
    assert_eq!(hit.sources, ["modrinth"]);
    assert_eq!(hit.status, "importing");
    assert!(hit.loaders.is_empty() && hit.category_ids.is_empty() && hit.cover.is_none());
    let hit = Hit::from(&json!({
        "id": id(1), "kind": "mod", "loaders": ["fabric", "evil"],
        "categoryIds": ["mod.utility", "mod utility"], "cover": PICTURE,
    }))
    .unwrap();
    assert_eq!(hit.loaders, ["fabric"]);
    assert_eq!(hit.category_ids, ["mod.utility"]);
    assert_eq!(hit.cover.as_deref(), Some(PICTURE));
    let hit =
        Hit::from(&json!({"id": id(1), "kind": "mod", "cover": "https://evil.example/c.jpg"}))
            .unwrap();
    assert_eq!(hit.cover, None);

    let project = Project::from(&json!({
        "id": id(2), "kind": "mod", "body": "<script>x</script>hi",
        "sources": [{"source": "modrinth", "url": "https://evil.example/x"},
                    {"source": "curseforge", "url": "https://www.curseforge.com/minecraft/mc-mods/x"}],
        "gallery": [{"url": "https://evil.example/a.png"}, {"url": PICTURE}],
        "categories": ["ok", "bad category", "x".repeat(49), "512x+", "Upper"],
        "categoryIds": ["mod.optimization", "<b>"],
        "clientSide": "<b>", "authors": [{"name": ""}, {"name": "Someone", "role": "owner"}],
    }))
    .unwrap();
    assert_eq!(all_text(&project.body), "hi");
    assert_eq!(project.sources[0].url, None);
    assert!(project.sources[1].url.is_some());
    assert_eq!(project.gallery.len(), 1);
    assert_eq!(project.categories, ["ok", "512x+"]);
    assert_eq!(project.category_ids, ["mod.optimization"]);
    assert_eq!(project.client_side, "unknown");
    assert_eq!(project.authors.len(), 1);

    // CurseForge's description is HTML: Markdown's marks in it stay text.
    let project = Project::from(&json!({
        "id": id(3), "kind": "mod", "body": "<p>*not em*</p>\n\n    <p>not code</p>",
        "sources": [{"source": "curseforge"}],
    }))
    .unwrap();
    assert_eq!(all_kinds(&project.body), ["p", "text", "p", "text"]);
    assert_eq!(all_text(&project.body), "*not em*not code");
}

#[test]
fn versions_keep_only_files_the_catalog_serves() {
    let sha = hash(3);
    let version = Version::from(
        &json!({
            "id": id(1), "projectId": id(2), "channel": "nightly",
            "games": ["1.21.1", "1.21&x"], "loaders": ["fabric", "evil"],
            "files": [
                {"name": "a.jar", "size": 10, "sha256": sha, "primary": true,
                 "url": format!("https://files-mods.lapi.sh/f/{sha}/a.jar")},
                {"name": "b.jar", "sha256": "XYZ", "url": "https://evil.example/b.jar"},
            ],
            "dependencies": [
                {"kind": "required", "projectId": id(3), "title": "Fabric\u{1b} API",
                 "slug": "fabric-api", "iconThumb": PICTURE, "source": "modrinth",
                 "sourceProject": "P7dR8mSH"},
                {"kind": "weird"},
                {"kind": "optional", "source": "modrinth", "sourceProject": "../x",
                 "slug": "a b", "iconThumb": "https://evil.example/i.png"},
                {"kind": "optional", "source": "curseforge", "sourceProject": "238222"},
            ],
            "changelog": "x",
        }),
        false,
    )
    .unwrap();
    assert_eq!(version.channel, "release");
    assert_eq!(version.games, ["1.21.1"]);
    assert_eq!(version.loaders, ["fabric"]);
    assert!(version.files[0].url.is_some());
    assert_eq!(version.files[1].sha256, None);
    assert_eq!(version.files[1].url, None);
    assert_eq!(version.dependencies.len(), 3);
    let fabric = &version.dependencies[0];
    assert_eq!(fabric.title.as_deref(), Some("Fabric API"));
    assert_eq!(fabric.slug.as_deref(), Some("fabric-api"));
    assert_eq!(fabric.icon.as_deref(), Some(PICTURE));
    assert_eq!(
        fabric.source_page.as_deref(),
        Some("https://modrinth.com/project/P7dR8mSH")
    );
    let junk = &version.dependencies[1];
    assert!(junk.slug.is_none() && junk.icon.is_none() && junk.source_page.is_none());
    // CurseForge's pages need a slug the catalog does not keep.
    assert!(version.dependencies[2].source_page.is_none());
    // Not said: not checked.
    assert!(!version.files[0].checked);
    assert!(version.changelog.is_empty());
    assert_eq!(version.main_file().unwrap().name, "a.jar");
    assert!(Version::from(&json!({"id": id(1), "projectId": "x"}), true).is_none());
}

#[test]
fn collections_from_the_catalog_are_checked() {
    let collection = Collection::from(&json!({
        "id": "fps", "preset": false,
        "titles": {"ru": "Больше\u{1b} кадров", "de": "Mehr", "en": ""},
        "notes": {"en": "x".repeat(400)},
        "game": "1.21.1", "loader": "evil",
        "query": {"collection": "fps", "sort": "random", "since": 9999, "source": "own"},
        "count": -3,
        "icons": [PICTURE, "https://evil.example/i.png", PICTURE, PICTURE, PICTURE, PICTURE],
        "names": ["Sodium", "Lithium", "Iris"],
    }))
    .unwrap();
    assert_eq!(collection.titles.len(), 1);
    assert_eq!(collection.titles["ru"], "Больше кадров");
    assert_eq!(collection.notes["en"].chars().count(), 160);
    assert_eq!(collection.game.as_deref(), Some("1.21.1"));
    assert_eq!(collection.loader, None);
    assert_eq!(
        collection.query,
        CollectionQuery {
            sort: None,
            since: None,
            source: Some("own".to_owned()),
            collection: Some("fps".to_owned()),
        }
    );
    assert_eq!(collection.count, 0);
    assert_eq!(collection.icons.len(), 4);
    assert_eq!(collection.names, ["Sodium", "Lithium"]);
    assert!(Collection::from(&json!({"id": "../x"})).is_none());
    assert!(Collection::from(&json!({"id": "x".repeat(33)})).is_none());
}

#[test]
fn import_requests_are_checked() {
    assert!(ImportRequest::from(&json!({"id": id(1), "state": "hacked"})).is_none());
    let request = ImportRequest::from(&json!({
        "id": id(1), "state": "choosing", "error": "bad<script>",
        "candidates": [
            {"source": "modrinth", "id": "sodium", "title": "Sodium", "icon": PICTURE,
             "banner": "https://cdn.modrinth.com/poster.png"},
            {"source": "evil", "id": "x"},
            {"source": "modrinth", "id": "../x"},
        ],
    }))
    .unwrap();
    assert_eq!(request.error.as_deref(), Some("badscript"));
    assert_eq!(request.candidates.len(), 1);
    // Pictures only from the catalog.
    assert_eq!(request.candidates[0].icon.as_deref(), Some(PICTURE));
    assert_eq!(request.candidates[0].banner, None);
    assert!(import_input("https://modrinth.com/mod/sodium").is_ok());
    assert!(import_input("  ").is_err());
    assert!(import_input("a\nb").is_err());
    assert!(import_input(&"x".repeat(513)).is_err());
}

#[test]
fn searches_are_checked_and_bounded() {
    let url = SearchQuery {
        text: "sodium & co\u{0}".to_owned(),
        kind: Some("mod".to_owned()),
        game: Some("1.21.1".to_owned()),
        loader: Some("fabric".to_owned()),
        limit: Some(1000),
        offset: 999_999,
        ..Default::default()
    }
    .url()
    .unwrap();
    let pairs: HashMap<String, String> = url.query_pairs().into_owned().collect();
    assert_eq!(pairs["q"], "sodium & co");
    assert_eq!(pairs["limit"], "50");
    assert_eq!(pairs["offset"], "10000");
    for bad in [
        SearchQuery {
            kind: Some("plugin".to_owned()),
            ..Default::default()
        },
        SearchQuery {
            loader: Some("x&y".to_owned()),
            ..Default::default()
        },
        SearchQuery {
            sort: Some("random".to_owned()),
            ..Default::default()
        },
        SearchQuery {
            category: Some("a/b".to_owned()),
            ..Default::default()
        },
        SearchQuery {
            game: Some("1.21&x=y".to_owned()),
            ..Default::default()
        },
        SearchQuery {
            categories: (0..9).map(|n| format!("c{n}")).collect(),
            ..Default::default()
        },
        SearchQuery {
            categories: vec!["mod.optimization&x=y".to_owned()],
            ..Default::default()
        },
        SearchQuery {
            source: Some("elsewhere".to_owned()),
            ..Default::default()
        },
        SearchQuery {
            since: Some(0),
            ..Default::default()
        },
        SearchQuery {
            since: Some(366),
            ..Default::default()
        },
        SearchQuery {
            collection: Some("Not a key".to_owned()),
            ..Default::default()
        },
    ] {
        assert!(bad.url().is_err(), "{bad:?}");
    }
    // Several categories, IDs of the shared list or the sites' own slugs.
    let url = SearchQuery {
        category: Some("512x+".to_owned()),
        categories: vec!["mod.optimization".to_owned(), "mod.utility".to_owned()],
        source: Some("own".to_owned()),
        since: Some(7),
        collection: Some("fps".to_owned()),
        sort: Some("trending".to_owned()),
        ..Default::default()
    }
    .url()
    .unwrap();
    let pairs: HashMap<String, String> = url.query_pairs().into_owned().collect();
    assert_eq!(pairs["category"], "512x+,mod.optimization,mod.utility");
    assert_eq!(pairs["source"], "own");
    assert_eq!(pairs["since"], "7");
    assert_eq!(pairs["collection"], "fps");
    assert_eq!(pairs["sort"], "trending");
    assert!(serde_json::from_value::<SearchQuery>(json!({"text": "a", "evil": 1})).is_err());
}

#[test]
fn errors_of_the_catalog_are_ours() {
    assert_eq!(
        problem_of(401, &Value::Null).code,
        crate::error::ErrorCode::AuthReauthRequired
    );
    assert_eq!(
        problem_of(429, &json!({"error": "too_much_today"})).code,
        crate::error::ErrorCode::RateLimited
    );
    // What the server says is never shown.
    let error = problem_of(500, &json!({"error": "<b>secret</b>"}));
    assert!(!error.message.contains("secret"));
}

// ----- content paths and names -----

#[test]
fn content_paths_stay_in_content_folders() {
    let game = Path::new("/data/instances/x/game");
    for ok in [
        "mods/sodium.jar",
        "mods/sodium.jar.disabled",
        "resourcepacks/Faithful.zip",
        "shaderpacks/BSL",
        "saves/My World/datapacks/pack.zip",
    ] {
        assert!(content::content_path(game, ok).is_ok(), "{ok}");
    }
    for bad in [
        "",
        "mods",
        "mods/",
        "mods/../options.txt",
        "../mods/a.jar",
        "mods/sub/a.jar",
        "mods\\..\\a.jar",
        "/etc/passwd",
        "C:/x/mods/a.jar",
        "config/a.toml",
        "mods/.hidden",
        "saves/../datapacks/x.zip",
        "saves/w/datapacks",
        "saves/w/region/r.0.0.mca",
        "mods/a\u{0}.jar",
        "mods/a:b.jar",
    ] {
        assert!(content::content_path(game, bad).is_err(), "{bad:?}");
    }
}

#[test]
fn catalog_file_names_are_safe_everywhere() {
    assert!(content::catalog_file_name("sodium-0.6.jar", "mod"));
    assert!(content::catalog_file_name("Pack.zip", "resourcepack"));
    for (bad, kind) in [
        ("sodium.zip", "mod"),
        ("pack.jar", "resourcepack"),
        ("../a.jar", "mod"),
        ("a/b.jar", "mod"),
        ("CON.jar", "mod"),
        ("com1.jar", "mod"),
        ("a .jar ", "mod"),
        ("a<b.jar", "mod"),
        ("a?.jar", "mod"),
        (".jar", "mod"),
        ("x.zip", "modpack"),
        ("x.zip", "world"),
    ] {
        assert!(!content::catalog_file_name(bad, kind), "{bad} {kind}");
    }
    assert!(!content::catalog_file_name(
        &format!("{}.jar", "a".repeat(250)),
        "mod"
    ));
}

// ----- plans against a stand-in catalog -----

#[derive(Default)]
struct Fake {
    projects: HashMap<String, Project>,
    versions: HashMap<String, Version>,
}

impl Fake {
    fn add_project(&mut self, n: u8, kind: &str, title: &str) {
        let p = Project::from(&json!({"id": id(n), "kind": kind, "title": title})).unwrap();
        self.projects.insert(p.id.clone(), p);
    }

    /// A version `v` of project `p` with a kept file `name`.
    fn add_version(&mut self, v: u8, p: u8, name: &str, extra: Value) -> String {
        let sha = hash(v);
        let mut value = json!({
            "id": id(v), "projectId": id(p), "number": format!("{v}.0"),
            "published": i64::from(v), "games": ["1.21.1"], "loaders": ["fabric"],
            "files": [{"name": name, "size": 100, "sha256": sha, "primary": true,
                       "url": format!("https://files-mods.lapi.sh/f/{sha}/{name}")}],
        });
        if let (Some(base), Some(extra)) = (value.as_object_mut(), extra.as_object()) {
            for (k, x) in extra {
                base.insert(k.clone(), x.clone());
            }
        }
        let version = Version::from(&value, true).unwrap();
        self.versions.insert(version.id.clone(), version);
        id(v)
    }

    /// What the jar of version `v` says, as the catalog read it.
    fn says(&mut self, v: u8, manifests: Value) {
        let file = &mut self.versions.get_mut(&id(v)).unwrap().files[0];
        file.manifests = Some(serde_json::from_value(manifests).unwrap());
    }
}

/// A Fabric jar's manifests: the mod, the version it is, and what it needs.
fn fabric_jar(mod_id: &str, version: &str, needs: &[(&str, &str)]) -> Value {
    let needs: Vec<Value> = needs
        .iter()
        // As the reader gives it: `*` is no range at all.
        .map(|(id, range)| {
            let versions: Vec<&str> = [*range].into_iter().filter(|r| *r != "*").collect();
            json!({"by": mod_id, "id": id, "versions": versions})
        })
        .collect();
    json!([{"loader": "fabric", "mods": [{"id": mod_id, "version": version}], "needs": needs}])
}

impl Catalog for Fake {
    async fn version(&self, id: &str) -> LauncherResult<Version> {
        self.versions.get(id).cloned().ok_or_else(not_found)
    }

    async fn project(&self, id: &str) -> LauncherResult<Project> {
        self.projects.get(id).cloned().ok_or_else(not_found)
    }

    async fn versions(
        &self,
        project: &str,
        game: Option<&str>,
        loader: Option<&str>,
    ) -> LauncherResult<Vec<Version>> {
        let mut out: Vec<Version> = self
            .versions
            .values()
            .filter(|v| v.project_id == project)
            .filter(|v| game.is_none_or(|g| v.games.iter().any(|x| x == g)))
            .filter(|v| loader.is_none_or(|l| v.loaders.iter().any(|x| x == l)))
            .cloned()
            .collect();
        out.sort_by_key(|v| std::cmp::Reverse(v.published));
        Ok(out)
    }

    async fn fetch(&self, _id: &str) -> LauncherResult<()> {
        Ok(())
    }

    async fn provides(
        &self,
        mod_id: &str,
        game: &str,
        loader: &str,
    ) -> LauncherResult<Vec<String>> {
        let mut out: Vec<String> = self
            .versions
            .values()
            .filter(|v| v.games.iter().any(|g| g == game) && v.loaders.iter().any(|l| l == loader))
            .filter(|v| {
                v.files[0]
                    .manifests
                    .iter()
                    .flatten()
                    .any(|m| m.mods.iter().any(|p| p.id == mod_id && !p.nested))
            })
            .map(|v| v.project_id.clone())
            .collect();
        out.sort();
        out.dedup();
        Ok(out)
    }

    async fn lookup(&self, hashes: &[String]) -> LauncherResult<Vec<Found>> {
        Ok(self
            .versions
            .values()
            .flat_map(|v| {
                v.files.iter().filter_map(|f| {
                    let sha = f.sha256.clone()?;
                    hashes.contains(&sha).then(|| Found {
                        hash: sha,
                        project_id: v.project_id.clone(),
                        version_id: v.id.clone(),
                    })
                })
            })
            .collect())
    }
}

fn fabric() -> Target {
    Target {
        game: "1.21.1".to_owned(),
        loader: Some(LoaderKind::Fabric),
        loader_version: None,
    }
}

fn wanted(version: &str) -> Wanted<'_> {
    Wanted {
        version,
        world: None,
        replaces: None,
    }
}

fn locked(path: &str, project: u8, version: u8) -> LockedItem {
    LockedItem {
        path: path.to_owned(),
        sha256: hash(version),
        size: 100,
        project_id: id(project),
        version_id: id(version),
        kind: "mod".to_owned(),
        title: format!("Project {project}"),
        version_number: format!("{version}.0"),
        published: i64::from(version),
        icon: None,
        dependency: false,
        requires: None,
        installed_at: 0,
    }
}

fn touch(game: &Path, relative: &str) {
    let path = game.join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, b"x").unwrap();
}

#[tokio::test]
async fn a_mod_brings_what_it_requires() {
    let mut fake = Fake::default();
    fake.add_project(1, "mod", "Sodium Extra");
    fake.add_project(2, "mod", "Sodium");
    fake.add_project(3, "mod", "Gone");
    let chosen = fake.add_version(
        10,
        1,
        "extra.jar",
        json!({"dependencies": [
            {"kind": "required", "projectId": id(2)},
            {"kind": "optional", "projectId": id(3)},
        ]}),
    );
    fake.add_version(20, 2, "sodium-old.jar", json!({}));
    fake.add_version(21, 2, "sodium.jar", json!({}));
    fake.add_version(22, 2, "sodium-beta.jar", json!({"channel": "beta"}));
    fake.add_version(23, 2, "sodium-forge.jar", json!({"loaders": ["forge"]}));
    let dir = tempfile::tempdir().unwrap();
    let plan = content::plan(
        &fake,
        dir.path(),
        &fabric(),
        &ContentLock::default(),
        &wanted(&chosen),
        &[],
    )
    .await
    .unwrap();
    assert!(plan.ready(), "{plan:?}");
    let names: Vec<&str> = plan.items.iter().map(|i| i.file_name.as_str()).collect();
    // The newest release that fits, not the beta or the Forge one.
    assert_eq!(names, ["extra.jar", "sodium.jar"]);
    assert_eq!(plan.items[1].path, "mods/sodium.jar");
    assert!(plan.items[1].dependency && plan.items[1].kept);
    assert_eq!(plan.total_bytes, 200);
}

#[tokio::test]
async fn every_dependency_down_a_long_chain_comes_and_a_loop_ends() {
    let mut fake = Fake::default();
    for n in 1..=30u8 {
        fake.add_project(n, "mod", &format!("Link {n}"));
        let next = if n == 30 { 1 } else { n + 1 };
        fake.add_version(
            100 + n,
            n,
            &format!("link-{n}.jar"),
            json!({"dependencies": [{"kind": "required", "projectId": id(next)}]}),
        );
    }
    let dir = tempfile::tempdir().unwrap();
    let plan = content::plan(
        &fake,
        dir.path(),
        &fabric(),
        &ContentLock::default(),
        &wanted(&id(101)),
        &[],
    )
    .await
    .unwrap();
    assert!(plan.ready(), "{plan:?}");
    assert_eq!(plan.items.len(), 30);
    assert_eq!(plan.items.last().unwrap().file_name, "link-30.jar");
}

/// Fabric with its loader's version known.
fn fabric_at(loader: &str) -> Target {
    Target {
        loader_version: Some(loader.to_owned()),
        ..fabric()
    }
}

#[test]
fn loaders_are_checked_by_their_own_number() {
    let forge = Target {
        game: "1.21.1".to_owned(),
        loader: Some(LoaderKind::Forge),
        loader_version: Some("1.21.1-52.1.0".to_owned()),
    };
    let setup = forge.setup().unwrap();
    assert_eq!(setup.loader, mod_meta::Loader::Forge);
    assert_eq!(setup.loader_version, Some("52.1.0"));
    let old = Target {
        game: "1.7.10".to_owned(),
        loader_version: Some("1.7.10-10.13.4.1614-1.7.10".to_owned()),
        ..forge.clone()
    };
    assert_eq!(old.setup().unwrap().loader_version, Some("10.13.4.1614"));
    assert_eq!(
        fabric_at("0.16.14").setup().unwrap().loader_version,
        Some("0.16.14")
    );
    // OptiFine on its own and no loader: nothing to check.
    let optifine = Target {
        loader: Some(LoaderKind::OptiFine),
        ..forge
    };
    assert!(optifine.setup().is_none());
    assert!(
        Target {
            loader: None,
            ..fabric()
        }
        .setup()
        .is_none()
    );
}

#[test]
fn what_the_catalog_read_in_a_jar_is_taken_only_when_clean() {
    let file = |manifests: Value| {
        let value = json!({
            "id": id(1), "projectId": id(2),
            "files": [{"name": "a.jar", "size": 1, "manifests": manifests}],
        });
        Version::from(&value, true).unwrap().files[0]
            .manifests
            .clone()
    };
    let read = file(fabric_jar("sodium", "0.6.0", &[("minecraft", ">=1.21")])).unwrap();
    assert_eq!(read[0].mods[0].id, "sodium");
    assert_eq!(read[0].needs[0].versions, [">=1.21"]);
    assert_eq!(file(json!([])), Some(Vec::new()));
    for junk in [
        Value::Null,
        json!("x"),
        fabric_jar("Sodium", "1", &[]),
        fabric_jar("sodium", "1\u{7}", &[]),
        fabric_jar("sodium", "1", &[("a\u{202e}", "*")]),
        json!([{"loader": "rift", "mods": [{"id": "a"}]}]),
        json!(vec![fabric_jar("a", "1", &[])[0].clone(); 9]),
    ] {
        assert_eq!(file(junk.clone()), None, "{junk}");
    }
}

#[tokio::test]
async fn a_dependency_is_a_version_whose_jar_runs_on_the_loader() {
    let mut fake = Fake::default();
    fake.add_project(1, "mod", "Extra");
    fake.add_project(2, "mod", "Library");
    let chosen = fake.add_version(
        10,
        1,
        "extra.jar",
        json!({"dependencies": [{"kind": "required", "projectId": id(2)}]}),
    );
    fake.add_version(20, 2, "library-1.jar", json!({}));
    fake.add_version(21, 2, "library-2.jar", json!({}));
    fake.says(
        20,
        fabric_jar("library", "1.0", &[("fabricloader", ">=0.14")]),
    );
    fake.says(
        21,
        fabric_jar("library", "2.0", &[("fabricloader", ">=0.16")]),
    );
    let dir = tempfile::tempdir().unwrap();
    let lock = ContentLock::default();
    let names = |plan: &content::Plan| -> Vec<String> {
        plan.items.iter().map(|i| i.file_name.clone()).collect()
    };
    // Loader 0.15: the newest release asks for 0.16, the one before runs.
    let plan = content::plan(
        &fake,
        dir.path(),
        &fabric_at("0.15.11"),
        &lock,
        &wanted(&chosen),
        &[],
    )
    .await
    .unwrap();
    assert_eq!(names(&plan), ["extra.jar", "library-1.jar"]);
    assert!(plan.problems.is_empty(), "{plan:?}");
    // Loader 0.16, or a loader version not known: the newest.
    for target in [fabric_at("0.16.14"), fabric()] {
        let plan = content::plan(&fake, dir.path(), &target, &lock, &wanted(&chosen), &[])
            .await
            .unwrap();
        assert_eq!(names(&plan), ["extra.jar", "library-2.jar"]);
    }
    // No version runs: the newest, and why it does not.
    fake.says(
        20,
        fabric_jar("library", "1.0", &[("fabricloader", ">=0.16")]),
    );
    let plan = content::plan(
        &fake,
        dir.path(),
        &fabric_at("0.15.11"),
        &lock,
        &wanted(&chosen),
        &[],
    )
    .await
    .unwrap();
    assert_eq!(names(&plan), ["extra.jar", "library-2.jar"]);
    assert_eq!(plan.problems.len(), 1, "{plan:?}");
    let problem = &plan.problems[0];
    assert_eq!(problem.kind, "wrong");
    assert_eq!(problem.titles, ["Library"]);
    assert_eq!(problem.id, "fabricloader");
    assert_eq!(problem.versions, [">=0.16"]);
    assert_eq!(problem.found.as_deref(), Some("0.15.11"));
    assert_eq!(problem.from, None);
    // Shown, not in the way.
    assert!(plan.ready());
}

#[tokio::test]
async fn a_version_that_runs_is_offered_instead_of_one_that_does_not() {
    let mut fake = Fake::default();
    fake.add_project(1, "mod", "Extra");
    let old = fake.add_version(10, 1, "extra-1.jar", json!({}));
    let new = fake.add_version(11, 1, "extra-2.jar", json!({}));
    fake.says(10, fabric_jar("extra", "1.0", &[("minecraft", "1.21.x")]));
    fake.says(11, fabric_jar("extra", "2.0", &[("minecraft", ">=1.21.2")]));
    let dir = tempfile::tempdir().unwrap();
    let plan = content::plan(
        &fake,
        dir.path(),
        &fabric_at("0.16.14"),
        &ContentLock::default(),
        &wanted(&new),
        &[],
    )
    .await
    .unwrap();
    let instead = plan.instead.as_ref().unwrap();
    assert_eq!(instead.version_id, old);
    assert_eq!(instead.number, "10.0");
    assert_eq!(plan.problems[0].id, "minecraft");
    assert_eq!(plan.problems[0].found.as_deref(), Some("1.21.1"));
    // The one that runs offers nothing else.
    let plan = content::plan(
        &fake,
        dir.path(),
        &fabric_at("0.16.14"),
        &ContentLock::default(),
        &wanted(&old),
        &[],
    )
    .await
    .unwrap();
    assert!(
        plan.instead.is_none() && plan.problems.is_empty(),
        "{plan:?}"
    );
}

#[tokio::test]
async fn a_mod_only_the_jar_names_is_found_by_its_id() {
    let mut fake = Fake::default();
    fake.add_project(1, "mod", "Extra");
    fake.add_project(4, "mod", "Cloth Config");
    fake.add_project(5, "mod", "Cloth Config (old)");
    // The page names nothing; the jar needs Cloth Config 15.
    let chosen = fake.add_version(10, 1, "extra.jar", json!({}));
    fake.says(10, fabric_jar("extra", "1.0", &[("cloth-config", ">=15")]));
    fake.add_version(40, 4, "cloth-config-15.jar", json!({}));
    fake.says(40, fabric_jar("cloth-config", "15.0.140", &[]));
    // A project that gives it too, but too old.
    fake.add_version(50, 5, "cloth-config-11.jar", json!({}));
    fake.says(50, fabric_jar("cloth-config", "11.1.0", &[]));
    let dir = tempfile::tempdir().unwrap();
    let plan = content::checked_plan(
        &fake,
        dir.path(),
        &fabric_at("0.16.14"),
        &ContentLock::default(),
        &wanted(&chosen),
        &[],
        &[],
    )
    .await
    .unwrap();
    let names: Vec<&str> = plan.items.iter().map(|i| i.file_name.as_str()).collect();
    assert_eq!(names, ["extra.jar", "cloth-config-15.jar"], "{plan:?}");
    assert!(plan.items[1].dependency);
    // Noted as needed by Extra, so it is not taken for unused.
    assert_eq!(plan.items[0].requires, [id(4)]);
    assert!(plan.problems.is_empty() && plan.ready(), "{plan:?}");

    // One there already, turned on, gives it: nothing to add.
    let local = [content::LocalJar {
        path: "mods/cloth.jar".to_owned(),
        manifests: serde_json::from_value(fabric_jar("cloth-config", "15.0.1", &[])).unwrap(),
    }];
    touch(dir.path(), "mods/cloth.jar");
    let plan = content::checked_plan(
        &fake,
        dir.path(),
        &fabric_at("0.16.14"),
        &ContentLock::default(),
        &wanted(&chosen),
        &[],
        &local,
    )
    .await
    .unwrap();
    assert_eq!(plan.items.len(), 1, "{plan:?}");
    assert!(plan.problems.is_empty());
}

#[tokio::test]
async fn a_mod_there_already_too_old_for_a_new_one_is_updated() {
    let mut fake = Fake::default();
    fake.add_project(1, "mod", "Extra");
    fake.add_project(5, "mod", "Library");
    let chosen = fake.add_version(10, 1, "extra.jar", json!({}));
    fake.says(10, fabric_jar("extra", "1.0", &[("library", ">=2.0")]));
    fake.add_version(50, 5, "library-1.jar", json!({}));
    fake.says(50, fabric_jar("library", "1.0", &[]));
    fake.add_version(51, 5, "library-2.jar", json!({}));
    fake.says(51, fabric_jar("library", "2.1", &[]));
    let dir = tempfile::tempdir().unwrap();
    touch(dir.path(), "mods/library-1.jar");
    let mut lock = ContentLock::default();
    let mut ours = locked("mods/library-1.jar", 5, 50);
    ours.dependency = true;
    lock.items.push(ours);
    let local = [content::LocalJar {
        path: "mods/library-1.jar".to_owned(),
        manifests: serde_json::from_value(fabric_jar("library", "1.0", &[])).unwrap(),
    }];
    let plan = content::checked_plan(
        &fake,
        dir.path(),
        &fabric_at("0.16.14"),
        &lock,
        &wanted(&chosen),
        &[],
        &local,
    )
    .await
    .unwrap();
    assert_eq!(plan.items.len(), 2, "{plan:?}");
    let update = &plan.items[1];
    assert_eq!(update.file_name, "library-2.jar");
    assert_eq!(update.replaces.as_deref(), Some("mods/library-1.jar"));
    assert!(update.dependency);
    assert!(plan.problems.is_empty(), "{plan:?}");

    // The player's own copy, recognized, is updated too and stays theirs.
    let identified = [content::Identified {
        path: "mods/library-1.jar".to_owned(),
        project_id: id(5),
        version_id: id(50),
    }];
    let plan = content::checked_plan(
        &fake,
        dir.path(),
        &fabric_at("0.16.14"),
        &ContentLock::default(),
        &wanted(&chosen),
        &identified,
        &local,
    )
    .await
    .unwrap();
    assert_eq!(
        plan.items[1].replaces.as_deref(),
        Some("mods/library-1.jar")
    );
    assert!(!plan.items[1].dependency);

    // Not known to the catalog: shown, the file left be.
    let plan = content::checked_plan(
        &fake,
        dir.path(),
        &fabric_at("0.16.14"),
        &ContentLock::default(),
        &wanted(&chosen),
        &[],
        &local,
    )
    .await
    .unwrap();
    assert_eq!(plan.items.len(), 1);
    let problem = &plan.problems[0];
    assert_eq!(problem.kind, "wrong");
    assert_eq!(problem.titles, ["Extra"]);
    assert_eq!(problem.id, "library");
    assert_eq!(problem.found.as_deref(), Some("1.0"));
    assert_eq!(problem.from.as_deref(), Some("library-1.jar"));
}

#[tokio::test]
async fn only_problems_the_install_has_to_do_with_are_shown() {
    let mut fake = Fake::default();
    fake.add_project(1, "mod", "Extra");
    let chosen = fake.add_version(10, 1, "extra.jar", json!({}));
    fake.says(10, fabric_jar("extra", "1.0", &[("nowhere", "*")]));
    let dir = tempfile::tempdir().unwrap();
    let jar = |path: &str, manifests: Value| content::LocalJar {
        path: path.to_owned(),
        manifests: serde_json::from_value(manifests).unwrap(),
    };
    let local = [
        // The player's own copy of the same mod.
        jar("mods/extra-own.jar", fabric_jar("extra", "0.9", &[])),
        // Something wrong before the install, and off.
        jar(
            "mods/broken.jar",
            fabric_jar("broken", "1", &[("gone", "*")]),
        ),
        jar(
            "mods/off.jar.disabled",
            fabric_jar("off", "1", &[("extra", "<1")]),
        ),
        // Made for Forge.
        jar(
            "mods/forge.jar",
            json!([{"loader": "forge", "mods": [{"id": "forgemod"}]}]),
        ),
    ];
    let plan = content::checked_plan(
        &fake,
        dir.path(),
        &fabric_at("0.16.14"),
        &ContentLock::default(),
        &wanted(&chosen),
        &[],
        &local,
    )
    .await
    .unwrap();
    let mut kinds: Vec<(&str, Vec<String>)> = plan
        .problems
        .iter()
        .map(|p| (p.kind.as_str(), p.titles.clone()))
        .collect();
    kinds.sort();
    assert_eq!(
        kinds,
        [
            ("missing", vec!["Extra".to_owned()]),
            (
                "twice",
                vec!["extra-own.jar".to_owned(), "Extra".to_owned()]
            ),
        ],
        "{plan:?}"
    );
    let missing = plan.problems.iter().find(|p| p.kind == "missing").unwrap();
    assert_eq!(missing.id, "nowhere");
    assert!(missing.versions.is_empty());
}

#[tokio::test]
async fn a_mod_the_game_needs_is_found_to_put_in_or_update() {
    let mut fake = Fake::default();
    fake.add_project(1, "mod", "Sodium");
    fake.add_version(10, 1, "sodium-0.5.jar", json!({}));
    fake.says(10, fabric_jar("sodium", "0.5.0", &[]));
    fake.add_version(11, 1, "sodium-0.6.jar", json!({}));
    fake.says(11, fabric_jar("sodium", "0.6.0", &[]));
    // A beta that is newer still: a release comes first.
    let beta = fake.add_version(12, 1, "sodium-0.7.jar", json!({"channel": "beta"}));
    fake.says(12, fabric_jar("sodium", "0.7.0", &[]));
    let jar = |path: &str, manifests: Value| content::LocalJar {
        path: path.to_owned(),
        manifests: serde_json::from_value(manifests).unwrap(),
    };
    let iris = jar(
        "mods/iris.jar",
        fabric_jar("iris", "1.8", &[("sodium", ">=0.6.0")]),
    );
    let empty = ContentLock::default();
    let fix =
        |lock: ContentLock, local: Vec<content::LocalJar>, identified: Vec<content::Identified>| {
            let fake = &fake;
            async move {
                content::mod_fix(fake, &fabric(), &lock, &local, &identified, "sodium")
                    .await
                    .unwrap()
            }
        };

    // Missing: found by its ID, the newest release that gives it.
    let found = fix(empty.clone(), vec![iris.clone()], vec![])
        .await
        .unwrap();
    assert_eq!(found.project_id, id(1));
    assert_eq!(found.title, "Sodium");
    assert_eq!(found.version_id, id(11));
    assert_eq!(found.number, "11.0");
    assert_eq!((found.installed, found.replaces), (None, None));

    // Too old, put there by the launcher: an update in place of its file.
    let lock = ContentLock {
        items: vec![locked("mods/sodium-0.5.jar", 1, 10)],
        ..ContentLock::default()
    };
    let local = vec![
        iris.clone(),
        jar("mods/sodium-0.5.jar", fabric_jar("sodium", "0.5.0", &[])),
    ];
    let found = fix(lock, local.clone(), vec![]).await.unwrap();
    assert_eq!(found.version_id, id(11));
    assert_eq!(found.installed.as_deref(), Some("mods/sodium-0.5.jar"));
    assert_eq!(found.replaces, None);

    // The player's own copy the catalog recognized: it is replaced.
    let identified = vec![content::Identified {
        path: "mods/sodium-0.5.jar".to_owned(),
        project_id: id(1),
        version_id: id(10),
    }];
    let found = fix(empty.clone(), local.clone(), identified).await.unwrap();
    assert_eq!(found.replaces.as_deref(), Some("mods/sodium-0.5.jar"));

    // The newest that gives what is needed is there already: nothing better.
    let lock = ContentLock {
        items: vec![locked("mods/sodium-0.6.jar", 1, 11)],
        ..ContentLock::default()
    };
    let local = vec![
        jar(
            "mods/iris.jar",
            fabric_jar("iris", "1.8", &[("sodium", ">=0.8.0")]),
        ),
        jar("mods/sodium-0.6.jar", fabric_jar("sodium", "0.6.0", &[])),
    ];
    assert_eq!(fix(lock, local, vec![]).await, None);

    // Only a beta gives the version needed: that one.
    let wants_beta = jar(
        "mods/iris.jar",
        fabric_jar("iris", "1.9", &[("sodium", ">=0.7.0")]),
    );
    let found = fix(empty.clone(), vec![wants_beta], vec![]).await.unwrap();
    assert_eq!(found.version_id, beta);

    // Nothing gives it, or the ID cannot be one.
    let other = jar("mods/a.jar", fabric_jar("a", "1", &[("nowhere", "*")]));
    assert_eq!(
        content::mod_fix(&fake, &fabric(), &empty, &[other], &[], "nowhere")
            .await
            .unwrap(),
        None
    );
    for bad in ["", "Sodium", "../a", "a b"] {
        assert_eq!(
            content::mod_fix(
                &fake,
                &fabric(),
                &empty,
                std::slice::from_ref(&iris),
                &[],
                bad
            )
            .await
            .unwrap(),
            None
        );
    }
}

#[test]
fn a_folder_is_checked_before_a_launch() {
    let jar = |path: &str, manifests: Value| content::LocalJar {
        path: path.to_owned(),
        manifests: serde_json::from_value(manifests).unwrap(),
    };
    let local = [
        jar(
            "mods/extra.jar",
            fabric_jar(
                "extra",
                "1.0",
                &[("library", ">=2"), ("fabricloader", ">=0.17")],
            ),
        ),
        jar("mods/library.jar", fabric_jar("library", "1.5", &[])),
        // Off: not loaded, so not checked.
        jar(
            "mods/off.jar.disabled",
            fabric_jar("off", "1", &[("nowhere", "*")]),
        ),
        // No mod at all.
        jar("mods/notes.zip", json!([])),
    ];
    let mut lock = ContentLock::default();
    lock.items.push(locked("mods/library.jar", 5, 50));
    let problems = content::check_folder(&fabric_at("0.16.14"), &lock, &local, &[]);
    let mut found: Vec<(&str, &str, Option<&str>, Option<&str>)> = problems
        .iter()
        .map(|p| {
            (
                p.id.as_str(),
                p.titles[0].as_str(),
                p.found.as_deref(),
                p.from.as_deref(),
            )
        })
        .collect();
    found.sort();
    assert_eq!(
        found,
        [
            ("fabricloader", "extra.jar", Some("0.16.14"), None),
            ("library", "extra.jar", Some("1.5"), Some("Project 5")),
        ]
    );
    // Nothing to check without a mod loader.
    let vanilla = Target {
        loader: None,
        ..fabric()
    };
    assert!(content::check_folder(&vanilla, &lock, &local, &[]).is_empty());
}

#[test]
fn mods_known_not_to_start_together_are_named_before_a_launch() {
    let jar = |path: &str, manifests: Value| content::LocalJar {
        path: path.to_owned(),
        manifests: serde_json::from_value(manifests).unwrap(),
    };
    let optifine =
        json!([{"loader": "fabric", "mods": [{"id": "optifine", "side": "client"}], "needs": []}]);
    let local = [
        jar("mods/sodium.jar", fabric_jar("sodium", "0.6.13", &[])),
        jar("mods/OptiFine_1.21.1_HD_U_J1.jar", optifine),
    ];
    let lock = ContentLock::default();
    let problems = content::check_folder(&fabric_at("0.17.2"), &lock, &local, &mod_meta::clashes());
    let found: Vec<(&str, &str, &str, Option<&str>)> = problems
        .iter()
        .map(|p| {
            (
                p.kind.as_str(),
                p.titles[0].as_str(),
                p.id.as_str(),
                p.from.as_deref(),
            )
        })
        .collect();
    assert_eq!(
        found,
        [(
            "breaks",
            "sodium.jar",
            "optifine",
            Some("OptiFine_1.21.1_HD_U_J1.jar")
        )]
    );
    // A pair the owner took off the list is no longer named.
    assert!(content::check_folder(&fabric_at("0.17.2"), &lock, &local, &[]).is_empty());
    // On its own, either one is fine.
    assert!(
        content::check_folder(
            &fabric_at("0.17.2"),
            &lock,
            &local[..1],
            &mod_meta::clashes()
        )
        .is_empty()
    );
}

#[tokio::test]
async fn missing_and_incompatible_projects_stop_a_plan() {
    let mut fake = Fake::default();
    fake.add_project(1, "mod", "A");
    fake.add_project(2, "mod", "Needed");
    fake.add_project(3, "mod", "Enemy");
    let chosen = fake.add_version(
        10,
        1,
        "a.jar",
        json!({"dependencies": [
            {"kind": "required", "projectId": id(2)},
            {"kind": "required"},
            {"kind": "incompatible", "projectId": id(3)},
        ]}),
    );
    // Only for another loader.
    fake.add_version(20, 2, "needed.jar", json!({"loaders": ["neoforge"]}));
    let dir = tempfile::tempdir().unwrap();
    touch(dir.path(), "mods/enemy.jar.disabled");
    let lock = ContentLock {
        schema_version: 1,
        items: vec![locked("mods/enemy.jar", 3, 30)],
        pack: None,
        published: None,
    };
    let plan = content::plan(&fake, dir.path(), &fabric(), &lock, &wanted(&chosen), &[])
        .await
        .unwrap();
    assert!(!plan.ready());
    assert_eq!(plan.missing.len(), 2);
    assert_eq!(plan.missing[0].title.as_deref(), Some("Needed"));
    assert_eq!(plan.conflicts.len(), 1);
    assert_eq!(plan.conflicts[0].reason, "incompatible");
}

#[tokio::test]
async fn a_file_the_catalog_holds_back_is_not_installed() {
    let mut fake = Fake::default();
    fake.add_project(1, "mod", "A");
    let sha = hash(10);
    let held = fake.add_version(
        10,
        1,
        "a.jar",
        json!({"files": [{"name": "a.jar", "size": 100, "sha256": sha, "primary": true, "held": true,
                          "url": format!("https://files-mods.lapi.sh/f/{sha}/a.jar")}]}),
    );
    let file = &fake.versions[&held].files[0];
    assert!(file.held);
    // Even if an address came with it.
    assert!(file.url.is_none());
    let dir = tempfile::tempdir().unwrap();
    let plan = content::plan(
        &fake,
        dir.path(),
        &fabric(),
        &ContentLock::default(),
        &wanted(&held),
        &[],
    )
    .await
    .unwrap();
    assert!(!plan.ready());
    assert_eq!(plan.conflicts.len(), 1);
    assert_eq!(plan.conflicts[0].reason, "held");
    assert_eq!(plan.conflicts[0].title.as_deref(), Some("A"));
}

#[test]
fn harmful_files_are_found_by_their_sha1_on_or_off() {
    use sha1::Digest as _;
    let (_dir, root, id) = instance();
    let game = root.game_dir(id);
    fs::create_dir_all(game.join("mods")).unwrap();
    fs::write(game.join("mods/bad.jar.disabled"), b"bad").unwrap();
    fs::write(game.join("mods/good.jar"), b"good").unwrap();
    let installer = content::Installer::new("0.0.0-test").unwrap();
    let harmful = std::collections::HashSet::from([hex::encode(sha1::Sha1::digest(b"bad"))]);
    assert_eq!(
        installer.harmful_files(&root, id, &harmful),
        vec!["mods/bad.jar.disabled".to_owned()]
    );
    assert!(
        installer
            .harmful_files(&root, id, &std::collections::HashSet::new())
            .is_empty()
    );
}

#[tokio::test]
async fn mods_must_fit_the_installation() {
    let mut fake = Fake::default();
    fake.add_project(1, "mod", "A");
    let forge = fake.add_version(10, 1, "a.jar", json!({"loaders": ["forge"]}));
    let old = fake.add_version(11, 1, "b.jar", json!({"games": ["1.20.1"]}));
    let dir = tempfile::tempdir().unwrap();
    let lock = ContentLock::default();
    assert!(
        content::plan(&fake, dir.path(), &fabric(), &lock, &wanted(&forge), &[])
            .await
            .is_err()
    );
    assert!(
        content::plan(&fake, dir.path(), &fabric(), &lock, &wanted(&old), &[])
            .await
            .is_err()
    );
    let vanilla = Target {
        game: "1.21.1".to_owned(),
        loader: None,
        loader_version: None,
    };
    let fits = fake.add_version(12, 1, "c.jar", json!({}));
    assert!(
        content::plan(&fake, dir.path(), &vanilla, &lock, &wanted(&fits), &[])
            .await
            .is_err()
    );
    // Quilt runs Fabric's mods.
    let quilt = Target {
        game: "1.21.1".to_owned(),
        loader: Some(LoaderKind::Quilt),
        loader_version: None,
    };
    assert!(
        content::plan(&fake, dir.path(), &quilt, &lock, &wanted(&fits), &[])
            .await
            .is_ok()
    );
}

#[tokio::test]
async fn packs_may_not_fit_with_a_warning() {
    let mut fake = Fake::default();
    fake.add_project(1, "resourcepack", "Faithful");
    let v = fake.add_version(
        10,
        1,
        "faithful.zip",
        json!({"games": ["1.20.1"], "loaders": ["minecraft"]}),
    );
    let dir = tempfile::tempdir().unwrap();
    let vanilla = Target {
        game: "1.21.1".to_owned(),
        loader: None,
        loader_version: None,
    };
    let plan = content::plan(
        &fake,
        dir.path(),
        &vanilla,
        &ContentLock::default(),
        &wanted(&v),
        &[],
    )
    .await
    .unwrap();
    assert_eq!(plan.misfit, Some(Misfit::WrongGame));
    assert_eq!(plan.items[0].path, "resourcepacks/faithful.zip");
}

#[tokio::test]
async fn kinds_without_a_place_are_refused() {
    let mut fake = Fake::default();
    fake.add_project(1, "modpack", "Pack");
    fake.add_project(2, "datapack", "Data");
    fake.add_project(3, "mod", "Bad name");
    let pack = fake.add_version(10, 1, "pack.mrpack", json!({}));
    let data = fake.add_version(11, 2, "data.zip", json!({}));
    let bad = fake.add_version(12, 3, "CON.jar", json!({}));
    let dir = tempfile::tempdir().unwrap();
    let lock = ContentLock::default();
    let target = fabric();
    assert!(
        content::plan(&fake, dir.path(), &target, &lock, &wanted(&pack), &[])
            .await
            .is_err()
    );
    // A data pack needs an existing world with a plain name.
    assert!(
        content::plan(&fake, dir.path(), &target, &lock, &wanted(&data), &[])
            .await
            .is_err()
    );
    for world in ["../x", "nope"] {
        let w = Wanted {
            version: &data,
            world: Some(world),
            replaces: None,
        };
        assert!(
            content::plan(&fake, dir.path(), &target, &lock, &w, &[])
                .await
                .is_err(),
            "{world}"
        );
    }
    fs::create_dir_all(dir.path().join("saves/World")).unwrap();
    let w = Wanted {
        version: &data,
        world: Some("World"),
        replaces: None,
    };
    let plan = content::plan(&fake, dir.path(), &target, &lock, &w, &[])
        .await
        .unwrap();
    assert_eq!(plan.items[0].path, "saves/World/datapacks/data.zip");
    assert!(
        content::plan(&fake, dir.path(), &target, &lock, &wanted(&bad), &[])
            .await
            .is_err()
    );
}

#[tokio::test]
async fn a_mods_data_pack_version_goes_into_a_world() {
    let mut fake = Fake::default();
    fake.add_project(1, "mod", "Terralith");
    let jar = fake.add_version(10, 1, "terralith.jar", json!({}));
    let zip = fake.add_version(11, 1, "terralith.zip", json!({"loaders": ["datapack"]}));
    assert_eq!(
        content::install_kind("mod", &fake.versions[&zip]),
        "datapack"
    );
    assert_eq!(content::install_kind("mod", &fake.versions[&jar]), "mod");
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("saves/World")).unwrap();
    touch(dir.path(), "mods/terralith.jar");
    let lock = ContentLock {
        schema_version: 1,
        items: vec![locked("mods/terralith.jar", 1, 10)],
        pack: None,
        published: None,
    };
    // A world to go into is needed, as for any data pack.
    assert!(
        content::plan(&fake, dir.path(), &fabric(), &lock, &wanted(&zip), &[])
            .await
            .is_err()
    );
    let w = Wanted {
        version: &zip,
        world: Some("World"),
        replaces: None,
    };
    let plan = content::plan(&fake, dir.path(), &fabric(), &lock, &w, &[])
        .await
        .unwrap();
    assert!(plan.ready());
    assert_eq!(plan.items[0].path, "saves/World/datapacks/terralith.zip");
    assert_eq!(plan.items[0].kind, "datapack");
    // The mod stays.
    assert_eq!(plan.items[0].replaces, None);
    // And on a vanilla installation, where the mod could not run.
    let vanilla = Target {
        game: "1.21.1".to_owned(),
        loader: None,
        loader_version: None,
    };
    let plan = content::plan(&fake, dir.path(), &vanilla, &lock, &w, &[])
        .await
        .unwrap();
    assert_eq!(plan.misfit, None);

    // Updates: the data pack to the next data pack, the mod to the next jar.
    let newer_zip = fake.add_version(12, 1, "terralith-2.zip", json!({"loaders": ["datapack"]}));
    let newer_jar = fake.add_version(13, 1, "terralith-2.jar", json!({}));
    touch(dir.path(), "saves/World/datapacks/terralith.zip");
    let mut pack = locked("saves/World/datapacks/terralith.zip", 1, 11);
    pack.kind = "datapack".to_owned();
    let lock = ContentLock {
        schema_version: 1,
        items: vec![locked("mods/terralith.jar", 1, 10), pack],
        pack: None,
        published: None,
    };
    let offers = content::updates(&fake, &fabric(), &lock, &[], dir.path())
        .await
        .unwrap();
    let to = |path: &str| {
        offers
            .iter()
            .find(|o| o.path == path)
            .map(|o| o.to_version_id.clone())
    };
    assert_eq!(to("mods/terralith.jar"), Some(newer_jar));
    assert_eq!(to("saves/World/datapacks/terralith.zip"), Some(newer_zip));
}

#[tokio::test]
async fn a_data_pack_in_one_world_leaves_another_worlds_be() {
    let mut fake = Fake::default();
    fake.add_project(2, "datapack", "Data");
    let data = fake.add_version(11, 2, "data.zip", json!({"loaders": ["datapack"]}));
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("saves/B")).unwrap();
    touch(dir.path(), "saves/A/datapacks/data.zip");
    let mut in_a = locked("saves/A/datapacks/data.zip", 2, 11);
    in_a.kind = "datapack".to_owned();
    let lock = ContentLock {
        schema_version: 1,
        items: vec![in_a],
        pack: None,
        published: None,
    };
    let w = Wanted {
        version: &data,
        world: Some("B"),
        replaces: None,
    };
    let plan = content::plan(&fake, dir.path(), &fabric(), &lock, &w, &[])
        .await
        .unwrap();
    assert_eq!(plan.items[0].path, "saves/B/datapacks/data.zip");
    assert_eq!(plan.items[0].replaces, None);
}

#[tokio::test]
async fn files_of_the_player_are_never_overwritten() {
    let mut fake = Fake::default();
    fake.add_project(1, "mod", "A");
    let v = fake.add_version(10, 1, "a.jar", json!({}));
    let dir = tempfile::tempdir().unwrap();
    touch(dir.path(), "mods/a.jar");
    let lock = ContentLock::default();
    let plan = content::plan(&fake, dir.path(), &fabric(), &lock, &wanted(&v), &[])
        .await
        .unwrap();
    assert_eq!(plan.conflicts.len(), 1);
    assert_eq!(plan.conflicts[0].reason, "name_taken");
    // Unless the player chose to update that file.
    let w = Wanted {
        version: &v,
        world: None,
        replaces: Some("mods/a.jar"),
    };
    let plan = content::plan(&fake, dir.path(), &fabric(), &lock, &w, &[])
        .await
        .unwrap();
    assert!(plan.ready());
    assert_eq!(plan.items[0].replaces.as_deref(), Some("mods/a.jar"));
    // The file to replace must be content.
    let w = Wanted {
        version: &v,
        world: None,
        replaces: Some("../options.txt"),
    };
    assert!(
        content::plan(&fake, dir.path(), &fabric(), &lock, &w, &[])
            .await
            .is_err()
    );
}

#[tokio::test]
async fn an_update_replaces_the_launchers_own_file() {
    let mut fake = Fake::default();
    fake.add_project(1, "mod", "A");
    fake.add_version(10, 1, "a-1.jar", json!({}));
    let new = fake.add_version(11, 1, "a-2.jar", json!({}));
    let dir = tempfile::tempdir().unwrap();
    touch(dir.path(), "mods/a-1.jar.disabled");
    let lock = ContentLock {
        schema_version: 1,
        items: vec![locked("mods/a-1.jar", 1, 10)],
        pack: None,
        published: None,
    };
    let plan = content::plan(&fake, dir.path(), &fabric(), &lock, &wanted(&new), &[])
        .await
        .unwrap();
    assert!(plan.ready());
    assert_eq!(plan.items[0].replaces.as_deref(), Some("mods/a-1.jar"));

    let offers = content::updates(&fake, &fabric(), &lock, &[], dir.path())
        .await
        .unwrap();
    assert_eq!(offers.len(), 1);
    assert_eq!(offers[0].path, "mods/a-1.jar.disabled");
    assert_eq!(offers[0].to_version_id, new);
}

#[tokio::test]
async fn the_players_files_are_recognized_by_hash() {
    let mut fake = Fake::default();
    fake.add_project(1, "mod", "A");
    let v = fake.add_version(10, 1, "a.jar", json!({}));
    let files = vec![
        ("mods/mine.jar".to_owned(), hash(10)),
        ("mods/other.jar".to_owned(), hash(99)),
    ];
    let found = content::identify(&fake, &files).await.unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].path, "mods/mine.jar");
    assert_eq!(found[0].version_id, v);
}

// ----- the game folder -----

fn instance() -> (tempfile::TempDir, DataRoot, InstanceId) {
    let dir = tempfile::tempdir().unwrap();
    let root = DataRoot::new(dir.path().to_path_buf()).unwrap();
    let id = InstanceId::generate();
    fs::create_dir_all(root.game_dir(id)).unwrap();
    (dir, root, id)
}

#[test]
fn the_content_view_lists_only_content() {
    let (_dir, root, id) = instance();
    let game = root.game_dir(id);
    touch(&game, "mods/a.jar");
    touch(&game, "mods/b.jar.disabled");
    touch(&game, "mods/notes.txt");
    touch(&game, "mods/.hidden.jar");
    touch(&game, "resourcepacks/pack.zip");
    fs::create_dir_all(game.join("shaderpacks/BSL")).unwrap();
    fs::create_dir_all(game.join("saves/World")).unwrap();
    let lock = ContentLock {
        schema_version: 1,
        items: vec![locked("mods/b.jar", 1, 10)],
        pack: None,
        published: None,
    };
    crate::storage::document::save(
        &root.instance_dir(id).join("content.lock.json"),
        &lock,
        None,
    )
    .unwrap();
    let view = content::list(&root, id);
    let paths: Vec<&str> = view.items.iter().map(|i| i.path.as_str()).collect();
    assert_eq!(
        paths,
        [
            "mods/a.jar",
            "mods/b.jar.disabled",
            "resourcepacks/pack.zip",
            "shaderpacks/BSL"
        ]
    );
    let b = &view.items[1];
    assert!(!b.enabled);
    assert_eq!(b.name, "b.jar");
    assert_eq!(b.title.as_deref(), Some("Project 1"));
    assert!(view.items[3].directory);
    assert_eq!(view.worlds, ["World"]);
}

#[test]
fn installed_content_is_found_across_installations() {
    let (_dir, root, first) = instance();
    let second = InstanceId::generate();
    touch(&root.game_dir(first), "mods/sodium-fabric-0.6.jar");
    touch(&root.game_dir(first), "mods/lithium.jar.disabled");
    touch(&root.game_dir(second), "mods/b.jar");
    touch(&root.game_dir(second), "shaderpacks/Sodium Shaders.zip");
    let lock = ContentLock {
        schema_version: 1,
        items: vec![locked("mods/b.jar", 1, 10)],
        pack: None,
        published: None,
    };
    crate::storage::document::save(
        &root.instance_dir(second).join("content.lock.json"),
        &lock,
        None,
    )
    .unwrap();
    let instances = [(first, "Fabric".to_owned()), (second, "Forge".to_owned())];
    let find = |text: &str, project: Option<&str>| -> Vec<(String, String)> {
        let query = content::InstalledQuery::new(text, project)
            .unwrap()
            .unwrap();
        content::find_installed(&root, &instances, &query)
            .into_iter()
            .map(|m| (m.instance_name, m.item.path))
            .collect()
    };
    let pair = |a: &str, b: &str| (a.to_owned(), b.to_owned());
    assert_eq!(
        find("SODIUM", None),
        [
            pair("Fabric", "mods/sodium-fabric-0.6.jar"),
            pair("Forge", "shaderpacks/Sodium Shaders.zip")
        ]
    );
    // Every word, in the title from the catalog or the file name.
    assert_eq!(
        find("sodium fabric", None),
        [pair("Fabric", "mods/sodium-fabric-0.6.jar")]
    );
    assert_eq!(find("project 1", None), [pair("Forge", "mods/b.jar")]);
    assert_eq!(
        find("lithium", None),
        [pair("Fabric", "mods/lithium.jar.disabled")]
    );
    // A catalog project wherever it is installed.
    let one = id(1);
    assert_eq!(find("", Some(&one)), [pair("Forge", "mods/b.jar")]);
    assert!(find("sodium", Some(&one)).is_empty());
    // Nothing to look for, or a project that cannot be one.
    assert_eq!(content::InstalledQuery::new("  \t ", None).unwrap(), None);
    assert!(content::InstalledQuery::new("x", Some("../a")).is_err());
    assert!(content::InstalledQuery::new(&"x ".repeat(10_000), None).is_ok());
}

#[test]
fn files_are_turned_off_on_and_removed() {
    let (_dir, root, id) = instance();
    let game = root.game_dir(id);
    touch(&game, "mods/a.jar");
    content::set_enabled(&root, id, "mods/a.jar", false).unwrap();
    assert!(game.join("mods/a.jar.disabled").is_file());
    // Asking again changes nothing.
    content::set_enabled(&root, id, "mods/a.jar.disabled", false).unwrap();
    content::set_enabled(&root, id, "mods/a.jar.disabled", true).unwrap();
    assert!(game.join("mods/a.jar").is_file());
    // A name already taken is not overwritten.
    touch(&game, "mods/a.jar.disabled");
    assert!(content::set_enabled(&root, id, "mods/a.jar", false).is_err());
    assert!(content::set_enabled(&root, id, "mods/none.jar", false).is_err());
    assert!(content::set_enabled(&root, id, "../options.txt", false).is_err());

    let lock = ContentLock {
        schema_version: 1,
        items: vec![locked("mods/a.jar", 1, 10)],
        pack: None,
        published: None,
    };
    crate::storage::document::save(
        &root.instance_dir(id).join("content.lock.json"),
        &lock,
        None,
    )
    .unwrap();
    content::remove(&root, id, "mods/a.jar").unwrap();
    assert!(!game.join("mods/a.jar").exists());
    assert!(content::load_lock(&root, id).items.is_empty());
    // Folders and anything outside stay.
    fs::create_dir_all(game.join("shaderpacks/BSL")).unwrap();
    assert!(content::remove(&root, id, "shaderpacks/BSL").is_err());
    touch(&game, "options.txt");
    assert!(content::remove(&root, id, "options.txt").is_err());
    assert!(game.join("options.txt").is_file());
}

#[cfg(unix)]
#[test]
fn links_are_not_followed() {
    let (dir, root, id) = instance();
    let game = root.game_dir(id);
    let outside = dir.path().join("outside.jar");
    fs::write(&outside, b"keep").unwrap();
    fs::create_dir_all(game.join("mods")).unwrap();
    std::os::unix::fs::symlink(&outside, game.join("mods/link.jar")).unwrap();
    assert!(content::list(&root, id).items.is_empty());
    assert!(content::remove(&root, id, "mods/link.jar").is_err());
    assert!(content::set_enabled(&root, id, "mods/link.jar", false).is_err());
    assert!(outside.is_file());
}

fn known(path: &str, project: u8, version: u8) -> content::Identified {
    content::Identified {
        path: path.to_owned(),
        project_id: id(project),
        version_id: id(version),
    }
}

#[tokio::test]
async fn what_is_there_already_is_used_and_turned_on_when_off() {
    let mut fake = Fake::default();
    for (n, title) in [
        (1, "A"),
        (2, "Library"),
        (3, "Off library"),
        (4, "Ours off"),
        (5, "Enemy"),
    ] {
        fake.add_project(n, "mod", title);
    }
    let chosen = fake.add_version(
        10,
        1,
        "a.jar",
        json!({"dependencies": [
            {"kind": "required", "projectId": id(2)},
            {"kind": "required", "projectId": id(3)},
            {"kind": "required", "projectId": id(4)},
        ]}),
    );
    for (v, p) in [(20, 2), (30, 3), (40, 4)] {
        fake.add_version(v, p, &format!("lib-{p}.jar"), json!({}));
    }
    let dir = tempfile::tempdir().unwrap();
    let game = dir.path();
    // The player's own copies, one turned off; one the launcher put there,
    // turned off; and the player's older copy of the chosen mod.
    touch(game, "mods/my-library.jar");
    touch(game, "mods/my-off-library.jar.disabled");
    touch(game, "mods/lib-4.jar.disabled");
    touch(game, "mods/a-old.jar");
    let lock = ContentLock {
        schema_version: 1,
        items: vec![locked("mods/lib-4.jar", 4, 40)],
        pack: None,
        published: None,
    };
    let identified = [
        known("mods/my-library.jar", 2, 21),
        known("mods/my-off-library.jar.disabled", 3, 31),
        known("mods/a-old.jar", 1, 11),
    ];
    let plan = content::plan(&fake, game, &fabric(), &lock, &wanted(&chosen), &identified)
        .await
        .unwrap();
    assert!(plan.ready(), "{plan:?}");
    // Nothing brought twice; the old copy of the chosen mod goes.
    let names: Vec<&str> = plan.items.iter().map(|i| i.file_name.as_str()).collect();
    assert_eq!(names, ["a.jar"]);
    assert_eq!(plan.items[0].replaces.as_deref(), Some("mods/a-old.jar"));
    let on: Vec<&str> = plan.turn_on.iter().map(|t| t.path.as_str()).collect();
    assert_eq!(
        on,
        [
            "mods/my-off-library.jar.disabled",
            "mods/lib-4.jar.disabled"
        ]
    );

    // A project that does not work with the chosen one is seen in the
    // player's files too.
    let enemy = fake.add_version(
        12,
        1,
        "a2.jar",
        json!({"dependencies": [{"kind": "incompatible", "projectId": id(5)}]}),
    );
    touch(game, "mods/enemy.jar");
    let plan = content::plan(
        &fake,
        game,
        &fabric(),
        &ContentLock::default(),
        &wanted(&enemy),
        &[known("mods/enemy.jar", 5, 50)],
    )
    .await
    .unwrap();
    assert_eq!(plan.conflicts.len(), 1, "{plan:?}");
    assert_eq!(plan.conflicts[0].reason, "incompatible");
    assert_eq!(plan.conflicts[0].path, "mods/enemy.jar");
}

#[tokio::test]
async fn an_install_turns_on_what_it_needs() {
    let mut fake = Fake::default();
    fake.add_project(1, "mod", "A");
    fake.add_project(2, "mod", "Library");
    let chosen = fake.add_version(
        10,
        1,
        "a.jar",
        json!({"dependencies": [{"kind": "required", "projectId": id(2)}]}),
    );
    fake.add_version(20, 2, "lib.jar", json!({}));
    let (_dir, root, instance_id) = instance();
    let game = root.game_dir(instance_id);
    // The chosen file is there already, so nothing is downloaded.
    touch(&game, "mods/a.jar");
    touch(&game, "mods/lib.jar.disabled");
    let mut lock = ContentLock {
        schema_version: 1,
        items: vec![locked("mods/a.jar", 1, 10), locked("mods/lib.jar", 2, 20)],
        pack: None,
        published: None,
    };
    lock.items[1].dependency = true;
    content::save_lock(&root, instance_id, &lock).unwrap();
    let plan = content::plan(&fake, &game, &fabric(), &lock, &wanted(&chosen), &[])
        .await
        .unwrap();
    assert!(plan.items[0].present && plan.total_bytes == 0, "{plan:?}");
    let installer = content::Installer::new("0.0.0-test").unwrap();
    installer
        .install(
            &fake,
            &root,
            instance_id,
            plan,
            &crate::download::CancelToken::new(),
            &|_| {},
        )
        .await
        .unwrap();
    assert!(game.join("mods/lib.jar").is_file());
    assert!(!game.join("mods/lib.jar.disabled").exists());
    // The lock now says what the chosen mod requires.
    let lock = content::load_lock(&root, instance_id);
    let a = lock.items.iter().find(|i| i.path == "mods/a.jar").unwrap();
    assert_eq!(a.requires.as_deref(), Some(&[id(2)][..]));
}

#[tokio::test]
async fn turning_off_or_removing_a_mod_says_who_needs_it_and_what_goes_unused() {
    let (_dir, root, instance_id) = instance();
    let game = root.game_dir(instance_id);
    let mut items = Vec::new();
    // (path, project, requires, dependency, on)
    for (path, n, requires, dependency, on) in [
        ("mods/a.jar", 1, vec![2, 5], false, true),
        ("mods/c.jar", 3, vec![2], false, true),
        ("mods/b.jar", 2, vec![], true, true),
        ("mods/e.jar", 5, vec![6], true, true),
        ("mods/f.jar", 6, vec![], true, true),
        ("mods/g.jar", 7, vec![8], false, false),
        ("mods/h.jar", 8, vec![], true, true),
    ] {
        touch(
            &game,
            &if on {
                path.to_owned()
            } else {
                format!("{path}.disabled")
            },
        );
        let mut item = locked(path, n, 100 + n);
        item.requires = Some(requires.into_iter().map(id).collect());
        item.dependency = dependency;
        items.push(item);
    }
    content::save_lock(
        &root,
        instance_id,
        &ContentLock {
            schema_version: 1,
            items,
            pack: None,
            published: None,
        },
    )
    .unwrap();
    // The player's own mod, which the catalog knows requires B.
    touch(&game, "mods/mine.jar");
    let mut fake = Fake::default();
    fake.add_version(
        90,
        9,
        "mine.jar",
        json!({"dependencies": [{"kind": "required", "projectId": id(2)}]}),
    );
    let mine = [known("mods/mine.jar", 9, 90)];
    let installer = content::Installer::new("0.0.0-test").unwrap();
    let ask = |path: &'static str, online: bool| {
        let (installer, root, fake, mine) = (&installer, &root, &fake, &mine);
        async move {
            installer
                .needed_by(
                    online.then_some(fake),
                    root,
                    instance_id,
                    if online { mine } else { &[] },
                    path,
                )
                .await
        }
    };

    let b = ask("mods/b.jar", true).await;
    assert_eq!(b.needed_by, ["Project 1", "Project 3", "mine.jar"]);
    // Without the catalog, what the launcher noted.
    assert_eq!(
        ask("mods/b.jar", false).await.needed_by,
        ["Project 1", "Project 3"]
    );
    // Removing A leaves E and F unused; B is still needed by C.
    let a = ask("mods/a.jar", true).await;
    assert!(a.needed_by.is_empty());
    let unused: Vec<&str> = a.unused.iter().map(|u| u.path.as_str()).collect();
    assert_eq!(unused, ["mods/e.jar", "mods/f.jar"]);
    assert_eq!(ask("mods/f.jar", true).await.needed_by, ["Project 5"]);
    // A mod turned off needs nothing now, but what it needs stays for it.
    assert!(ask("mods/h.jar", true).await.needed_by.is_empty());
    let g = ask("mods/g.jar.disabled", true).await;
    assert_eq!(g.unused.len(), 1);
    assert_eq!(g.unused[0].path, "mods/h.jar");
    // Something the catalog does not know.
    touch(&game, "mods/unknown.jar");
    assert_eq!(
        ask("mods/unknown.jar", true).await,
        content::NeededBy::default()
    );
}

#[test]
fn one_change_per_installation_at_a_time() {
    let installer = content::Installer::new("0.0.0-test").unwrap();
    let id = InstanceId::generate();
    let claim = installer.claim(id).unwrap();
    assert!(installer.claim(id).is_err());
    assert!(installer.ensure_idle(id).is_err());
    assert!(installer.claim(InstanceId::generate()).is_ok());
    assert!(installer.cancel(id));
    assert!(claim.cancel.is_cancelled());
    drop(claim);
    assert!(installer.ensure_idle(id).is_ok());
    assert!(!installer.cancel(id));
}

// ----- modpacks -----

mod packs {
    use std::io::Write as _;

    use super::*;
    use crate::download::CancelToken;
    use crate::mods::pack::{Fate, PackFile, PackInfo, PackMeta, fate, pack_path};

    fn sha(bytes: &[u8]) -> String {
        use sha2::Digest as _;
        hex::encode(sha2::Sha256::digest(bytes))
    }

    fn url_of(bytes: &[u8], name: &str) -> String {
        format!("https://files-mods.lapi.sh/f/{}/{name}", sha(bytes))
    }

    fn file_json(path: &str, bytes: &[u8]) -> Value {
        let name = path.rsplit('/').next().unwrap();
        json!({ "path": path, "size": bytes.len(), "sha256": sha(bytes), "url": url_of(bytes, name), "optional": false })
    }

    fn info(files: Vec<Value>, archive: &[u8], loader: Value) -> Value {
        json!({
            "state": "ready", "error": null, "game": "1.20.1", "loader": loader, "skipped": 1,
            "files": files, "kept": 0, "total": 0,
            "archive": { "name": "p.mrpack", "size": archive.len(), "sha256": sha(archive), "url": url_of(archive, "p.mrpack") },
        })
    }

    fn mrpack(entries: &[(&str, &[u8])], link: Option<&str>) -> Vec<u8> {
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        zip.start_file("modrinth.index.json", options).unwrap();
        zip.write_all(b"{}").unwrap();
        zip.add_directory("overrides/config/", options).unwrap();
        for (name, bytes) in entries {
            zip.start_file(*name, options).unwrap();
            zip.write_all(bytes).unwrap();
        }
        if let Some(link) = link {
            zip.add_symlink(link, "/etc/passwd", options).unwrap();
        }
        zip.finish().unwrap().into_inner()
    }

    fn put(path: &Path, bytes: &[u8]) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }

    fn meta(version: u8) -> PackMeta {
        PackMeta {
            project_id: id(1),
            version_id: id(version),
            title: "Pack".to_owned(),
            version_number: format!("{version}.0"),
            icon: None,
        }
    }

    #[test]
    fn pack_paths_match_the_catalogs_rules() {
        for path in [
            "mods/a.jar",
            "mods/a.jar.disabled",
            "config/a/b.toml",
            "kubejs/startup_scripts/x.js",
            "options.txt",
            "saves/My World/level.dat",
        ] {
            assert!(pack_path(path), "{path}");
        }
        for path in [
            "",
            "../a",
            "mods/../a.jar",
            "/mods/a.jar",
            "mods\\a.jar",
            "C:/a",
            "mods/aux.jar",
            "mods/a.dll",
            "config/a.jar",
            "logs/x.log",
            "content.lock.json",
            "mods/a\u{7}.jar",
            "mods/a. ",
        ] {
            assert!(!pack_path(path), "{path:?}");
        }
    }

    #[test]
    fn the_catalogs_answer_is_checked() {
        let jar = b"jar".as_slice();
        let archive = b"zip".as_slice();
        let good = PackInfo::from(&info(
            vec![file_json("mods/a.jar", jar)],
            archive,
            json!({ "kind": "forge", "version": "47.2.0" }),
        ));
        assert!(good.ready(), "{good:?}");
        assert_eq!(good.mods, 1);
        assert_eq!(good.total_bytes, 6);
        let choice = good.loader_choice().unwrap().unwrap();
        assert_eq!(choice.kind, LoaderKind::Forge);
        assert_eq!(choice.version, "1.20.1-47.2.0");
        let neo = PackInfo::from(&info(
            vec![],
            archive,
            json!({ "kind": "neoforge", "version": "20.1.5" }),
        ));
        assert_eq!(neo.loader_choice().unwrap().unwrap().version, "20.1.5");
        assert_eq!(
            PackInfo::from(&info(vec![], archive, Value::Null))
                .loader_choice()
                .unwrap(),
            None
        );
        // Still being prepared: no addresses yet, and that is fine.
        let working = PackInfo::from(&json!({
            "state": "working", "files": [{ "path": "mods/a.jar", "size": 3, "sha256": null, "url": null }],
            "archive": { "name": "p.mrpack", "size": 3, "sha256": null, "url": null },
        }));
        assert_eq!(working.state, "working");
        assert_eq!((working.kept, working.total), (0, 1));
        // A CurseForge pack whose files are still being found: counted, not
        // listed yet; a count past the limit is not believed.
        let finding = PackInfo::from(&json!({
            "state": "working", "total": 40, "files": [], "archive": {},
        }));
        assert_eq!((finding.kept, finding.total), (0, 40));
        let huge = PackInfo::from(&json!({
            "state": "working", "total": u64::MAX, "files": [], "archive": {},
        }));
        assert_eq!(huge.total, 5000);

        let mut bad_path = file_json("mods/a.jar", jar);
        bad_path["path"] = json!("../../evil.jar");
        let mut elsewhere = file_json("mods/a.jar", jar);
        elsewhere["url"] = json!(format!("https://evil.example/f/{}/a.jar", sha(jar)));
        let mut other_hash = file_json("mods/a.jar", jar);
        other_hash["url"] = json!(url_of(b"other", "a.jar"));
        let mut not_kept = file_json("mods/a.jar", jar);
        not_kept["sha256"] = Value::Null;
        let twice = vec![file_json("mods/a.jar", jar), file_json("mods/A.jar", jar)];
        for (files, loader) in [
            (vec![bad_path], Value::Null),
            (vec![elsewhere], Value::Null),
            (vec![other_hash], Value::Null),
            (vec![not_kept], Value::Null),
            (twice, Value::Null),
            (vec![], json!({ "kind": "rift", "version": "1" })),
            (
                vec![],
                json!({ "kind": "fabric", "version": "0.15; rm -rf /" }),
            ),
        ] {
            let read = PackInfo::from(&info(files.clone(), archive, loader.clone()));
            assert_eq!(read.state, "failed", "{files:?} {loader}");
            assert_eq!(read.error.as_deref(), Some("bad_pack"));
            assert!(!read.ready());
        }
        // A pack with a file the catalog holds back fails with that reason.
        let mut held = file_json("mods/a.jar", jar);
        held["url"] = Value::Null;
        held["held"] = json!(true);
        let mut answer = info(vec![held], archive, Value::Null);
        answer["state"] = json!("failed");
        answer["error"] = json!("held");
        let read = PackInfo::from(&answer);
        assert_eq!(read.error.as_deref(), Some("held"));
        assert!(!read.ready());

        let mut wrong_game = info(vec![], archive, Value::Null);
        wrong_game["game"] = json!("1.20.1/../../x");
        assert_eq!(PackInfo::from(&wrong_game).state, "failed");
        let mut no_archive = info(vec![], archive, Value::Null);
        no_archive["archive"] = json!({});
        assert_eq!(PackInfo::from(&no_archive).state, "failed");
        assert_eq!(PackInfo::from(&json!("junk")).state, "failed");
    }

    #[test]
    fn a_file_is_replaced_only_while_it_is_as_the_pack_left_it() {
        let dir = tempfile::tempdir().unwrap();
        let game = dir.path();
        let file = |path: &str, bytes: &[u8]| PackFile {
            path: path.to_owned(),
            size: bytes.len() as u64,
            sha256: Some(sha(bytes)),
            url: None,
        };
        let old = sha(b"old");
        assert_eq!(
            fate(game, &file("mods/a.jar", b"new"), None),
            Fate::Fetch { off: false }
        );
        put(&game.join("mods/a.jar"), b"new");
        assert_eq!(fate(game, &file("mods/a.jar", b"new"), None), Fate::Have);
        put(&game.join("mods/b.jar.disabled"), b"old");
        assert_eq!(
            fate(game, &file("mods/b.jar", b"new"), Some(&old)),
            Fate::Fetch { off: true }
        );
        put(&game.join("mods/c.jar"), b"the player's");
        assert_eq!(
            fate(game, &file("mods/c.jar", b"new"), Some(&old)),
            Fate::Yours
        );
        fs::create_dir_all(game.join("mods/d.jar")).unwrap();
        assert_eq!(fate(game, &file("mods/d.jar", b"new"), None), Fate::Yours);
    }

    #[tokio::test]
    async fn a_pack_is_applied_then_updated_around_the_players_changes() {
        let dir = tempfile::tempdir().unwrap();
        let root = DataRoot::new(dir.path().to_path_buf()).unwrap();
        let id = InstanceId::generate();
        let game = root.game_dir(id);
        let installer = content::Installer::new("0.0.0-test").unwrap();
        let cancel = CancelToken::new();
        let seen = std::sync::Mutex::new(Vec::new());
        let emit = |p: content::Progress| seen.lock().unwrap().push(p.phase);

        // Version 1: two mods, configs, options, and what must stay out.
        let archive = mrpack(
            &[
                ("overrides/config/x.toml", b"x = 1"),
                ("client-overrides/config/x.toml", b"x = client"),
                ("overrides/options.txt", b"fov:70"),
                // The pack's own fix of a file it lists.
                ("overrides/mods/b.jar", b"B1 fixed"),
                ("overrides/../escape.txt", b"no"),
                ("overrides/run.exe", b"MZ"),
                ("overrides/mods/native.dll", b"MZ"),
                ("server-overrides/config/s.toml", b"server"),
            ],
            Some("overrides/config/link"),
        );
        let v1 = PackInfo::from(&info(
            vec![
                file_json("mods/a.jar", b"A1"),
                file_json("mods/b.jar", b"B1"),
            ],
            &archive,
            json!({ "kind": "fabric", "version": "0.16.5" }),
        ));
        assert!(v1.ready());
        // The executor finds them in place (nothing is downloaded in a test).
        put(&game.join("mods/a.jar"), b"A1");
        put(&game.join("mods/b.jar"), b"B1");
        put(&root.instance_dir(id).join("pack/archive.mrpack"), &archive);
        let report = installer
            .apply_pack(&root, id, &v1, meta(10), &cancel, &emit)
            .await
            .unwrap();
        assert_eq!(report.files, 5, "{report:?}");
        assert_eq!(fs::read(game.join("mods/b.jar")).unwrap(), b"B1 fixed");
        assert_eq!(report.kept_yours, 0);
        // One from the catalog, four here.
        assert_eq!(report.left_out, 5, "{report:?}");
        assert_eq!(fs::read(game.join("config/x.toml")).unwrap(), b"x = client");
        assert_eq!(fs::read(game.join("options.txt")).unwrap(), b"fov:70");
        assert!(!dir.path().join("instances").join("escape.txt").exists());
        assert!(!game.join("run.exe").exists());
        assert!(!game.join("mods/native.dll").exists());
        assert!(!game.join("config/s.toml").exists());
        assert!(!game.join("config/link").exists());
        assert!(!root.instance_dir(id).join("pack").exists());
        let lock = content::load_lock(&root, id);
        let pack = lock.pack.unwrap();
        assert_eq!(pack.version_id, super::id(10));
        assert_eq!(pack.files.len(), 2);
        assert_eq!(pack.overrides.len(), 3);
        assert!(seen.lock().unwrap().contains(&"done"));
        // The content view shows the pack and what it brought.
        let view = content::list(&root, id);
        assert_eq!(
            view.pack.as_ref().map(|p| p.version_number.as_str()),
            Some("10.0")
        );
        assert!(view.items.iter().all(|i| i.from_pack), "{:?}", view.items);
        put(&game.join("mods/mine.jar"), b"mine");
        let view = content::list(&root, id);
        assert!(
            !view
                .items
                .iter()
                .find(|i| i.name == "mine.jar")
                .unwrap()
                .from_pack
        );
        fs::remove_file(game.join("mods/mine.jar")).unwrap();

        // The player changes a config and turns a mod off.
        put(&game.join("config/x.toml"), b"x = mine");
        fs::rename(game.join("mods/b.jar"), game.join("mods/b.jar.disabled")).unwrap();

        // Version 2: b.jar and options.txt are gone, c.jar and a config are new.
        let archive = mrpack(
            &[
                ("overrides/config/x.toml", b"x = 2"),
                ("overrides/config/new.toml", b"new"),
            ],
            None,
        );
        let v2 = PackInfo::from(&info(
            vec![
                file_json("mods/a.jar", b"A1"),
                file_json("mods/c.jar", b"C1"),
            ],
            &archive,
            json!({ "kind": "fabric", "version": "0.16.9" }),
        ));
        put(&game.join("mods/c.jar"), b"C1");
        put(&root.instance_dir(id).join("pack/archive.mrpack"), &archive);
        let report = installer
            .apply_pack(&root, id, &v2, meta(11), &cancel, &emit)
            .await
            .unwrap();
        assert_eq!(report.kept_yours, 1, "{report:?}");
        assert_eq!(report.removed, 2, "{report:?}");
        assert_eq!(fs::read(game.join("config/x.toml")).unwrap(), b"x = mine");
        assert_eq!(fs::read(game.join("config/new.toml")).unwrap(), b"new");
        assert!(!game.join("mods/b.jar.disabled").exists());
        assert!(!game.join("options.txt").exists());
        let pack = content::load_lock(&root, id).pack.unwrap();
        assert_eq!(pack.version_id, super::id(11));
        // The pack's own config is recorded, so the next update still sees
        // the player's change.
        assert!(
            pack.overrides
                .iter()
                .any(|e| e.path == "config/x.toml" && e.sha256 == sha(b"x = 2"))
        );
    }

    fn read_zip(path: &Path) -> (Value, Vec<String>) {
        use std::io::Read as _;
        let mut zip = zip::ZipArchive::new(fs::File::open(path).unwrap()).unwrap();
        let mut text = String::new();
        zip.by_name("modrinth.index.json")
            .unwrap()
            .read_to_string(&mut text)
            .unwrap();
        let mut names: Vec<String> = zip.file_names().map(str::to_owned).collect();
        names.sort();
        (serde_json::from_str(&text).unwrap(), names)
    }

    #[tokio::test]
    async fn an_installation_is_exported_as_a_modrinth_pack() {
        use crate::mods::pack::{ExportChoice, ExportInfo, export};
        let dir = tempfile::tempdir().unwrap();
        let game = dir.path().join("game");
        put(&game.join("mods/known.jar"), b"known");
        put(&game.join("mods/own.jar"), b"own");
        put(&game.join("mods/off.jar.disabled"), b"off");
        put(&game.join("mods/run.exe"), b"MZ");
        put(&game.join("config/a.toml"), b"a = 1");
        put(&game.join("config/b.jar"), b"not here");
        put(&game.join("options.txt"), b"fov:70");
        put(&game.join("servers.dat"), b"private");
        put(&game.join("saves/World/level.dat"), b"level");
        put(&game.join("logs/latest.log"), b"log");
        #[cfg(unix)]
        std::os::unix::fs::symlink("/etc/passwd", game.join("config/link")).unwrap();

        // The catalog knows known.jar from Modrinth; a kept file without
        // an address there is copied like any other.
        let mut catalog = Fake::default();
        catalog.add_project(1, "mod", "Known");
        let modrinth = "https://cdn.modrinth.com/data/AAAA/versions/BBBB/known.jar".to_owned();
        let mut known = catalog.add_version(2, 1, "known.jar", json!({}));
        let version = catalog.versions.get_mut(&known).unwrap();
        version.files[0].sha256 = Some(sha(b"known"));
        version.files[0].origin = Some(modrinth.clone());
        known = catalog.add_version(3, 1, "own.jar", json!({}));
        catalog.versions.get_mut(&known).unwrap().files[0].sha256 = Some(sha(b"own"));

        let info = ExportInfo {
            name: "My \"pack\"".to_owned(),
            version_number: "1.0.0".to_owned(),
            game: "1.20.1".to_owned(),
            loader: Some(crate::loader::LoaderChoice {
                kind: LoaderKind::Forge,
                version: "1.20.1-47.2.0".to_owned(),
            }),
        };
        let to = dir.path().join("out.mrpack");
        let report = export(
            &catalog,
            &game,
            info.clone(),
            ExportChoice::default(),
            to.clone(),
            &CancelToken::new(),
        )
        .await
        .unwrap();
        assert_eq!((report.linked, report.bundled), (1, 2), "{report:?}");
        let (index, names) = read_zip(&to);
        assert_eq!(
            names,
            [
                "modrinth.index.json",
                "overrides/config/a.toml",
                "overrides/mods/own.jar"
            ]
        );
        assert_eq!(index["formatVersion"], 1);
        assert_eq!(index["name"], "My \"pack\"");
        assert_eq!(index["dependencies"]["minecraft"], "1.20.1");
        assert_eq!(index["dependencies"]["forge"], "47.2.0");
        let files = index["files"].as_array().unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0]["path"], "mods/known.jar");
        assert_eq!(files[0]["downloads"][0], modrinth.as_str());
        assert_eq!(files[0]["fileSize"], 5);
        assert_eq!(files[0]["hashes"]["sha512"].as_str().unwrap().len(), 128);
        assert!(!dir.path().join("out.mrpack.part").exists());

        // What the catalog reads back of it.
        assert!(crate::mods::pack::pack_path(
            files[0]["path"].as_str().unwrap()
        ));

        // Worlds and settings only when asked; never the server list.
        let report = export(
            &catalog,
            &game,
            info,
            ExportChoice {
                worlds: true,
                options: true,
            },
            to.clone(),
            &CancelToken::new(),
        )
        .await
        .unwrap();
        assert_eq!(report.bundled, 4, "{report:?}");
        let (_, names) = read_zip(&to);
        assert!(names.contains(&"overrides/saves/World/level.dat".to_owned()));
        assert!(names.contains(&"overrides/options.txt".to_owned()));
        assert!(
            !names
                .iter()
                .any(|n| n.contains("servers.dat") || n.contains("logs"))
        );
    }

    #[test]
    fn only_modrinths_cdn_is_named_in_an_export() {
        use crate::mods::pack::modrinth_download;
        assert!(modrinth_download(
            "https://cdn.modrinth.com/data/a/versions/b/x.jar"
        ));
        for bad in [
            "http://cdn.modrinth.com/x.jar",
            "https://cdn.modrinth.com.evil.example/x.jar",
            "https://user@cdn.modrinth.com/x.jar",
            "https://cdn.modrinth.com:8443/x.jar",
            "https://cdn.modrinth.com/a b.jar",
            "https://github.com/x.jar",
            "junk",
        ] {
            assert!(!modrinth_download(bad), "{bad}");
        }
    }

    #[tokio::test]
    async fn a_broken_archive_stops_the_pack() {
        let dir = tempfile::tempdir().unwrap();
        let root = DataRoot::new(dir.path().to_path_buf()).unwrap();
        let id = InstanceId::generate();
        let installer = content::Installer::new("0.0.0-test").unwrap();
        let junk = b"not a zip".to_vec();
        let pack = PackInfo::from(&info(vec![], &junk, Value::Null));
        put(&root.instance_dir(id).join("pack/archive.mrpack"), &junk);
        let error = installer
            .apply_pack(&root, id, &pack, meta(10), &CancelToken::new(), &|_| {})
            .await
            .unwrap_err();
        assert_eq!(
            error.code,
            crate::error::ErrorCode::UnsupportedVersionMetadata
        );
        assert!(content::load_lock(&root, id).pack.is_none());
        assert!(!root.instance_dir(id).join("pack").exists());
    }
}

#[tokio::test]
async fn projects_the_catalog_joined_are_followed_in_the_lock() {
    let mut fake = Fake::default();
    fake.add_project(1, "mod", "Sodium Extra");
    fake.add_project(2, "mod", "Sodium");
    let chosen = fake.add_version(
        10,
        1,
        "extra.jar",
        json!({"dependencies": [{"kind": "required", "projectId": id(2)}]}),
    );
    // Installed when its project was another one (9), joined into 2 since.
    fake.add_version(21, 2, "sodium.jar", json!({}));
    let dir = tempfile::tempdir().unwrap();
    touch(dir.path(), "mods/sodium.jar");
    touch(dir.path(), "mods/other.jar");
    let mut other = locked("mods/other.jar", 5, 50);
    other.requires = Some(vec![id(9), id(7)]);
    let mut lock = ContentLock {
        items: vec![locked("mods/sodium.jar", 9, 21), other],
        ..ContentLock::default()
    };
    content::follow_joins(&fake, &mut lock).await;
    assert_eq!(lock.items[0].project_id, id(2));
    // Not in the catalog: as it was.
    assert_eq!(lock.items[1].project_id, id(5));
    assert_eq!(lock.items[1].requires, Some(vec![id(2), id(7)]));

    // So Sodium is there already, not brought again.
    let plan = content::plan(&fake, dir.path(), &fabric(), &lock, &wanted(&chosen), &[])
        .await
        .unwrap();
    let names: Vec<&str> = plan.items.iter().map(|i| i.file_name.as_str()).collect();
    assert_eq!(names, ["extra.jar"]);
}

// ----- maps -----

mod maps {
    use std::io::Write as _;

    use super::*;
    use crate::download::CancelToken;
    use crate::mods::world;

    fn sha(bytes: &[u8]) -> String {
        use sha2::Digest as _;
        hex::encode(sha2::Sha256::digest(bytes))
    }

    fn map_zip(entries: &[(&str, &[u8])], link: Option<&str>) -> Vec<u8> {
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        for (name, bytes) in entries {
            if name.ends_with('/') {
                zip.add_directory(*name, options).unwrap();
            } else {
                zip.start_file(*name, options).unwrap();
                zip.write_all(bytes).unwrap();
            }
        }
        if let Some(link) = link {
            zip.add_symlink(link, "/etc/passwd", options).unwrap();
        }
        zip.finish().unwrap().into_inner()
    }

    fn unpack(bytes: &[u8], saves: &Path, name: &str) -> LauncherResult<String> {
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("map.zip");
        fs::write(&archive, bytes).unwrap();
        world::unpack(&archive, saves, name, &CancelToken::new())
    }

    #[test]
    fn world_names_are_safe_and_never_taken() {
        assert_eq!(
            world::world_name("Sky Island: Remastered"),
            "Sky Island Remastered"
        );
        assert_eq!(world::world_name("../../etc"), "etc");
        assert_eq!(world::world_name("a\u{0}b\\c/d"), "a b c d");
        assert_eq!(world::world_name("CON"), "Map");
        assert_eq!(world::world_name("..."), "Map");
        assert_eq!(world::world_name(""), "Map");
        assert_eq!(world::world_name(&"x".repeat(300)).len(), 64);

        let dir = tempfile::tempdir().unwrap();
        assert_eq!(world::free_name(dir.path(), "Map"), "Map");
        fs::create_dir(dir.path().join("Map")).unwrap();
        fs::create_dir(dir.path().join(".Map (2).part")).unwrap();
        assert_eq!(world::free_name(dir.path(), "Map"), "Map (3)");
    }

    #[test]
    fn a_map_becomes_a_new_world_beside_the_players() {
        let dir = tempfile::tempdir().unwrap();
        let saves = dir.path().join("saves");
        // The world one folder down, with the junk zips carry.
        let bytes = map_zip(
            &[
                ("__MACOSX/Sky/level.dat", b"no"),
                ("Sky/", b""),
                ("Sky/level.dat", b"LEVEL"),
                ("Sky/region/r.0.0.mca", b"REGION"),
                ("Sky/../escape.txt", b"no"),
                ("Sky/CON/x", b"no"),
                ("Sky/a<b", b"no"),
                ("readme.txt", b"outside the world"),
            ],
            Some("Sky/link"),
        );
        fs::create_dir_all(saves.join("Sky")).unwrap();
        fs::write(saves.join("Sky/level.dat"), b"THE PLAYER'S").unwrap();
        let name = unpack(&bytes, &saves, "Sky").unwrap();
        assert_eq!(name, "Sky (2)");
        assert_eq!(
            fs::read(saves.join("Sky/level.dat")).unwrap(),
            b"THE PLAYER'S"
        );
        let world = saves.join("Sky (2)");
        assert_eq!(fs::read(world.join("level.dat")).unwrap(), b"LEVEL");
        assert_eq!(fs::read(world.join("region/r.0.0.mca")).unwrap(), b"REGION");
        assert!(!world.join("readme.txt").exists());
        assert!(!world.join("link").exists());
        assert!(!world.join("CON").exists());
        assert!(!saves.join("escape.txt").exists() && !dir.path().join("escape.txt").exists());
        // Nothing half-made is left beside it.
        let names: Vec<_> = fs::read_dir(&saves)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        assert_eq!(names.len(), 2, "{names:?}");

        // A world at the top of the zip.
        let top = map_zip(
            &[
                ("level.dat", b"TOP"),
                ("data/x.dat", b"X"),
                ("__MACOSX/._level.dat", b"junk"),
            ],
            None,
        );
        let name = unpack(&top, &saves, "Top").unwrap();
        assert_eq!(
            fs::read(saves.join(&name).join("data/x.dat")).unwrap(),
            b"X"
        );
        assert!(!saves.join(&name).join("__MACOSX").exists());
    }

    #[test]
    fn archives_without_a_world_are_refused_and_leave_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let saves = dir.path().join("saves");
        for bytes in [
            map_zip(&[("readme.txt", b"no world")], None),
            map_zip(&[("a/b/level.dat", b"too deep")], None),
            map_zip(&[("../level.dat", b"outside")], None),
            map_zip(&[("__MACOSX/level.dat", b"junk")], None),
            b"not a zip at all".to_vec(),
        ] {
            assert!(unpack(&bytes, &saves, "Map").is_err());
        }
        let left = fs::read_dir(&saves).map(|d| d.count()).unwrap_or(0);
        assert_eq!(left, 0);
    }

    fn catalog_with_map(archive: &[u8], name: &str, kind: &str) -> (Fake, String) {
        let mut fake = Fake::default();
        fake.add_project(1, kind, "Sky Island");
        let sha = sha(archive);
        let value = json!({
            "id": id(2), "projectId": id(1), "number": "1.0", "published": 1,
            "games": ["1.21.1"], "loaders": [],
            "files": [{"name": name, "size": archive.len(), "sha256": sha, "primary": true,
                       "url": format!("https://files-mods.lapi.sh/f/{sha}/{name}")}],
        });
        let version = Version::from(&value, true).unwrap();
        fake.versions.insert(version.id.clone(), version);
        (fake, id(2))
    }

    #[tokio::test]
    async fn only_a_maps_zip_is_planned() {
        let dir = tempfile::tempdir().unwrap();
        let archive = map_zip(&[("level.dat", b"L")], None);
        let (fake, version) = catalog_with_map(&archive, "sky.zip", "world");
        let plan = world::plan(&fake, dir.path(), &version).await.unwrap();
        assert_eq!(plan.name, "Sky Island");
        assert_eq!(plan.size, archive.len() as u64);
        assert!(plan.kept);

        let (fake, version) = catalog_with_map(&archive, "sky.zip", "mod");
        assert!(world::plan(&fake, dir.path(), &version).await.is_err());
        let (fake, version) = catalog_with_map(&archive, "sky.jar", "world");
        assert!(world::plan(&fake, dir.path(), &version).await.is_err());
    }

    #[tokio::test]
    async fn a_map_is_installed_and_its_zip_removed() {
        let dir = tempfile::tempdir().unwrap();
        let root = DataRoot::new(dir.path().to_path_buf()).unwrap();
        let id = InstanceId::generate();
        let installer = content::Installer::new("0.0.0-test").unwrap();
        let archive = map_zip(&[("Sky/level.dat", b"L"), ("Sky/icon.png", b"P")], None);
        let (fake, version) = catalog_with_map(&archive, "sky.zip", "world");
        // The executor finds it in place (nothing is downloaded in a test).
        let zip = root.instance_dir(id).join("pack/map.zip");
        fs::create_dir_all(zip.parent().unwrap()).unwrap();
        fs::write(&zip, &archive).unwrap();
        let seen = std::sync::Mutex::new(Vec::new());
        let emit = |p: content::Progress| seen.lock().unwrap().push(p.phase);
        let name = installer
            .install_world(&fake, &root, id, &version, &CancelToken::new(), &emit)
            .await
            .unwrap();
        assert_eq!(name, "Sky Island");
        let world = root.game_dir(id).join("saves/Sky Island");
        assert_eq!(fs::read(world.join("icon.png")).unwrap(), b"P");
        assert!(!zip.exists());
        let seen = seen.lock().unwrap();
        assert!(seen.contains(&"unpacking"), "{seen:?}");
        assert_eq!(seen.last(), Some(&"done"));
    }
}

// ----- installations from other launchers and folders -----

mod imports {
    use std::io::Write as _;

    use super::*;
    use crate::download::CancelToken;
    use crate::loader::meta::LoaderVersion;
    use crate::mods::import;

    fn zip_of(entries: &[(&str, &[u8])], link: Option<&str>) -> Vec<u8> {
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        for (name, bytes) in entries {
            zip.start_file(*name, options).unwrap();
            zip.write_all(bytes).unwrap();
        }
        if let Some(link) = link {
            zip.add_symlink(link, "/etc/passwd", options).unwrap();
        }
        zip.finish().unwrap().into_inner()
    }

    fn fabric_jar(minecraft: &str) -> Vec<u8> {
        let meta = json!({"schemaVersion": 1, "id": "x", "depends": {"minecraft": minecraft}});
        zip_of(&[("fabric.mod.json", meta.to_string().as_bytes())], None)
    }

    fn forge_jar(toml: &str) -> Vec<u8> {
        zip_of(&[("META-INF/mods.toml", toml.as_bytes())], None)
    }

    fn no_events(_: content::Progress) {}

    fn listing(dir: &Path) -> Vec<String> {
        let mut out = Vec::new();
        let mut stack = vec![dir.to_path_buf()];
        while let Some(d) = stack.pop() {
            for e in fs::read_dir(&d).unwrap() {
                let p = e.unwrap().path();
                if p.is_dir() {
                    stack.push(p);
                } else {
                    let rel = p
                        .strip_prefix(dir)
                        .unwrap()
                        .to_string_lossy()
                        .replace('\\', "/");
                    out.push(rel);
                }
            }
        }
        out.sort();
        out
    }

    #[test]
    fn versions_are_read_from_dependency_ranges() {
        for (range, want) in [
            ("~1.20.1", Some("1.20.1")),
            (">=1.20.1", Some("1.20.1")),
            ("[1.20.1,1.21)", Some("1.20.1")),
            ("1.20.x", Some("1.20")),
            ("[\"1.19.4\", \"1.20\"]", Some("1.19.4")),
            ("26.1.x", Some("26.1")),
            ("*", None),
            ("0.14.0", None),
            ("abc1.20", None),
        ] {
            assert_eq!(import::first_version(range).as_deref(), want, "{range}");
        }
        let toml = "modLoader=\"javafml\"\n[[dependencies.x]]\n modId=\"forge\"\n versionRange=\"[47,)\"\n[[dependencies.x]]\n modId = \"minecraft\"\n versionRange = \"[1.20.1,1.21)\"\n";
        assert_eq!(import::toml_game(toml).as_deref(), Some("1.20.1"));
        let (game, loader, hint) = import::prism_components(
            r#"{"components":[{"uid":"net.minecraft","version":"1.20.1"},{"uid":"net.minecraftforge","version":"47.2.0"},{"uid":"x","version":"../../x"}]}"#,
        );
        assert_eq!(game.as_deref(), Some("1.20.1"));
        assert_eq!(loader, Some(LoaderKind::Forge));
        assert_eq!(hint.as_deref(), Some("47.2.0"));
        let (game, loader, hint) = import::prism_components("not json");
        assert_eq!((game, loader, hint), (None, None, None));
    }

    #[test]
    fn loader_versions_follow_the_source_then_the_recommendation() {
        let v = |id: &str, label: &str, stable, recommended| LoaderVersion {
            id: id.to_owned(),
            label: label.to_owned(),
            stable,
            recommended,
        };
        let forge = [
            v("1.20.1-47.3.0", "47.3.0", true, true),
            v("1.20.1-47.2.0", "47.2.0", true, false),
        ];
        assert_eq!(
            import::pick_loader_version(&forge, Some("47.2.0")).as_deref(),
            Some("1.20.1-47.2.0")
        );
        assert_eq!(
            import::pick_loader_version(&forge, Some("1.0")).as_deref(),
            Some("1.20.1-47.3.0")
        );
        let fabric = [
            v("0.17.0-beta", "0.17.0-beta", false, false),
            v("0.16.5", "0.16.5", true, false),
        ];
        assert_eq!(
            import::pick_loader_version(&fabric, None).as_deref(),
            Some("0.16.5")
        );
        assert_eq!(import::pick_loader_version(&[], None), None);
    }

    #[test]
    fn a_prism_instance_comes_with_its_version_and_only_its_game_content() {
        let dir = tempfile::tempdir().unwrap();
        let jar = forge_jar("[[dependencies.x]]\nmodId=\"minecraft\"\nversionRange=\"[1.19.2,)\"");
        let bytes = zip_of(
            &[
                ("Cool/instance.cfg", b"InstanceType=OneSix\nname=Cool \x07Pack\n"),
                (
                    "Cool/mmc-pack.json",
                    br#"{"components":[{"uid":"net.minecraft","version":"1.20.1"},{"uid":"net.minecraftforge","version":"47.2.0"}]}"#,
                ),
                ("Cool/.minecraft/mods/a.jar", &jar),
                ("Cool/.minecraft/config/x.toml", b"x = 1"),
                ("Cool/.minecraft/options.txt", b"fov:70"),
                ("Cool/.minecraft/saves/W/level.dat", b"L"),
                ("Cool/.minecraft/logs/latest.log", b"log"),
                ("Cool/.minecraft/mods/run.exe", b"MZ"),
                ("Cool/.minecraft/config/../../escape.txt", b"no"),
                ("Cool/.minecraft/run.sh", b"#!"),
                ("Cool/libraries/x.jar", b"lib"),
                ("Cool/icon.png", b"P"),
            ],
            Some("Cool/.minecraft/config/link"),
        );
        let path = dir.path().join("export.zip");
        fs::write(&path, &bytes).unwrap();
        let picked = import::read(&path).unwrap();
        let found = &picked.found;
        assert_eq!(found.format, "prism");
        assert_eq!(found.name, "Cool Pack");
        assert_eq!(found.game.as_deref(), Some("1.20.1"));
        assert_eq!(found.loader, Some(LoaderKind::Forge));
        assert!(!found.guessed);
        assert_eq!(picked.loader_hint.as_deref(), Some("47.2.0"));
        assert_eq!((found.files, found.mods, found.left_out), (4, 1, 4));

        let game = dir.path().join("game");
        let id = InstanceId::generate();
        let report = import::copy(&picked, &game, id, &CancelToken::new(), &no_events).unwrap();
        assert_eq!((report.files, report.kept, report.left_out), (4, 0, 4));
        assert_eq!(
            listing(&game),
            [
                "config/x.toml",
                "mods/a.jar",
                "options.txt",
                "saves/W/level.dat"
            ]
        );
        assert_eq!(fs::read(game.join("mods/a.jar")).unwrap(), jar);
        assert!(!dir.path().join("escape.txt").exists());

        // Again (after a stop): what is there is kept.
        let report = import::copy(&picked, &game, id, &CancelToken::new(), &no_events).unwrap();
        assert_eq!((report.files, report.kept), (0, 4));
    }

    #[test]
    fn a_folder_of_mods_has_its_version_and_loader_guessed() {
        let dir = tempfile::tempdir().unwrap();
        let mods = dir.path().join("My mods");
        fs::create_dir(&mods).unwrap();
        fs::write(mods.join("a.jar"), fabric_jar("~1.20.1")).unwrap();
        fs::write(mods.join("b.jar"), fabric_jar(">=1.20.1")).unwrap();
        fs::write(mods.join("c.jar.disabled"), fabric_jar("1.19.2")).unwrap();
        fs::write(
            mods.join("d.jar"),
            forge_jar("[[dependencies.d]]\nmodId=\"minecraft\"\nversionRange=\"[1.19.2,1.20)\""),
        )
        .unwrap();
        fs::write(mods.join("notes.txt"), b"hi").unwrap();
        fs::write(mods.join("broken.jar"), b"not a zip").unwrap();
        let picked = import::read(&mods).unwrap();
        let found = &picked.found;
        assert_eq!(found.format, "folder");
        assert_eq!(found.name, "My mods");
        assert_eq!(found.game.as_deref(), Some("1.20.1"));
        assert_eq!(found.loader, Some(LoaderKind::Fabric));
        assert!(found.guessed);
        assert_eq!((found.files, found.mods, found.left_out), (5, 4, 1));
        let game = dir.path().join("game");
        import::copy(
            &picked,
            &game,
            InstanceId::generate(),
            &CancelToken::new(),
            &no_events,
        )
        .unwrap();
        assert_eq!(
            listing(&game),
            [
                "mods/a.jar",
                "mods/b.jar",
                "mods/broken.jar",
                "mods/c.jar.disabled",
                "mods/d.jar"
            ]
        );
    }

    #[test]
    fn a_zipped_game_folder_is_found_one_folder_down() {
        let dir = tempfile::tempdir().unwrap();
        let neo = zip_of(
            &[(
                "META-INF/neoforge.mods.toml",
                b"[[dependencies.x]]\nmodId=\"minecraft\"\nversionRange=\"[1.21.1,)\"" as &[u8],
            )],
            None,
        );
        let quilt = zip_of(
            &[(
                "quilt.mod.json",
                br#"{"quilt_loader":{"depends":[{"id":"minecraft","versions":">=1.21.1"}]}}"#
                    as &[u8],
            )],
            None,
        );
        let bytes = zip_of(
            &[
                ("versions/1.21.1/1.21.1.jar", b"game"),
                ("pack/.minecraft/mods/n.jar", &neo),
                ("pack/.minecraft/resourcepacks/r.zip", b"R"),
            ],
            None,
        );
        let path = dir.path().join("Pack.zip");
        fs::write(&path, &bytes).unwrap();
        let found = import::read(&path).unwrap().found;
        assert_eq!(found.name, "pack");
        assert_eq!(found.loader, Some(LoaderKind::NeoForge));
        assert_eq!(found.game.as_deref(), Some("1.21.1"));
        assert_eq!((found.files, found.left_out), (2, 0));

        // One Quilt mod makes it Quilt (it runs Fabric's mods too).
        let bytes = zip_of(
            &[
                ("mods/a.jar", &fabric_jar("1.21.1")),
                ("mods/b.jar", &fabric_jar("1.21.1")),
                ("mods/q.jar", &quilt),
            ],
            None,
        );
        let path = dir.path().join("Quilt.zip");
        fs::write(&path, &bytes).unwrap();
        let found = import::read(&path).unwrap().found;
        assert_eq!(found.name, "Quilt");
        assert_eq!(found.loader, Some(LoaderKind::Quilt));
    }

    #[test]
    fn nothing_to_bring_in_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        for (name, bytes) in [
            ("a.zip", zip_of(&[("readme.txt", b"hi")], None)),
            ("b.zip", zip_of(&[("x/y/z/mods/a.jar", b"too deep")], None)),
            ("c.zip", zip_of(&[("mods/run.exe", b"MZ")], None)),
            ("d.zip", b"not a zip".to_vec()),
        ] {
            let path = dir.path().join(name);
            fs::write(&path, bytes).unwrap();
            assert!(import::read(&path).is_err(), "{name}");
        }
        let empty = dir.path().join("empty");
        fs::create_dir(&empty).unwrap();
        assert!(import::read(&empty).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn links_in_a_folder_are_not_followed() {
        let dir = tempfile::tempdir().unwrap();
        let outside = dir.path().join("outside");
        fs::create_dir(&outside).unwrap();
        fs::write(outside.join("secret.jar"), b"S").unwrap();
        let game = dir.path().join("src");
        fs::create_dir_all(game.join("mods")).unwrap();
        fs::write(game.join("mods/a.jar"), fabric_jar("1.20.1")).unwrap();
        std::os::unix::fs::symlink(&outside, game.join("mods/linked")).unwrap();
        std::os::unix::fs::symlink(outside.join("secret.jar"), game.join("mods/s.jar")).unwrap();
        std::os::unix::fs::symlink(&outside, game.join("config")).unwrap();
        let picked = import::read(&game).unwrap();
        assert_eq!(picked.found.files, 1);
        let target = dir.path().join("game");
        import::copy(
            &picked,
            &target,
            InstanceId::generate(),
            &CancelToken::new(),
            &no_events,
        )
        .unwrap();
        assert_eq!(listing(&target), ["mods/a.jar"]);
    }
}

#[test]
fn a_packwiz_pack_keeps_its_source_without_a_page_to_open() {
    let project = Project::from(&json!({
        "id": id(1), "kind": "modpack", "title": "My Pack",
        "sources": [
            { "source": "packwiz", "url": "https://raw.githubusercontent.com/me/pack/main/pack.toml" },
            { "source": "elsewhere", "url": "https://example.com" },
        ],
    }))
    .unwrap();
    assert_eq!(project.sources.len(), 1);
    assert_eq!(project.sources[0].source, "packwiz");
    assert_eq!(project.sources[0].url, None);
}

#[test]
fn a_project_from_another_site_opens_its_authors_page_over_https_only() {
    let project = Project::from(&json!({
        "id": id(1), "kind": "mod", "title": "OptiFine",
        "sources": [{ "source": "external", "url": "https://optifine.net/home" }],
    }))
    .unwrap();
    assert_eq!(project.sources[0].source, "external");
    assert_eq!(
        project.sources[0].url.as_deref(),
        Some("https://optifine.net/home")
    );
    for url in [
        "http://optifine.net/home",
        "javascript:alert(1)",
        "file:///etc/passwd",
        "https://user:pw@optifine.net/",
    ] {
        let project = Project::from(&json!({
            "id": id(1), "kind": "mod",
            "sources": [{ "source": "external", "url": url }],
        }))
        .unwrap();
        assert_eq!(project.sources[0].url, None, "{url}");
    }
}

mod publishing {
    use std::io::Read as _;

    use super::*;
    use crate::download::CancelToken;
    use crate::mods::pack::ExportChoice;
    use crate::mods::publish::{
        PublishForm, Published, Target as Game, icon_name, meta, overridable, problem, refused,
        sort, write,
    };

    fn put(path: &Path, bytes: &[u8]) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }

    fn sha(bytes: &[u8]) -> String {
        use sha2::Digest as _;
        hex::encode(sha2::Sha256::digest(bytes))
    }

    fn form() -> PublishForm {
        PublishForm {
            title: " My pack\n".to_owned(),
            summary: "Fast".to_owned(),
            body: "# Mine\nall\u{0} of it".to_owned(),
            version: "1.0.2".to_owned(),
            changes: String::new(),
            choice: ExportChoice::default(),
        }
    }

    #[test]
    fn the_form_and_icon_are_checked_as_the_catalog_checks_them() {
        let m = meta(&form()).unwrap();
        assert_eq!(m["title"], "My pack");
        assert_eq!(m["body"], "# Mine\nall of it");
        assert_eq!(m["version"], "1.0.2");
        for (field, value) in [
            ("title", "x"),
            ("title", "  !! "),
            ("title", &"x".repeat(81)),
            ("version", ""),
            ("version", "1 0"),
            ("version", "../1"),
            ("version", &"1".repeat(49)),
            ("summary", &"x".repeat(301)),
        ] {
            let mut f = form();
            match field {
                "title" => f.title = value.to_owned(),
                "version" => f.version = value.to_owned(),
                _ => f.summary = value.to_owned(),
            }
            assert!(meta(&f).is_err(), "{field} {value:?}");
        }
        assert_eq!(icon_name(b"\x89PNG\r\n\x1a\nrest").unwrap(), "icon.png");
        assert_eq!(icon_name(&[0xFF, 0xD8, 0xFF, 0]).unwrap(), "icon.jpg");
        assert_eq!(icon_name(b"RIFF....WEBPVP8 ").unwrap(), "icon.webp");
        assert!(icon_name(b"GIF89a").is_err());
        assert!(icon_name(b"<svg>").is_err());
        let mut big = b"\x89PNG\r\n\x1a\n".to_vec();
        big.resize(512 * 1024 + 1, 0);
        assert!(icon_name(&big).is_err());
        for ok in ["config/a.json", "options.txt", "resourcepacks/x.zip"] {
            assert!(overridable(ok), "{ok}");
        }
        for bad in [
            "mods/a.jar",
            "mods/a.txt",
            "config/a.jar",
            "config/a.JAR.disabled",
            "config/a.class",
            "config/a.dll.txt",
            "x/a.txt",
        ] {
            assert!(!overridable(bad), "{bad}");
        }
    }

    #[tokio::test]
    async fn an_installation_is_sorted_and_written_for_the_catalog() {
        let dir = tempfile::tempdir().unwrap();
        let game = dir.path().join("game");
        put(&game.join("mods/known.jar"), b"known");
        put(&game.join("mods/unknown.jar"), b"unknown");
        put(&game.join("resourcepacks/mine.zip"), b"my pack");
        put(&game.join("config/a.toml"), b"a = 1");
        put(&game.join("config/b.jar"), b"code");
        put(&game.join("options.txt"), b"fov:70");
        let mut catalog = Fake::default();
        catalog.add_project(1, "mod", "Known");
        let known = catalog.add_version(2, 1, "known.jar", json!({}));
        catalog.versions.get_mut(&known).unwrap().files[0].sha256 = Some(sha(b"known"));
        let choice = ExportChoice {
            worlds: false,
            options: true,
        };
        let sorted = sort(&catalog, &game, choice).await.unwrap();
        let check = &sorted.check;
        assert_eq!(check.linked, 1);
        assert_eq!(check.missing, ["mods/unknown.jar"]);
        // config/b.jar is not a path a pack writes at all: left out.
        assert!(check.refused.is_empty(), "{check:?}");
        assert_eq!(check.bundled, 3, "{check:?}");
        assert!(!check.ready());
        fs::remove_file(game.join("mods/unknown.jar")).unwrap();
        let sorted = sort(&catalog, &game, choice).await.unwrap();
        assert!(sorted.check.ready(), "{:?}", sorted.check);
        let to = dir.path().join("out.mrpack");
        let target = Game {
            game: "1.20.1".to_owned(),
            loader: Some(crate::loader::LoaderChoice {
                kind: LoaderKind::Forge,
                version: "1.20.1-47.2.0".to_owned(),
            }),
        };
        let icon = Some(("icon.png", b"\x89PNG\r\n\x1a\n".to_vec()));
        write(
            &to,
            &sorted,
            &target,
            &meta(&form()).unwrap(),
            icon,
            &CancelToken::new(),
        )
        .unwrap();
        let mut zip = zip::ZipArchive::new(fs::File::open(&to).unwrap()).unwrap();
        let mut names: Vec<String> = zip.file_names().map(str::to_owned).collect();
        names.sort();
        assert_eq!(
            names,
            [
                "catalog.json",
                "icon.png",
                "modrinth.index.json",
                "overrides/config/a.toml",
                "overrides/options.txt",
                "overrides/resourcepacks/mine.zip",
            ]
        );
        let mut text = String::new();
        zip.by_name("modrinth.index.json")
            .unwrap()
            .read_to_string(&mut text)
            .unwrap();
        let index: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(index["files"][0]["path"], "mods/known.jar");
        assert_eq!(index["dependencies"]["forge"], "47.2.0");
        assert_eq!(index["versionId"], "1.0.2");
    }

    #[test]
    fn the_catalogs_refusals_are_read_with_care() {
        let answer =
            json!({ "error": "not_in_catalog", "paths": ["mods/a.jar", 5, "mods/b\u{0}.jar"] });
        assert_eq!(
            refused(422, &answer),
            Some(Published::Refused {
                code: "not_in_catalog".to_owned(),
                paths: vec!["mods/a.jar".to_owned(), "mods/b.jar".to_owned()],
            })
        );
        assert_eq!(refused(422, &json!({ "error": "evil" })), None);
        assert_eq!(refused(400, &answer), None);
        assert!(problem(409, "version_exists").is_some());
        assert!(problem(429, "too_many_packs").is_some());
        assert!(problem(400, "anything").is_none());
        assert!(problem(400, "bad_upload").is_some());
        assert!(problem(429, "busy").is_some());
        let unconfirmed = problem(403, "email_not_confirmed").unwrap();
        assert_eq!(unconfirmed.code, ErrorCode::AuthForbidden);
        assert!(unconfirmed.message.contains("confirmed email"));
    }

    #[test]
    fn an_archive_is_read_in_parts_with_its_hash() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pack.mrpack");
        let bytes: Vec<u8> = (0..10_000u32).map(|n| (n % 251) as u8).collect();
        fs::write(&path, &bytes).unwrap();
        let (size, sha256) = crate::mods::publish::size_and_sha256(&path).unwrap();
        assert_eq!((size, sha256), (10_000, sha(&bytes)));
        let mut joined = Vec::new();
        for n in 0..10_000u64.div_ceil(4096) {
            joined.extend(crate::mods::publish::read_part(&path, n * 4096, 4096).unwrap());
        }
        assert_eq!(joined, bytes);
        assert!(
            crate::mods::publish::read_part(&path, 20_000, 4096)
                .unwrap()
                .is_empty()
        );
    }
}
