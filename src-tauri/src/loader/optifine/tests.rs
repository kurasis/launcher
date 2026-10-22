use std::io::Write as _;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::*;
use crate::loader::LoaderChoice;
use crate::loader::manifest::{Resolver, load};
use crate::mods::{Version, VersionFile};

fn zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, bytes) in entries {
        writer
            .start_file(*name, zip::write::SimpleFileOptions::default())
            .unwrap();
        writer.write_all(bytes).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

const WRAPPER: &[u8] = b"PK launchwrapper";

/// A build's jar as OptiFine ships it, without the parts the launcher skips.
fn build_jar(wrapper_version: Option<&str>) -> Vec<u8> {
    let mut entries: Vec<(&str, &[u8])> = vec![
        (
            "META-INF/MANIFEST.MF",
            b"Main-Class: optifine.InstallerFrame\n",
        ),
        ("optifine/Patcher.class", b"\xca\xfe\xba\xbe"),
    ];
    let name;
    if let Some(version) = wrapper_version {
        entries.push((LAUNCHWRAPPER_VERSION, version.as_bytes()));
        name = format!("launchwrapper-of-{}.jar", version.trim());
        entries.push((&name, WRAPPER));
    }
    zip(&entries)
}

fn release(jar: &[u8]) -> Release {
    Release {
        id: "1.21.1_HD_U_J1".into(),
        game: "1.21.1".into(),
        label: "HD U J1".into(),
        stable: true,
        url: format!("https://cdn-mods.lapi.sh/f/{}", sha256_hex(jar)),
        sha256: sha256_hex(jar),
        size: jar.len() as u64,
    }
}

#[test]
fn build_ids_come_from_file_names() {
    assert_eq!(
        build_id("OptiFine_1.21.1_HD_U_J1.jar", "1.21.1").as_deref(),
        Some("1.21.1_HD_U_J1")
    );
    assert_eq!(
        build_id("preview_OptiFine_1.21.10_HD_U_J7_pre11.jar", "1.21.10").as_deref(),
        Some("1.21.10_HD_U_J7_pre11")
    );
    assert_eq!(
        build_id("OptiFine_26.2_HD_U_K2.jar", "26.2").as_deref(),
        Some("26.2_HD_U_K2")
    );
    assert_eq!(
        edition("1.21.10_HD_U_J7_pre11", "1.21.10").as_deref(),
        Some("HD_U_J7_pre11")
    );
    for (bad, game) in [
        // Another game version, or none.
        ("OptiFine_1.21.1_HD_U_J1.jar", "1.21"),
        ("OptiFine_1.21.1_HD_U_J1.jar", "1.21.11"),
        ("OptiFine_HD_U_J1.jar", "1.21.1"),
        // Not a build's jar.
        ("OptiFine_1.21.1_HD_U_J1.zip", "1.21.1"),
        ("Sodium_1.21.1_HD_U_J1.jar", "1.21.1"),
        ("OptiFine_1.21.1_.jar", "1.21.1"),
        ("OptiFine_1.21.1_HD U.jar", "1.21.1"),
        ("OptiFine_1.21.1_HD/../x.jar", "1.21.1"),
        ("OptiFine_1.21.1_HD-U.jar", "1.21.1"),
        ("OptiFine_1.21.1__x.jar", "1.21.1"),
        ("OptiFine_../x_HD.jar", "../x"),
        ("", ""),
    ] {
        assert_eq!(build_id(bad, game), None, "{bad} {game}");
    }
    let long = format!("OptiFine_1.21.1_HD{}.jar", "U".repeat(60));
    assert_eq!(build_id(&long, "1.21.1"), None);
}

fn version(name: &str, games: &[&str], channel: &str) -> Version {
    Version {
        id: "01a15e7a-f485-772f-acb6-2c8e72ee9f08".into(),
        project_id: "01a15e7a-f485-772f-acb6-2c8e72ee9f09".into(),
        number: String::new(),
        name: String::new(),
        changelog: Vec::new(),
        channel: channel.into(),
        published: 0,
        downloads: 0,
        saved: "all".into(),
        games: games.iter().map(|g| (*g).to_owned()).collect(),
        loaders: Vec::new(),
        files: vec![VersionFile {
            name: name.into(),
            size: 7,
            sha256: Some("a".repeat(64)),
            primary: true,
            url: Some(format!("https://cdn-mods.lapi.sh/f/{}", "a".repeat(64))),
            held: false,
            origin: None,
            manifests: None,
        }],
        dependencies: Vec::new(),
        gone_at: None,
    }
}

#[test]
fn only_served_builds_for_one_game_are_offered() {
    let good = release_of(&version(
        "OptiFine_1.21.1_HD_U_J1.jar",
        &["1.21.1"],
        "release",
    ))
    .unwrap();
    assert_eq!(good.id, "1.21.1_HD_U_J1");
    assert_eq!(good.label, "HD U J1");
    assert!(good.stable);
    let preview = release_of(&version(
        "preview_OptiFine_1.21.1_HD_U_J2_pre1.jar",
        &["1.21.1"],
        "beta",
    ))
    .unwrap();
    assert!(!preview.stable);
    assert_eq!(preview.label, "HD U J2 pre1");

    let both = version(
        "OptiFine_1.21.1_HD_U_J1.jar",
        &["1.21.1", "1.21"],
        "release",
    );
    assert_eq!(release_of(&both), None);
    let mut held = version("OptiFine_1.21.1_HD_U_J1.jar", &["1.21.1"], "release");
    held.files[0].held = true;
    assert_eq!(release_of(&held), None);
    let mut waiting = version("OptiFine_1.21.1_HD_U_J1.jar", &["1.21.1"], "release");
    waiting.files[0].url = None;
    assert_eq!(release_of(&waiting), None);
    let other = version("OptiFine_1.20.1_HD_U_I6.jar", &["1.21.1"], "release");
    assert_eq!(release_of(&other), None);
}

/// Every fetch fails: OptiFine needs no loader host.
struct Offline;

impl Fetch for Offline {
    fn fetch<'a>(
        &'a self,
        _url: &'a str,
        _etag: Option<&'a str>,
        _last_modified: Option<&'a str>,
        _max_bytes: usize,
    ) -> crate::catalog::source::FetchFuture<'a> {
        Box::pin(async { Err(LauncherError::new(ErrorCode::NetworkError, "offline")) })
    }
}

/// Serves a list of builds and their jars; counts the jars fetched.
struct Served {
    releases: Vec<Release>,
    jar: Vec<u8>,
    fetched: AtomicUsize,
}

impl Releases for Served {
    fn releases<'a>(
        &'a self,
        game: Option<&'a str>,
    ) -> BoxFuture<'a, LauncherResult<Vec<Release>>> {
        Box::pin(async move {
            Ok(self
                .releases
                .iter()
                .filter(|r| game.is_none_or(|g| g == r.game))
                .cloned()
                .collect())
        })
    }

    fn file<'a>(&'a self, _url: &'a str, _max: usize) -> BoxFuture<'a, LauncherResult<Vec<u8>>> {
        Box::pin(async move {
            self.fetched.fetch_add(1, Ordering::SeqCst);
            Ok(self.jar.clone())
        })
    }
}

#[tokio::test]
async fn the_newest_release_is_recommended() {
    let mut list = Vec::new();
    for (id, stable) in [
        ("1.21.1_HD_U_J2_pre3", false),
        ("1.21.1_HD_U_J1", true),
        ("1.21.1_HD_U_I9", true),
    ] {
        list.push(Release {
            id: id.into(),
            game: "1.21.1".into(),
            label: edition(id, "1.21.1").unwrap().replace('_', " "),
            stable,
            url: String::new(),
            sha256: String::new(),
            size: 0,
        });
    }
    let mut other = list[1].clone();
    other.game = "1.20.1".into();
    list.push(other);
    let served = Served {
        releases: list,
        jar: Vec::new(),
        fetched: AtomicUsize::new(0),
    };
    let versions = loader_versions(&served, "1.21.1").await.unwrap();
    let ids: Vec<_> = versions.iter().map(|v| v.id.as_str()).collect();
    assert_eq!(
        ids,
        ["1.21.1_HD_U_J2_pre3", "1.21.1_HD_U_J1", "1.21.1_HD_U_I9"]
    );
    let recommended: Vec<_> = versions.iter().map(|v| v.recommended).collect();
    assert_eq!(recommended, [false, true, false]);
    assert_eq!(versions[0].label, "HD U J2 pre3");
    let mut games = game_versions(&served).await.unwrap();
    games.sort();
    assert_eq!(games, ["1.20.1", "1.21.1"]);
    assert!(loader_versions(&served, "../x").await.is_err());
}

#[test]
fn a_build_starts_through_launchwrapper_with_its_tweaker() {
    let jar = build_jar(Some("2.3\n"));
    let release = release(&jar);
    let path = Coordinate::parse(&installer_coordinate(&release.id))
        .unwrap()
        .path();
    assert_eq!(
        path,
        "optifine/OptiFine/1.21.1_HD_U_J1/OptiFine-1.21.1_HD_U_J1-installer.jar"
    );
    let manifest = manifest(&jar, &release, &path).unwrap();
    assert_eq!(manifest.kind, LoaderKind::OptiFine);
    assert_eq!(manifest.version, "1.21.1_HD_U_J1");
    assert_eq!(manifest.profile_id, "1.21.1-OptiFine_HD_U_J1");
    assert_eq!(manifest.main_class, "net.minecraft.launchwrapper.Launch");
    assert_eq!(
        manifest.game_arguments,
        [
            serde_json::json!("--tweakClass"),
            serde_json::json!("optifine.OptiFineTweaker")
        ]
    );
    assert!(manifest.minecraft_arguments.is_none());
    let names: Vec<_> = manifest.libraries.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "optifine:OptiFine:1.21.1_HD_U_J1",
            "optifine:launchwrapper-of:2.3"
        ]
    );
    assert!(manifest.libraries.iter().all(|l| l.url.is_empty()));
    assert_eq!(manifest.libraries[0].sha1, None, "built by the patcher");
    let installer = manifest.installer.as_ref().unwrap();
    assert_eq!(installer.path, path);
    assert_eq!(installer.sha1.as_str(), sha1_hex(&jar));
    assert_eq!(installer.embedded.len(), 1);
    let wrapper = &installer.embedded[0];
    assert_eq!(
        wrapper.path,
        "optifine/launchwrapper-of/2.3/launchwrapper-of-2.3.jar"
    );
    assert_eq!(wrapper.entry.as_deref(), Some("launchwrapper-of-2.3.jar"));
    assert_eq!(wrapper.sha1.as_str(), sha1_hex(WRAPPER));
    assert_eq!(
        manifest.libraries[1].sha1.as_ref(),
        Some(&wrapper.sha1),
        "unpacked and checked"
    );
    let patcher = &installer.processors[0];
    assert_eq!(installer.processors.len(), 1);
    assert_eq!(patcher.jar, "optifine:OptiFine:1.21.1_HD_U_J1:installer");
    assert_eq!(patcher.main_class.as_deref(), Some("optifine.Patcher"));
    assert_eq!(
        patcher.args,
        [
            "{MINECRAFT_JAR}",
            "{INSTALLER}",
            "[optifine:OptiFine:1.21.1_HD_U_J1]"
        ]
    );
    assert!(patcher.outputs.is_empty());
    // The record reads back as written.
    let text = serde_json::to_string(&manifest).unwrap();
    assert_eq!(
        serde_json::from_str::<LoaderManifest>(&text).unwrap(),
        manifest
    );
}

#[test]
fn builds_without_launchwrapper_or_with_junk_are_refused() {
    let release = release(&build_jar(None));
    for jar in [
        // 1.7.10 builds start through Mojang's LaunchWrapper.
        build_jar(None),
        build_jar(Some("../2.3")),
        build_jar(Some("2.3 x")),
        build_jar(Some("")),
        build_jar(Some(&"9".repeat(17))),
        // The version names a jar that is not there.
        zip(&[
            (LAUNCHWRAPPER_VERSION, b"2.3"),
            ("optifine/Patcher.class", b"x"),
        ]),
        // No patcher.
        zip(&[
            (LAUNCHWRAPPER_VERSION, b"2.3"),
            ("launchwrapper-of-2.3.jar", WRAPPER),
        ]),
        b"not a zip".to_vec(),
    ] {
        let error = manifest(&jar, &release, "optifine/x.jar").unwrap_err();
        assert_eq!(
            error.code,
            ErrorCode::UnsupportedVersionMetadata,
            "{error:?}"
        );
    }
    let mut other = release;
    other.game = "1.20.1".into();
    assert!(manifest(&build_jar(Some("2.3")), &other, "optifine/x.jar").is_err());
}

#[tokio::test]
async fn a_build_is_resolved_from_the_catalog_and_kept() {
    let dir = tempfile::tempdir().unwrap();
    let root = DataRoot::new(dir.path().to_path_buf()).unwrap();
    let jar = build_jar(Some("2.3"));
    let served = Served {
        releases: vec![release(&jar)],
        jar: jar.clone(),
        fetched: AtomicUsize::new(0),
    };
    let offline = Offline;
    let resolver = Resolver {
        loaders: &offline,
        mojang: &offline,
        optifine: Some(&served),
        root: &root,
    };
    let choice = LoaderChoice {
        kind: LoaderKind::OptiFine,
        version: "1.21.1_HD_U_J1".into(),
    };
    let (manifest, sha1) = resolver.resolve(&choice, "1.21.1").await.unwrap();
    assert_eq!(load(&root, &sha1), Some(manifest.clone()));
    let kept = root
        .maven_root()
        .join("optifine/OptiFine/1.21.1_HD_U_J1/OptiFine-1.21.1_HD_U_J1-installer.jar");
    assert_eq!(std::fs::read(&kept).unwrap(), jar);
    assert_eq!(served.fetched.load(Ordering::SeqCst), 1);
    // Again: the same record, from the kept jar.
    let (_, again) = resolver.resolve(&choice, "1.21.1").await.unwrap();
    assert_eq!(again, sha1);
    assert_eq!(served.fetched.load(Ordering::SeqCst), 1);

    // A build of another game version, or one not served, is refused.
    assert!(resolver.resolve(&choice, "1.20.1").await.is_err());
    let missing = LoaderChoice {
        kind: LoaderKind::OptiFine,
        version: "1.21.1_HD_U_J9".into(),
    };
    assert!(resolver.resolve(&missing, "1.21.1").await.is_err());
    // No worlds with OptiFine.
    assert!(resolver.resolve_server(&choice, "1.21.1").await.is_err());

    // A jar that is not the one the catalog names is not kept.
    std::fs::remove_file(&kept).unwrap();
    let wrong = Served {
        releases: vec![release(&jar)],
        jar: build_jar(Some("2.2")),
        fetched: AtomicUsize::new(0),
    };
    let resolver = Resolver {
        optifine: Some(&wrong),
        ..resolver
    };
    let error = resolver.resolve(&choice, "1.21.1").await.unwrap_err();
    assert_eq!(error.code, ErrorCode::HashMismatch);
    assert!(!kept.exists());
}
