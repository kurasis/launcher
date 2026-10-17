//! Plans every vanilla version Mojang lists for every supported target with the
//! real catalog, descriptors and asset indexes (D-035). Ignored by default; run
//! `cargo test --test live_all_versions -- --ignored --nocapture`
//! (`LAUNCHER_LIVE_ROOT=<dir>` reuses a metadata cache). Nothing but metadata is
//! downloaded.

#![allow(clippy::print_stdout, clippy::unwrap_used, clippy::expect_used)] // prints the summary

use std::collections::BTreeMap;
use std::path::PathBuf;

use launcher_lib::catalog::source::MetadataService;
use launcher_lib::error::ErrorCode;
use launcher_lib::install::rules::{MetaOs, RuleContext};
use launcher_lib::net::{PublicClient, SourcePolicy};
use launcher_lib::storage::paths::DataRoot;

#[tokio::test(flavor = "multi_thread")]
#[ignore = "fetches every version descriptor from Mojang"]
async fn every_version_plans_on_every_target() {
    let dir = tempfile::tempdir().unwrap();
    let root = std::env::var_os("LAUNCHER_LIVE_ROOT")
        .map_or_else(|| dir.path().to_path_buf(), PathBuf::from);
    let service = MetadataService::new(
        PublicClient::new(SourcePolicy::MojangMetadata, "live-test").unwrap(),
        DataRoot::new(root).unwrap(),
    );
    let view = service.supported_versions(true).await;
    assert!(view.error.is_none(), "{:?}", view.error);
    // OS versions as the app reports them (`os.version` rules need one).
    let targets = [
        ("windows-x64", MetaOs::Windows, "x86_64", "10.0"),
        ("linux-x64", MetaOs::Linux, "x86_64", "6.8.0"),
        ("macos-x64", MetaOs::Osx, "x86_64", "14.5"),
        ("macos-arm64", MetaOs::Osx, "arm64", "14.5"),
    ];
    let mut summary: BTreeMap<(&str, String), BTreeMap<String, usize>> = BTreeMap::new();
    let mut failures = Vec::new();
    for version in &view.versions {
        for (target, os, arch, os_version) in targets {
            let ctx = RuleContext::with_default_features(os, arch, Some(os_version.to_owned()));
            let outcome = match service.plan(&version.id, &ctx).await {
                Ok(plan) => format!(
                    "ok java{} {:?}{}",
                    plan.runtime.major,
                    plan.asset_layout,
                    if plan.natives.is_empty() {
                        ""
                    } else {
                        " natives"
                    }
                ),
                // Apple silicon cannot run Intel-only natives: an expected refusal.
                Err(e) if e.code == ErrorCode::UnsupportedPlatform && target == "macos-arm64" => {
                    "unsupported on Apple silicon".to_owned()
                }
                Err(e) => {
                    failures.push(format!(
                        "{} {target} | {:?} {:?}",
                        version.id, e.code, e.safe_details
                    ));
                    format!("FAILED {:?}", e.code)
                }
            };
            *summary
                .entry((target, version.version_type.clone()))
                .or_default()
                .entry(outcome)
                .or_default() += 1;
        }
    }
    for ((target, version_type), outcomes) in &summary {
        println!("{target} {version_type}: {outcomes:?}");
    }
    println!(
        "{} versions x {} targets",
        view.versions.len(),
        targets.len()
    );
    let mut reasons: BTreeMap<String, (usize, String)> = BTreeMap::new();
    for failure in &failures {
        let (id, reason) = failure.split_once(" | ").unwrap();
        let entry = reasons
            .entry(reason.to_owned())
            .or_insert((0, id.to_owned()));
        entry.0 += 1;
    }
    for (reason, (count, example)) in &reasons {
        println!("  {count:4}  {reason}  e.g. {example}");
    }
    assert!(failures.is_empty(), "{} plans failed", failures.len());
}
