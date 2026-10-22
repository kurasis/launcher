use std::cmp::Ordering;
use std::io::Write;

use serde_json::json;

use crate::{
    Clash, Jar, Loader, Manifest, Need, Problem, Setup, Side, allows, check, check_with, clashes,
    compare, for_loader, read_bytes,
};

fn zip_of(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for (name, bytes) in entries {
        zip.start_file(*name, options).unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

fn fabric_jar(meta: serde_json::Value, nested: &[(&str, Vec<u8>)]) -> Vec<u8> {
    let text = meta.to_string();
    let mut entries: Vec<(&str, &[u8])> = vec![("fabric.mod.json", text.as_bytes())];
    for (name, bytes) in nested {
        entries.push((name, bytes));
    }
    zip_of(&entries)
}

fn fabric_mod(id: &str, version: &str, depends: serde_json::Value) -> Vec<u8> {
    fabric_jar(
        json!({"schemaVersion": 1, "id": id, "version": version, "depends": depends}),
        &[],
    )
}

fn strings(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| (*s).to_owned()).collect()
}

// ----- versions -----

#[test]
fn versions_compare_as_the_loaders_do() {
    let cases = [
        ("1.21", "1.21.0", Ordering::Equal),
        ("1.21.1", "1.21", Ordering::Greater),
        ("1.21-pre1", "1.21", Ordering::Less),
        ("1.21-rc1", "1.21-pre2", Ordering::Greater),
        ("0.16.10", "0.16.9", Ordering::Greater),
        ("1.0.0-beta.2", "1.0.0-beta.1", Ordering::Greater),
        ("1.0.0-alpha", "1.0.0-beta", Ordering::Less),
        ("1.0.0+build.7", "1.0.0", Ordering::Equal),
        ("47.3.0", "47.2.11", Ordering::Greater),
        ("26.1.2", "26.1", Ordering::Greater),
        ("20.4.80-beta", "20.4.80", Ordering::Less),
        ("v2.1", "2.0", Ordering::Greater),
        ("1.0.1", "1.0-beta", Ordering::Greater),
    ];
    for (a, b, want) in cases {
        assert_eq!(compare(a, b), Some(want), "{a} {b}");
        assert_eq!(compare(b, a), Some(want.reverse()), "{b} {a}");
    }
    for (a, b) in [
        ("mc1.21-2.0", "2.0"),
        ("${version}", "1.0"),
        ("", "1.0"),
        ("1.0", "1.\n0"),
        ("1 0", "1.0"),
    ] {
        assert_eq!(compare(a, b), None, "{a} {b}");
    }
    assert_eq!(compare(&"9".repeat(200), "1"), None);
    assert_eq!(compare("99999999999999999999999", "1"), None);
}

#[test]
fn maven_ranges_are_read() {
    let forge = |range: &str, version: &str| allows(Loader::Forge, &strings(&[range]), version);
    assert_eq!(forge("[1.21.1,)", "1.21"), Some(false));
    assert_eq!(forge("[1.21.1,)", "1.21.1"), Some(true));
    assert_eq!(forge("[1.21.1,)", "1.21.4"), Some(true));
    assert_eq!(forge("[1.20,1.21)", "1.20.6"), Some(true));
    assert_eq!(forge("[1.20,1.21)", "1.21"), Some(false));
    assert_eq!(forge("(1.20,1.21]", "1.20"), Some(false));
    assert_eq!(forge("(1.20,1.21]", "1.21"), Some(true));
    assert_eq!(forge("(,1.20]", "1.19.2"), Some(true));
    assert_eq!(forge("[1.20.1]", "1.20.1"), Some(true));
    assert_eq!(forge("[1.20.1]", "1.20.2"), Some(false));
    assert_eq!(forge("[1.0,2.0),[3.0,)", "2.5"), Some(false));
    assert_eq!(forge("[1.0,2.0),[3.0,)", "3.1"), Some(true));
    assert_eq!(forge(" [ 47 , ) ", "47.3.0"), Some(true));
    assert_eq!(forge("[47,)", "46.0.14"), Some(false));
    // A bare version only recommends itself.
    assert_eq!(forge("1.20.1", "1.21"), Some(true));
    assert_eq!(forge("*", "1.21"), Some(true));
    assert_eq!(forge("", "1.21"), Some(true));
    assert_eq!(forge("[1.0", "1.21"), None);
    assert_eq!(forge("[1.0,2.0)x", "1.5"), Some(true));
    assert_eq!(forge("[1.0,2.0)x", "2.5"), None);
    assert_eq!(forge("[1.21.1,)", "${version}"), None);
    assert_eq!(allows(Loader::NeoForge, &[], "anything"), Some(true));
}

#[test]
fn fabric_predicates_are_read() {
    let fabric = |range: &[&str], version: &str| allows(Loader::Fabric, &strings(range), version);
    assert_eq!(fabric(&[">=0.16"], "0.15.11"), Some(false));
    assert_eq!(fabric(&[">=0.16"], "0.16.5"), Some(true));
    assert_eq!(fabric(&[">=0.16 <1"], "1.0.0"), Some(false));
    assert_eq!(fabric(&["~1.21.1"], "1.21.4"), Some(true));
    assert_eq!(fabric(&["~1.21.1"], "1.21"), Some(false));
    assert_eq!(fabric(&["~1.21.1"], "1.22"), Some(false));
    assert_eq!(fabric(&["~1.21"], "1.21.5"), Some(true));
    assert_eq!(fabric(&["1.21.x"], "1.21.4"), Some(true));
    assert_eq!(fabric(&["1.21.x"], "1.22"), Some(false));
    assert_eq!(fabric(&["1.21.x"], "1.21"), Some(true));
    assert_eq!(fabric(&[">=1.20.x"], "1.20"), Some(true));
    assert_eq!(fabric(&["<1.21.x"], "1.21.1"), Some(false));
    assert_eq!(fabric(&["1.21"], "1.21.1"), Some(false));
    assert_eq!(fabric(&["=1.21.1"], "1.21.1"), Some(true));
    assert_eq!(fabric(&["^1.2.3"], "1.9"), Some(true));
    assert_eq!(fabric(&["^1.2.3"], "2.0"), Some(false));
    assert_eq!(fabric(&["*"], "26.1"), Some(true));
    assert_eq!(fabric(&[">=1.21", "1.20.x"], "1.20.4"), Some(true));
    assert_eq!(fabric(&[">=1.21", "1.20.x"], "1.19.4"), Some(false));
    assert_eq!(fabric(&[">=1.21- <1.21.2-"], "1.21.1"), Some(true));
    assert_eq!(fabric(&[">1.21"], "1.21"), Some(false));
    assert_eq!(fabric(&["<=1.21"], "1.21"), Some(true));
    assert_eq!(fabric(&[">=1.0"], "${version}"), None);
    // One range that cannot be told and one that says no: can't tell.
    assert_eq!(fabric(&[">=1.0", ">=mc2"], "0.5"), None);
    assert_eq!(fabric(&[&"1".repeat(600)], "1"), None);
    assert_eq!(
        allows(Loader::Quilt, &strings(&[">=0.16 <1"]), "0.17"),
        Some(true)
    );
}

// ----- reading jars -----

#[test]
fn a_fabric_jar_and_the_jars_inside_it_are_read() {
    let deep = fabric_mod("deep", "1.0.0", json!({"fabricloader": ">=0.15"}));
    let inner = fabric_jar(
        json!({"schemaVersion": 1, "id": "fabric-api-base", "version": "0.4.42",
               "depends": {"fabricloader": ">=0.16.9"},
               "jars": [{"file": "META-INF/jars/deep.jar"}]}),
        &[("META-INF/jars/deep.jar", deep)],
    );
    let jar = fabric_jar(
        json!({"schemaVersion": 1, "id": "fabric-api", "version": "0.110.0+1.21.1",
               "environment": "*", "provides": ["fabric", "Bad Id"],
               "depends": {"fabricloader": ">=0.16.9", "minecraft": ["~1.21.1", "1.21"], "java": ">=21",
                           "bad": {"x": 1}},
               "breaks": {"optifabric": "<1.13"},
               "jars": [{"file": "META-INF/jars/base.jar"}, {"file": "META-INF/jars/none.jar"}]}),
        &[("META-INF/jars/base.jar", inner)],
    );
    let read = read_bytes(&jar);
    assert_eq!(read.len(), 1, "{read:?}");
    let m = &read[0];
    assert_eq!(m.loader, Loader::Fabric);
    let mods: Vec<(&str, Option<&str>, bool)> = m
        .mods
        .iter()
        .map(|p| (p.id.as_str(), p.version.as_deref(), p.nested))
        .collect();
    assert_eq!(
        mods,
        [
            ("fabric-api", Some("0.110.0+1.21.1"), false),
            ("fabric", Some("0.110.0+1.21.1"), false),
            ("fabric-api-base", Some("0.4.42"), true),
            ("deep", Some("1.0.0"), true),
        ]
    );
    let needs: Vec<(&str, &str, Vec<String>)> = m
        .needs
        .iter()
        .map(|n| (n.by.as_str(), n.id.as_str(), n.versions.clone()))
        .collect();
    assert!(
        needs.contains(&("fabric-api", "minecraft", strings(&["~1.21.1", "1.21"]))),
        "{needs:?}"
    );
    assert!(
        needs.contains(&("fabric-api-base", "fabricloader", strings(&[">=0.16.9"]))),
        "{needs:?}"
    );
    assert!(
        needs.contains(&("deep", "fabricloader", strings(&[">=0.15"]))),
        "{needs:?}"
    );
    assert!(!needs.iter().any(|n| n.1 == "bad"), "{needs:?}");
    assert_eq!(m.breaks.len(), 1);
    assert_eq!(m.breaks[0].id, "optifabric");
    // As the catalog keeps it.
    let kept = serde_json::to_string(&read).unwrap();
    let back: Vec<Manifest> = serde_json::from_str(&kept).unwrap();
    assert_eq!(back, read);
}

#[test]
fn a_quilt_jar_is_read() {
    let meta = json!({"schema_version": 1, "quilt_loader": {
        "id": "qsl", "version": "8.0.0",
        "provides": ["quilted_fabric_api", {"id": "fabric-api", "version": "0.100.0"}],
        "depends": [
            "quilt_loader",
            {"id": "minecraft", "versions": {"any": ["~1.20.1", "1.20"]}},
            {"id": "org.quiltmc:quilt_base", "versions": ">=1"},
            {"id": "sodium", "optional": true},
            {"id": "iris", "unless": "oculus"}
        ],
        "breaks": [{"id": "fabric-api", "versions": {"all": [">=0.1", "<0.50"]}}]
    }, "minecraft": {"environment": "client"}});
    let jar = zip_of(&[("quilt.mod.json", meta.to_string().as_bytes())]);
    let read = read_bytes(&jar);
    assert_eq!(read.len(), 1);
    let m = &read[0];
    assert_eq!(m.loader, Loader::Quilt);
    assert_eq!(m.mods.len(), 3);
    assert_eq!(m.mods[2].version.as_deref(), Some("0.100.0"));
    assert!(m.mods.iter().all(|p| p.side == Side::Client));
    let ids: Vec<&str> = m.needs.iter().map(|n| n.id.as_str()).collect();
    assert_eq!(ids, ["quilt_loader", "minecraft", "quilt_base"]);
    assert_eq!(m.needs[1].versions, strings(&["~1.20.1", "1.20"]));
    assert_eq!(m.breaks[0].versions, strings(&[">=0.1 <0.50"]));
}

#[test]
fn mods_toml_is_read_for_forge_and_neoforge() {
    let forge = r#"
modLoader="javafml"
loaderVersion="[47,)"
[[mods]]
modId="jei"
version="${file.jarVersion}"
[[dependencies.jei]]
    modId="forge"
    mandatory=true
    versionRange="[47.1.3,)"
    side="BOTH"
[[dependencies.jei]]
    modId="minecraft"
    mandatory=true
    versionRange="[1.20.1,1.20.2)"
[[dependencies.jei]]
    modId="rei"
    mandatory=false
[[dependencies.jei]]
    modId="optifine"
    type="incompatible"
    versionRange="*"
"#;
    let manifest = "Manifest-Version: 1.0\r\nImplementation-Version: 15.20.0.105\r\n";
    let jar = zip_of(&[
        ("META-INF/mods.toml", forge.as_bytes()),
        ("META-INF/MANIFEST.MF", manifest.as_bytes()),
    ]);
    let read = read_bytes(&jar);
    assert_eq!(read.len(), 1);
    let m = &read[0];
    assert_eq!(m.loader, Loader::Forge);
    assert_eq!(m.mods[0].id, "jei");
    assert_eq!(m.mods[0].version.as_deref(), Some("15.20.0.105"));
    let needs: Vec<(&str, Vec<String>)> = m
        .needs
        .iter()
        .map(|n| (n.id.as_str(), n.versions.clone()))
        .collect();
    assert_eq!(
        needs,
        [
            ("forge", strings(&["[47.1.3,)"])),
            ("minecraft", strings(&["[1.20.1,1.20.2)"]))
        ]
    );
    assert_eq!(m.breaks.len(), 1);
    assert!(m.breaks[0].versions.is_empty());

    let neo = r#"
[[mods]]
modId="jei"
version="19.21.0.247"
[[dependencies.jei]]
modId="neoforge"
type="required"
versionRange="[21.1.0,)"
[[dependencies.jei]]
modId="minecraft"
type="REQUIRED"
versionRange="[1.21.1,)"
side="CLIENT"
[[dependencies.jei]]
modId="emi"
type="discouraged"
"#;
    // NeoForge's own file name, and a mods.toml that names NeoForge.
    for name in ["META-INF/neoforge.mods.toml", "META-INF/mods.toml"] {
        let read = read_bytes(&zip_of(&[(name, neo.as_bytes())]));
        assert_eq!(read.len(), 1, "{name}");
        assert_eq!(read[0].loader, Loader::NeoForge, "{name}");
        assert_eq!(read[0].needs.len(), 2, "{name}");
        assert_eq!(read[0].needs[1].side, Side::Client, "{name}");
    }
    // Made for both: each file on its own.
    let both = read_bytes(&zip_of(&[
        ("META-INF/neoforge.mods.toml", neo.as_bytes()),
        ("META-INF/mods.toml", forge.as_bytes()),
    ]));
    let loaders: Vec<Loader> = both.iter().map(|m| m.loader).collect();
    assert_eq!(loaders, [Loader::NeoForge, Loader::Forge]);
    let twice = read_bytes(&zip_of(&[
        ("META-INF/neoforge.mods.toml", neo.as_bytes()),
        ("META-INF/mods.toml", neo.as_bytes()),
    ]));
    assert_eq!(twice.len(), 1);
}

#[test]
fn jar_in_jar_mods_are_read() {
    let inner = r#"
[[mods]]
modId="kotlinforforge"
version="5.5.0"
"#;
    let outer = r#"
[[mods]]
modId="mymod"
version="1.0"
[[dependencies.mymod]]
modId="kotlinforforge"
type="required"
versionRange="[5,)"
"#;
    let inner_jar = zip_of(&[("META-INF/neoforge.mods.toml", inner.as_bytes())]);
    let meta = json!({"jars": [{"path": "META-INF/jarjar/kff.jar"}, {"path": "../../etc/passwd"}]});
    let jar = zip_of(&[
        ("META-INF/neoforge.mods.toml", outer.as_bytes()),
        ("META-INF/jarjar/metadata.json", meta.to_string().as_bytes()),
        ("META-INF/jarjar/kff.jar", &inner_jar),
    ]);
    let read = read_bytes(&jar);
    assert_eq!(read[0].mods.len(), 2);
    assert!(read[0].mods[1].nested);
    let setup = setup("1.21.1", Loader::NeoForge, Some("21.1.77"));
    assert!(
        check(
            &setup,
            &[Jar {
                file: "mymod.jar",
                manifests: &read
            }]
        )
        .is_empty()
    );
}

#[test]
fn old_forge_mods_name_themselves_in_mcmod_info() {
    let list = json!([{"modid": "JourneyMap", "version": "5.7.1", "mcversion": "1.12.2"}]);
    let read = read_bytes(&zip_of(&[("mcmod.info", list.to_string().as_bytes())]));
    assert_eq!(read.len(), 1);
    assert_eq!(read[0].loader, Loader::Forge);
    assert_eq!(read[0].mods[0].id, "journeymap");
    assert_eq!(read[0].needs[0].versions, strings(&["1.12.2"]));
    let jar = [Jar {
        file: "journeymap.jar",
        manifests: &read,
    }];
    // The game version it names is no requirement.
    assert!(check(&setup("1.12", Loader::Forge, None), &jar).is_empty());
    let wrapped = json!({"modListVersion": 2, "modList": [{"modid": "ic2", "version": "2.8"}]});
    let read = read_bytes(&zip_of(&[("mcmod.info", wrapped.to_string().as_bytes())]));
    assert_eq!(read[0].mods[0].id, "ic2");
}

#[test]
fn junk_is_no_mod() {
    assert!(read_bytes(b"").is_empty());
    assert!(read_bytes(b"PK\x03\x04 not really a zip").is_empty());
    assert!(read_bytes(&zip_of(&[("readme.txt", b"hello")])).is_empty());
    assert!(read_bytes(&zip_of(&[("fabric.mod.json", b"{not json")])).is_empty());
    assert!(read_bytes(&zip_of(&[("fabric.mod.json", b"{\"id\": \"Has Spaces\"}")])).is_empty());
    assert!(read_bytes(&zip_of(&[("fabric.mod.json", b"[1, 2]")])).is_empty());
    assert!(read_bytes(&zip_of(&[("META-INF/mods.toml", b"[[mods]\nmodId=")])).is_empty());
    assert!(
        read_bytes(&zip_of(&[(
            "META-INF/mods.toml",
            b"[[mods]]\nversion=\"1\""
        )]))
        .is_empty()
    );
    assert!(read_bytes(&zip_of(&[("mcmod.info", b"{}")])).is_empty());
    // A descriptor over the limit is not read.
    let big = json!({"id": "big", "pad": "x".repeat(1024 * 1024 + 1)}).to_string();
    assert!(read_bytes(&zip_of(&[("fabric.mod.json", big.as_bytes())])).is_empty());
    // A byte order mark is fine.
    let bom = format!("\u{feff}{}", json!({"id": "bom", "version": "1"}));
    assert_eq!(
        read_bytes(&zip_of(&[("fabric.mod.json", bom.as_bytes())])).len(),
        1
    );
    // Control characters in a version or a range are left out.
    let odd = json!({"id": "odd", "version": "1\n2", "depends": {"x": "1\u{0}"}});
    let read = read_bytes(&zip_of(&[("fabric.mod.json", odd.to_string().as_bytes())]));
    assert_eq!(read[0].mods[0].version, None);
    assert!(read[0].needs.is_empty());
}

#[test]
fn jars_inside_jars_stop_at_a_depth() {
    // Each level holds the next; only three levels are opened.
    let mut jar = fabric_mod("level5", "1", json!({}));
    for level in (0..5).rev() {
        jar = fabric_jar(
            json!({"id": format!("level{level}"), "version": "1", "jars": [{"file": "inner.jar"}]}),
            &[("inner.jar", jar)],
        );
    }
    let read = read_bytes(&jar);
    let ids: Vec<&str> = read[0].mods.iter().map(|p| p.id.as_str()).collect();
    assert_eq!(ids, ["level0", "level1", "level2", "level3"]);
    // A jar that names itself is opened a bounded number of times.
    let mut looping = fabric_jar(
        json!({"id": "loop", "version": "1", "jars": [{"file": "self.jar"}]}),
        &[],
    );
    for _ in 0..3 {
        looping = fabric_jar(
            json!({"id": "loop", "version": "1", "jars": vec![json!({"file": "self.jar"}); 300]}),
            &[("self.jar", looping)],
        );
    }
    assert_eq!(read_bytes(&looping)[0].mods.len(), 1);
}

// ----- whether a set starts -----

fn setup<'a>(game: &'a str, loader: Loader, version: Option<&'a str>) -> Setup<'a> {
    Setup {
        game,
        loader,
        loader_version: version,
        java: Some(21),
        side: Side::Client,
    }
}

fn run(setup: &Setup<'_>, jars: &[(&str, Vec<u8>)]) -> Vec<Problem> {
    let read: Vec<(&str, Vec<Manifest>)> = jars.iter().map(|(f, b)| (*f, read_bytes(b))).collect();
    let jars: Vec<Jar<'_>> = read
        .iter()
        .map(|(file, manifests)| Jar { file, manifests })
        .collect();
    check(setup, &jars)
}

#[test]
fn a_mod_made_for_a_later_game_version_is_caught() {
    // JEI for NeoForge tagged 1.21 on its page, asking for 1.21.1 inside.
    let toml = r#"
[[mods]]
modId="jei"
version="19.21.0.247"
[[dependencies.jei]]
modId="minecraft"
type="required"
versionRange="[1.21.1,1.22)"
[[dependencies.jei]]
modId="neoforge"
type="required"
versionRange="[21.1.0,)"
"#;
    let jar = zip_of(&[("META-INF/neoforge.mods.toml", toml.as_bytes())]);
    let problems = run(
        &setup("1.21", Loader::NeoForge, Some("21.0.167")),
        &[("jei.jar", jar.clone())],
    );
    assert_eq!(
        problems,
        [
            Problem::Wrong {
                file: "jei.jar".into(),
                by: "jei".into(),
                id: "minecraft".into(),
                versions: strings(&["[1.21.1,1.22)"]),
                found: "1.21".into(),
                from: None,
            },
            Problem::Wrong {
                file: "jei.jar".into(),
                by: "jei".into(),
                id: "neoforge".into(),
                versions: strings(&["[21.1.0,)"]),
                found: "21.0.167".into(),
                from: None,
            }
        ]
    );
    assert!(
        run(
            &setup("1.21.1", Loader::NeoForge, Some("21.1.77")),
            &[("jei.jar", jar.clone())]
        )
        .is_empty()
    );
    // Without the loader's version, only the game is checked.
    assert_eq!(
        run(&setup("1.21", Loader::NeoForge, None), &[("jei.jar", jar)]).len(),
        1
    );
}

#[test]
fn needs_are_checked_across_the_set() {
    let mine = fabric_mod(
        "mine",
        "1.0",
        json!({"fabricloader": ">=0.16", "fabric-api": ">=0.100", "cloth-config": "*", "java": ">=21",
               "mixinextras": ">=0.3"}),
    );
    let old_api = fabric_jar(
        json!({"id": "fabric-api", "version": "0.92.2+1.20.1", "provides": ["fabric"]}),
        &[],
    );
    let new_api = fabric_mod("fabric-api", "0.110.0+1.21.1", json!({}));
    let fabric = setup("1.21.1", Loader::Fabric, Some("0.15.11"));
    let problems = run(&fabric, &[("mine.jar", mine.clone()), ("api.jar", old_api)]);
    let kinds: Vec<(&str, &str)> = problems
        .iter()
        .map(|p| match p {
            Problem::Missing { id, .. } => ("missing", id.as_str()),
            Problem::Wrong { id, .. } => ("wrong", id.as_str()),
            _ => ("other", ""),
        })
        .collect();
    assert_eq!(
        kinds,
        [
            ("missing", "cloth-config"),
            ("wrong", "fabric-api"),
            ("wrong", "fabricloader")
        ]
    );
    assert!(problems.contains(&Problem::Wrong {
        file: "mine.jar".into(),
        by: "mine".into(),
        id: "fabric-api".into(),
        versions: strings(&[">=0.100"]),
        found: "0.92.2+1.20.1".into(),
        from: Some("api.jar".into()),
    }));
    let cloth = fabric_mod("cloth-config", "15.0.140", json!({}));
    let fine = setup("1.21.1", Loader::Fabric, Some("0.16.9"));
    assert!(
        run(
            &fine,
            &[
                ("mine.jar", mine.clone()),
                ("api.jar", new_api.clone()),
                ("cloth.jar", cloth.clone())
            ]
        )
        .is_empty()
    );
    // Java 17 is too old for it.
    let mut java17 = fine;
    java17.java = Some(17);
    let problems = run(
        &java17,
        &[
            ("mine.jar", mine.clone()),
            ("api.jar", new_api.clone()),
            ("cloth.jar", cloth.clone()),
        ],
    );
    assert!(
        matches!(&problems[..], [Problem::Wrong { id, .. }] if id == "java"),
        "{problems:?}"
    );
    // Quilt runs the same Fabric mods.
    let quilt = setup("1.21.1", Loader::Quilt, Some("0.27.1"));
    assert!(
        run(
            &quilt,
            &[
                ("mine.jar", mine),
                ("api.jar", new_api),
                ("cloth.jar", cloth)
            ]
        )
        .is_empty()
    );
}

#[test]
fn a_mod_inside_another_counts_as_there() {
    let lib = fabric_mod("cloth-config", "15.0.140", json!({}));
    let bundle = fabric_jar(
        json!({"id": "bundle", "version": "1", "jars": [{"file": "META-INF/jars/cloth.jar"}]}),
        &[("META-INF/jars/cloth.jar", lib.clone())],
    );
    let user = fabric_mod("user", "1", json!({"cloth-config": ">=15"}));
    let fabric = setup("1.21.1", Loader::Fabric, Some("0.16.9"));
    assert!(
        run(
            &fabric,
            &[("user.jar", user), ("bundle.jar", bundle.clone())]
        )
        .is_empty()
    );
    // The same library inside a jar and on its own is no clash; two files
    // of the same mod are.
    assert!(
        run(
            &fabric,
            &[("bundle.jar", bundle), ("cloth.jar", lib.clone())]
        )
        .is_empty()
    );
    assert_eq!(
        run(
            &fabric,
            &[("cloth-a.jar", lib.clone()), ("cloth-b.jar", lib)]
        ),
        [Problem::Twice {
            id: "cloth-config".into(),
            files: strings(&["cloth-a.jar", "cloth-b.jar"]),
        }]
    );
}

#[test]
fn breaks_are_caught_whichever_mod_says_so() {
    let optifabric = fabric_mod("optifabric", "1.14.3", json!({}));
    let sodium = fabric_jar(
        json!({"id": "sodium", "version": "0.6.0", "breaks": {"optifabric": "*", "iris": "<1.8"}}),
        &[],
    );
    let iris = fabric_mod("iris", "1.7.2", json!({}));
    let fabric = setup("1.21.1", Loader::Fabric, Some("0.16.9"));
    let problems = run(
        &fabric,
        &[
            ("optifabric.jar", optifabric),
            ("sodium.jar", sodium.clone()),
            ("iris.jar", iris),
        ],
    );
    let ids: Vec<&str> = problems
        .iter()
        .map(|p| match p {
            Problem::Breaks { id, file, .. } => {
                assert_eq!(file, "sodium.jar");
                id.as_str()
            }
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(ids, ["iris", "optifabric"]);
    let newer_iris = fabric_mod("iris", "1.8.1", json!({}));
    assert!(run(&fabric, &[("sodium.jar", sodium), ("iris.jar", newer_iris)]).is_empty());
}

#[test]
fn mods_for_another_loader_and_sides_are_told_apart() {
    let forge = zip_of(&[(
        "META-INF/mods.toml",
        b"[[mods]]\nmodId=\"jei\"\nversion=\"15.0\"\n" as &[u8],
    )]);
    let fabric = setup("1.21.1", Loader::Fabric, Some("0.16.9"));
    assert_eq!(
        run(&fabric, &[("jei.jar", forge.clone())]),
        [Problem::OtherLoader {
            file: "jei.jar".into(),
            loader: Loader::Forge,
        }]
    );
    // NeoForge for 1.20.1 still runs Forge's mods, and gives `forge`.
    let forge_dep = zip_of(&[(
        "META-INF/mods.toml",
        b"[[mods]]\nmodId=\"a\"\n[[dependencies.a]]\nmodId=\"forge\"\nmandatory=true\nversionRange=\"[47,)\"\n" as &[u8],
    )]);
    assert!(
        run(
            &setup("1.20.1", Loader::NeoForge, Some("47.1.106")),
            &[("a.jar", forge_dep.clone())]
        )
        .is_empty()
    );
    assert_eq!(
        run(
            &setup("1.21.1", Loader::NeoForge, Some("21.1.77")),
            &[("a.jar", forge_dep)]
        )
        .len(),
        1
    );
    // A library jar is no mod.
    assert!(
        run(
            &fabric,
            &[("lib.jar", zip_of(&[("a/B.class", b"\xca\xfe")]))]
        )
        .is_empty()
    );

    // A client mod on a server: not loaded, so what it needs does not count.
    let client = fabric_jar(
        json!({"id": "minimap", "version": "1", "environment": "client", "depends": {"cloth-config": "*"}}),
        &[],
    );
    let user = fabric_mod("user", "1", json!({"minimap": "*"}));
    let mut server = fabric;
    server.side = Side::Server;
    assert_eq!(
        run(
            &server,
            &[("minimap.jar", client.clone()), ("user.jar", user)]
        ),
        [Problem::Missing {
            file: "user.jar".into(),
            by: "user".into(),
            id: "minimap".into(),
            versions: Vec::new(),
        }]
    );
    assert_eq!(run(&fabric, &[("minimap.jar", client)]).len(), 1);
}

#[test]
fn what_cannot_be_compared_is_taken_as_fine() {
    let dev = fabric_mod("dev", "${version}", json!({}));
    let user = fabric_mod(
        "user",
        "1",
        json!({"dev": ">=2.0", "minecraft": ">=mc1.21"}),
    );
    let fabric = setup("1.21.1", Loader::Fabric, Some("0.16.9"));
    assert!(run(&fabric, &[("dev.jar", dev), ("user.jar", user.clone())]).is_empty());
    // A snapshot cannot be compared with a release.
    let snapshot = setup("24w14a", Loader::Fabric, Some("0.16.9"));
    let needs_release = fabric_mod("r", "1", json!({"minecraft": ">=1.21"}));
    assert!(run(&snapshot, &[("r.jar", needs_release)]).is_empty());
}

#[test]
fn the_descriptor_a_loader_reads_is_picked() {
    let fabric = read_bytes(&fabric_mod("a", "1", json!({})));
    let quilt = read_bytes(&zip_of(&[
        (
            "quilt.mod.json",
            json!({"quilt_loader": {"id": "a", "version": "1"}})
                .to_string()
                .as_bytes(),
        ),
        (
            "fabric.mod.json",
            json!({"id": "a", "version": "1"}).to_string().as_bytes(),
        ),
    ]));
    assert_eq!(quilt.len(), 2);
    assert_eq!(
        for_loader(&quilt, Loader::Quilt, "1.21.1").map(|m| m.loader),
        Some(Loader::Quilt)
    );
    assert_eq!(
        for_loader(&quilt, Loader::Fabric, "1.21.1").map(|m| m.loader),
        Some(Loader::Fabric)
    );
    assert_eq!(
        for_loader(&fabric, Loader::Quilt, "1.21.1").map(|m| m.loader),
        Some(Loader::Fabric)
    );
    assert!(for_loader(&fabric, Loader::Forge, "1.21.1").is_none());
}

#[test]
fn manifests_from_elsewhere_are_checked() {
    let read = read_bytes(&fabric_mod(
        "sodium",
        "0.6.0",
        json!({"minecraft": ">=1.21", "fabricloader": ">=0.16"}),
    ));
    assert!(read[0].is_clean());
    // As the catalog gives them back.
    let back: Vec<Manifest> = serde_json::from_value(serde_json::to_value(&read).unwrap()).unwrap();
    assert_eq!(back, read);
    assert!(back[0].is_clean());

    let bad = |change: &dyn Fn(&mut Manifest)| {
        let mut m = read[0].clone();
        change(&mut m);
        !m.is_clean()
    };
    assert!(bad(&|m| m.mods.clear()));
    assert!(bad(&|m| m.mods[0].id = "Sodium".to_owned()));
    assert!(bad(&|m| m.mods[0].id = "a\u{202e}b".to_owned()));
    assert!(bad(&|m| m.mods[0].id = "x".repeat(65)));
    assert!(bad(&|m| m.mods[0].version = Some("1.0\u{7}".to_owned())));
    assert!(bad(&|m| m.mods[0].version = Some("${version}".to_owned())));
    assert!(bad(&|m| m.mods[0].version = Some("1".repeat(129))));
    assert!(bad(&|m| m.needs[0].by = "../x".to_owned()));
    assert!(bad(&|m| m.needs[0].versions = vec!["1\n2".to_owned()]));
    assert!(bad(&|m| m.needs[0].versions = vec!["1".to_owned(); 17]));
    assert!(bad(&|m| m.breaks.push(Need {
        by: "sodium".to_owned(),
        id: "".to_owned(),
        versions: Vec::new(),
        side: Side::Both,
    })));
    // Junk does not deserialize at all.
    for junk in [
        json!([{"loader": "rift", "mods": []}]),
        json!([{"loader": "fabric"}]),
        json!([{"loader": "fabric", "mods": [{"id": 5}]}]),
        json!({"loader": "fabric"}),
    ] {
        assert!(serde_json::from_value::<Vec<Manifest>>(junk).is_err());
    }
}

#[test]
fn optifine_and_what_does_not_start_with_it_are_known() {
    let optifine = zip_of(&[
        ("net/optifine/Config.class", b"\xca\xfe"),
        ("optifine/Installer.class", b"\xca\xfe"),
    ]);
    let read = read_bytes(&optifine);
    assert_eq!(read.len(), 2);
    assert_eq!(read[0].loader, Loader::Forge);
    assert_eq!(read[1].loader, Loader::Fabric);
    assert_eq!(read[0].mods[0].id, "optifine");
    assert_eq!(read[0].mods[0].side, Side::Client);
    // A mod that only mentions it is no OptiFine.
    let mentions = zip_of(&[
        ("fabric.mod.json", br#"{"id": "compat", "version": "1"}"#),
        ("net/optifine/Shim.class", b"\xca\xfe"),
    ]);
    assert_eq!(read_bytes(&mentions)[0].mods[0].id, "compat");
    assert!(read_bytes(&zip_of(&[("a/B.class", b"\xca\xfe")])).is_empty());

    let sodium = fabric_mod("sodium", "0.6.0", json!({}));
    let jars = [("sodium.jar", sodium), ("optifine.jar", optifine)];
    let fabric = setup("1.21.1", Loader::Fabric, Some("0.16.14"));
    // Neither says so.
    assert!(run(&fabric, &jars).is_empty());
    let known = clashes();
    assert!(known.iter().all(Clash::is_clean));
    let read: Vec<(&str, Vec<Manifest>)> = jars.iter().map(|(f, b)| (*f, read_bytes(b))).collect();
    let set: Vec<Jar<'_>> = read
        .iter()
        .map(|(file, manifests)| Jar { file, manifests })
        .collect();
    assert_eq!(
        check_with(&fabric, &set, &known),
        [Problem::Breaks {
            file: "sodium.jar".to_owned(),
            by: "sodium".to_owned(),
            id: "optifine".to_owned(),
            found: None,
            from: Some("optifine.jar".to_owned()),
        }]
    );
    // On a server OptiFine is not there.
    let server = Setup {
        side: Side::Server,
        ..fabric
    };
    assert!(check_with(&server, &set, &known).is_empty());

    for bad in [
        ("Sodium", "optifine"),
        ("sodium", "sodium"),
        ("", "x"),
        ("a b", "c"),
    ] {
        let clash = Clash {
            a: bad.0.to_owned(),
            b: bad.1.to_owned(),
        };
        assert!(!clash.is_clean(), "{bad:?}");
    }
}
