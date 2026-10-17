//! Live metadata check against the real Mojang services. Ignored by default; run
//! with `cargo test --test live_metadata -- --ignored --nocapture`. It downloads
//! only metadata (catalog, two descriptors, two asset indexes), no game files.

#![allow(clippy::print_stdout, clippy::unwrap_used)] // prints the evidence summary

use launcher_lib::catalog::source::{Availability, MetadataService};
use launcher_lib::install::rules::{MetaOs, RuleContext};
use launcher_lib::net::{PublicClient, SourcePolicy};
use launcher_lib::storage::paths::DataRoot;

#[tokio::test(flavor = "multi_thread")]
#[ignore = "network: contacts piston-meta.mojang.com"]
async fn live_catalog_and_plans() {
    let dir = tempfile::tempdir().unwrap();
    let client = PublicClient::new(SourcePolicy::MojangMetadata, "live-test").unwrap();
    let service = MetadataService::new(client, DataRoot::new(dir.path().to_path_buf()).unwrap());
    let view = service.supported_versions(true).await;
    assert!(view.error.is_none(), "{:?}", view.error);
    println!("latest.release (informational): {:?}", view.latest_release);
    for version in &view.versions {
        println!(
            "{} {:?} catalog sha1 {:?}",
            version.id, version.availability, version.catalog_sha1
        );
        assert_eq!(version.availability, Availability::Available);
    }
    for (id, os, arch) in [
        ("1.21.1", MetaOs::Windows, "x86_64"),
        ("1.21.1", MetaOs::Linux, "x86_64"),
        ("1.21.1", MetaOs::Osx, "x86_64"),
        ("1.21.1", MetaOs::Osx, "arm64"),
        ("26.3", MetaOs::Windows, "x86_64"),
        ("26.3", MetaOs::Linux, "x86_64"),
        ("26.3", MetaOs::Osx, "x86_64"),
        ("26.3", MetaOs::Osx, "arm64"),
    ] {
        let ctx = RuleContext::with_default_features(os, arch, None);
        let plan = service.plan(id, &ctx).await.unwrap();
        let summary = service.summarize(&plan);
        println!(
            "{id} {os:?}/{arch}: java {} libraries {} asset objects {} game artifacts {} game bytes {} missing {}",
            summary.java_major,
            summary.libraries,
            summary.asset_objects,
            summary.game_artifacts,
            summary.game_bytes,
            summary.missing_game_bytes
        );
    }
}
