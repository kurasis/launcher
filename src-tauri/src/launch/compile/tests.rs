use std::path::PathBuf;

use super::*;
use crate::install::plan::plan_game;
use crate::install::rules::{MetaOs, RuleContext};
use crate::storage::paths::Sha1;

const V1_21_1: &[u8] = include_bytes!("../../../../tests/fixtures/mojang/1.21.1.json");
const V26_3: &[u8] = include_bytes!("../../../../tests/fixtures/mojang/26.3.json");
const V1_6_4: &[u8] = include_bytes!("../../../../tests/fixtures/mojang/1.6.4.json");
const B1_7_3: &[u8] = include_bytes!("../../../../tests/fixtures/mojang/b1.7.3.json");
const TOKEN: &str = "eyJhbGciOi.test-token.not-real";

fn plan(bytes: &[u8], version: &str, os: MetaOs, arch: &'static str) -> GamePlan {
    let sha = Sha1::parse(&hex::encode(<sha1::Sha1 as sha1::Digest>::digest(bytes))).unwrap();
    plan_game(
        bytes,
        "https://piston-meta.mojang.com/v1/packages/x/v.json",
        &sha,
        version,
        // Old descriptors match libraries on `os.version`, as the app supplies.
        &RuleContext::with_default_features(os, arch, Some("10.0".to_owned())),
    )
    .unwrap()
}

fn identity() -> LaunchIdentity {
    LaunchIdentity {
        player_name: "Player One".into(),
        uuid: "0123456789abcdef0123456789abcdef".into(),
        access_token: SecretString::new(TOKEN),
        xuid: Some("2535400000000000".into()),
        client_id: "00000000-0000-0000-0000-000000000000".into(),
        notoken: false,
        offline: false,
    }
}

fn root() -> DataRoot {
    let base = if cfg!(windows) {
        r"C:\Data\Launcher"
    } else {
        "/home/Jürgen A/.local/share/launcher"
    };
    DataRoot::new(PathBuf::from(base)).unwrap()
}

fn compile_for(plan: &GamePlan) -> CompiledLaunch {
    compile_as(plan, &identity())
}

fn compile_as(plan: &GamePlan, identity: &LaunchIdentity) -> CompiledLaunch {
    let root = root();
    let id = "0f1e2d3c-4b5a-4978-8796-a5b4c3d2e1f0";
    let game_dir = root.path().join("instances").join(id).join("game");
    let natives = root
        .path()
        .join("instances")
        .join(id)
        .join("runs")
        .join("r1")
        .join("natives");
    compile(&LaunchInputs {
        plan,
        root: &root,
        game_dir: &game_dir,
        game_assets: &game_dir,
        natives_dir: &natives,
        max_heap_mib: 4096,
        resolution: Resolution {
            width: 1280,
            height: 720,
        },
        identity,
        launcher_version: "0.1.0",
        extra_jvm_args: &[],
        extra_game_args: &[],
        fullscreen: false,
    })
    .unwrap()
}

#[test]
fn all_placeholders_resolve_and_order_is_kept() {
    for (bytes, version) in [(V1_21_1, "1.21.1"), (V26_3, "26.3")] {
        for (os, arch) in [
            (MetaOs::Linux, "x86_64"),
            (MetaOs::Windows, "x86_64"),
            (MetaOs::Osx, "arm64"),
        ] {
            let p = plan(bytes, version, os, arch);
            let c = compile_for(&p);
            let all: Vec<&String> = c.jvm_args.iter().chain(c.game_args.iter()).collect();
            assert!(all.iter().all(|a| !a.contains("${")), "{version} {os:?}");
            assert_eq!(c.jvm_args[..2], ["-Xmx4096M", "-Xms512M"]);
            assert_eq!(c.main_class, "net.minecraft.client.main.Main");
            let cp = c.jvm_args.iter().position(|a| a == "-cp").unwrap();
            let entries: Vec<&str> = c.jvm_args[cp + 1].split(CLASSPATH_SEPARATOR).collect();
            assert_eq!(entries.len(), p.classpath.len());
            assert!(entries.last().unwrap().ends_with("client.jar"));
            assert!(
                c.jvm_args
                    .last()
                    .unwrap()
                    .starts_with("-Dlog4j.configurationFile=")
            );
            let pairs: Vec<(&str, &str)> = c
                .game_args
                .windows(2)
                .map(|w| (w[0].as_str(), w[1].as_str()))
                .collect();
            for expected in [
                ("--username", "Player One"),
                ("--version", version),
                ("--uuid", "0123456789abcdef0123456789abcdef"),
                ("--accessToken", TOKEN),
                ("--width", "1280"),
                ("--height", "720"),
                ("--assetIndex", p.asset_index.id.as_str()),
            ] {
                assert!(pairs.contains(&expected), "{version} {os:?} {expected:?}");
            }
        }
    }
}

#[test]
fn notoken_mode_adds_exactly_one_demo_argument() {
    for (bytes, version) in [(V1_21_1, "1.21.1"), (V26_3, "26.3")] {
        let p = plan(bytes, version, MetaOs::Linux, "x86_64");
        let normal = compile_for(&p);
        assert!(!normal.game_args.iter().any(|a| a == "--demo"), "{version}");
        let notoken = compile_as(
            &p,
            &LaunchIdentity {
                notoken: true,
                offline: false,
                ..identity()
            },
        );
        assert_eq!(
            notoken.game_args.iter().filter(|a| *a == "--demo").count(),
            1
        );
        assert_eq!(
            notoken.game_args.len(),
            normal.game_args.len() + 1,
            "{version}"
        );
    }
}

#[test]
fn the_token_never_reaches_jvm_arguments_or_previews() {
    let c = compile_for(&plan(V26_3, "26.3", MetaOs::Linux, "x86_64"));
    assert!(c.jvm_args.iter().all(|a| !a.contains(TOKEN)));
    assert_eq!(c.game_args.iter().filter(|a| a.contains(TOKEN)).count(), 1);
    let preview = c.preview(Some(root().path().parent().unwrap()));
    assert!(preview.iter().all(|a| !a.contains(TOKEN)));
    assert!(preview.iter().any(|a| a == REDACTED));
    assert!(preview.iter().any(|a| a.starts_with("~")) || cfg!(windows));
}

#[test]
fn platform_flags_and_native_directories() {
    let mac = compile_for(&plan(V1_21_1, "1.21.1", MetaOs::Osx, "arm64"));
    assert_eq!(mac.jvm_args[2], "-XstartOnFirstThread");
    let linux = compile_for(&plan(V26_3, "26.3", MetaOs::Linux, "x86_64"));
    let names: Vec<String> = linux
        .native_dirs
        .iter()
        .map(|d| d.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    for expected in ["java", "jna", "lwjgl", "netty"] {
        assert!(names.contains(&expected.to_owned()), "{names:?}");
    }
    assert!(!linux.game_args.iter().any(|a| a == "--demo"));
}

#[test]
fn unknown_or_misplaced_placeholders_are_errors() {
    let mut p = plan(V1_21_1, "1.21.1", MetaOs::Linux, "x86_64");
    p.game_arguments.push("${launcher_secret_dir}".into());
    let root = root();
    let dir = PathBuf::from("/tmp/g");
    let id = identity();
    let inputs = |p| LaunchInputs {
        plan: p,
        root: &root,
        game_dir: &dir,
        game_assets: &dir,
        natives_dir: &dir,
        max_heap_mib: 2048,
        resolution: Resolution::default(),
        identity: &id,
        launcher_version: "0.1.0",
        extra_jvm_args: &[],
        extra_game_args: &[],
        fullscreen: false,
    };
    let error = compile(&inputs(&p)).err().unwrap();
    assert_eq!(error.code, ErrorCode::UnsupportedVersionMetadata);

    let mut p = plan(V1_21_1, "1.21.1", MetaOs::Linux, "x86_64");
    p.jvm_arguments.push("-Dt=${auth_access_token}".into());
    assert!(
        compile(&inputs(&p)).is_err(),
        "a token in an argfile-bound argument"
    );

    let mut p = plan(V26_3, "26.3", MetaOs::Linux, "x86_64");
    p.jvm_arguments
        .push("-Dx=${natives_directory}/../escape".into());
    assert!(compile(&inputs(&p)).is_err());
}

#[test]
fn user_jvm_options_come_last_and_are_not_substituted() {
    let p = plan(V1_21_1, "1.21.1", MetaOs::Linux, "x86_64");
    let root = root();
    let dir = PathBuf::from("/tmp/g");
    let id = identity();
    let extra = ["-Xmx6G".to_owned(), "-Dlabel=${version_name}".to_owned()];
    let c = compile(&LaunchInputs {
        plan: &p,
        root: &root,
        game_dir: &dir,
        game_assets: &dir,
        natives_dir: &dir,
        max_heap_mib: 2048,
        resolution: Resolution::default(),
        identity: &id,
        launcher_version: "0.1.0",
        extra_jvm_args: &extra,
        extra_game_args: &[],
        fullscreen: false,
    })
    .unwrap();
    assert_eq!(c.jvm_args[..2], ["-Xmx2048M", "-Xms512M"]);
    assert_eq!(c.jvm_args[c.jvm_args.len() - 2..], extra);
    assert_eq!(c.main_class, "net.minecraft.client.main.Main");
}

#[test]
fn fullscreen_and_user_game_arguments_come_last_and_are_not_substituted() {
    let p = plan(V1_21_1, "1.21.1", MetaOs::Linux, "x86_64");
    let root = root();
    let dir = PathBuf::from("/tmp/g");
    let id = identity();
    let extra = [
        "--quickPlaySingleplayer".to_owned(),
        "My ${version_name}".to_owned(),
    ];
    let c = compile(&LaunchInputs {
        plan: &p,
        root: &root,
        game_dir: &dir,
        game_assets: &dir,
        natives_dir: &dir,
        max_heap_mib: 2048,
        resolution: Resolution::default(),
        identity: &id,
        launcher_version: "0.1.0",
        extra_jvm_args: &[],
        extra_game_args: &extra,
        fullscreen: true,
    })
    .unwrap();
    let n = c.game_args.len();
    assert_eq!(
        c.game_args[n - 3..],
        [
            "--fullscreen",
            "--quickPlaySingleplayer",
            "My ${version_name}"
        ]
    );
    // The window size stays for when fullscreen is left.
    assert!(c.game_args.contains(&"--width".to_owned()));
}

#[test]
fn substitution_is_exact() {
    let mut values = HashMap::new();
    values.insert("a", "1".to_owned());
    values.insert("b", "${a}".to_owned());
    assert_eq!(
        substitute("x${a}y${b}z", &values).unwrap(),
        "x1y${a}z",
        "no recursion"
    );
    assert!(substitute("${c}", &values).is_err());
    assert!(substitute("${a", &values).is_err());
}

#[test]
fn legacy_arguments_resolve_and_the_session_stays_secret() {
    let p = plan(V1_6_4, "1.6.4", MetaOs::Linux, "x86_64");
    let c = compile_for(&p);
    let all: Vec<&String> = c.jvm_args.iter().chain(c.game_args.iter()).collect();
    assert!(all.iter().all(|a| !a.contains("${")));
    let session = format!("token:{TOKEN}:0123456789abcdef0123456789abcdef");
    let pairs: Vec<(&str, &str)> = c
        .game_args
        .windows(2)
        .map(|w| (w[0].as_str(), w[1].as_str()))
        .collect();
    assert!(pairs.contains(&("--session", session.as_str())));
    assert!(pairs.contains(&("--username", "Player One")));
    // The session is a game argument only, and the preview hides all of it.
    assert!(!c.jvm_args.iter().any(|a| a.contains(TOKEN)));
    assert!(c.secrets().contains(&session));
    let preview = c.preview(None).join(" ");
    assert!(!preview.contains(TOKEN) && !preview.contains("token:"));
    assert!(
        c.jvm_args
            .iter()
            .any(|a| a.starts_with("-Djava.library.path="))
    );
}

#[test]
fn launch_wrapper_versions_take_positional_name_and_session() {
    let p = plan(B1_7_3, "b1.7.3", MetaOs::Windows, "x86_64");
    assert_eq!(p.main_class, "net.minecraft.launchwrapper.Launch");
    assert!(!p.natives.is_empty(), "LWJGL 2 natives are extracted");
    assert_eq!(p.runtime.major, 8);
    let c = compile_for(&p);
    assert!(c.game_args.iter().all(|a| !a.contains("${")));
    // `${auth_player_name} ${auth_session} --gameDir … --assetsDir …`
    assert_eq!(c.game_args[0], "Player One");
    assert!(c.game_args[1].starts_with("token:"));
    let assets = c.game_args.iter().position(|a| a == "--assetsDir").unwrap();
    assert!(
        c.game_args[assets + 1].ends_with("game"),
        "the by-name assets directory"
    );
}
