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

    let project = Project::from(&json!({
        "id": id(2), "kind": "mod", "body": "<script>x</script>hi",
        "sources": [{"source": "modrinth", "url": "https://evil.example/x"},
                    {"source": "curseforge", "url": "https://www.curseforge.com/minecraft/mc-mods/x"}],
        "gallery": [{"url": "https://evil.example/a.png"}, {"url": PICTURE}],
        "categories": ["ok", "bad category", "x".repeat(40)],
        "clientSide": "<b>", "authors": [{"name": ""}, {"name": "Someone", "role": "owner"}],
    }))
    .unwrap();
    assert_eq!(all_text(&project.body), "hi");
    assert_eq!(project.sources[0].url, None);
    assert!(project.sources[1].url.is_some());
    assert_eq!(project.gallery.len(), 1);
    assert_eq!(project.categories, ["ok"]);
    assert_eq!(project.client_side, "unknown");
    assert_eq!(project.authors.len(), 1);
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
            "dependencies": [{"kind": "required", "projectId": id(3)}, {"kind": "weird"}],
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
    assert_eq!(version.dependencies.len(), 1);
    assert!(version.changelog.is_empty());
    assert_eq!(version.main_file().unwrap().name, "a.jar");
    assert!(Version::from(&json!({"id": id(1), "projectId": "x"}), true).is_none());
}

#[test]
fn import_requests_are_checked() {
    assert!(ImportRequest::from(&json!({"id": id(1), "state": "hacked"})).is_none());
    let request = ImportRequest::from(&json!({
        "id": id(1), "state": "choosing", "error": "bad<script>",
        "candidates": [
            {"source": "modrinth", "id": "sodium", "title": "Sodium"},
            {"source": "evil", "id": "x"},
            {"source": "modrinth", "id": "../x"},
        ],
    }))
    .unwrap();
    assert_eq!(request.error.as_deref(), Some("badscript"));
    assert_eq!(request.candidates.len(), 1);
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
    ] {
        assert!(bad.url().is_err(), "{bad:?}");
    }
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
    };
    let plan = content::plan(&fake, dir.path(), &fabric(), &lock, &wanted(&chosen))
        .await
        .unwrap();
    assert!(!plan.ready());
    assert_eq!(plan.missing.len(), 2);
    assert_eq!(plan.missing[0].title.as_deref(), Some("Needed"));
    assert_eq!(plan.conflicts.len(), 1);
    assert_eq!(plan.conflicts[0].reason, "incompatible");
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
        content::plan(&fake, dir.path(), &fabric(), &lock, &wanted(&forge))
            .await
            .is_err()
    );
    assert!(
        content::plan(&fake, dir.path(), &fabric(), &lock, &wanted(&old))
            .await
            .is_err()
    );
    let vanilla = Target {
        game: "1.21.1".to_owned(),
        loader: None,
    };
    let fits = fake.add_version(12, 1, "c.jar", json!({}));
    assert!(
        content::plan(&fake, dir.path(), &vanilla, &lock, &wanted(&fits))
            .await
            .is_err()
    );
    // Quilt runs Fabric's mods.
    let quilt = Target {
        game: "1.21.1".to_owned(),
        loader: Some(LoaderKind::Quilt),
    };
    assert!(
        content::plan(&fake, dir.path(), &quilt, &lock, &wanted(&fits))
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
    };
    let plan = content::plan(
        &fake,
        dir.path(),
        &vanilla,
        &ContentLock::default(),
        &wanted(&v),
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
        content::plan(&fake, dir.path(), &target, &lock, &wanted(&pack))
            .await
            .is_err()
    );
    // A data pack needs an existing world with a plain name.
    assert!(
        content::plan(&fake, dir.path(), &target, &lock, &wanted(&data))
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
            content::plan(&fake, dir.path(), &target, &lock, &w)
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
    let plan = content::plan(&fake, dir.path(), &target, &lock, &w)
        .await
        .unwrap();
    assert_eq!(plan.items[0].path, "saves/World/datapacks/data.zip");
    assert!(
        content::plan(&fake, dir.path(), &target, &lock, &wanted(&bad))
            .await
            .is_err()
    );
}

#[tokio::test]
async fn files_of_the_player_are_never_overwritten() {
    let mut fake = Fake::default();
    fake.add_project(1, "mod", "A");
    let v = fake.add_version(10, 1, "a.jar", json!({}));
    let dir = tempfile::tempdir().unwrap();
    touch(dir.path(), "mods/a.jar");
    let lock = ContentLock::default();
    let plan = content::plan(&fake, dir.path(), &fabric(), &lock, &wanted(&v))
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
    let plan = content::plan(&fake, dir.path(), &fabric(), &lock, &w)
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
        content::plan(&fake, dir.path(), &fabric(), &lock, &w)
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
    };
    let plan = content::plan(&fake, dir.path(), &fabric(), &lock, &wanted(&new))
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
