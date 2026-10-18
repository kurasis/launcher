use super::*;

/// The run's output as the launcher keeps it: one entry per line.
fn output(text: &str) -> Vec<String> {
    text.lines().map(str::to_owned).collect()
}

fn failed(code: i32) -> ExitInfo {
    ExitInfo {
        code: Some(code),
        signal: None,
    }
}

fn input<'a>(exit: ExitInfo, lines: &'a [String]) -> CrashInput<'a> {
    CrashInput {
        exit,
        output: lines,
        crash_report: None,
        jvm_error: None,
        max_heap_mib: 4096,
        recommended_heap_mib: None,
    }
}

fn run(code: i32, text: &str) -> CrashDiagnosis {
    let lines = output(text);
    diagnose(&input(failed(code), &lines)).expect("diagnosis")
}

fn detail<'a>(d: &'a CrashDiagnosis, key: &str) -> &'a str {
    d.details.get(key).map_or("", String::as_str)
}

// ------------------------------------------------- reproduced crashes ----

#[test]
fn unknown_jvm_option() {
    let d = run(1, include_str!("fixtures/jvm-option.log"));
    assert_eq!(d.kind, CrashKind::JvmOption);
    assert_eq!(detail(&d, "option"), "NoSuchFlagForTest");
    assert_eq!(detail(&d, "exitCode"), "1");
}

#[test]
fn java_too_old_names_both_versions() {
    let d = run(1, include_str!("fixtures/java-too-old.log"));
    assert_eq!(d.kind, CrashKind::JavaTooOld);
    assert_eq!(detail(&d, "required"), "21");
    assert_eq!(detail(&d, "found"), "8");
}

#[test]
fn fabric_dependency_at_wrong_version() {
    let d = run(1, include_str!("fixtures/fabric-wrong-dependency.log"));
    assert_eq!(d.kind, CrashKind::WrongDependencyVersion);
    // 22 mods want a newer Fabric API; the first one stands for them.
    assert_eq!(detail(&d, "mod"), "Accessories");
    assert_eq!(detail(&d, "count"), "22");
    assert_eq!(detail(&d, "dependency"), "Fabric API");
    assert_eq!(detail(&d, "dependencyId"), "fabric-api");
    assert_eq!(detail(&d, "required"), "0.92.0 or later");
    assert_eq!(detail(&d, "found"), "0.83.0+1.20.1");
    assert_eq!(d.actions, vec![CrashAction::OpenMods]);
    assert_eq!(d.evidence.len(), MAX_EVIDENCE);
}

#[test]
fn fabric_missing_dependency_named_by_id() {
    // Reproduced: GeckoLib for 1.20.1 without Fabric API; Fabric's error window
    // closed with Exit.
    let d = run(1, include_str!("fixtures/fabric-missing-dependency.log"));
    assert_eq!(d.kind, CrashKind::MissingDependency);
    assert_eq!(detail(&d, "mod"), "GeckoLib 4");
    assert_eq!(detail(&d, "dependency"), "fabric-api");
    assert_eq!(detail(&d, "required"), "0.92.0+1.20.1 or later");
    assert!(!d.details.contains_key("found"));
    assert_eq!(d.actions, vec![CrashAction::OpenMods]);
}

#[test]
fn fabric_corrupt_mod_jar() {
    let d = run(1, include_str!("fixtures/fabric-corrupt-jar.log"));
    assert_eq!(d.kind, CrashKind::CorruptModFile);
    assert_eq!(detail(&d, "file"), "geckolib-fabric-1.20.1-4.8.4.jar");
    assert_eq!(detail(&d, "place"), "mods");
    assert_eq!(d.actions, vec![CrashAction::OpenMods]);
}

#[test]
fn forge_out_of_memory_suggests_recommendation() {
    let lines = output(include_str!("fixtures/forge-oom.log"));
    let report = include_str!("fixtures/forge-oom-crash-report.txt");
    let d = diagnose(&CrashInput {
        crash_report: Some(RunFile {
            name: "crash-2026-10-18_12.21.54-client.txt",
            text: report,
        }),
        max_heap_mib: 512,
        recommended_heap_mib: Some(4096),
        ..input(failed(-1), &lines)
    })
    .expect("diagnosis");
    assert_eq!(d.kind, CrashKind::OutOfMemory);
    assert_eq!(detail(&d, "memory"), "512");
    assert_eq!(detail(&d, "suggested"), "4096");
    assert_eq!(
        d.report.as_deref(),
        Some("crash-2026-10-18_12.21.54-client.txt")
    );
    assert_eq!(
        d.actions,
        vec![CrashAction::MemorySettings, CrashAction::OpenCrashReports]
    );
    assert!(d.suspects.is_empty(), "NONE is not a suspect");
}

#[test]
fn out_of_memory_without_recommendation_adds_two_gib() {
    let d = run(1, "java.lang.OutOfMemoryError: Java heap space");
    assert_eq!(detail(&d, "suggested"), "6144");
}

#[test]
fn forge_missing_dependency_even_after_a_clean_exit() {
    // Forge shows its mod error screen; closing it exits with 0.
    let lines = output(include_str!("fixtures/forge-missing-dependency.log"));
    let report = include_str!("fixtures/forge-missing-dependency-crash-report.txt");
    let d = diagnose(&CrashInput {
        crash_report: Some(RunFile {
            name: "crash-2026-10-18_12.30.11-fml.txt",
            text: report,
        }),
        ..input(failed(0), &lines)
    })
    .expect("diagnosis");
    assert_eq!(d.kind, CrashKind::MissingDependency);
    assert_eq!(detail(&d, "mod"), "jeresources");
    assert_eq!(detail(&d, "dependency"), "jei");
    assert_eq!(detail(&d, "required"), "[15.2.0,)");
    assert!(!d.details.contains_key("found"));
    assert_eq!(
        d.actions,
        vec![CrashAction::OpenMods, CrashAction::OpenCrashReports]
    );
}

#[test]
fn forge_corrupt_config() {
    let lines = output(include_str!("fixtures/forge-corrupt-config.log"));
    let report = include_str!("fixtures/forge-corrupt-config-crash-report.txt");
    let d = diagnose(&CrashInput {
        crash_report: Some(RunFile {
            name: "crash-client.txt",
            text: report,
        }),
        ..input(failed(-1), &lines)
    })
    .expect("diagnosis");
    assert_eq!(d.kind, CrashKind::CorruptConfig);
    assert_eq!(detail(&d, "file"), "forge-client.toml");
    assert_eq!(detail(&d, "modId"), "forge");
    assert_eq!(d.actions[0], CrashAction::OpenConfig);
}

#[test]
fn fabric_mod_on_neoforge_is_a_note() {
    let text = format!(
        "{}\njava.lang.OutOfMemoryError: Java heap space",
        include_str!("fixtures/neoforge-fabric-mod.log")
    );
    let d = run(1, &text);
    assert_eq!(d.kind, CrashKind::OutOfMemory);
    assert_eq!(d.notes.len(), 1);
    assert_eq!(d.notes[0].kind, CrashKind::ModForOtherLoader);
    assert_eq!(
        d.notes[0].details.get("file").map(String::as_str),
        Some("sodium-fabric-0.5.13+mc1.20.1.jar")
    );
    assert_eq!(
        d.notes[0].details.get("loader").map(String::as_str),
        Some("Fabric")
    );
}

#[test]
fn native_crash_from_jvm_error_file() {
    let lines = output(include_str!("fixtures/native-crash.log"));
    let d = diagnose(&CrashInput {
        exit: ExitInfo {
            code: None,
            signal: Some(6),
        },
        jvm_error: Some(RunFile {
            name: "hs_err_pid19701.log",
            text: include_str!("fixtures/native-crash-hs_err.log"),
        }),
        ..input(ExitInfo::default(), &lines)
    })
    .expect("diagnosis");
    assert_eq!(d.kind, CrashKind::NativeCrash);
    assert_eq!(detail(&d, "module"), "libc.so.6");
    assert_eq!(detail(&d, "signal"), "SIGSEGV");
    assert_eq!(d.report.as_deref(), Some("hs_err_pid19701.log"));
    assert_eq!(d.actions, vec![CrashAction::OpenGameFolder]);
}

// ----------------------------------------- documented loader messages ----

#[test]
fn fabric_missing_mod() {
    let d = run(
        1,
        "net.fabricmc.loader.impl.FormattedException: Some of your mods are incompatible with the game or each other!\n\
         \t - Mod 'Sodium Extra' (sodium-extra) 0.5.4+mc1.20.1-build.115 requires any version of mod 'Sodium' (sodium), which is missing!\n\
         \t - Mod 'Iris' (iris) 1.7.0 requires any version of mod 'Sodium' (sodium), which is missing!",
    );
    assert_eq!(d.kind, CrashKind::MissingDependency);
    assert_eq!(detail(&d, "mod"), "Sodium Extra");
    assert_eq!(detail(&d, "dependency"), "Sodium");
    assert_eq!(detail(&d, "count"), "2");
    assert_eq!(d.evidence.len(), 2);
}

#[test]
fn fabric_mod_for_other_minecraft() {
    let d = run(
        1,
        "\t - Mod 'Lithium' (lithium) 0.11.2 requires version 1.20.1 of 'Minecraft' (minecraft), but only the wrong version is present: 1.20.4!",
    );
    assert_eq!(d.kind, CrashKind::WrongMinecraftVersion);
    assert_eq!(detail(&d, "required"), "1.20.1");
    assert_eq!(detail(&d, "found"), "1.20.4");
}

#[test]
fn fabric_loader_too_old() {
    let d = run(
        1,
        "\t - Mod 'Fabric API' (fabric-api) 0.92.2+1.20.1 requires version 0.15.0 or later of 'Fabric Loader' (fabricloader), but only the wrong version is present: 0.14.21!",
    );
    assert_eq!(d.kind, CrashKind::LoaderTooOld);
    assert_eq!(
        d.actions,
        vec![CrashAction::OpenMods, CrashAction::LoaderSettings]
    );
}

#[test]
fn fabric_java_requirement() {
    let d = run(
        1,
        "\t - Mod 'Some Mod' (somemod) 1.0 requires version 21 or later of 'OpenJDK 64-Bit Server VM' (java), but only the wrong version is present: 17!",
    );
    assert_eq!(d.kind, CrashKind::JavaTooOld);
    assert_eq!(d.actions, vec![CrashAction::JavaSettings]);
}

#[test]
fn fabric_incompatible_mods() {
    let d = run(
        1,
        "\t - Mod 'Sodium' (sodium) 0.5.8 is incompatible with any version of mod 'OptiFabric' (optifabric), but it is present!",
    );
    assert_eq!(d.kind, CrashKind::IncompatibleMods);
    assert_eq!(detail(&d, "mod"), "Sodium");
    assert_eq!(detail(&d, "dependency"), "OptiFabric");
}

#[test]
fn duplicate_mods() {
    let d = run(
        1,
        "net.minecraftforge.fml.ModLoadingException: Found duplicate mods:\n\tMod ID: 'jei' from mod files: jei-1.20.1-forge-15.2.0.27.jar, jei-1.20.1-forge-15.3.0.4.jar",
    );
    assert_eq!(d.kind, CrashKind::DuplicateMods);
    assert_eq!(detail(&d, "modId"), "jei");
    assert_eq!(
        detail(&d, "files"),
        "jei-1.20.1-forge-15.2.0.27.jar, jei-1.20.1-forge-15.3.0.4.jar"
    );
    let fabric = run(
        1,
        "net.fabricmc.loader.impl.FormattedException: Mod resolution encountered an incompatible mod set!\nFound duplicate mods: sodium",
    );
    assert_eq!(fabric.kind, CrashKind::DuplicateMods);
}

#[test]
fn forge_language_provider() {
    let d = run(
        1,
        "Mod File: /game/mods/kotlinforforge.jar needs language provider kotlinforforge:4.0 or above to load",
    );
    assert_eq!(d.kind, CrashKind::LoaderTooOld);
    assert_eq!(detail(&d, "required"), "kotlinforforge:4.0");
}

#[test]
fn mixin_names_the_mod() {
    let d = run(
        -1,
        "org.spongepowered.asm.mixin.transformer.throwables.MixinTransformerError: An unexpected critical error was encountered\n\
         Caused by: org.spongepowered.asm.mixin.injection.throwables.InvalidInjectionException: Critical injection failure: @Inject annotation on onRender could not find any targets matching 'render' in net.minecraft.class_757. [PREINJECT Applicator Phase -> examplemod.mixins.json:GameRendererMixin from mod examplemod -> Prepare Injections]",
    );
    assert_eq!(d.kind, CrashKind::MixinFailed);
    assert_eq!(detail(&d, "modId"), "examplemod");
}

#[test]
fn fabric_entrypoint_failure() {
    let d = run(
        1,
        "java.lang.RuntimeException: Could not execute entrypoint stage 'client' due to errors, provided by 'badmod' at 'com.example.BadMod'!",
    );
    assert_eq!(d.kind, CrashKind::ModFailedToLoad);
    assert_eq!(detail(&d, "modId"), "badmod");
}

#[test]
fn forge_mod_instance_failure() {
    let d = run(
        1,
        "net.minecraftforge.fml.ModLoadingException: Failed to create mod instance. ModID: badmod, class com.example.BadMod",
    );
    assert_eq!(d.kind, CrashKind::ModFailedToLoad);
    assert_eq!(detail(&d, "modId"), "badmod");
}

#[test]
fn optifine_conflict() {
    let d = run(
        -1,
        "java.lang.NoSuchMethodError: 'void net.optifine.reflect.Reflector.callVoid(...)' OptiFine reflector failed",
    );
    assert_eq!(d.kind, CrashKind::OptifineConflict);
}

// ----------------------------------------------- graphics and natives ----

#[test]
fn opengl_not_available() {
    let d = run(
        -1,
        "[LWJGL] GLFW error 65542: WGL: The driver does not appear to support OpenGL",
    );
    assert_eq!(d.kind, CrashKind::OpenglUnsupported);
    assert_eq!(detail(&d, "error"), "65542");
}

fn hs_err(frame: &str) -> String {
    format!(
        "#\n# A fatal error has been detected by the Java Runtime Environment:\n#\n\
         #  EXCEPTION_ACCESS_VIOLATION (0xc0000005) at pc=0x00007ff8, pid=4242, tid=1\n#\n\
         # Problematic frame:\n# {frame}\n#\n\
         # If you would like to submit a bug report, please visit:\n\
         #   https://bugreport.java.com/bugreport/crash.jsp\n\n\
         ---------------  S U M M A R Y ------------\n\n\
         Command Line: -Xmx4G net.minecraft.client.main.Main --accessToken SECRET-TOKEN-1234\n\n\
         Native frames: (J=compiled Java code, j=interpreted, Vv=VM code, C=native code)\n\
         {frame}\nC  [lwjgl_opengl.dll+0x1234]\n\n\
         jvm_args: -Xmx4G\njava_command: net.minecraft.client.main.Main --accessToken SECRET-TOKEN-1234\n"
    )
}

fn native(frame: &str) -> CrashDiagnosis {
    let text = hs_err(frame);
    diagnose(&CrashInput {
        jvm_error: Some(RunFile {
            name: "hs_err_pid4242.log",
            text: &text,
        }),
        ..input(failed(-1_073_741_819), &[])
    })
    .expect("diagnosis")
}

#[test]
fn graphics_driver_vendors() {
    for (frame, module, vendor) in [
        ("C  [atio6axx.dll+0x1a2b3c]", "atio6axx.dll", "amd"),
        ("C  [ig9icd64.dll+0x5f00]", "ig9icd64.dll", "intel"),
        ("C  [nvoglv64.dll+0xd0f3c1]", "nvoglv64.dll", "nvidia"),
        ("C  [radeonsi_dri.so+0x7e1c2]", "radeonsi_dri.so", "mesa"),
    ] {
        let d = native(frame);
        assert_eq!(d.kind, CrashKind::GraphicsDriver, "{frame}");
        assert_eq!(detail(&d, "module"), module);
        assert_eq!(detail(&d, "vendor"), vendor);
    }
}

#[test]
fn overlay_in_frames() {
    let d = native("C  [RTSSHooks64.dll+0x3b2c]");
    assert_eq!(d.kind, CrashKind::Overlay);
    assert_eq!(detail(&d, "program"), "RTSSHooks64");
}

#[test]
fn jvm_error_file_command_line_stays_out() {
    let d = native("C  [nvoglv64.dll+0xd0f3c1]");
    let shown = format!("{:?}", d);
    assert!(!shown.contains("SECRET-TOKEN"), "{shown}");
    assert!(!shown.contains("java_command"), "{shown}");
}

#[test]
fn missing_lwjgl_native() {
    let d = run(
        1,
        "java.lang.UnsatisfiedLinkError: Failed to locate library: liblwjgl.so",
    );
    assert_eq!(d.kind, CrashKind::MissingNativeLibrary);
    assert_eq!(d.actions, vec![CrashAction::Repair]);
}

// ------------------------------------------------------------- system ----

#[test]
fn native_memory_exhausted() {
    let d = run(
        1,
        "# There is insufficient memory for the Java Runtime Environment to continue.\n# Native memory allocation (mmap) failed to map 65536 bytes",
    );
    assert_eq!(d.kind, CrashKind::SystemMemory);
}

#[test]
fn heap_too_large_for_the_system() {
    let d = run(
        1,
        "Error occurred during initialization of VM\nCould not reserve enough space for 8388608KB object heap",
    );
    assert_eq!(d.kind, CrashKind::HeapUnavailable);
    assert_eq!(detail(&d, "memory"), "4096");
}

#[test]
fn disk_full() {
    let d = run(-1, "java.io.IOException: No space left on device");
    assert_eq!(d.kind, CrashKind::DiskFull);
}

#[test]
fn file_locked() {
    let d = run(
        -1,
        "java.nio.file.AccessDeniedException: C:\\Games\\launcher\\game\\options.txt",
    );
    assert_eq!(d.kind, CrashKind::FileAccess);
    assert_eq!(detail(&d, "file"), "options.txt");
}

// ------------------------------------------------- crash report kinds ----

fn report(text: &str) -> CrashDiagnosis {
    diagnose(&CrashInput {
        crash_report: Some(RunFile {
            name: "crash-server.txt",
            text,
        }),
        ..input(failed(-1), &[])
    })
    .expect("diagnosis")
}

#[test]
fn ticking_entity_offers_backups() {
    let d = report(
        "---- Minecraft Crash Report ----\nDescription: Ticking entity\n\njava.lang.NullPointerException: Cannot invoke \"Object.hashCode()\"\n\n\
         -- Head --\nThread: Server thread\nSuspected Mod: \n\tAlex's Mobs (alexsmobs), Version: 1.22.8\n\t\tat TRANSFORMER/alexsmobs@1.22.8/com.github.alexthe666.alexsmobs.entity.EntityCrow.tick(EntityCrow.java:1)\n\
         -- Entity being ticked --\nDetails:\n\tEntity Type: alexsmobs:crow (com.github.alexthe666.alexsmobs.entity.EntityCrow)\n\
         \tEntity's Exact location: 12.50, 64.00, -8.25\n",
    );
    assert_eq!(d.kind, CrashKind::TickingEntity);
    assert_eq!(detail(&d, "entity"), "alexsmobs:crow");
    assert_eq!(detail(&d, "position"), "12.50, 64.00, -8.25");
    assert_eq!(d.suspects, vec!["Alex's Mobs (alexsmobs)"]);
    assert_eq!(
        d.actions,
        vec![
            CrashAction::Backups,
            CrashAction::OpenCrashReports,
            CrashAction::OpenMods
        ]
    );
}

#[test]
fn ticking_block_entity() {
    let d = report(
        "Description: Ticking block entity\n\njava.lang.IllegalStateException: boom\n\n\
         -- Block entity being ticked --\nDetails:\n\tName: create:mechanical_press // com.simibubi.create.PressBlockEntity\n\
         \tBlock location: World: (100,70,-20), Section: (at 4,6,12 in 6,4,-2)\n",
    );
    assert_eq!(d.kind, CrashKind::TickingBlockEntity);
    assert_eq!(detail(&d, "block"), "create:mechanical_press");
}

#[test]
fn watchdog_and_debug_crash() {
    let hang = report(
        "Description: Watching Server\n\njava.lang.Error: ServerHangWatchdog detected that a single server tick took 60.00 seconds (should be max 0.05)\n",
    );
    assert_eq!(hang.kind, CrashKind::ServerHang);
    assert_eq!(detail(&hang, "seconds"), "60.00");
    let debug = report("Description: Manually triggered debug crash\n\njava.lang.Throwable\n");
    assert_eq!(debug.kind, CrashKind::DebugCrash);
}

#[test]
fn other_crash_report_names_category_and_transformer_suspects() {
    let d = report(
        "Description: Unexpected error\n\njava.lang.NullPointerException: boom\n\
         \tat net.minecraft.client.Minecraft.tick(Minecraft.java:1) ~[client.jar:?] {re:mixin,pl:mixin:APP:examplemod.mixins.json:MinecraftMixin,pl:mixin:A}\n\
         \tat com.example.Foo.bar(Foo.java:1) ~[examplemod-1.0.jar%23190!/:?] {}\n\
         \tat TRANSFORMER/examplemod@1.0/com.example.Foo.bar(Foo.java:1)\n",
    );
    assert_eq!(d.kind, CrashKind::GameCrash);
    assert_eq!(detail(&d, "description"), "Unexpected error");
    assert_eq!(
        detail(&d, "exception"),
        "java.lang.NullPointerException: boom"
    );
    assert_eq!(d.suspects, vec!["examplemod"]);
}

// ---------------------------------------------- exit codes and signals ----

#[test]
fn windows_exit_codes() {
    for (code, kind) in [
        (-805_306_369, CrashKind::OutOfMemory),
        (-1_073_741_819, CrashKind::NativeCrash),
        (-1_073_740_791, CrashKind::NativeCrash),
        (-1_073_741_571, CrashKind::NativeCrash),
        (-1_073_740_777, CrashKind::Overlay),
        (-1_073_741_515, CrashKind::BrokenInstallation),
        (1, CrashKind::Unknown),
    ] {
        let d = run(code, "");
        assert_eq!(d.kind, kind, "{code}");
        if kind == CrashKind::OutOfMemory {
            assert_eq!(detail(&d, "suggested"), "6144");
        }
        assert_eq!(detail(&d, "exitCode"), code.to_string());
    }
}

#[test]
fn unix_signals() {
    for (signal, kind) in [
        (9, CrashKind::Killed),
        (15, CrashKind::Terminated),
        (11, CrashKind::NativeCrash),
    ] {
        let d = diagnose(&input(
            ExitInfo {
                code: None,
                signal: Some(signal),
            },
            &[],
        ))
        .expect("diagnosis");
        assert_eq!(d.kind, kind, "{signal}");
        assert_eq!(detail(&d, "signal"), signal.to_string());
        assert!(!d.details.contains_key("exitCode"));
    }
}

#[test]
fn clean_exit_needs_no_explanation() {
    let lines = output("[Render thread/INFO]: Stopping!");
    assert_eq!(diagnose(&input(failed(0), &lines)), None);
    // A familiar message alone does not turn a clean exit into a crash.
    let lines = output("Skipping jar. File mods/a.jar is a Fabric mod and cannot be loaded");
    assert_eq!(diagnose(&input(failed(0), &lines)), None);
}

#[test]
fn evidence_is_short() {
    let long = format!(
        "java.lang.OutOfMemoryError: Java heap space {}",
        "x".repeat(1000)
    );
    let d = run(1, &long);
    assert!(d.evidence[0].chars().count() <= MAX_EVIDENCE_CHARS + 1);
}

#[test]
fn log4j_wrapping_is_removed() {
    let d = run(
        1,
        "<log4j:Event logger=\"x\" level=\"ERROR\">\n  <log4j:Message><![CDATA[Exception: java.lang.OutOfMemoryError: Java heap space]]></log4j:Message>\n</log4j:Event>",
    );
    assert_eq!(
        d.evidence,
        vec!["Exception: java.lang.OutOfMemoryError: Java heap space"]
    );
}

#[test]
fn serialized_names() {
    let d = run(1, "java.lang.OutOfMemoryError: Java heap space");
    let json = serde_json::to_value(&d).expect("json");
    assert_eq!(json["kind"], "out_of_memory");
    assert_eq!(json["actions"][0], "memory_settings");
    assert!(json.get("evidence").is_some());
}
