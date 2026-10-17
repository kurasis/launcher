//! Live checks of launch arguments against real managed Java. Ignored by default;
//! run with
//! `KURASIS_LIVE_ROOT=<installed data dir> cargo test --test live_launch_args -- --ignored --nocapture --test-threads=1`
//! where the data directory holds a completed installation (P08).
//!
//! 1. Argument echo: tricky values reach Java unchanged through an argfile and as
//!    direct arguments (`-XshowSettings:properties -version` prints them back).
//! 2. The real compiled JVM arguments of the installed version are accepted by
//!    its managed Java and produce the expected classpath and library path.
//!    `-version` replaces the main class, so Minecraft is never started and no
//!    account session is used.

#![allow(clippy::print_stdout, clippy::unwrap_used, clippy::expect_used)] // prints the evidence summary

use std::path::PathBuf;

use kurasis_launcher_lib::install::lock::runtime_from_lock;
use kurasis_launcher_lib::install::rules::RuleContext;
use kurasis_launcher_lib::launch::compile::{self, LaunchIdentity, LaunchInputs};
use kurasis_launcher_lib::launch::{argfile, encoding};
use kurasis_launcher_lib::runtime::{Verification, probe, verify_installed};
use kurasis_launcher_lib::secrets::SecretString;
use kurasis_launcher_lib::storage::models::Resolution;
use kurasis_launcher_lib::storage::paths::DataRoot;
use kurasis_launcher_lib::storage::reconcile::{LockState, read_lock};

fn root() -> DataRoot {
    let path = std::env::var("KURASIS_LIVE_ROOT").expect("set KURASIS_LIVE_ROOT");
    DataRoot::new(PathBuf::from(path)).unwrap()
}

fn installed() -> (
    DataRoot,
    kurasis_launcher_lib::storage::models::InstallLock,
    PathBuf,
) {
    let root = root();
    let instances = std::fs::read_dir(root.instances_dir()).unwrap();
    for entry in instances.flatten() {
        if let Ok(LockState::Complete(lock)) = read_lock(&entry.path().join("install.lock.json")) {
            let package = runtime_from_lock(&lock).unwrap();
            let Verification::Valid(runtime) = verify_installed(&root, &package).unwrap() else {
                panic!("runtime not valid");
            };
            return (root, *lock, runtime.java);
        }
    }
    panic!("no completed installation under KURASIS_LIVE_ROOT");
}

/// Parses `-XshowSettings:properties`; path-like properties continue on the
/// following, more deeply indented lines (one entry per line).
fn properties(output: &[u8]) -> Vec<(String, Vec<String>)> {
    let mut props: Vec<(String, Vec<String>)> = Vec::new();
    for line in String::from_utf8_lossy(output).lines() {
        let top = line.starts_with("    ") && !line.starts_with("        ");
        if top && let Some((key, value)) = line.trim().split_once(" = ") {
            props.push((key.to_owned(), vec![value.to_owned()]));
        } else if line.starts_with("        ")
            && let Some(last) = props.last_mut()
        {
            last.1.push(line.trim().to_owned());
        }
    }
    props
}

fn get(props: &[(String, Vec<String>)], key: &str) -> Vec<String> {
    props
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.clone())
        .unwrap_or_default()
}

#[test]
#[ignore = "needs an installed data directory with managed Java"]
fn tricky_values_survive_argfiles_and_direct_arguments() {
    let (_, _, java) = installed();
    let dir = tempfile::tempdir().unwrap();
    let values = [
        r"C:\Users\A B\AppData\Roaming\x.jar",
        r#"quote " inside"#,
        "#not a comment",
        "@not-a-file",
        "trailing backslash \\",
        "Unicode: Jürgen игры 世界",
    ];
    let args: Vec<String> = values
        .iter()
        .enumerate()
        .map(|(i, v)| format!("-Dkurasis.echo{i}={v}"))
        .collect();
    let file = dir.path().join("jvm args.txt");
    std::fs::write(&file, argfile::encode(&args, false).unwrap()).unwrap();
    for via_file in [true, false] {
        let mut command = probe::sanitized_command(&java);
        // A UTF-8 locale; the POSIX case is checked below.
        command.env("LC_ALL", "C.UTF-8");
        if via_file {
            command.arg(format!("@{}", file.display()));
        } else {
            command.args(&args);
        }
        let output = command
            .args(["-XshowSettings:properties", "-version"])
            .output()
            .unwrap();
        assert!(output.status.success());
        let props = properties(&output.stderr);
        for (i, value) in values.iter().enumerate() {
            let got = get(&props, &format!("kurasis.echo{i}"));
            assert_eq!(got, vec![(*value).to_owned()], "via_file={via_file} #{i}");
        }
        let how = if via_file {
            "argfile"
        } else {
            "direct arguments"
        };
        println!(
            "argument echo via {how}: {} values round-tripped",
            values.len()
        );
    }
    // A POSIX locale cannot carry non-ASCII arguments; the preflight check sees it.
    let sent = values[5];
    for (locale, expect) in [("C.UTF-8", true), ("POSIX", false)] {
        let output = probe::sanitized_command(&java)
            .env("LC_ALL", locale)
            .arg(format!("-Dkurasis.argument.check={sent}"))
            .args(["-XshowSettings:properties", "-version"])
            .output()
            .unwrap();
        let mut bytes = output.stderr.clone();
        bytes.extend(output.stdout);
        assert_eq!(encoding::intact(sent, &bytes), expect, "{locale}");
        println!("LC_ALL={locale}: non-ASCII argument intact = {expect}");
    }
}

#[test]
#[ignore = "needs an installed data directory with managed Java"]
fn real_jvm_arguments_are_accepted_by_the_managed_java() {
    let (root, lock, java) = installed();
    let ctx = RuleContext::current().unwrap();
    let descriptor = std::fs::read(root.version_descriptor(
        &kurasis_launcher_lib::storage::paths::SafeName::parse(&lock.version_id).unwrap(),
        &lock.descriptor.sha1,
    ))
    .unwrap();
    let plan = kurasis_launcher_lib::install::plan::plan_game(
        &descriptor,
        &lock.descriptor.url,
        &lock.descriptor.sha1,
        &lock.version_id,
        &ctx,
    )
    .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let natives = dir.path().join("natives");
    let game_dir = root.game_dir(lock.instance_id);
    // Only the JVM part is executed; the identity fills game arguments that are
    // never passed to any process here.
    let identity = LaunchIdentity {
        player_name: "unused".into(),
        uuid: "00000000000000000000000000000000".into(),
        access_token: SecretString::new("unused-never-passed"),
        xuid: None,
        client_id: "unused".into(),
    };
    let compiled = compile::compile(&LaunchInputs {
        plan: &plan,
        root: &root,
        game_dir: &game_dir,
        natives_dir: &natives,
        max_heap_mib: 2048,
        resolution: Resolution {
            width: 1280,
            height: 720,
        },
        identity: &identity,
        launcher_version: "0.1.0",
    })
    .unwrap();
    assert!(
        compiled
            .jvm_args
            .iter()
            .all(|a| !a.contains("unused-never-passed"))
    );
    for dir in &compiled.native_dirs {
        std::fs::create_dir_all(dir).unwrap();
    }
    let argfile_path = dir.path().join("jvm.args");
    std::fs::write(
        &argfile_path,
        argfile::encode(&compiled.jvm_args, cfg!(windows)).unwrap(),
    )
    .unwrap();
    let output = probe::sanitized_command(&java)
        .arg(format!("@{}", argfile_path.display()))
        .args(["-XshowSettings:properties", "-version"])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stderr}");
    let props = properties(&output.stderr);
    let classpath = get(&props, "java.class.path");
    assert_eq!(classpath.len(), lock.classpath.len());
    for entry in &classpath {
        assert!(std::path::Path::new(entry).is_file(), "{entry}");
    }
    assert_eq!(
        get(&props, "java.library.path"),
        vec![natives.to_str().unwrap().to_owned()]
    );
    println!(
        "Java {} accepted all {} JVM arguments for {} (classpath {} entries, all present; log4j config {})",
        get(&props, "java.version").join(""),
        compiled.jvm_args.len(),
        lock.version_id,
        classpath.len(),
        get(&props, "log4j.configurationFile").join("")
    );
}
