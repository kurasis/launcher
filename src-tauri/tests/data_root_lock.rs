//! MV09: two launcher processes against the same DATA_ROOT. The child process is
//! this test binary re-invoked with an environment variable selecting the probe.

#![allow(clippy::print_stdout)] // the child reports its result on stdout

use std::process::Command;

use launcher_lib::error::ErrorCode;
use launcher_lib::storage::Store;
use launcher_lib::storage::paths::DataRoot;

const PROBE_ENV: &str = "LAUNCHER_LOCK_PROBE_ROOT";

/// Runs only inside the child process; reports whether it could open the store.
#[test]
fn probe_child() {
    let Some(root) = std::env::var_os(PROBE_ENV) else {
        return;
    };
    let root = DataRoot::new(root.into()).expect("valid root");
    match Store::open(root) {
        Ok(_) => println!("PROBE=acquired"),
        Err(error) if error.code == ErrorCode::DataRootLocked => println!("PROBE=locked"),
        Err(error) => println!("PROBE=error:{:?}", error.code),
    }
}

fn run_probe(root: &std::path::Path) -> String {
    let output = Command::new(std::env::current_exe().expect("test binary"))
        .args(["--exact", "probe_child", "--nocapture", "--test-threads=1"])
        .env(PROBE_ENV, root)
        .output()
        .expect("spawn probe");
    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout
        .lines()
        .find_map(|line| line.split_once("PROBE=").map(|(_, result)| result.trim()))
        .unwrap_or("missing")
        .to_owned()
}

#[test]
fn second_process_cannot_open_a_locked_data_root() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().join("Data Root ü");
    let store = Store::open(DataRoot::new(root.clone()).expect("root")).expect("first owner");
    assert_eq!(run_probe(&root), "locked");
    drop(store);
    assert_eq!(run_probe(&root), "acquired");
}
