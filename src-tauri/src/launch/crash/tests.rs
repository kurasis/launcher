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
        console_question: false,
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
    assert_eq!(
        d.actions,
        vec![CrashAction::UpdateMod, CrashAction::OpenMods]
    );
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
    assert_eq!(
        d.actions,
        vec![CrashAction::UpdateMod, CrashAction::OpenMods]
    );
}

#[test]
fn quilt_missing_dependency() {
    // Reproduced: Sodium Extra without Sodium on Quilt 0.30.1.
    let d = run(1, include_str!("fixtures/quilt-missing-dependency.log"));
    assert_eq!(d.kind, CrashKind::MissingDependency);
    assert_eq!(detail(&d, "mod"), "Sodium Extra");
    assert_eq!(detail(&d, "dependency"), "sodium");
    assert_eq!(detail(&d, "required"), "[0.8.13+mc1.21.1, ∞)");
    assert_eq!(
        d.actions,
        vec![CrashAction::UpdateMod, CrashAction::OpenMods]
    );
}

#[test]
fn quilt_cause_from_its_report_when_its_window_failed() {
    // Reproduced: Quilt could not open its error window; the output names only
    // the report, which holds the cause.
    let lines = output(include_str!("fixtures/quilt-error-window-failed.log"));
    let d = diagnose(&CrashInput {
        crash_report: Some(RunFile {
            name: "crash-2026-10-18_13.39.43.7208-quilt_loader.txt",
            text: include_str!("fixtures/quilt-missing-dependency-crash-report.txt"),
        }),
        ..input(failed(1), &lines)
    })
    .expect("diagnosis");
    assert_eq!(d.kind, CrashKind::MissingDependency);
    assert_eq!(detail(&d, "mod"), "Sodium Extra");
    assert_eq!(detail(&d, "dependency"), "sodium");
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
        console_question: false,
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
        vec![
            CrashAction::UpdateMod,
            CrashAction::OpenMods,
            CrashAction::OpenCrashReports
        ]
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
fn mixin_culprit_is_the_failing_mod_not_the_first_config_in_the_log() {
    // NeoForge 26.3 with Sodium 0.9.2 (game test, 2026-10-20): every mod's config is
    // listed at startup, Balm's kuma_api and sodium-frapi long before the failure.
    let d = run(
        -1,
        "[08:17:36] [main/INFO]: Mixin config kuma_api.mixins.json from jar(mods/balm-neoforge.jar)\n\
         [08:17:36] [main/INFO]: Mixin config sodium-frapi.mixins.json from jar(mods/sodium-neoforge-0.9.2+mc26.3.jar)\n\
         [08:17:36] [main/WARN]: Reference map 'xaerolib.refmap.json' for xaerolib.mixins.json could not be read.\n\
         [08:17:48] [Render thread/FATAL]: Mixin apply for mod sodium failed sodium-neoforge.mixins.json:platform.neoforge.LevelExtractorMixin from mod sodium -> net.minecraft.client.renderer.extract.LevelExtractor: org.spongepowered.asm.mixin.injection.throwables.InvalidInjectionException Critical injection failure",
    );
    assert_eq!(d.kind, CrashKind::MixinFailed);
    assert_eq!(detail(&d, "modId"), "sodium");
}

#[test]
fn mixin_culprit_from_the_failing_config_when_no_line_names_the_mod() {
    let d = run(
        -1,
        "[main/INFO]: Mixin config kuma_api.mixins.json from jar(mods/balm.jar)\n\
         org.spongepowered.asm.mixin.throwables.MixinApplyError: Mixin [iris.mixins.json:MixinLevelRenderer] FAILED during APPLY",
    );
    assert_eq!(d.kind, CrashKind::MixinFailed);
    assert_eq!(detail(&d, "modId"), "iris");
}

#[test]
fn forge_module_is_not_a_suspect() {
    let d = run(
        1,
        "Failed to create mod instance. ModID: journeymap, class journeymap.common.JourneymapForge\n\
         \tat TRANSFORMER/journeymap@1.21.1-6.0.9/journeymap.common.JourneymapForge.<init>(JourneymapForge.java:41)\n\
         \tat TRANSFORMER/net.minecraftforge.forge@52.1.0/net.minecraftforge.fml.ModContainer.x(ModContainer.java:1)",
    );
    assert_eq!(d.kind, CrashKind::ModFailedToLoad);
    assert_eq!(d.suspects, vec!["journeymap"]);
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
fn closing_an_old_game_window_is_no_crash() {
    // 1.5.2: the window listener prints this and exits with code 1.
    let lines = output(
        "2026-10-22 04:22:00 [CLIENT] [INFO] [CHAT] Saved screenshot\nSomeone is closing me!\nAL lib: (EE) alc_cleanup: 1 device not closed",
    );
    assert!(closed_by_its_window(failed(1), &lines));
    assert_eq!(diagnose(&input(failed(1), &lines)), None);
    // Another code, or the line long before the end, is still a failure.
    assert!(!closed_by_its_window(failed(2), &lines));
    let mut early = lines.clone();
    early.extend((0..20).map(|i| format!("line {i}")));
    assert!(!closed_by_its_window(failed(1), &early));
    assert!(diagnose(&input(failed(1), &early)).is_some());
}

#[test]
fn closing_the_oldest_game_window_is_no_crash() {
    // Classic to Alpha 1.0: launchwrapper's frame exits with code 1 without a word;
    // the launcher's tweaker prints its line first.
    let lines = output(
        "Starting up SoundSystem...\nOpenAL initialized.\nLauncher: the game window was closed\nAL lib: (EE) alc_cleanup: 1 device not closed",
    );
    assert!(closed_by_its_window(failed(1), &lines));
    assert_eq!(diagnose(&input(failed(1), &lines)), None);
    // Classic's exit hangs on Windows; the tweaker ends it with the same code.
    let halted = output(
        "OpenAL initialized.\nLauncher: the game window was closed\nLauncher: the game did not end after its window closed; ended",
    );
    assert!(closed_by_its_window(failed(1), &halted));
    assert_eq!(diagnose(&input(failed(1), &halted)), None);
    // Without the line, code 1 is a failure, as a start that threw is.
    let silent = output("Starting up SoundSystem...\nOpenAL initialized.");
    assert!(!closed_by_its_window(failed(1), &silent));
    assert!(diagnose(&input(failed(1), &silent)).is_some());
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

// ------------------------------------- loader message templates (D-059) ----
// From the loaders' own message files: Fabric's Messages.properties, Quilt's
// quilt_loader.properties, Forge's and NeoForge's lang/en_us.json.

fn asked(text: &str) -> CrashDiagnosis {
    let lines = output(text);
    let exit = ExitInfo {
        code: None,
        signal: Some(9),
    };
    diagnose(&CrashInput {
        console_question: true,
        ..input(exit, &lines)
    })
    .expect("diagnosis")
}

#[test]
fn quilt_duplicate_lists_the_files() {
    // Reproduced: Sodium twice under two file names on Quilt.
    let d = run(1, include_str!("fixtures/quilt-duplicate.log"));
    assert_eq!(d.kind, CrashKind::DuplicateMods);
    assert_eq!(detail(&d, "modId"), "sodium");
    assert_eq!(
        detail(&d, "files"),
        "sodium-fabric-0.8.13+mc1.21.1.jar, sodium-copy.jar"
    );
}

#[test]
fn neoforge_missing_dependency() {
    // Reproduced: Supplementaries without Moonlight Lib on NeoForge 21.1 (the
    // error screen's clean exit counts with its crash report; see above).
    let d = run(1, include_str!("fixtures/neoforge-missing-dependency.log"));
    assert_eq!(d.kind, CrashKind::MissingDependency);
    assert_eq!(detail(&d, "mod"), "supplementaries");
    assert_eq!(detail(&d, "dependency"), "moonlight");
}

#[test]
fn forge_52_missing_dependency() {
    // Reproduced: Biomes O' Plenty without GlitchCore and TerraBlender on Forge
    // 52.0.57; the first unmet requirement stands for both.
    let d = run(1, include_str!("fixtures/forge-52-missing-dependency.log"));
    assert_eq!(d.kind, CrashKind::MissingDependency);
    assert_eq!(detail(&d, "mod"), "biomesoplenty");
    assert_eq!(detail(&d, "dependency"), "glitchcore");
    assert_eq!(detail(&d, "count"), "2");
}

#[test]
fn neoforge_screen_texts_without_the_mod_id_table() {
    let d = run(
        1,
        "Error during pre-loading phase: Mod supplementaries requires moonlight 1.21.1-3.6.4 or above\nCurrently, moonlight is not installed",
    );
    assert_eq!(d.kind, CrashKind::MissingDependency);
    assert_eq!(detail(&d, "required"), "1.21.1-3.6.4 or above");
    assert!(!d.details.contains_key("found"));

    let d = run(
        1,
        "Mod create only supports flywheel between 1.0 and 2.0 (exclusive)\nCurrently, flywheel is 0.6.9",
    );
    assert_eq!(d.kind, CrashKind::WrongDependencyVersion);
    assert_eq!(detail(&d, "found"), "0.6.9");

    let d = run(
        1,
        "Mod jei requires minecraft 1.21.1\nCurrently, minecraft is 26.3",
    );
    assert_eq!(d.kind, CrashKind::WrongMinecraftVersion);
    assert_eq!(detail(&d, "required"), "1.21.1");
    assert_eq!(detail(&d, "found"), "26.3");

    let d = run(
        1,
        "Mod embeddium is incompatible with sodium any\nCurrently, sodium is 0.6.0",
    );
    assert_eq!(d.kind, CrashKind::IncompatibleMods);
    assert_eq!(detail(&d, "dependency"), "sodium");

    let d = run(
        1,
        "Mod geckolib requires neoforge 21.1.200 or above\nCurrently, neoforge is 21.1.77",
    );
    assert_eq!(d.kind, CrashKind::LoaderTooOld);
    assert!(d.actions.contains(&CrashAction::LoaderSettings));

    let d = run(
        1,
        "Mod jei is present in multiple files: jei-a.jar, jei-b.jar",
    );
    assert_eq!(d.kind, CrashKind::DuplicateMods);
    assert_eq!(detail(&d, "modId"), "jei");
    assert_eq!(detail(&d, "files"), "jei-a.jar, jei-b.jar");
}

#[test]
fn fabric_version_phrases() {
    let d = run(
        1,
        "\t - Mod 'Create' (create) 6.0.8 requires any version after 1.0 of mod 'Flywheel' (flywheel), but only the wrong versions are present: 0.6.9, 0.6.10!",
    );
    assert_eq!(d.kind, CrashKind::WrongDependencyVersion);
    assert_eq!(detail(&d, "required"), "any version after 1.0");
    assert_eq!(detail(&d, "found"), "0.6.9, 0.6.10");

    let d = run(
        1,
        "\t - Mod 'Iris' (iris) 1.8.0 requires any 0.6.x version of sodium, which is missing!",
    );
    assert_eq!(d.kind, CrashKind::MissingDependency);
    assert_eq!(detail(&d, "required"), "any 0.6.x version");

    let d = run(
        1,
        "\t - Mod 'Mod Menu' (modmenu) 11.0.0 requires any version of fabric-api, which is missing!",
    );
    assert_eq!(d.kind, CrashKind::MissingDependency);
    assert!(!d.details.contains_key("required"));

    let d = run(
        1,
        "\t - Mod 'Some Mod' (somemod) 1.0 requires version 1.0 or later of mod 'Server Lib' (serverlib), which is disabled for this environment (client/server only)!",
    );
    assert_eq!(d.kind, CrashKind::WrongDependencyVersion);
    assert_eq!(detail(&d, "dependency"), "Server Lib");

    let d = run(
        1,
        "\t - Mod 'Iris' (iris) 1.8.0 is incompatible with any version before 0.6 of mod 'Sodium' (sodium), yet a conflicting version is present: 0.5.11!",
    );
    assert_eq!(d.kind, CrashKind::IncompatibleMods);
    assert_eq!(detail(&d, "dependency"), "Sodium");
}

#[test]
fn quilt_titles() {
    let d = run(
        1,
        "GeckoLib 4 requires version 1.20.1 of Minecraft, but only a different version is present: 1.21.1",
    );
    assert_eq!(d.kind, CrashKind::WrongMinecraftVersion);
    assert_eq!(detail(&d, "required"), "1.20.1");
    assert_eq!(detail(&d, "found"), "1.21.1");

    let d = run(
        1,
        "Create transitively requires at least version 1.0 or any newer version of flywheel, which is missing!",
    );
    assert_eq!(d.kind, CrashKind::MissingDependency);
    assert_eq!(detail(&d, "mod"), "Create");
    assert_eq!(
        detail(&d, "required"),
        "at least version 1.0 or any newer version"
    );

    let d = run(
        1,
        "Sodium Extra requires any version of Quilt Loader, but only different versions are present!",
    );
    assert_eq!(d.kind, CrashKind::LoaderTooOld);

    let d = run(
        1,
        "Iris breaks with all versions below 0.6 of Sodium, but an incompatible version is present: 0.5.11",
    );
    assert_eq!(d.kind, CrashKind::IncompatibleMods);
    assert_eq!(detail(&d, "mod"), "Iris");
    assert_eq!(detail(&d, "dependency"), "Sodium");

    let d = run(1, "Duplicate mod: sodium");
    assert_eq!(d.kind, CrashKind::DuplicateMods);
    assert_eq!(detail(&d, "modId"), "sodium");

    let d = run(
        1,
        "Broken zip file: sodium-fabric.jar\nFailed to read as a zip: <mods>/sodium-fabric.jar",
    );
    assert_eq!(d.kind, CrashKind::CorruptModFile);
}

#[test]
fn mods_for_other_loaders_are_noted() {
    let d = run(
        1,
        "Skipping jar. File mods/JustEnoughResources-Forge-1.21.1-1.6.0.17.jar is for Minecraft Forge or an older version of NeoForge, and cannot be loaded\njava.lang.OutOfMemoryError: Java heap space",
    );
    assert_eq!(d.kind, CrashKind::OutOfMemory);
    assert_eq!(d.notes[0].kind, CrashKind::ModForOtherLoader);
    assert_eq!(
        d.notes[0].details.get("file").map(String::as_str),
        Some("JustEnoughResources-Forge-1.21.1-1.6.0.17.jar")
    );
    assert_eq!(
        d.notes[0].details.get("loader").map(String::as_str),
        Some("Forge")
    );

    let d = run(
        1,
        "Found 2 Forge mods.\njava.lang.OutOfMemoryError: Java heap space",
    );
    assert_eq!(d.notes[0].kind, CrashKind::ModForOtherLoader);
    assert!(!d.notes[0].details.contains_key("file"));
}

// --------------------------------------------- unanswered console questions ----

#[test]
fn console_questions_are_recognized() {
    for line in [
        "y/n: ",
        "[O]kay/[C]ancel: ",
        "[Y]es/[N]o: ",
        "Continue? y/n",
    ] {
        assert!(is_console_question(line), "{line}");
    }
    for line in [
        "Loading 3 mods: y/n compat",
        "[Render thread/INFO]: Ready",
        "",
    ] {
        assert!(!is_console_question(line), "{line}");
    }
}

#[test]
fn neoforge_broken_jar_behind_a_console_question() {
    // Reproduced: a truncated mod jar on NeoForge 21.1; its loading window
    // failed and asked "y/n:" on the console without end.
    let d = asked(include_str!("fixtures/neoforge-corrupt-jar-question.log"));
    assert_eq!(d.kind, CrashKind::CorruptModFile);
    assert_eq!(detail(&d, "place"), "mods");
    assert_eq!(d.actions, vec![CrashAction::OpenMods]);
    assert_eq!(d.notes[0].kind, CrashKind::ConsoleQuestion);
}

#[test]
fn modpack_memory_check_behind_a_console_question() {
    // Reproduced: Prominence II with 1024 MiB asks "[O]kay/[C]ancel:".
    let d = asked(include_str!("fixtures/pack-memory-question.log"));
    assert_eq!(d.kind, CrashKind::PackNeedsMemory);
    assert_eq!(detail(&d, "memory"), "1024");
    assert_eq!(detail(&d, "required"), "4096");
    assert_eq!(d.actions, vec![CrashAction::MemorySettings]);
    assert_eq!(d.notes[0].kind, CrashKind::ConsoleQuestion);
}

#[test]
fn unknown_console_question_shows_the_message_above_it() {
    let d = asked(
        "[main/INFO]: Starting\nSomething needs your attention.\nOpen the help page?\n[Y]es/[N]o: \n[Y]es/[N]o: ",
    );
    assert_eq!(d.kind, CrashKind::ConsoleQuestion);
    assert_eq!(
        d.evidence,
        vec![
            "[main/INFO]: Starting",
            "Something needs your attention.",
            "Open the help page?"
        ]
    );
}

#[test]
fn quilt_broken_jar() {
    // Reproduced: a truncated Sodium jar on Quilt.
    let d = run(1, include_str!("fixtures/quilt-corrupt-jar.log"));
    assert_eq!(d.kind, CrashKind::CorruptModFile);
    assert_eq!(detail(&d, "file"), "sodium-fabric-0.8.13+mc1.21.1.jar");
    assert_eq!(detail(&d, "place"), "mods");
}

// ------------------------------------------------- Forge for 1.12.2 ----

#[test]
fn legacy_forge_missing_dependency_after_its_error_screen() {
    // Reproduced: Tinkers' Construct without Mantle on Forge 14.23.5.2859. The
    // error screen writes no crash report; closing the game exits with 0.
    let d = run(
        0,
        include_str!("fixtures/forge-1.12.2-missing-dependency.log"),
    );
    assert_eq!(d.kind, CrashKind::MissingDependency);
    assert_eq!(detail(&d, "mod"), "Tinkers' Construct");
    assert_eq!(detail(&d, "dependency"), "mantle");
    assert_eq!(detail(&d, "required"), "[1.12-1.3.3.49,)");
    assert!(!d.details.contains_key("found"));
}

#[test]
fn legacy_forge_duplicate_after_its_error_screen() {
    // Reproduced: JEI twice under two file names on Forge 14.23.5.2859.
    let d = run(0, include_str!("fixtures/forge-1.12.2-duplicate.log"));
    assert_eq!(d.kind, CrashKind::DuplicateMods);
    assert_eq!(detail(&d, "modId"), "jei");
    assert_eq!(
        detail(&d, "files"),
        "jei_1.12.2-4.22.0.1036.jar, jei-1.12.2-copy.jar"
    );
}

#[test]
fn legacy_forge_wrong_dependency_version() {
    let d = run(
        0,
        "net.minecraftforge.fml.common.MissingModsException: Mod jeresources (Just Enough Resources) requires [jei@[4.15.0,)]\n\tunknown : need [4.15.0,): have 4.8.5",
    );
    assert_eq!(d.kind, CrashKind::WrongDependencyVersion);
    assert_eq!(detail(&d, "found"), "4.8.5");
}

#[test]
fn clean_exits_of_normal_games_stay_unexplained() {
    // Reproduced: games that loaded and were closed by the player: NeoForge
    // keeping one of two equal jars, NeoForge warning about a Forge mod (the
    // player went on), Forge 52 with JEI.
    for text in [
        include_str!("fixtures/clean-neoforge-same-mod-twice.log"),
        include_str!("fixtures/clean-neoforge-forge-mod-warning.log"),
        include_str!("fixtures/clean-forge-normal-game.log"),
    ] {
        let lines = output(text);
        assert_eq!(diagnose(&input(failed(0), &lines)), None);
    }
}
