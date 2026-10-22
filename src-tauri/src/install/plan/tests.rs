use std::collections::HashSet;

use serde_json::json;
use sha1::{Digest, Sha1 as Sha1Hasher};

use super::*;

const V1_21_1: &[u8] = include_bytes!("../../../../tests/fixtures/mojang/1.21.1.json");
const V26_3: &[u8] = include_bytes!("../../../../tests/fixtures/mojang/26.3.json");
const SMALL_INDEX: &[u8] =
    include_bytes!("../../../../tests/fixtures/mojang/asset-index-small.json");
const SHA_1_21_1: &str = "499680d659b7a0ccb2fa9a809c345c5baf2ace7d";
const SHA_26_3: &str = "11f1aefe473eefba0af62e3286c33b692807b2eb";

fn sha1_hex(bytes: &[u8]) -> String {
    hex::encode(Sha1Hasher::digest(bytes))
}

fn ctx(os: MetaOs, arch: &'static str) -> RuleContext {
    RuleContext::with_default_features(os, arch, Some("10.0".into()))
}

fn plan(bytes: &[u8], version: &str, sha: &str, os: MetaOs, arch: &'static str) -> GamePlan {
    plan_game(
        bytes,
        &format!("https://piston-meta.mojang.com/v1/packages/{sha}/{version}.json"),
        &Sha1::parse(sha).unwrap(),
        version,
        &ctx(os, arch),
    )
    .unwrap()
}

#[test]
fn fixtures_are_the_catalog_descriptors() {
    assert_eq!(sha1_hex(V1_21_1), SHA_1_21_1);
    assert_eq!(sha1_hex(V26_3), SHA_26_3);
}

/// Expected counts were computed independently (Python script in
/// docs/VERIFICATION.md, P05) and match document 10 for Windows x64.
#[test]
fn library_selection_matches_the_independent_reference() {
    let cases = [
        (
            V1_21_1,
            "1.21.1",
            SHA_1_21_1,
            MetaOs::Windows,
            "x86_64",
            70,
            66_739_500_u64,
        ),
        (
            V1_21_1,
            "1.21.1",
            SHA_1_21_1,
            MetaOs::Linux,
            "x86_64",
            56,
            61_944_833,
        ),
        (
            V1_21_1,
            "1.21.1",
            SHA_1_21_1,
            MetaOs::Osx,
            "x86_64",
            63,
            64_939_887,
        ),
        (
            V1_21_1,
            "1.21.1",
            SHA_1_21_1,
            MetaOs::Osx,
            "arm64",
            63,
            64_939_887,
        ),
        (
            V26_3,
            "26.3",
            SHA_26_3,
            MetaOs::Windows,
            "x86_64",
            74,
            87_763_720,
        ),
        (
            V26_3,
            "26.3",
            SHA_26_3,
            MetaOs::Linux,
            "x86_64",
            66,
            83_353_549,
        ),
        (
            V26_3,
            "26.3",
            SHA_26_3,
            MetaOs::Osx,
            "x86_64",
            80,
            91_265_044,
        ),
        (
            V26_3,
            "26.3",
            SHA_26_3,
            MetaOs::Osx,
            "arm64",
            80,
            91_265_044,
        ),
    ];
    for (bytes, version, sha, os, arch, count, total) in cases {
        let plan = plan(bytes, version, sha, os, arch);
        assert_eq!(plan.libraries.len(), count, "{version} {os:?} {arch}");
        assert_eq!(plan.libraries.iter().map(|l| l.size).sum::<u64>(), total);
        assert_eq!(plan.classpath.len(), count + 1);
    }
}

#[test]
fn java_major_comes_from_metadata() {
    assert_eq!(
        plan(V1_21_1, "1.21.1", SHA_1_21_1, MetaOs::Linux, "x86_64")
            .runtime
            .major,
        21
    );
    let p = plan(V26_3, "26.3", SHA_26_3, MetaOs::Osx, "arm64");
    assert_eq!(p.runtime.major, 25);
    assert_eq!(p.runtime.os, TargetOs::Mac);
    assert_eq!(p.runtime.arch, TargetArch::Aarch64);
}

#[test]
fn foreign_os_libraries_are_excluded() {
    for (os, foreign) in [
        (MetaOs::Linux, ["natives-windows", "natives-macos"]),
        (MetaOs::Windows, ["natives-linux", "natives-macos"]),
        (MetaOs::Osx, ["natives-windows", "natives-linux"]),
    ] {
        for (bytes, version, sha) in [(V1_21_1, "1.21.1", SHA_1_21_1), (V26_3, "26.3", SHA_26_3)] {
            let p = plan(bytes, version, sha, os, "x86_64");
            for library in &p.libraries {
                let name = library.provenance.as_deref().unwrap();
                for marker in foreign {
                    assert!(!name.contains(marker), "{version} {os:?}: {name}");
                }
            }
            // OS-allowed native JARs (including other architectures) are kept.
            let own = match os {
                MetaOs::Linux => "natives-linux",
                MetaOs::Windows => "natives-windows",
                MetaOs::Osx => "natives-macos",
            };
            assert!(
                p.libraries
                    .iter()
                    .any(|l| l.provenance.as_deref().unwrap().contains(own))
            );
        }
    }
}

#[test]
fn classpath_is_ordered_canonical_and_ends_with_the_client() {
    let p = plan(V1_21_1, "1.21.1", SHA_1_21_1, MetaOs::Windows, "x86_64");
    assert_eq!(p.classpath.last(), Some(&p.client.path));
    assert_eq!(
        p.client.path,
        "shared/clients/30c73b1c5da787909b2f73340419fdf13b9def88/client.jar"
    );
    assert_eq!(p.client.size, 26_836_906);
    let unique: HashSet<_> = p.classpath.iter().collect();
    assert_eq!(unique.len(), p.classpath.len());
    for (entry, library) in p.classpath.iter().zip(&p.libraries) {
        assert_eq!(entry, &library.path, "classpath follows descriptor order");
        assert_eq!(
            entry,
            &format!("shared/libraries/{}/artifact.jar", library.sha1)
        );
    }
    assert_eq!(
        p.libraries.first().unwrap().provenance.as_deref(),
        Some("com.github.oshi:oshi-core:6.4.10")
    );
}

#[test]
fn arguments_follow_rules_per_os() {
    let mac = plan(V1_21_1, "1.21.1", SHA_1_21_1, MetaOs::Osx, "arm64");
    assert_eq!(
        mac.jvm_arguments.first().map(String::as_str),
        Some("-XstartOnFirstThread")
    );
    let win = plan(V1_21_1, "1.21.1", SHA_1_21_1, MetaOs::Windows, "x86_64");
    assert!(
        win.jvm_arguments
            .iter()
            .any(|a| a.starts_with("-XX:HeapDumpPath="))
    );
    assert!(
        !win.jvm_arguments
            .iter()
            .any(|a| a == "-XstartOnFirstThread")
    );
    for p in [&mac, &win] {
        assert!(
            !p.jvm_arguments.iter().any(|a| a == "-Xss1M"),
            "x86 rule never matches"
        );
        let cp = p.jvm_arguments.iter().position(|a| a == "-cp").unwrap();
        assert_eq!(p.jvm_arguments[cp + 1], "${classpath}");
        assert!(
            p.game_arguments
                .windows(2)
                .any(|w| w == ["--width", "${resolution_width}"])
        );
        assert!(
            !p.game_arguments
                .iter()
                .any(|a| a == "--demo" || a.starts_with("--quickPlay"))
        );
    }
    let new = plan(V26_3, "26.3", SHA_26_3, MetaOs::Linux, "x86_64");
    for suffix in ["java", "jna", "lwjgl", "netty"] {
        assert!(
            new.jvm_arguments
                .iter()
                .any(|a| a.ends_with(&format!("${{natives_directory}}/{suffix}")))
        );
    }
    assert!(new.game_arguments.iter().any(|a| a == "${auth_xuid}"));
    assert!(
        !new.game_arguments.iter().any(|a| a == "--userType"),
        "26.3 dropped userType"
    );
}

#[test]
fn logging_and_asset_index_are_planned() {
    let p = plan(V1_21_1, "1.21.1", SHA_1_21_1, MetaOs::Linux, "x86_64");
    let logging = p.logging.as_ref().unwrap();
    assert_eq!(logging.id.as_str(), "client-1.12.xml");
    assert_eq!(logging.argument, "-Dlog4j.configurationFile=${path}");
    assert_eq!(
        logging.artifact.path,
        "shared/log-configs/bd65e7d2e3c237be76cfbef4c2405033d7f91521/client-1.12.xml"
    );
    assert_eq!(p.asset_index.id.as_str(), "17");
    assert_eq!(
        p.asset_index.artifact.sha1.as_str(),
        "15200809663f45954c2c9a33ce88849e61aaaecc"
    );
    assert_eq!(p.asset_index.artifact.size, 449_557);
    assert_eq!(
        p.asset_index.artifact.path,
        "metadata/asset-indexes/15200809663f45954c2c9a33ce88849e61aaaecc.json"
    );
}

#[test]
fn every_url_is_on_the_source_allowlist() {
    for (bytes, version, sha) in [(V1_21_1, "1.21.1", SHA_1_21_1), (V26_3, "26.3", SHA_26_3)] {
        let p = plan(bytes, version, sha, MetaOs::Windows, "x86_64");
        for a in p.artifacts() {
            let policy = if a.role == ArtifactRole::AssetIndex {
                SourcePolicy::MojangMetadata
            } else {
                SourcePolicy::MojangArtifacts
            };
            assert!(policy.check(&a.url).is_ok(), "{}", a.url);
        }
    }
}

#[test]
fn plans_are_deterministic_and_serializable() {
    let a = plan(V26_3, "26.3", SHA_26_3, MetaOs::Linux, "x86_64");
    let b = plan(V26_3, "26.3", SHA_26_3, MetaOs::Linux, "x86_64");
    assert_eq!(a, b);
    let json = serde_json::to_string(&a).unwrap();
    assert_eq!(serde_json::from_str::<GamePlan>(&json).unwrap(), a);
}

#[test]
fn asset_objects_are_deduplicated_by_hash_and_names_are_not_paths() {
    let mut p = plan(V1_21_1, "1.21.1", SHA_1_21_1, MetaOs::Linux, "x86_64");
    plan_assets(&mut p, SMALL_INDEX).unwrap();
    assert_eq!(p.assets.len(), 4, "five names, four unique hashes");
    let evil = p
        .assets
        .iter()
        .find(|a| a.provenance.as_deref() == Some("../../evil/path"))
        .unwrap();
    assert_eq!(
        evil.path,
        "shared/assets/objects/ab/ab00000000000000000000000000000000000002"
    );
    assert!(
        p.assets.windows(2).all(|w| w[0].sha1 < w[1].sha1),
        "sorted by hash"
    );
    for a in &p.assets {
        assert!(SourcePolicy::MojangArtifacts.check(&a.url).is_ok());
    }
    let total = p.total_bytes();
    assert_eq!(
        total,
        p.client.size
            + p.libraries.iter().map(|l| l.size).sum::<u64>()
            + 449_557
            + 888
            + 3665
            + 5362
            + 10
            + 20
    );
}

#[test]
fn asset_index_problems_are_rejected() {
    let mut p = plan(V1_21_1, "1.21.1", SHA_1_21_1, MetaOs::Linux, "x86_64");
    let conflict = json!({"objects": {"a": {"hash": "0000000000000000000000000000000000000001", "size": 1},
                                       "b": {"hash": "0000000000000000000000000000000000000001", "size": 2}}});
    assert_eq!(
        plan_assets(&mut p, conflict.to_string().as_bytes())
            .unwrap_err()
            .code,
        ErrorCode::MetadataConflict
    );
    for bad in [
        json!({"virtual": true, "objects": {"../escape": {"hash": "0000000000000000000000000000000000000001", "size": 1}}}),
        json!({"objects": {"a": {"hash": "xyz", "size": 1}}}),
        json!({"objects": {"a": {"hash": "0000000000000000000000000000000000000001"}}}),
        json!({}),
    ] {
        assert_eq!(
            plan_assets(&mut p, bad.to_string().as_bytes())
                .unwrap_err()
                .code,
            ErrorCode::UnsupportedVersionMetadata,
            "{bad}"
        );
    }
}

fn mutate(edit: impl FnOnce(&mut Value)) -> Vec<u8> {
    let mut value: Value = serde_json::from_slice(V1_21_1).unwrap();
    edit(&mut value);
    serde_json::to_vec(&value).unwrap()
}

/// Index of the first library without rules, i.e. applicable on every target.
fn uni() -> usize {
    let v: Value = serde_json::from_slice(V1_21_1).unwrap();
    v["libraries"]
        .as_array()
        .unwrap()
        .iter()
        .position(|l| l.get("rules").is_none())
        .unwrap()
}

fn try_plan(bytes: &[u8]) -> LauncherResult<GamePlan> {
    plan_game(
        bytes,
        "https://piston-meta.mojang.com/x.json",
        &Sha1::parse(SHA_1_21_1).unwrap(),
        "1.21.1",
        &ctx(MetaOs::Linux, "x86_64"),
    )
}

type Edit = Box<dyn FnOnce(&mut Value)>;

#[test]
fn unsupported_formats_and_unsafe_metadata_are_rejected() {
    let cases: Vec<Edit> = vec![
        Box::new(|v| v["inheritsFrom"] = json!("1.21")),
        Box::new(|v| v["type"] = json!("experiment")),
        Box::new(|v| {
            v.as_object_mut().unwrap().remove("arguments");
        }),
        Box::new(|v| {
            v.as_object_mut().unwrap().remove("arguments");
            v["minecraftArguments"] = json!("--username ${auth_player_name} ${unknown}");
        }),
        Box::new(|v| v["id"] = json!("1.21.2")),
        Box::new(|v| {
            v["downloads"]
                .as_object_mut()
                .unwrap()
                .remove("client")
                .map(|_| ())
                .unwrap()
        }),
        Box::new(|v| v["mainClass"] = json!("net.minecraft;rm -rf")),
        Box::new(|v| v["libraries"][uni()]["natives"] = json!({"linux": "natives-linux"})),
        Box::new(|v| v["libraries"][uni()]["downloads"]["classifiers"] = json!({})),
        Box::new(|v| {
            v["libraries"][uni()]["downloads"]["artifact"]["path"] = json!("../../evil.jar")
        }),
        Box::new(|v| {
            v["libraries"][uni()]["downloads"]["artifact"]["url"] =
                json!("https://evil.example/a.jar")
        }),
        Box::new(|v| {
            v["libraries"][uni()]["downloads"]["artifact"]["url"] =
                json!("http://libraries.minecraft.net/a.jar")
        }),
        Box::new(|v| v["libraries"][uni()]["downloads"]["artifact"]["sha1"] = json!("nothex")),
        Box::new(|v| v["downloads"]["client"]["url"] = json!("https://cdn.example/client.jar")),
        Box::new(|v| {
            v["arguments"]["jvm"]
                .as_array_mut()
                .unwrap()
                .push(json!("-p ${launcher_secret_dir}"))
        }),
        Box::new(|v| {
            v["arguments"]["game"]
                .as_array_mut()
                .unwrap()
                .push(json!("${unknown_value}"))
        }),
        Box::new(|v| {
            v["libraries"][uni()]["rules"] = json!([{"action": "allow", "os": {"distro": "x"}}])
        }),
        Box::new(|v| v["assetIndex"]["id"] = json!("../x")),
        Box::new(|v| v["logging"]["client"]["file"]["id"] = json!("../log.xml")),
    ];
    for (i, edit) in cases.into_iter().enumerate() {
        let error = try_plan(&mutate(edit)).unwrap_err();
        assert_eq!(
            error.code,
            ErrorCode::UnsupportedVersionMetadata,
            "case {i}"
        );
    }
    assert!(try_plan(b"{not json").is_err());
}

#[test]
fn feature_gated_placeholders_need_no_implementation() {
    // quickPlay placeholders exist in the descriptors but are excluded by features.
    let p = plan(V1_21_1, "1.21.1", SHA_1_21_1, MetaOs::Linux, "x86_64");
    for argument in p.jvm_arguments.iter().chain(&p.game_arguments) {
        assert!(check_placeholders(argument).is_ok(), "{argument}");
    }
}

/// MV03: same Maven coordinate, different digests stay distinct.
#[test]
fn same_coordinate_with_different_digests_is_distinct_content() {
    let bytes = mutate(|v| {
        let mut copy = v["libraries"][uni()].clone();
        copy["downloads"]["artifact"]["sha1"] = json!("1111111111111111111111111111111111111111");
        v["libraries"].as_array_mut().unwrap().push(copy);
    });
    let p = try_plan(&bytes).unwrap();
    let name = p.libraries[0].provenance.clone();
    let same_name: Vec<_> = p
        .libraries
        .iter()
        .filter(|l| l.provenance == name)
        .collect();
    assert_eq!(same_name.len(), 2);
    assert_ne!(same_name[0].path, same_name[1].path);
}

/// MV04: different coordinates with identical digest and size are one object; a
/// conflicting size for that digest is METADATA_CONFLICT.
#[test]
fn identical_digest_is_reused_and_conflicting_size_is_rejected() {
    let base = try_plan(V1_21_1).unwrap();
    let bytes = mutate(|v| {
        let mut copy = v["libraries"][uni()].clone();
        copy["name"] = json!("com.example:renamed:1.0");
        copy["downloads"]["artifact"]["path"] = json!("com/example/renamed/1.0/renamed-1.0.jar");
        copy["downloads"]["artifact"]["url"] =
            json!("https://libraries.minecraft.net/com/example/renamed-1.0.jar");
        v["libraries"].as_array_mut().unwrap().push(copy);
    });
    let p = try_plan(&bytes).unwrap();
    assert_eq!(p.libraries.len(), base.libraries.len());
    assert_eq!(p.classpath, base.classpath);

    let conflict = mutate(|v| {
        let mut copy = v["libraries"][uni()].clone();
        copy["name"] = json!("com.example:renamed:1.0");
        copy["downloads"]["artifact"]["size"] = json!(1);
        v["libraries"].as_array_mut().unwrap().push(copy);
    });
    assert_eq!(
        try_plan(&conflict).unwrap_err().code,
        ErrorCode::MetadataConflict
    );
}

#[test]
fn unsupported_targets_are_explicit() {
    let linux_arm = RuleContext::with_default_features(MetaOs::Linux, "arm64", None);
    assert_eq!(
        plan_game(
            V1_21_1,
            "u",
            &Sha1::parse(SHA_1_21_1).unwrap(),
            "1.21.1",
            &linux_arm
        )
        .unwrap_err()
        .code,
        ErrorCode::UnsupportedPlatform
    );
}

#[test]
fn windows_on_arm_runs_new_versions_natively_and_old_ones_with_x64_java() {
    let p = plan(V1_21_1, "1.21.1", SHA_1_21_1, MetaOs::Windows, "arm64");
    assert_eq!(p.runtime.os, TargetOs::Windows);
    assert_eq!(p.runtime.arch, TargetArch::Aarch64);
    assert!(
        p.libraries
            .iter()
            .any(|l| l.provenance.as_deref() == Some("org.lwjgl:lwjgl:3.3.3:natives-windows-arm64")),
        "LWJGL picks its ARM natives from the classpath"
    );

    let legacy = plan_legacy(MetaOs::Windows, "arm64").unwrap();
    assert_eq!(legacy.runtime.arch, TargetArch::X64);
    assert!(legacy.natives[0].path.contains("6666"), "the x64 natives");
}

/// A pre-1.13 descriptor in the legacy shape: `minecraftArguments`, native
/// classifier jars with `${arch}`, no `javaVersion`.
fn legacy_descriptor() -> Vec<u8> {
    let lib = |sha: &str| json!({"url": format!("https://libraries.minecraft.net/{sha}.jar"), "sha1": sha, "size": 10, "path": format!("x/{sha}.jar")});
    json!({
        "id": "1.12.2", "type": "release", "mainClass": "net.minecraft.client.main.Main",
        "minecraftArguments": "--username ${auth_player_name} --version ${version_name} --gameDir ${game_directory} --assetsDir ${assets_root} --assetIndex ${assets_index_name} --uuid ${auth_uuid} --accessToken ${auth_access_token} --userType ${user_type} --versionType ${version_type}",
        "downloads": {"client": {"url": "https://piston-data.mojang.com/c.jar", "sha1": "1111111111111111111111111111111111111111", "size": 5}},
        "assetIndex": {"id": "1.12", "url": "https://piston-meta.mojang.com/i.json", "sha1": "2222222222222222222222222222222222222222", "size": 3, "totalSize": 4},
        "libraries": [
            {"name": "org.lwjgl.lwjgl:lwjgl:2.9.4", "downloads": {"artifact": lib("3333333333333333333333333333333333333333")}},
            {"name": "org.lwjgl.lwjgl:lwjgl-platform:2.9.4",
             "natives": {"linux": "natives-linux", "osx": "natives-osx", "windows": "natives-windows-${arch}"},
             "extract": {"exclude": ["META-INF/"]},
             "downloads": {"classifiers": {
                 "natives-linux": lib("4444444444444444444444444444444444444444"),
                 "natives-osx": lib("5555555555555555555555555555555555555555"),
                 "natives-windows-64": lib("6666666666666666666666666666666666666666")}}}
        ]
    })
    .to_string()
    .into_bytes()
}

fn plan_legacy(os: MetaOs, arch: &'static str) -> LauncherResult<GamePlan> {
    plan_game(
        &legacy_descriptor(),
        "https://piston-meta.mojang.com/1.12.2.json",
        &Sha1::parse(SHA_1_21_1).unwrap(),
        "1.12.2",
        &ctx(os, arch),
    )
}

#[test]
fn legacy_descriptors_plan_natives_and_launcher_jvm_arguments() {
    let linux = plan_legacy(MetaOs::Linux, "x86_64").unwrap();
    assert_eq!(linux.runtime.major, 8, "no javaVersion means Java 8");
    assert_eq!(linux.natives.len(), 1);
    assert_eq!(
        linux.natives[0].path,
        "shared/libraries/4444444444444444444444444444444444444444/artifact.jar"
    );
    assert_eq!(linux.natives[0].exclude, ["META-INF/"]);
    // Native jars are downloaded but never on the classpath.
    assert!(
        linux
            .libraries
            .iter()
            .any(|l| l.path == linux.natives[0].path)
    );
    assert!(!linux.classpath.contains(&linux.natives[0].path));
    assert_eq!(linux.classpath.len(), 2, "lwjgl jar and client");
    assert_eq!(linux.game_arguments[0], "--username");
    assert_eq!(linux.game_arguments[1], "${auth_player_name}");
    let cp = linux.jvm_arguments.iter().position(|a| a == "-cp").unwrap();
    assert_eq!(linux.jvm_arguments[cp + 1], "${classpath}");
    assert!(
        linux
            .jvm_arguments
            .contains(&"-Djava.library.path=${natives_directory}".to_owned())
    );

    let windows = plan_legacy(MetaOs::Windows, "x86_64").unwrap();
    assert!(
        windows.natives[0].path.contains("6666"),
        "the arch template resolves to 64"
    );
    assert!(windows.jvm_arguments[0].starts_with("-XX:HeapDumpPath="));
    let mac = plan_legacy(MetaOs::Osx, "x86_64").unwrap();
    assert!(mac.natives[0].path.contains("5555"));

    // Intel-only natives cannot run on Apple silicon.
    assert_eq!(
        plan_legacy(MetaOs::Osx, "arm64").unwrap_err().code,
        ErrorCode::UnsupportedPlatform
    );
}

#[test]
fn snapshots_and_old_versions_are_accepted_and_java_16_runs_on_17() {
    for version_type in ["snapshot", "old_beta", "old_alpha"] {
        let p = try_plan(&mutate(|v| v["type"] = json!(version_type))).unwrap();
        assert_eq!(p.version_type, version_type);
    }
    let p = try_plan(&mutate(|v| v["javaVersion"]["majorVersion"] = json!(16))).unwrap();
    assert_eq!(p.runtime.major, 17);
}

#[test]
fn legacy_asset_indexes_choose_their_layout() {
    let mut p = plan(V1_21_1, "1.21.1", SHA_1_21_1, MetaOs::Linux, "x86_64");
    let object = json!({"hash": "0000000000000000000000000000000000000001", "size": 1});
    plan_assets(
        &mut p,
        json!({"virtual": true, "objects": {"sound/a.ogg": object}})
            .to_string()
            .as_bytes(),
    )
    .unwrap();
    assert_eq!(p.asset_layout, AssetLayout::Virtual);
    plan_assets(
        &mut p,
        json!({"map_to_resources": true, "objects": {"sound/a.ogg": object}})
            .to_string()
            .as_bytes(),
    )
    .unwrap();
    assert_eq!(p.asset_layout, AssetLayout::Resources);
    plan_assets(
        &mut p,
        json!({"objects": {"x": object}}).to_string().as_bytes(),
    )
    .unwrap();
    assert_eq!(p.asset_layout, AssetLayout::Modern);
}

fn loader_library(name: &str, url: &str) -> crate::loader::manifest::LoaderLibrary {
    let path = Coordinate::parse(name).unwrap().path();
    crate::loader::manifest::LoaderLibrary {
        name: name.into(),
        url: if url.is_empty() {
            String::new()
        } else {
            format!("{url}{path}")
        },
        path,
        sha1: Some(Sha1::parse(&"c".repeat(40)).unwrap()),
        size: Some(10),
        rules: None,
    }
}

fn maven_plan() -> GamePlan {
    plan_game_in(
        V1_21_1,
        "https://piston-meta.mojang.com/v1/packages/x/1.21.1.json",
        &Sha1::parse(SHA_1_21_1).unwrap(),
        "1.21.1",
        &ctx(MetaOs::Linux, "x86_64"),
        LibraryLayout::Maven,
    )
    .unwrap()
}

#[test]
fn maven_layout_keeps_real_library_names() {
    let p = maven_plan();
    assert!(
        p.classpath
            .iter()
            .any(|c| c == "shared/maven/com/mojang/brigadier/1.3.10/brigadier-1.3.10.jar"),
        "{:?}",
        p.classpath
    );
    assert!(
        p.libraries
            .iter()
            .all(|l| l.path.starts_with("shared/maven/"))
    );
    assert!(p.mappings.is_some(), "1.21.1 publishes client mappings");
    assert!(p.loader.is_none() && p.loader_files.is_empty());
}

#[test]
fn a_loader_profile_merges_onto_the_game() {
    use crate::loader::manifest::LoaderManifest;
    let base = maven_plan();
    let manifest = LoaderManifest {
        schema_version: 1,
        kind: LoaderKind::Fabric,
        version: "0.16.14".into(),
        game_version: "1.21.1".into(),
        source: "https://meta.fabricmc.net/v2/x".into(),
        profile_id: "fabric-loader-0.16.14-1.21.1".into(),
        main_class: "net.fabricmc.loader.impl.launch.knot.KnotClient".into(),
        libraries: vec![
            loader_library("org.ow2.asm:asm:9.8", "https://maven.fabricmc.net/"),
            // Replaces the game's gson 2.11.0.
            loader_library(
                "com.google.code.gson:gson:2.99",
                "https://maven.fabricmc.net/",
            ),
            loader_library(
                "net.fabricmc:fabric-loader:0.16.14",
                "https://maven.fabricmc.net/",
            ),
        ],
        jvm_arguments: vec![json!("-DFabricMcEmu= net.minecraft.client.main.Main ")],
        game_arguments: vec![],
        minecraft_arguments: None,
        installer: None,
        server_arguments: None,
    };
    let sha = Sha1::parse(&"d".repeat(40)).unwrap();
    let merged =
        apply_loader(base.clone(), &manifest, &sha, &ctx(MetaOs::Linux, "x86_64")).unwrap();
    assert_eq!(merged.main_class, manifest.main_class);
    assert_eq!(merged.game_main_class(), "net.minecraft.client.main.Main");
    assert_eq!(merged.version_name(), "fabric-loader-0.16.14-1.21.1");
    assert_eq!(
        merged.classpath[0],
        "shared/maven/org/ow2/asm/asm/9.8/asm-9.8.jar"
    );
    assert!(
        !merged.classpath.iter().any(|c| c.contains("gson/2.11")),
        "replaced"
    );
    assert!(merged.classpath.iter().any(|c| c.contains("gson/2.99")));
    assert_eq!(
        merged.classpath.last().unwrap(),
        &format!(
            "shared/clients/{}/fabric-loader-0.16.14-1.21.1.jar",
            base.client.sha1
        )
    );
    assert_eq!(merged.client.path, *merged.classpath.last().unwrap());
    assert_eq!(merged.classpath.len(), base.classpath.len() + 2);
    assert_eq!(
        merged.jvm_arguments.last().unwrap(),
        "-DFabricMcEmu= net.minecraft.client.main.Main "
    );
    assert_eq!(merged.game_arguments, base.game_arguments);
    assert!(merged.loader_files.is_empty());
    let loader = merged.loader.as_ref().unwrap();
    assert_eq!(loader.manifest_sha1, sha);
    assert!(loader.provided.is_empty());

    // Another game version, a second loader, or a host off the lists: refused.
    let mut other = manifest.clone();
    other.game_version = "1.20.1".into();
    assert!(apply_loader(base.clone(), &other, &sha, &ctx(MetaOs::Linux, "x86_64")).is_err());
    assert!(apply_loader(merged, &manifest, &sha, &ctx(MetaOs::Linux, "x86_64")).is_err());
    let mut evil = manifest.clone();
    evil.libraries
        .push(loader_library("a.b:c:1", "https://cdn.example/"));
    assert!(apply_loader(base.clone(), &evil, &sha, &ctx(MetaOs::Linux, "x86_64")).is_err());
    // Files only an installer can provide need an installer.
    let mut provided = manifest;
    provided.libraries.push(loader_library("a.b:c:1", ""));
    assert!(apply_loader(base, &provided, &sha, &ctx(MetaOs::Linux, "x86_64")).is_err());
}

/// OptiFine's own patcher, run as an installation runs it: set
/// `OPTIFINE_JAR` (a 1.21.1 build), `CLIENT_JAR` (Mojang's 1.21.1 client) and
/// `JAVA`, then run with `--ignored`.
#[test]
#[ignore = "needs an OptiFine build, the 1.21.1 client and Java"]
fn optifine_builds_its_library_with_its_own_patcher() {
    use crate::loader::optifine::{Release, installer_coordinate, manifest};
    let var = |name: &str| std::env::var(name).unwrap_or_else(|_| panic!("set {name}"));
    let jar = std::fs::read(var("OPTIFINE_JAR")).unwrap();
    let release = Release {
        id: "1.21.1_HD_U_J1".into(),
        game: "1.21.1".into(),
        label: "HD U J1".into(),
        stable: true,
        url: "https://cdn-mods.lapi.sh/f/x".into(),
        sha256: String::new(),
        size: jar.len() as u64,
    };
    let path = crate::loader::coordinate::Coordinate::parse(&installer_coordinate(&release.id))
        .unwrap()
        .path();
    let manifest = manifest(&jar, &release, &path).unwrap();
    let sha = Sha1::parse(&"d".repeat(40)).unwrap();
    let plan = apply_loader(maven_plan(), &manifest, &sha, &ctx(MetaOs::Linux, "x86_64")).unwrap();
    assert_eq!(plan.loader_files.len(), 1);
    assert!(
        plan.game_arguments
            .ends_with(&["--tweakClass".into(), "optifine.OptiFineTweaker".into()])
    );

    let dir = tempfile::tempdir().unwrap();
    let root = crate::storage::paths::DataRoot::new(dir.path().to_path_buf()).unwrap();
    let place = |relative: &str, bytes: &[u8]| {
        let file = crate::download::resolve_managed_path(&root, relative).unwrap();
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, bytes).unwrap();
    };
    place(
        &plan.client.path,
        &std::fs::read(var("CLIENT_JAR")).unwrap(),
    );
    place(&crate::loader::manifest::maven_path(&path), &jar);
    let java = std::path::PathBuf::from(var("JAVA"));
    let lines = std::sync::Mutex::new(Vec::new());
    let log = |line: String| lines.lock().unwrap().push(line);
    let cancel = crate::download::CancelToken::new();
    let generated = crate::loader::processors::prepare(&crate::loader::processors::Inputs {
        root: &root,
        manifest: &manifest,
        plan: &plan,
        java: &java,
        work_dir: dir.path().join("work"),
        cancel: &cancel,
        log: &log,
        quiet_limit: crate::loader::processors::QUIET_LIMIT,
    })
    .unwrap_or_else(|e| panic!("{e:?} {:?}", lines.lock().unwrap()));
    let paths: Vec<_> = generated.iter().map(|a| a.path.as_str()).collect();
    assert_eq!(
        paths,
        [
            "shared/maven/optifine/OptiFine/1.21.1_HD_U_J1/OptiFine-1.21.1_HD_U_J1.jar",
            "shared/maven/optifine/launchwrapper-of/2.3/launchwrapper-of-2.3.jar",
        ]
    );
    assert!(generated[0].size > 1_000_000, "{:?}", generated[0]);
    assert!(crate::loader::processors::verify_generated(
        &root, &generated
    ));
}

#[test]
fn forge_bootstrap_gets_the_library_directory_when_the_profile_leaves_it_out() {
    use crate::loader::manifest::LoaderManifest;
    let manifest = |jvm: Vec<serde_json::Value>| LoaderManifest {
        schema_version: 1,
        kind: LoaderKind::Forge,
        version: "49.0.2".into(),
        game_version: "1.21.1".into(),
        source: "https://maven.minecraftforge.net/x".into(),
        profile_id: "1.20.3-forge-49.0.2".into(),
        main_class: "net.minecraftforge.bootstrap.ForgeBootstrap".into(),
        libraries: vec![],
        jvm_arguments: jvm,
        game_arguments: vec![],
        minecraft_arguments: None,
        installer: None,
        server_arguments: None,
    };
    let sha = Sha1::parse(&"d".repeat(40)).unwrap();
    let count = |plan: &GamePlan| {
        plan.jvm_arguments
            .iter()
            .filter(|a| a.starts_with("-DlibraryDirectory="))
            .count()
    };
    let without = manifest(vec![json!("-Djava.net.preferIPv6Addresses=system")]);
    let plan = apply_loader(maven_plan(), &without, &sha, &ctx(MetaOs::Linux, "x86_64")).unwrap();
    assert_eq!(
        plan.jvm_arguments.last().map(String::as_str),
        Some("-DlibraryDirectory=${library_directory}")
    );
    assert_eq!(count(&plan), 1);
    let with = manifest(vec![json!("-DlibraryDirectory=${library_directory}")]);
    let plan = apply_loader(maven_plan(), &with, &sha, &ctx(MetaOs::Linux, "x86_64")).unwrap();
    assert_eq!(count(&plan), 1);
}
