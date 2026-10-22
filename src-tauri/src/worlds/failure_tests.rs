use super::*;

fn named(id: &str, name: Option<&str>) -> Named {
    Named {
        id: id.to_owned(),
        name: name.map(str::to_owned),
    }
}

/// NeoForge 21.0.167 with JEI's release for 1.21 (which asks for 1.21.1's
/// NeoForge), as its console printed it.
const NEOFORGE: &str = "\
[10:42:42] [main/ERROR]: Missing or unsupported mandatory dependencies:
\tMod ID: 'minecraft', Requested by: 'mezz_config', Expected range: '[1.21.1,1.21.1]', Actual version: '1.21'
\tMod ID: 'neoforge', Requested by: 'jei', Expected range: '[21.1.238,)', Actual version: '21.0.167'
[10:42:50] [main/FATAL] [ne.ne.fm.ModLoader/CORE]: Error during pre-loading phase: Mod jei requires neoforge 21.1.238 or above
Currently, neoforge is 21.0.167
-- MOD mezz_config --
Details:
\tMod File: /data/worlds/x/server/mods/jei.jar#122!/META-INF/jarjar/mezz_config-1.21.1-neoforge-0.6.6.jar
\tFailure message: fml.modloadingissue.missingdependency
\tFailure message arg 1: minecraft";

/// Forge 47.4.28 (1.20.1) with JEI's release for 1.19.2.
const FORGE: &str = "\
[10:44:36] [main/ERROR]: Missing or unsupported mandatory dependencies:
\tMod ID: 'minecraft', Requested by: 'mezz_config', Expected range: '[1.19.2,1.19.2]', Actual version: '1.20.1'
\tMod ID: 'minecraft', Requested by: 'jei', Expected range: '[1.19,1.19.2]', Actual version: '1.20.1'
[10:44:42] [main/FATAL]: Error during pre-loading phase
net.minecraftforge.fml.ModLoadingException: Mod \u{a7}emezz_config\u{a7}r requires \u{a7}6minecraft\u{a7}r \u{a7}o1.19.2\u{a7}r
\tFailure message: Mod mezz_config requires minecraft 1.19.2
\t\tCurrently, minecraft is 1.20.1";

/// Fabric Loader 0.19.5 (1.21.1) with Fabric API for 1.20.1, then AppleSkin
/// without Fabric API.
const FABRIC_WRONG: &str = "\
[10:43:22] [main/ERROR]: Incompatible mods found!
net.fabricmc.loader.impl.FormattedException: Some of your mods are incompatible with the game or each other!
A potential solution has been determined, this may resolve your problem:
\t - Replace 'Minecraft' (minecraft) 1.21.1 with any version between 1.20 (inclusive) and 1.20.2- (exclusive).
More details:
\t - Mod 'Fabric API' (fabric-api) 0.92.12+1.20.1 requires any version between 1.20 (inclusive) and 1.20.2- (exclusive) of 'Minecraft' (minecraft), but only the wrong version is present: 1.21.1!
\tat net.fabricmc.loader.impl.FormattedException.ofLocalized(FormattedException.java:51) ~[fabric-loader-0.19.5.jar:?]";
const FABRIC_MISSING: &str = "\
\t - Install fabric-api, any version.
More details:
\t - Mod 'AppleSkin' (appleskin) 3.0.6+mc1.21 requires any version of fabric-api, which is missing!";

fn problems(text: &str) -> Vec<Problem> {
    let mut found = Vec::new();
    for line in text.lines() {
        note(&mut found, line);
    }
    found
}

#[test]
fn forge_and_neoforge_name_the_mod_the_dependency_and_both_versions() {
    let found = problems(NEOFORGE);
    assert_eq!(
        found,
        [
            Problem::Wrong {
                by: named("mezz_config", None),
                dependency: named("minecraft", None),
                needs: Needs::Exactly("1.21.1".into()),
                actual: "1.21".into(),
            },
            Problem::Wrong {
                by: named("jei", None),
                dependency: named("neoforge", None),
                needs: Needs::AtLeast("21.1.238".into()),
                actual: "21.0.167".into(),
            },
        ]
    );
    // JEI is the world's mod; MezzConfig is inside its jar.
    let told = chief(&found, &["jei.jar".to_owned()]).unwrap();
    let error = explain(told);
    assert_eq!(error.code, ErrorCode::GameExitedWithError);
    assert_eq!(
        error.message,
        "jei needs NeoForge 21.1.238 or newer, and the world has 21.0.167. Take an older release of jei or remove it."
    );
    let text = error.text.unwrap();
    assert_eq!(text.params["mod"].to_string(), "jei");
    assert_eq!(text.params["dependency"].to_string(), "NeoForge");
    // A world file named as the catalog names it.
    assert_eq!(
        chief(&found, &["jei-1.21-neoforge-19.5.0.66.jar".to_owned()]),
        Some(&found[1])
    );
    // No file to go by: the first.
    assert_eq!(chief(&found, &[]), Some(&found[0]));

    let found = problems(FORGE);
    assert_eq!(found.len(), 2, "{found:?}");
    let error = explain(chief(&found, &["jei.jar".to_owned()]).unwrap());
    assert_eq!(
        error.message,
        "jei is made for another Minecraft version than the world's 1.20.1. Take its release for 1.20.1 or remove it."
    );
    let error = explain(&found[0]);
    assert_eq!(
        error.message,
        "mezz_config is made for Minecraft 1.19.2, and the world is 1.20.1. Take its release for 1.20.1 or remove it."
    );
    assert_eq!(
        read(
            "\tMod ID: 'jei', Requested by: 'jeresources', Expected range: '[15.2,)', Actual version: '[MISSING]'"
        ),
        Some(Problem::Missing {
            by: named("jeresources", None),
            dependency: named("jei", None),
        })
    );
}

#[test]
fn fabric_names_the_mod_by_its_name() {
    let found = problems(FABRIC_WRONG);
    assert_eq!(
        found,
        [Problem::Wrong {
            by: named("fabric-api", Some("Fabric API")),
            dependency: named("minecraft", Some("Minecraft")),
            needs: Needs::Other,
            actual: "1.21.1".into(),
        }]
    );
    assert_eq!(
        explain(&found[0]).message,
        "Fabric API is made for another Minecraft version than the world's 1.21.1. Take its release for 1.21.1 or remove it."
    );

    let found = problems(FABRIC_MISSING);
    assert_eq!(
        found,
        [Problem::Missing {
            by: named("appleskin", Some("AppleSkin")),
            dependency: named("fabric-api", None),
        }]
    );
    assert_eq!(
        explain(&found[0]).message,
        "AppleSkin needs Fabric API, which is not in the world. Add it to the installation or remove AppleSkin, then update the world's mods."
    );

    assert_eq!(
        read(
            " - Mod 'Sodium' (sodium) 0.6.0 requires version 0.16.0 or later of 'Fabric Loader' (fabricloader), but only the wrong version is present: 0.15.11!"
        ),
        Some(Problem::Wrong {
            by: named("sodium", Some("Sodium")),
            dependency: named("fabricloader", Some("Fabric Loader")),
            needs: Needs::AtLeast("0.16.0".into()),
            actual: "0.15.11".into(),
        })
    );
    assert_eq!(
        explain(&read(" - Mod 'Sodium' (sodium) 0.6.0 requires version 0.16.0 or later of 'Fabric Loader' (fabricloader), but only the wrong version is present: 0.15.11!").unwrap()).message,
        "Sodium needs Fabric Loader 0.16.0 or newer, and the world has 0.15.11. Take an older release of Sodium or remove it."
    );
}

#[test]
fn forge_before_1_13_names_a_mod_for_another_version() {
    // Forge 14.23.5.2864 (1.12.2) with JEI's release for 1.11.2.
    let found = problems(
        "[10:46:25] [Server thread/FATAL]: The mod jei does not wish to run in Minecraft version Minecraft 1.12.2. You will have to remove it to play.
[10:46:25] [Server thread/FATAL]: Wrong Minecraft version for jei
net.minecraftforge.fml.common.WrongMinecraftVersionException: Wrong Minecraft version for jei
Expected: [1.11,1.11]
Current: Minecraft 1.12.2",
    );
    assert_eq!(
        found,
        [Problem::Wrong {
            by: named("jei", None),
            dependency: named("minecraft", None),
            needs: Needs::Other,
            actual: "1.12.2".into(),
        }]
    );
}

#[test]
fn a_newer_java_and_a_failed_mod_are_named() {
    assert_eq!(
        read(
            "java.lang.UnsupportedClassVersionError: mezz/jei/Jei has been compiled by a more recent version of the Java Runtime (class file version 65.0), this version of the Java Runtime only recognizes class file versions up to 61.0"
        ),
        Some(Problem::Java { release: 21 })
    );
    assert_eq!(
        explain(&Problem::Java { release: 21 }).message,
        "A mod in this world needs Java 21 or newer, which this Minecraft version does not run on. Remove the mod or take its release for this version."
    );
    assert_eq!(
        read(
            "java.lang.RuntimeException: Could not execute entrypoint stage 'main' due to errors, provided by 'lithium' at 'me.jellysquid.mods.lithium.LithiumMod'!"
        ),
        Some(Problem::Failed {
            by: named("lithium", None)
        })
    );
    assert_eq!(
        read("\tFailure message: Spark (spark) has failed to load correctly"),
        Some(Problem::Failed {
            by: named("spark", Some("Spark"))
        })
    );
    assert_eq!(
        explain(&read("\tFailure message: Spark (spark) has failed to load correctly").unwrap())
            .message,
        "Spark stopped the world server while it started. Take another release of Spark or remove it."
    );
    // What a mod needs is told before what failed after it.
    let found = vec![
        Problem::Failed {
            by: named("spark", None),
        },
        Problem::Java { release: 21 },
    ];
    assert_eq!(chief(&found, &[]), Some(&found[1]));
}

#[test]
fn junk_and_hostile_lines_name_nothing() {
    for line in [
        "",
        "[10:42:42] [Server thread/INFO]: <Mallory> Mod ID: 'a', Requested by: 'b'",
        "\tMod ID: 'Neo Forge', Requested by: 'jei', Expected range: '[1,)', Actual version: '1'",
        "\tMod ID: 'neoforge', Requested by: '{evil}', Expected range: '[1,)', Actual version: '1'",
        "\tMod ID: 'neoforge', Requested by: 'jei', Expected range: '[1,)', Actual version: '1 <b>'",
        "\tMod ID: 'neoforge', Requested by: 'jei', Expected range: '[1,)'",
        " - Mod 'A' (A) 1 requires any version of y, which is missing!",
        " - Mod 'A' (a) 1 requires any version of Y Z, which is missing!",
        "class file version 52.0",
        "compiled by a more recent version of the Java Runtime (class file version 9999.0)",
        "\tFailure message: fml.modloadingissue.missingdependency",
        "\tFailure message: Spark (spark) was fine",
    ] {
        assert_eq!(read(line), None, "{line}");
    }
    // A name that is empty or not plain text is left out, the id is kept.
    assert_eq!(
        read(" - Mod '' (x) 1 requires any version of y, which is missing!"),
        Some(Problem::Missing {
            by: named("x", None),
            dependency: named("y", None),
        })
    );
    let long = "x".repeat(65);
    assert_eq!(
        read(&format!(
            " - Mod '{long}' (a) 1 requires any version of b, which is missing!"
        )),
        Some(Problem::Missing {
            by: named("a", None),
            dependency: named("b", None),
        })
    );
    assert_eq!(
        clean_name("Just Enough Items"),
        Some("Just Enough Items".into())
    );
    assert_eq!(clean_name("{mod}"), None);
    assert_eq!(clean_name("a\u{0}b"), None);
    assert_eq!(clean_name("<script>"), None);
    // At most so many problems, each once.
    let mut found = Vec::new();
    for i in 0..20 {
        let line = format!(" - Mod 'A' (a{i}) 1 requires any version of b, which is missing!");
        note(&mut found, &line);
        note(&mut found, &line);
    }
    assert_eq!(found.len(), MAX_PROBLEMS);
}

#[test]
fn crash_reports_of_this_start_are_read_and_others_are_not() {
    let dir = tempfile::tempdir().unwrap();
    let reports = dir.path().join("crash-reports");
    std::fs::create_dir_all(&reports).unwrap();
    let start = SystemTime::now();
    std::fs::write(
        reports.join("crash-2026-10-22_10.45.40-server.txt"),
        "---- Minecraft Crash Report ----\nDescription: Exception in server tick loop\n\n\
         -- Head --\nThread: Server thread\nSuspected Mods: \n\tSpark (spark), Version: 1.10.66\n\
         \t\tIssue tracker URL: https://github.com/lucko/spark/issues\n",
    )
    .unwrap();
    std::fs::write(
        reports.join("notes.txt"),
        "\tFailure message: X (x) has failed",
    )
    .unwrap();
    assert_eq!(
        crash_reports(dir.path(), start),
        [Problem::Failed {
            by: named("spark", Some("Spark"))
        }]
    );
    // Forge 50.2.14 (1.20.6) with spark 1.10.66: no suspected mods, the
    // first frame of its own module names it.
    std::fs::write(
        reports.join("crash-2026-10-22_10.48.16-server.txt"),
        "---- Minecraft Crash Report ----\n// But it works on my machine.\n\n\
         Time: 2026-10-22 10:48:16\nDescription: Exception in server tick loop\n\n\
         java.lang.NoSuchMethodError: 'java.lang.Thread net.minecraft.server.MinecraftServer.m_6304_()'\n\
         \tat TRANSFORMER/spark@1.10.66/me.lucko.spark.forge.plugin.ForgeServerSparkPlugin.<init>(ForgeServerSparkPlugin.java:101) ~[spark.jar!/:?]\n\
         \tat SECURE-BOOTSTRAP/net.minecraftforge.eventbus/net.minecraftforge.eventbus.ASMEventHandler.invoke(ASMEventHandler.java:55) ~[eventbus-6.2.33.jar!/:?]\n\
         \tat TRANSFORMER/forge@50.2.14/net.minecraftforge.server.ServerLifecycleHooks.handleServerAboutToStart(ServerLifecycleHooks.java:81)\n\
         \tat TRANSFORMER/minecraft@1.20.6/net.minecraft.server.MinecraftServer.runServer(MinecraftServer.java:659)\n\
         Transformer Audit:\n  me.lucko.spark.forge.ForgeSparkMod\n",
    )
    .unwrap();
    let found = crash_reports(dir.path(), start);
    assert!(
        found.contains(&Problem::Failed {
            by: named("spark", None)
        }),
        "{found:?}"
    );
    assert_eq!(
        first_mod_frame(
            "Description: x\n\njava.lang.Error\n\tat TRANSFORMER/minecraft@1.21/a.b(B.java:1)\n\tat TRANSFORMER/neoforge@21.0/c.d(D.java:1)\n\n\tat TRANSFORMER/jei@1/e.f(F.java:1)\n"
        ),
        None,
        "only the trace's own frames, and not the game's or the loader's"
    );
    std::fs::remove_file(reports.join("crash-2026-10-22_10.48.16-server.txt")).unwrap();
    // Written before this start: not this start's.
    let later = SystemTime::now() + Duration::from_secs(60);
    assert!(crash_reports(dir.path(), later).is_empty());
    // No folder: nothing.
    assert!(crash_reports(&dir.path().join("none"), start).is_empty());
    // A report too large is left alone.
    std::fs::remove_file(reports.join("crash-2026-10-22_10.45.40-server.txt")).unwrap();
    let big = std::fs::File::create(reports.join("crash-big.txt")).unwrap();
    big.set_len(MAX_REPORT_BYTES + 1).unwrap();
    assert!(crash_reports(dir.path(), start).is_empty());
    #[cfg(unix)]
    {
        let elsewhere = tempfile::tempdir().unwrap();
        let target = elsewhere.path().join("crash-x.txt");
        std::fs::write(&target, "\tFailure message: X (x) has failed").unwrap();
        std::os::unix::fs::symlink(&target, reports.join("crash-link.txt")).unwrap();
        assert!(
            crash_reports(dir.path(), start).is_empty(),
            "a link is not followed"
        );
    }
}

#[test]
fn a_world_file_is_matched_to_its_mod_id() {
    assert!(file_is("jei.jar", "jei"));
    assert!(file_is("jei-1.21.1-neoforge-19.21.0.247.jar", "jei"));
    assert!(file_is(
        "cloth-config-15.0.140-neoforge.jar",
        "cloth_config"
    ));
    assert!(file_is("Clumps-forge-1.20.1-12.0.0.4.jar", "clumps"));
    assert!(file_is("fabric-api-0.92.2+1.20.1.jar", "fabric-api"));
    assert!(!file_is("jeresources.jar", "jei"));
}

#[test]
fn what_the_jars_say_is_told_like_a_failed_start() {
    use mod_meta::{Loader, Problem as P};
    let wrong = |by: &str, id: &str, range: &str, found: &str| P::Wrong {
        file: "mods/a.jar".into(),
        by: by.into(),
        id: id.into(),
        versions: vec![range.into()],
        found: found.into(),
        from: None,
    };
    let told = |problem: &P, loader: Loader, name: Option<&str>| {
        from_jars(problem, loader, name).map(|p| explain(&p).message)
    };

    let missing = P::Missing {
        file: "mods/appleskin.jar".into(),
        by: "appleskin".into(),
        id: "fabric-api".into(),
        versions: Vec::new(),
    };
    assert_eq!(
        told(&missing, Loader::Fabric, Some("AppleSkin")).unwrap(),
        "AppleSkin needs Fabric API, which is not in the world. Add it to the installation or remove AppleSkin, then update the world's mods."
    );
    let range = wrong("jei", "neoforge", "[21.1.238,)", "21.0.167");
    assert_eq!(
        told(&range, Loader::NeoForge, None).unwrap(),
        "jei needs NeoForge 21.1.238 or newer, and the world has 21.0.167. Take an older release of jei or remove it."
    );
    // The jar's title names it; the game and the loaders keep their own names.
    assert!(
        told(&range, Loader::NeoForge, Some("Just Enough Items"))
            .unwrap()
            .starts_with("Just Enough Items needs NeoForge 21.1.238")
    );
    assert_eq!(
        told(
            &wrong("jei", "minecraft", "[1.19,1.19.2]", "1.20.1"),
            Loader::Forge,
            None
        )
        .unwrap(),
        "jei is made for another Minecraft version than the world's 1.20.1. Take its release for 1.20.1 or remove it."
    );

    // Fabric's predicates: one version, a lower bound, and the rest.
    let fabric = |range: &str| {
        from_jars(
            &wrong("lithium", "fabricloader", range, "0.15.0"),
            Loader::Fabric,
            None,
        )
    };
    let needs = |range: &str| match fabric(range) {
        Some(Problem::Wrong { needs, .. }) => needs,
        other => panic!("{other:?}"),
    };
    assert_eq!(needs(">=0.16.0"), Needs::AtLeast("0.16.0".into()));
    assert_eq!(needs("0.16.0"), Needs::Exactly("0.16.0".into()));
    assert_eq!(needs("=0.16.0"), Needs::Exactly("0.16.0".into()));
    assert_eq!(needs(">=0.16.0-"), Needs::AtLeast("0.16.0".into()));
    for other in ["~0.16", "0.16.x", "^0.16.0", ">=0.16 <1", "*", ">=0.16.0--"] {
        assert_eq!(needs(other), Needs::Other, "{other}");
    }
    assert_eq!(
        told(
            &wrong("fabric-api", "minecraft", "1.20.1", "1.21.1"),
            Loader::Fabric,
            None
        )
        .unwrap(),
        "Fabric API is made for Minecraft 1.20.1, and the world is 1.21.1. Take its release for 1.21.1 or remove it."
    );
    // Two ranges are told as another version.
    let either = P::Wrong {
        file: "mods/jei.jar".into(),
        by: "jei".into(),
        id: "minecraft".into(),
        versions: vec!["[1.20,1.20.1]".into(), "[1.21]".into()],
        found: "1.21.1".into(),
        from: None,
    };
    assert!(matches!(
        from_jars(&either, Loader::Forge, None),
        Some(Problem::Wrong {
            needs: Needs::Other,
            ..
        })
    ));

    // What a mod does not start with.
    let breaks = |found: Option<&str>| P::Breaks {
        file: "mods/sodium.jar".into(),
        by: "sodium".into(),
        id: "optifabric".into(),
        found: found.map(str::to_owned),
        from: Some("mods/optifabric.jar".into()),
    };
    assert_eq!(
        told(&breaks(Some("1.14.3")), Loader::Fabric, Some("Sodium")).unwrap(),
        "Sodium does not work with optifabric 1.14.3. Take another release of Sodium or remove it."
    );
    assert_eq!(from_jars(&breaks(None), Loader::Fabric, None), None);

    // Left to the loader and the start.
    let twice = P::Twice {
        id: "jei".into(),
        files: vec!["mods/a.jar".into(), "mods/b.jar".into()],
    };
    let other = P::OtherLoader {
        file: "mods/a.jar".into(),
        loader: Loader::Forge,
    };
    assert_eq!(from_jars(&twice, Loader::Fabric, None), None);
    assert_eq!(from_jars(&other, Loader::Fabric, None), None);

    // A name or version that is not plain is dropped; an id that is not one
    // tells nothing.
    assert!(
        told(&range, Loader::NeoForge, Some("<b>JEI</b>"))
            .unwrap()
            .starts_with("jei needs")
    );
    assert_eq!(
        from_jars(
            &wrong("jei", "neoforge", "[21.1.238,)", "21.0.167\n[x]"),
            Loader::NeoForge,
            None
        ),
        None
    );
    assert_eq!(
        from_jars(
            &wrong("Jei", "neoforge", "[21.1.238,)", "21.0.167"),
            Loader::NeoForge,
            None
        ),
        None
    );
}
