use std::collections::HashSet;
use std::path::Path;

use super::*;

/// An entry not filled in from a release: [`find`] never returns it.
const PLACEHOLDER_SHA256: &str = "PLACEHOLDER-fill-from-SHA256SUMS";
const PLACEHOLDER_SIZE: u64 = 0;

const H: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

const fn pinned(games: &'static [&'static str], target: Target, file: &'static str) -> Jar {
    Jar {
        games,
        target,
        file,
        sha256: H,
        size: 10,
    }
}

const TABLE: &[Jar] = &[
    pinned(
        &["26.3"],
        Target::Fabric,
        "launcherskins-fabric-26.3-0.1.2.jar",
    ),
    pinned(
        &["26.3"],
        Target::NeoForge,
        "launcherskins-neoforge-26.3-0.1.2.jar",
    ),
    pinned(
        &["26.3"],
        Target::Forge,
        "launcherskins-forge-26.3-0.1.2.jar",
    ),
    pinned(
        &["26.3"],
        Target::Agent,
        "launcherskins-agent-26.3-0.1.2.jar",
    ),
    // Not filled in yet.
    Jar {
        games: &["1.21.1"],
        target: Target::Fabric,
        file: "launcherskins-fabric-1.21.1-0.1.2.jar",
        sha256: PLACEHOLDER_SHA256,
        size: PLACEHOLDER_SIZE,
    },
    pinned(
        &["1.21.1"],
        Target::NeoForge,
        "launcherskins-neoforge-1.21.1-0.1.2.jar",
    ),
    pinned(
        &["1.21.1"],
        Target::Forge,
        "launcherskins-forge-1.21.1-0.1.2.jar",
    ),
    pinned(
        &["1.21.1"],
        Target::Agent,
        "launcherskins-agent-1.21.1-0.1.2.jar",
    ),
    pinned(
        &["1.12.2"],
        Target::Forge,
        "launcherskins-forge-1.12.2-0.1.2.jar",
    ),
];

fn file_for(game: &str, loader: Option<LoaderKind>) -> Option<&'static str> {
    find_in(TABLE, game, loader).map(|j| j.file)
}

#[test]
fn the_table_names_every_release_jar_once() {
    // One pinned jar per build, named and placed as the release has it.
    assert_eq!(JARS.len(), BUILDS.len());
    for (jar, build) in JARS.iter().zip(BUILDS) {
        assert_eq!((jar.games, jar.target), (build.games, build.target));
        assert_eq!(jar.file, build.file(MOD_VERSION));
        assert!(is_ours(jar.file));
        let url = jar.url();
        assert_eq!(
            url,
            format!(
                "https://github.com/kurasis/launcher/releases/download/skins-mod-v{MOD_VERSION}/{}",
                jar.file
            )
        );
        assert!(SourcePolicy::SkinsMod.check(&url).is_ok(), "{url}");
        // Every released jar is filled in from the release's SHA256SUMS.
        assert!(jar.pinned(), "{} is not filled in", jar.file);
    }
    // Each game version and loader finds the jar of its build.
    for game in BUILDS.iter().flat_map(|b| b.games) {
        for loader in [
            None,
            Some(LoaderKind::Fabric),
            Some(LoaderKind::Quilt),
            Some(LoaderKind::NeoForge),
            Some(LoaderKind::Forge),
        ] {
            let build = Build::find(game, loader).map(|b| b.file(MOD_VERSION));
            assert_eq!(
                find(game, loader).map(|j| j.file.to_owned()),
                build,
                "{game} {loader:?}"
            );
        }
    }
    // Versions the mod has no build for, and junk, get nothing.
    for game in [
        "26.3.1",
        "26.4",
        "1.21.1-pre1",
        "1.13.2",
        "1.12",
        "",
        " 26.3",
        "../26.3",
    ] {
        assert_eq!(find(game, Some(LoaderKind::Fabric)), None, "{game:?}");
        assert_eq!(find(game, None), None, "{game:?}");
    }
    assert!(RELEASE.ends_with(&format!("skins-mod-v{MOD_VERSION}/")));
}

#[test]
fn each_loader_and_version_gets_its_own_jar() {
    use LoaderKind::{Fabric, Forge, NeoForge, Quilt};
    assert_eq!(
        file_for("26.3", Some(Fabric)),
        Some("launcherskins-fabric-26.3-0.1.2.jar")
    );
    // Quilt runs the Fabric jar.
    assert_eq!(
        file_for("26.3", Some(Quilt)),
        Some("launcherskins-fabric-26.3-0.1.2.jar")
    );
    assert_eq!(
        file_for("26.3", Some(NeoForge)),
        Some("launcherskins-neoforge-26.3-0.1.2.jar")
    );
    assert_eq!(
        file_for("26.3", Some(Forge)),
        Some("launcherskins-forge-26.3-0.1.2.jar")
    );
    assert_eq!(
        file_for("26.3", None),
        Some("launcherskins-agent-26.3-0.1.2.jar")
    );
    assert_eq!(
        file_for("1.21.1", Some(NeoForge)),
        Some("launcherskins-neoforge-1.21.1-0.1.2.jar")
    );
    assert_eq!(
        file_for("1.21.1", Some(Forge)),
        Some("launcherskins-forge-1.21.1-0.1.2.jar")
    );
    assert_eq!(
        file_for("1.21.1", None),
        Some("launcherskins-agent-1.21.1-0.1.2.jar")
    );
    assert_eq!(
        file_for("1.12.2", Some(Forge)),
        Some("launcherskins-forge-1.12.2-0.1.2.jar")
    );
    // A placeholder entry is skipped, for Quilt too.
    assert_eq!(file_for("1.21.1", Some(Fabric)), None);
    assert_eq!(file_for("1.21.1", Some(Quilt)), None);
    // 1.12.2 has Forge only.
    for loader in [Some(Fabric), Some(Quilt), Some(NeoForge), None] {
        assert_eq!(file_for("1.12.2", loader), None, "{loader:?}");
    }
}

#[test]
fn other_versions_and_junk_get_nothing() {
    for game in [
        "26.1",
        "26.2",
        "26.3.1",
        "26.4",
        "1.21",
        "1.21.2",
        "1.21.10",
        "1.21.11",
        "1.12",
        "1.12.1",
        "1.20.1",
        "26.3 ",
        " 26.3",
        "26.3\0",
        "",
        "../26.3",
        "26.3/..",
        "1.21.1-pre1",
        "26.3-rc1",
    ] {
        for loader in [
            None,
            Some(LoaderKind::Fabric),
            Some(LoaderKind::Quilt),
            Some(LoaderKind::Forge),
            Some(LoaderKind::NeoForge),
        ] {
            assert_eq!(file_for(game, loader), None, "{game:?} {loader:?}");
        }
    }
}

#[test]
fn entries_that_are_not_sane_are_skipped() {
    let base = pinned(
        &["26.3"],
        Target::Fabric,
        "launcherskins-fabric-26.3-0.1.2.jar",
    );
    assert!(base.pinned());
    let upper = "0123456789ABCDEF0123456789ABCDEF0123456789ABCDEF0123456789ABCDEF";
    for bad in [
        Jar {
            sha256: PLACEHOLDER_SHA256,
            ..base
        },
        Jar { size: 0, ..base },
        Jar {
            size: MAX_JAR_BYTES + 1,
            ..base
        },
        Jar {
            sha256: &H[1..],
            ..base
        },
        Jar {
            sha256: upper,
            ..base
        },
        Jar {
            sha256: "g123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
            ..base
        },
        Jar {
            file: "sodium.jar",
            ..base
        },
        Jar {
            file: "launcherskins-../x.jar",
            ..base
        },
        Jar {
            file: "launcherskins-a\\x.jar",
            ..base
        },
        Jar {
            file: "launcherskins-.jar",
            ..base
        },
    ] {
        assert!(!bad.pinned(), "{bad:?}");
    }
}

#[test]
fn only_names_like_ours_count_as_ours() {
    for ours in [
        "launcherskins-fabric-26.3-0.1.2.jar",
        "launcherskins-old.jar",
        "LauncherSkins-Fabric-1.21.1-0.0.1.JAR",
    ] {
        assert!(is_ours(ours), "{ours}");
    }
    for other in [
        "launcherskins-.jar",
        "launcherskins.jar",
        "launcherskins-x.jar.disabled",
        "launcherskins-x.zip",
        "my-launcherskins-x.jar",
        "sodium-fabric-0.6.jar",
        ".launcherskins-x.jar",
        "",
    ] {
        assert!(!is_ours(other), "{other}");
    }
    assert!(is_our_temp(
        ".launcherskins-fabric-26.3-0.1.2.jar.tmp-0123abcd"
    ));
    assert!(!is_our_temp(".sodium.jar.tmp-0123abcd"));
    assert!(!is_our_temp("launcherskins-x.jar"));
}

#[test]
fn the_api_address_is_one_plain_url() {
    assert_eq!(
        api_value(" https://skins.example.test "),
        Some("https://skins.example.test")
    );
    assert_eq!(
        api_value("http://127.0.0.1:8899"),
        Some("http://127.0.0.1:8899")
    );
    for junk in [
        "",
        "   ",
        "skins.example.test",
        "ftp://skins.example.test",
        "file:///etc/passwd",
        "javascript:alert(1)",
        "https://skins.example.test/a b",
        "https://skins.example.test/\n-javaagent:x.jar",
        "https://skins.example.test/é",
        &format!("https://skins.example.test/{}", "a".repeat(300)),
    ] {
        assert_eq!(api_value(junk), None, "{junk:?}");
    }
}

/// A table with one jar of real content per target, as a release would be.
fn real_table() -> (&'static [Jar], Vec<(&'static str, Vec<u8>)>) {
    let mut jars = Vec::new();
    let mut contents = Vec::new();
    for (game, target, file) in [
        (
            "26.3",
            Target::Fabric,
            "launcherskins-fabric-26.3-0.1.2.jar",
        ),
        ("26.3", Target::Forge, "launcherskins-forge-26.3-0.1.2.jar"),
        ("26.3", Target::Agent, "launcherskins-agent-26.3-0.1.2.jar"),
    ] {
        let bytes = format!("jar of {file}").into_bytes();
        let sha = hex::encode(Sha256Hasher::digest(&bytes));
        jars.push(Jar {
            games: Box::leak(Box::new([game])),
            target,
            file,
            sha256: Box::leak(sha.into_boxed_str()),
            size: bytes.len() as u64,
        });
        contents.push((file, bytes));
    }
    (Box::leak(jars.into_boxed_slice()), contents)
}

fn put(path: &Path, bytes: &[u8]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, bytes).unwrap();
}

fn names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    names
}

#[test]
fn sync_replaces_our_jars_and_leaves_everything_else() {
    let (table, contents) = real_table();
    let dir = tempfile::tempdir().unwrap();
    let mods = dir.path().join("mods");
    let user = [
        ("sodium-fabric-0.6.jar", "player's mod"),
        ("launcherskins-notes.txt", "player's notes"),
        ("launcherskins-x.jar.disabled", "switched off by the player"),
        ("my-launcherskins-x.jar", "someone else's"),
    ];
    for (name, text) in user {
        put(&mods.join(name), text.as_bytes());
    }
    // A folder named like our jar, with something inside.
    put(
        &mods.join("launcherskins-x.jar").join("inside.txt"),
        b"kept",
    );
    // Ours: an old version, another loader's, other case, a stale temp file.
    for name in [
        "launcherskins-fabric-26.3-0.0.9.jar",
        "launcherskins-forge-26.3-0.1.2.jar",
        "LauncherSkins-Old.JAR",
        ".launcherskins-fabric-26.3-0.1.2.jar.tmp-0123abcd",
    ] {
        put(&mods.join(name), b"old");
    }
    // The current one, damaged.
    put(&mods.join(contents[0].0), b"damaged");

    sync_mods(&mods, Some((&Pick::of(&table[0]), &contents[0].1))).unwrap();
    let mut expected: Vec<String> = user.iter().map(|(n, _)| (*n).to_owned()).collect();
    expected.push("launcherskins-x.jar".into());
    expected.push(contents[0].0.into());
    expected.sort();
    assert_eq!(names(&mods), expected);
    assert_eq!(
        std::fs::read(mods.join(contents[0].0)).unwrap(),
        contents[0].1
    );
    for (name, text) in user {
        assert_eq!(std::fs::read(mods.join(name)).unwrap(), text.as_bytes());
    }
    assert_eq!(
        std::fs::read(mods.join("launcherskins-x.jar").join("inside.txt")).unwrap(),
        b"kept"
    );

    // Again: nothing changes.
    sync_mods(&mods, Some((&Pick::of(&table[0]), &contents[0].1))).unwrap();
    assert_eq!(names(&mods), expected);
}

#[test]
fn sync_without_a_jar_removes_ours_only_and_creates_nothing() {
    let (_, contents) = real_table();
    let dir = tempfile::tempdir().unwrap();
    let mods = dir.path().join("mods");
    sync_mods(&mods, None).unwrap();
    assert!(!mods.exists());

    put(&mods.join(contents[0].0), &contents[0].1);
    put(&mods.join("launcherskins-agent-26.3-0.1.2.jar"), b"x");
    put(&mods.join("sodium.jar"), b"player's");
    put(&mods.join("launcherskins-x.jar").join("a"), b"kept");
    sync_mods(&mods, None).unwrap();
    assert_eq!(names(&mods), ["launcherskins-x.jar", "sodium.jar"]);
}

#[test]
fn sync_into_a_mods_path_that_is_a_file_fails_without_harm() {
    let (table, contents) = real_table();
    let dir = tempfile::tempdir().unwrap();
    let mods = dir.path().join("mods");
    put(&mods, b"not a folder");
    assert!(sync_mods(&mods, Some((&Pick::of(&table[0]), &contents[0].1))).is_err());
    assert_eq!(std::fs::read(&mods).unwrap(), b"not a folder");
}

#[cfg(unix)]
#[test]
fn sync_removes_a_link_named_like_ours_but_not_its_target() {
    let (table, contents) = real_table();
    let dir = tempfile::tempdir().unwrap();
    let mods = dir.path().join("mods");
    let elsewhere = dir.path().join("elsewhere.jar");
    put(&elsewhere, b"player's file");
    std::fs::create_dir_all(&mods).unwrap();
    std::os::unix::fs::symlink(&elsewhere, mods.join("launcherskins-old.jar")).unwrap();
    std::os::unix::fs::symlink(&elsewhere, mods.join(contents[0].0)).unwrap();
    sync_mods(&mods, Some((&Pick::of(&table[0]), &contents[0].1))).unwrap();
    assert_eq!(names(&mods), [contents[0].0]);
    assert!(
        !std::fs::symlink_metadata(mods.join(contents[0].0))
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(
        std::fs::read(mods.join(contents[0].0)).unwrap(),
        contents[0].1
    );
    assert_eq!(std::fs::read(&elsewhere).unwrap(), b"player's file");
}

struct Install {
    _dir: tempfile::TempDir,
    root: DataRoot,
    game: PathBuf,
}

fn install(root_name: &str) -> Install {
    let dir = tempfile::tempdir().unwrap();
    let root = DataRoot::new(dir.path().join(root_name)).unwrap();
    let game = root.path().join("instances").join("a").join("game");
    std::fs::create_dir_all(&game).unwrap();
    Install {
        _dir: dir,
        root,
        game,
    }
}

impl Install {
    fn managed(&self, file: &str) -> PathBuf {
        self.root.path().join(MANAGED_DIR).join(file)
    }

    async fn prepare(
        &self,
        table: &'static [Jar],
        game: &str,
        loader: Option<LoaderKind>,
        enabled: bool,
    ) -> Prepared {
        prepare_with(
            table,
            &self.root,
            None,
            &Setup {
                game_version: game,
                loader,
                enabled,
                game_dir: &self.game,
                listed: None,
            },
            &CancelToken::new(),
        )
        .await
    }
}

#[tokio::test]
async fn a_loader_gets_the_verified_jar_in_mods() {
    let (table, contents) = real_table();
    let i = install("data");
    put(&i.managed(contents[0].0), &contents[0].1);
    // Quilt takes the Fabric jar.
    let prepared = i
        .prepare(table, "26.3", Some(LoaderKind::Quilt), true)
        .await;
    assert_eq!(
        prepared,
        Prepared {
            active: true,
            agent: None,
            problem: None
        }
    );
    let mods = i.game.join("mods");
    assert_eq!(names(&mods), [contents[0].0]);
    assert_eq!(
        std::fs::read(mods.join(contents[0].0)).unwrap(),
        contents[0].1
    );

    // Switched off: ours leave, the player's stay.
    put(&mods.join("sodium.jar"), b"player's");
    let prepared = i
        .prepare(table, "26.3", Some(LoaderKind::Quilt), false)
        .await;
    assert_eq!(prepared, Prepared::default());
    assert_eq!(names(&mods), ["sodium.jar"]);
}

#[tokio::test]
async fn a_jar_that_cannot_be_had_means_a_game_without_it() {
    let (table, contents) = real_table();
    let i = install("data");
    let mods = i.game.join("mods");
    put(&mods.join(contents[1].0), &contents[1].1);
    // Damaged in the managed folder and nothing to download it with.
    put(&i.managed(contents[1].0), b"tampered");
    let prepared = i
        .prepare(table, "26.3", Some(LoaderKind::Forge), true)
        .await;
    assert!(!prepared.active);
    assert!(prepared.problem.is_some());
    assert_eq!(names(&mods), Vec::<String>::new());
    // The damaged file is never copied.
    assert_eq!(
        std::fs::read(i.managed(contents[1].0)).unwrap(),
        b"tampered"
    );
}

#[tokio::test]
async fn an_unsupported_version_has_none_of_ours_in_mods() {
    let (table, _) = real_table();
    let i = install("data");
    let mods = i.game.join("mods");
    put(&mods.join("launcherskins-fabric-26.3-0.1.2.jar"), b"x");
    put(&mods.join("sodium.jar"), b"player's");
    let prepared = i
        .prepare(table, "1.21.4", Some(LoaderKind::Fabric), true)
        .await;
    assert_eq!(prepared, Prepared::default());
    assert_eq!(names(&mods), ["sodium.jar"]);
}

#[tokio::test]
async fn without_a_loader_the_agent_is_passed_from_the_managed_folder() {
    let (table, contents) = real_table();
    let i = install("data");
    let agent = i.managed(contents[2].0);
    put(&agent, &contents[2].1);
    let prepared = i.prepare(table, "26.3", None, true).await;
    assert_eq!(
        prepared,
        Prepared {
            active: true,
            agent: Some(agent.clone()),
            problem: None
        }
    );
    assert!(!i.game.join("mods").exists());

    assert_eq!(
        i.prepare(table, "26.3", None, false).await,
        Prepared::default()
    );
    // A damaged agent is not passed.
    put(&agent, b"tampered");
    let prepared = i.prepare(table, "26.3", None, true).await;
    assert_eq!(prepared.agent, None);
    assert!(prepared.problem.is_some());
}

#[tokio::test]
async fn an_agent_path_with_an_equals_sign_is_not_passed() {
    let (table, contents) = real_table();
    let i = install("a=b");
    put(&i.managed(contents[2].0), &contents[2].1);
    let prepared = i.prepare(table, "26.3", None, true).await;
    assert_eq!(prepared.agent, None);
    assert!(!prepared.active);
    assert!(prepared.problem.is_some());
}

#[tokio::test]
async fn the_real_table_never_hands_out_a_placeholder() {
    let i = install("data");
    for jar in JARS.iter().filter(|j| !j.pinned()) {
        // Even a file sitting in the managed folder is not used.
        put(&i.managed(jar.file), b"anything");
        let loader = match jar.target {
            Target::Fabric => Some(LoaderKind::Fabric),
            Target::NeoForge => Some(LoaderKind::NeoForge),
            Target::Forge => Some(LoaderKind::Forge),
            Target::Agent => None,
        };
        let prepared = prepare(
            &i.root,
            None,
            &Setup {
                game_version: jar.games[0],
                loader,
                enabled: true,
                game_dir: &i.game,
                listed: None,
            },
            &CancelToken::new(),
        )
        .await;
        assert_eq!(prepared, Prepared::default(), "{}", jar.file);
    }
}

#[test]
fn every_game_version_and_target_has_at_most_one_build() {
    let mut seen = HashSet::new();
    for b in BUILDS {
        assert!(!b.games.is_empty(), "{}", b.label);
        // A build is named after its one version, or its first and last.
        let label = match b.games {
            [one] => (*one).to_owned(),
            [first, .., last] => format!("{first}-{last}"),
            [] => unreachable!(),
        };
        assert_eq!(b.label, label);
        for game in b.games {
            assert!(
                seen.insert((*game, b.target.as_str())),
                "{game} {:?}",
                b.target
            );
        }
        assert!(is_ours(&b.file(MOD_VERSION)));
    }
}

#[test]
fn builds_are_found_by_version_and_loader() {
    use LoaderKind::{Fabric, Forge, NeoForge, Quilt};
    let label = |game: &str, loader: Option<LoaderKind>| {
        Build::find(game, loader).map(|b| format!("{}-{}", b.target.as_str(), b.label))
    };
    assert_eq!(
        label("26.1.2", Some(Quilt)).as_deref(),
        Some("fabric-26.1-26.3")
    );
    assert_eq!(label("26.2", None).as_deref(), Some("agent-26.1-26.3"));
    assert_eq!(
        label("1.21.11", Some(Forge)).as_deref(),
        Some("forge-1.21.11")
    );
    assert_eq!(
        label("1.21.3", Some(NeoForge)).as_deref(),
        Some("neoforge-1.21.2-1.21.4")
    );
    assert_eq!(label("1.21.3", None).as_deref(), Some("agent-1.21.3"));
    assert_eq!(
        label("1.21.7", Some(Fabric)).as_deref(),
        Some("fabric-1.21.5-1.21.8")
    );
    // From 1.20.2 the skin is a PlayerSkin: one Fabric jar to 1.21.1, NeoForge from 1.20.6.
    assert_eq!(
        label("1.21", Some(Fabric)).as_deref(),
        Some("fabric-1.20.2-1.21.1")
    );
    assert_eq!(
        label("1.20.3", Some(Quilt)).as_deref(),
        Some("fabric-1.20.2-1.21.1")
    );
    assert_eq!(
        label("1.20.6", Some(NeoForge)).as_deref(),
        Some("neoforge-1.20.6-1.21.1")
    );
    assert_eq!(
        label("1.20.2", Some(NeoForge)).as_deref(),
        Some("neoforge-1.20.2-1.20.4")
    );
    assert_eq!(
        label("1.20.4", Some(Forge)).as_deref(),
        Some("forge-1.20.2-1.20.4")
    );
    assert_eq!(
        label("1.20.6", Some(Forge)).as_deref(),
        Some("forge-1.20.6")
    );
    assert_eq!(label("1.20.5", None).as_deref(), Some("agent-1.20.5"));
    // Forge's jar for 1.21.1 is built for 1.21.1 alone.
    assert_eq!(label("1.21", Some(Forge)), None);
    // NeoForge for 1.20.1 runs Forge's jar.
    assert_eq!(
        label("1.20.1", Some(NeoForge)).as_deref(),
        Some("forge-1.17.1-1.20.1")
    );
    // One Fabric jar serves 1.15.2 to 1.20.1, one Forge jar 1.17.1 to 1.20.1; Forge 1.16.5
    // has its own.
    assert_eq!(
        label("1.16.1", Some(Quilt)).as_deref(),
        Some("fabric-1.15.2-1.20.1")
    );
    assert_eq!(
        label("1.15.2", Some(Fabric)).as_deref(),
        Some("fabric-1.15.2-1.20.1")
    );
    // Before 1.15.2 Fabric registers a texture another way: 1.15 and 1.15.1 share a jar,
    // 1.14.4 has its own. Forge 1.14.4 and 1.15.2 each have one (a coremod).
    assert_eq!(
        label("1.15", Some(Quilt)).as_deref(),
        Some("fabric-1.15-1.15.1")
    );
    assert_eq!(
        label("1.14.4", Some(Fabric)).as_deref(),
        Some("fabric-1.14.4")
    );
    assert_eq!(
        label("1.15.2", Some(Forge)).as_deref(),
        Some("forge-1.15.2")
    );
    assert_eq!(
        label("1.14.4", Some(Forge)).as_deref(),
        Some("forge-1.14.4")
    );
    assert_eq!(label("1.15.1", None).as_deref(), Some("agent-1.15.1"));
    assert_eq!(label("1.14.4", None).as_deref(), Some("agent-1.14.4"));
    assert_eq!(
        label("1.19.2", Some(Forge)).as_deref(),
        Some("forge-1.17.1-1.20.1")
    );
    assert_eq!(
        label("1.16.5", Some(Forge)).as_deref(),
        Some("forge-1.16.5")
    );
    assert_eq!(label("1.19.3", None).as_deref(), Some("agent-1.19.3"));
    assert_eq!(label("1.18.2", None).as_deref(), Some("agent-1.18.2"));
    assert_eq!(
        label("1.12.2", Some(Forge)).as_deref(),
        Some("forge-1.12.2")
    );
    assert_eq!(label("1.8.9", Some(Forge)).as_deref(), Some("forge-1.8.9"));
    assert_eq!(
        label("1.7.10", Some(Forge)).as_deref(),
        Some("forge-1.7.10")
    );
    for (game, loader) in [
        ("1.12.2", Some(Fabric)),
        ("1.12.2", None),
        // Before 1.12.2 the mod is Forge only, for 1.8.9 and 1.7.10.
        ("1.8.9", Some(Fabric)),
        ("1.8.9", None),
        ("1.7.10", None),
        ("1.8.8", Some(Forge)),
        ("1.7.2", Some(Forge)),
        ("1.18.2", Some(NeoForge)),
        // Forge has builds for 1.14.4, 1.15.2 and 1.16.5 only before 1.17.1.
        ("1.16.4", Some(Forge)),
        ("1.15.1", Some(Forge)),
        ("1.14.4", Some(NeoForge)),
        ("1.14.3", Some(Fabric)),
        // Neither Forge nor NeoForge has a stable build for 1.20.5.
        ("1.20.5", Some(Forge)),
        ("1.20.5", Some(NeoForge)),
        ("26.4", Some(Fabric)),
        ("1.21.1-pre1", None),
        ("", None),
    ] {
        assert_eq!(label(game, loader), None, "{game} {loader:?}");
    }
}

/// CI builds the signed list from `mod/builds.json`: it names the same builds.
#[test]
fn the_builds_file_for_the_signed_list_matches_the_builds() {
    let file: Vec<serde_json::Value> =
        serde_json::from_str(include_str!("../../../mod/builds.json")).unwrap();
    let ours: Vec<serde_json::Value> = BUILDS
        .iter()
        .map(|b| {
            serde_json::json!({
                "target": b.target.as_str(),
                "label": b.label,
                "games": b.games,
            })
        })
        .collect();
    assert_eq!(file, ours, "mod/builds.json and BUILDS differ");
}
