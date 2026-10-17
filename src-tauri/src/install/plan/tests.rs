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
                .push(json!("-p ${library_directory}"))
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
    let windows_arm = RuleContext::with_default_features(MetaOs::Windows, "arm64", None);
    assert_eq!(
        plan_game(
            V1_21_1,
            "u",
            &Sha1::parse(SHA_1_21_1).unwrap(),
            "1.21.1",
            &windows_arm
        )
        .unwrap_err()
        .code,
        ErrorCode::UnsupportedPlatform
    );
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
