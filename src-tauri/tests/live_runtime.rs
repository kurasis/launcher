//! Live managed-Java check. Ignored by default; run with
//! `cargo test --test live_runtime -- --ignored --nocapture --test-threads=1`.
//!
//! 1. Resolves Temurin 21 and 25 JREs for this computer, installs them (download,
//!    SHA-256, safe extraction, tree receipt, probe) and reuses them a second time.
//! 2. Resolves the other product targets and extracts their real archives without
//!    executing anything, to check the extractor and layout on every package type.
//!
//! 3. Resolves Mojang runtimes (D-036) of every component for this computer,
//!    installs them from their manifests and probes them; resolves the other
//!    targets' manifests without downloading their files.
//!
//! Downloads roughly 1 GB in total.

#![allow(clippy::print_stdout, clippy::unwrap_used)] // prints the evidence summary

use launcher_lib::download::transport::HttpTransport;
use launcher_lib::download::{
    CancelToken, Digest, DownloadRequest, Executor, ExecutorConfig, NoProgress,
    resolve_managed_path,
};
use launcher_lib::install::plan::{RuntimeRequirement, runtime_target};
use launcher_lib::install::rules::RuleContext;
use launcher_lib::net::{PublicClient, SourcePolicy};
use launcher_lib::runtime::adoptium::Adoptium;
use launcher_lib::runtime::extract::{self, Limits};
use launcher_lib::runtime::mojang::{self, Mojang};
use launcher_lib::runtime::tree::{self, EntryKind};
use launcher_lib::runtime::{RuntimeInstaller, Verification, probe, verify_installed};
use launcher_lib::storage::paths::{DataRoot, JavaProvider, TargetArch, TargetOs};
use url::Url;
use uuid::Uuid;

fn provider() -> Adoptium {
    Adoptium::new(PublicClient::new(SourcePolicy::Adoptium, "live-test").unwrap()).unwrap()
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "network: downloads Temurin runtimes"]
async fn install_host_runtimes_and_reuse() {
    let dir = tempfile::tempdir().unwrap();
    let root = DataRoot::new(dir.path().to_path_buf()).unwrap();
    let (os, arch) = runtime_target(&RuleContext::current().unwrap()).unwrap();
    let installer = RuntimeInstaller::new(
        root.clone(),
        HttpTransport::public("live-test").unwrap(),
        ExecutorConfig::default(),
    );
    for major in [21, 25] {
        let package = provider()
            .resolve(&RuntimeRequirement {
                major,
                os,
                arch,
                component: None,
            })
            .await
            .unwrap();
        println!(
            "Java {major}: {} {} ({} bytes, sha256 {}, jdk fallback {})",
            package.release_name,
            package.archive_name,
            package.archive_size,
            package.key.archive_sha256,
            package.jdk_fallback
        );
        let started = std::time::Instant::now();
        let installed = installer
            .ensure(&package, Uuid::new_v4(), &CancelToken::new(), &NoProgress)
            .await
            .unwrap();
        let probe = &installed.receipt.probe;
        println!(
            "  installed in {:.1?} at {}\n  probe: java.version={} spec={} os.arch={} vendor={:?}\n  tree receipt sha256 {}",
            started.elapsed(),
            installed.dir.strip_prefix(dir.path()).unwrap().display(),
            probe.java_version,
            probe.specification_version,
            probe.os_arch,
            probe.vendor,
            installed.receipt.tree_receipt_sha256
        );
        assert!(!installed.reused);
        assert_eq!(probe.specification_version, major);
        assert!(matches!(
            verify_installed(&root, &package).unwrap(),
            Verification::Valid(_)
        ));
        let again = installer
            .ensure(&package, Uuid::new_v4(), &CancelToken::new(), &NoProgress)
            .await
            .unwrap();
        assert!(again.reused);
        println!("  second install reused the verified runtime");
    }
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "network: downloads Temurin runtimes"]
async fn other_targets_resolve_and_extract_without_running() {
    let dir = tempfile::tempdir().unwrap();
    let root = DataRoot::new(dir.path().to_path_buf()).unwrap();
    let executor = Executor::new(
        HttpTransport::public("live-test").unwrap(),
        root.clone(),
        ExecutorConfig::default(),
    );
    for (major, os, arch) in [
        (21, TargetOs::Windows, TargetArch::X64),
        (25, TargetOs::Mac, TargetArch::Aarch64),
        (21, TargetOs::Mac, TargetArch::X64),
    ] {
        let package = provider()
            .resolve(&RuntimeRequirement {
                major,
                os,
                arch,
                component: None,
            })
            .await
            .unwrap();
        let path = format!("archives/{}", package.archive_name);
        executor
            .run(
                Uuid::new_v4(),
                vec![DownloadRequest {
                    source: SourcePolicy::Adoptium,
                    url: Url::parse(&package.archive_url).unwrap(),
                    path: path.clone(),
                    size: package.archive_size,
                    digest: Digest::Sha256(package.key.archive_sha256.clone()),
                    label: None,
                }],
                &CancelToken::new(),
                &NoProgress,
            )
            .await
            .unwrap();
        let archive = resolve_managed_path(&root, &path).unwrap();
        let out = dir
            .path()
            .join(format!("x-{}-{}-{major}", os.as_str(), arch.as_str()));
        std::fs::create_dir(&out).unwrap();
        let result = extract::extract(&archive, package.archive_format, &out, Limits::default());
        if cfg!(windows) && result.is_err() && os == TargetOs::Mac {
            println!(
                "{}: links cannot be created on this host",
                package.archive_name
            );
            continue;
        }
        result.unwrap();
        let top = extract::single_root(&out).unwrap();
        let receipt = tree::build(&top).unwrap();
        let count = |kind| receipt.entries.iter().filter(|e| e.kind == kind).count();
        let java = probe::java_relative_path(os);
        let java_entry = receipt.entries.iter().find(|e| e.path == java).unwrap();
        assert_eq!(java_entry.kind, EntryKind::File);
        let bytes: u64 = receipt.entries.iter().filter_map(|e| e.size).sum();
        println!(
            "{}: sha256 verified; {} files, {} dirs, {} internal links, {bytes} bytes expanded; {java} present (not executed)",
            package.archive_name,
            count(EntryKind::File),
            count(EntryKind::Dir),
            count(EntryKind::Symlink)
        );
        std::fs::remove_file(&archive).unwrap();
        std::fs::remove_dir_all(&out).unwrap();
    }
}

fn mojang() -> Mojang {
    Mojang::new(PublicClient::new(SourcePolicy::MojangMetadata, "live-test").unwrap()).unwrap()
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "network: downloads Mojang runtimes"]
async fn mojang_runtimes_install_probe_and_reuse() {
    let dir = tempfile::tempdir().unwrap();
    let root = DataRoot::new(dir.path().to_path_buf()).unwrap();
    let (os, arch) = runtime_target(&RuleContext::current().unwrap()).unwrap();
    let installer = RuntimeInstaller::new(
        root.clone(),
        HttpTransport::public("live-test").unwrap(),
        ExecutorConfig::default(),
    );
    for (component, major) in [
        ("jre-legacy", 8),
        ("java-runtime-alpha", 16),
        ("java-runtime-gamma", 17),
        ("java-runtime-delta", 21),
        ("java-runtime-epsilon", 25),
    ] {
        let req = RuntimeRequirement {
            major,
            os,
            arch,
            component: Some(component.to_owned()),
        };
        let Some(package) = mojang().resolve(&req).await.unwrap() else {
            println!("{component}: not published for this computer");
            continue;
        };
        assert_eq!(package.key.provider, JavaProvider::Mojang);
        let started = std::time::Instant::now();
        let installed = installer
            .ensure(&package, Uuid::new_v4(), &CancelToken::new(), &NoProgress)
            .await
            .unwrap();
        let probe = &installed.receipt.probe;
        let report = installed.archive_download.clone().unwrap_or_default();
        println!(
            "{component}: {} installed in {:.1?} ({} files downloaded, {} network bytes) at {}\n  probe: java.version={} spec={} os.arch={} vendor={:?}",
            package.openjdk_version,
            started.elapsed(),
            report.downloaded_files,
            report.network_bytes,
            installed.dir.strip_prefix(dir.path()).unwrap().display(),
            probe.java_version,
            probe.specification_version,
            probe.os_arch,
            probe.vendor,
        );
        assert!(!installed.reused);
        assert_eq!(probe.specification_version, package.key.major);
        let again = installer
            .ensure(&package, Uuid::new_v4(), &CancelToken::new(), &NoProgress)
            .await
            .unwrap();
        assert!(again.reused);
        assert!(
            !root.runtimes_dir().join(".archives").exists(),
            "downloaded files are removed after the install"
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "network: reads Mojang runtime manifests"]
async fn mojang_manifests_resolve_for_every_target() {
    for (os, arch) in [
        (TargetOs::Linux, TargetArch::X64),
        (TargetOs::Windows, TargetArch::X64),
        (TargetOs::Windows, TargetArch::Aarch64),
        (TargetOs::Mac, TargetArch::X64),
        (TargetOs::Mac, TargetArch::Aarch64),
        (TargetOs::Linux, TargetArch::Aarch64),
    ] {
        for (component, major) in [
            ("jre-legacy", 8),
            ("java-runtime-alpha", 17),
            ("java-runtime-beta", 17),
            ("java-runtime-delta", 21),
            ("java-runtime-epsilon", 25),
        ] {
            let req = RuntimeRequirement {
                major,
                os,
                arch,
                component: Some(component.to_owned()),
            };
            let found = mojang().resolve(&req).await.unwrap();
            println!(
                "{} {}: {component} -> {}",
                os.as_str(),
                arch.as_str(),
                found.as_ref().map_or_else(
                    || "none (Temurin is used)".to_owned(),
                    |p| format!(
                        "{} {} manifest bytes, java at {}",
                        p.openjdk_version,
                        p.archive_size,
                        mojang::java_relative_path(os)
                    )
                )
            );
            if let Some(package) = found {
                assert!(package.key.major >= 8);
            }
        }
    }
}
