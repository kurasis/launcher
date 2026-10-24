//! End-to-end runtime installation against the local fixture server with a fake
//! `java` script (Unix only: the script relies on `/bin/sh`).

use std::time::{Duration, SystemTime};

use sha1::Digest as _;

use super::extract::tests::{E, tar_gz};
use super::*;
use crate::download::NoProgress;
use crate::download::tests::{Server, fast_config};
use crate::storage::paths::{ImageType, JavaProvider, RuntimeKey, TargetArch, TargetOs};

struct Fixture {
    dir: tempfile::TempDir,
    root: DataRoot,
    server: Server,
}

impl Fixture {
    async fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = DataRoot::new(dir.path().join("data")).unwrap();
        Self {
            dir,
            root,
            server: Server::start().await,
        }
    }

    fn marker(&self) -> PathBuf {
        self.dir.path().join("java-was-executed")
    }

    /// A Linux-layout JRE whose `java` reports `major` and touches the marker.
    fn package_bytes(&self, major: u16, extra: &[E<'_>]) -> Vec<u8> {
        let script = format!(
            "#!/bin/sh\ntouch '{}'\ncat >&2 <<'X'\nProperty settings:\n    java.specification.version = {major}\n    java.version = {major}.0.1\n    os.arch = amd64\nX\n",
            self.marker().display()
        );
        let mut entries = vec![
            E::Dir("jdk-21-jre/"),
            E::File("jdk-21-jre/bin/java", script.as_bytes(), 0o755),
            E::File("jdk-21-jre/lib/libjli.so", b"native library", 0o644),
            E::Link("jdk-21-jre/lib/alias.so", "libjli.so"),
            E::File("jdk-21-jre/NOTICE", b"license notices", 0o644),
        ];
        entries.extend(extra.iter().map(|e| match e {
            E::Link(a, b) => E::Link(a, b),
            E::File(a, b, m) => E::File(a, b, *m),
            _ => unreachable!(),
        }));
        tar_gz(&entries)
    }

    fn package(&self, path: &str, bytes: &[u8], image: ImageType) -> RuntimePackage {
        self.server.serve(path, bytes);
        RuntimePackage {
            provider: "temurin".into(),
            key: RuntimeKey {
                provider: crate::storage::paths::JavaProvider::Temurin,
                os: TargetOs::Linux,
                arch: TargetArch::X64,
                major: 21,
                image_type: image,
                archive_sha256: Sha256::parse(&hex::encode(sha2::Sha256::digest(bytes))).unwrap(),
            },
            release_name: "jdk-21.0.12.1+1".into(),
            openjdk_version: "21.0.12.1+1-LTS".into(),
            archive_name: format!("test-{}.tar.gz", image.as_str()),
            archive_url: self.server.url(path).to_string(),
            archive_size: bytes.len() as u64,
            archive_format: adoptium::ArchiveFormat::TarGz,
            jdk_fallback: image == ImageType::Jdk,
        }
    }

    fn java_script(&self, major: u16) -> String {
        format!(
            "#!/bin/sh\ntouch '{}'\ncat >&2 <<'X'\nProperty settings:\n    java.specification.version = {major}\n    java.version = {major}.0.1\n    os.arch = amd64\nX\n",
            self.marker().display()
        )
    }

    /// A Mojang runtime manifest served with its files; `served` overrides the
    /// bytes the server returns for a path.
    fn mojang_package(
        &self,
        name: &str,
        files: &[(&str, &[u8], bool)],
        links: &[(&str, &str)],
        served: &[(&str, &[u8])],
    ) -> RuntimePackage {
        let mut listed = serde_json::Map::new();
        listed.insert("legal".into(), serde_json::json!({ "type": "directory" }));
        for (path, bytes, executable) in files {
            let sha1 = hex::encode(sha1::Sha1::digest(bytes));
            let object = format!("/{name}/objects/{sha1}");
            let body = served
                .iter()
                .find(|(p, _)| p == path)
                .map_or(*bytes, |(_, b)| *b);
            self.server.serve(&object, body);
            listed.insert(
                (*path).to_owned(),
                serde_json::json!({
                    "type": "file",
                    "executable": executable,
                    "downloads": { "raw": {
                        "sha1": sha1,
                        "size": bytes.len(),
                        "url": self.server.url(&object).to_string(),
                    } },
                }),
            );
        }
        for (path, target) in links {
            listed.insert(
                (*path).to_owned(),
                serde_json::json!({ "type": "link", "target": target }),
            );
        }
        let manifest = serde_json::to_vec(&serde_json::json!({ "files": listed })).unwrap();
        let path = format!("/{name}/manifest.json");
        self.server.serve(&path, &manifest);
        RuntimePackage {
            provider: "mojang".into(),
            key: RuntimeKey {
                provider: JavaProvider::Mojang,
                os: TargetOs::Linux,
                arch: TargetArch::X64,
                major: 21,
                image_type: ImageType::Jre,
                archive_sha256: Sha256::parse(&hex::encode(sha2::Sha256::digest(&manifest)))
                    .unwrap(),
            },
            release_name: "21.0.7".into(),
            openjdk_version: "21.0.7 (java-runtime-delta)".into(),
            archive_name: "java-runtime-delta".into(),
            archive_url: self.server.url(&path).to_string(),
            archive_size: manifest.len() as u64,
            archive_format: ArchiveFormat::MojangManifest,
            jdk_fallback: false,
        }
    }

    fn installer(&self) -> RuntimeInstaller {
        RuntimeInstaller::new(self.root.clone(), HttpTransport::loopback(), fast_config())
    }

    async fn ensure(&self, package: &RuntimePackage) -> LauncherResult<InstalledRuntime> {
        self.installer()
            .ensure(package, Uuid::new_v4(), &CancelToken::new(), &NoProgress)
            .await
    }

    fn leftovers(&self) -> Vec<String> {
        let mut found = Vec::new();
        if let Ok(entries) = fs::read_dir(self.root.runtimes_dir()) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                if name != "temurin" && name != "mojang" {
                    found.push(name);
                }
            }
        }
        found
    }
}

#[tokio::test]
async fn installs_verifies_probes_and_then_reuses() {
    let f = Fixture::new().await;
    let package = f.package("/jre.tar.gz", &f.package_bytes(21, &[]), ImageType::Jre);
    let installed = f.ensure(&package).await.unwrap();
    assert!(!installed.reused);
    assert_eq!(installed.dir, f.root.runtime_dir(&package.key));
    assert!(installed.java.ends_with("bin/java"));
    assert_eq!(installed.receipt.probe.specification_version, 21);
    assert!(installed.dir.join("NOTICE").exists(), "notices retained");
    assert!(f.marker().exists());
    assert!(
        f.leftovers().is_empty(),
        "no staging or archive left: {:?}",
        f.leftovers()
    );
    let locked = installed.locked();
    assert_eq!(locked.archive_sha256, package.key.archive_sha256);
    assert_eq!(
        locked.tree_receipt_sha256,
        installed.receipt.tree_receipt_sha256
    );

    // Second install: verified tree reused, no download, Java probed again.
    fs::remove_file(f.marker()).unwrap();
    let again = f.ensure(&package).await.unwrap();
    assert!(again.reused);
    assert_eq!(f.server.hits("/jre.tar.gz"), 1);
    assert!(f.marker().exists());
}

#[tokio::test]
async fn corruption_is_detected_before_execution_and_rebuilt() {
    let f = Fixture::new().await;
    let package = f.package("/jre.tar.gz", &f.package_bytes(21, &[]), ImageType::Jre);
    let installed = f.ensure(&package).await.unwrap();

    for (damage, path) in [
        (0, "lib/libjli.so"),
        (1, "lib/unexpected.so"),
        (2, "lib/alias.so"),
        (3, ".launcher-tree.json"),
    ] {
        match damage {
            0 => fs::write(installed.dir.join(path), b"tampered!!!!!!").unwrap(),
            1 => fs::write(installed.dir.join(path), b"x").unwrap(),
            2 => {
                fs::remove_file(installed.dir.join(path)).unwrap();
                std::os::unix::fs::symlink("../NOTICE", installed.dir.join(path)).unwrap();
            }
            _ => fs::write(installed.dir.join(path), b"{}").unwrap(),
        }
        assert_eq!(
            verify_installed(&f.root, &package).unwrap(),
            Verification::Invalid(path.into())
        );
        fs::remove_file(f.marker()).unwrap();
        let repaired = f.ensure(&package).await.unwrap();
        assert!(!repaired.reused, "rebuilt from the pinned archive");
        assert!(matches!(
            verify_installed(&f.root, &package).unwrap(),
            Verification::Valid(_)
        ));
        assert_eq!(
            fs::read(installed.dir.join("lib/libjli.so")).unwrap(),
            b"native library"
        );
    }
    assert_eq!(f.server.hits("/jre.tar.gz"), 5);
    assert!(f.leftovers().is_empty());
}

/// Moves every file's time an hour back, as for a runtime installed earlier.
fn settle(dir: &Path) {
    let then = SystemTime::now() - Duration::from_secs(3600);
    for entry in fs::read_dir(dir).unwrap() {
        let entry = entry.unwrap();
        let kind = entry.file_type().unwrap();
        if kind.is_dir() {
            settle(&entry.path());
        } else if kind.is_file() {
            let file = fs::File::options().write(true).open(entry.path()).unwrap();
            file.set_modified(then).unwrap();
        }
    }
}

/// Rewrites a file with other bytes of the same size, keeping its time.
fn rewrite_unseen(path: &Path, bytes: &[u8]) {
    let modified = fs::metadata(path).unwrap().modified().unwrap();
    assert_eq!(fs::metadata(path).unwrap().len(), bytes.len() as u64);
    fs::write(path, bytes).unwrap();
    let file = fs::File::options().write(true).open(path).unwrap();
    file.set_modified(modified).unwrap();
}

fn valid(verification: Verification) -> bool {
    matches!(verification, Verification::Valid(_))
}

#[tokio::test]
async fn a_launch_hashes_again_only_when_the_files_look_different() {
    let f = Fixture::new().await;
    let package = f.package("/jre.tar.gz", &f.package_bytes(21, &[]), ImageType::Jre);
    let installed = f.ensure(&package).await.unwrap();
    let stamp = installed.dir.join(stamp::STAMP);

    // Files written a moment ago are hashed, but not stamped: a change in the
    // same clock tick would not show.
    assert!(valid(verify_for_launch(&f.root, &package).unwrap()));
    assert!(!stamp.exists());

    settle(&installed.dir);
    assert!(valid(verify_for_launch(&f.root, &package).unwrap()));
    assert!(stamp.exists(), "a full check stamps settled files");
    // The stamp is the launcher's own file, not part of the tree.
    assert!(valid(verify_installed(&f.root, &package).unwrap()));

    // Same size and time: the launch trusts the stamp; a full check (Repair)
    // finds it and forgets the stamp, so the next launch finds it too.
    rewrite_unseen(&installed.dir.join("lib/libjli.so"), b"tampered!!!!!!");
    assert!(valid(verify_for_launch(&f.root, &package).unwrap()));
    assert_eq!(
        verify_installed(&f.root, &package).unwrap(),
        Verification::Invalid("lib/libjli.so".into())
    );
    assert!(!stamp.exists());
    assert_eq!(
        verify_for_launch(&f.root, &package).unwrap(),
        Verification::Invalid("lib/libjli.so".into())
    );
    fs::write(installed.dir.join("lib/libjli.so"), b"native library").unwrap();

    // Anything the file system shows is caught at launch.
    for (damage, path) in [
        (0, "lib/libjli.so"),
        (1, "lib/unexpected.so"),
        (2, "lib/alias.so"),
        (3, "NOTICE"),
        (4, "bin/java"),
    ] {
        settle(&installed.dir);
        assert!(valid(verify_for_launch(&f.root, &package).unwrap()));
        assert!(stamp.exists());
        let full = installed.dir.join(path);
        match damage {
            // Same size, new time.
            0 => fs::write(&full, b"tampered!!!!!!").unwrap(),
            1 => fs::write(&full, b"x").unwrap(),
            2 => {
                fs::remove_file(&full).unwrap();
                std::os::unix::fs::symlink("../NOTICE", &full).unwrap();
            }
            3 => fs::remove_file(&full).unwrap(),
            _ => {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&full, fs::Permissions::from_mode(0o644)).unwrap();
            }
        }
        assert_eq!(
            verify_for_launch(&f.root, &package).unwrap(),
            Verification::Invalid(path.into()),
            "{path}"
        );
        assert!(!stamp.exists());
        let repaired = f.ensure(&package).await.unwrap();
        assert!(!repaired.reused);
    }

    // A stamp for another tree, or junk, is ignored and replaced.
    settle(&installed.dir);
    for junk in [
        br#"{"schemaVersion":1,"treeReceiptSha256":"00","entries":[]}"#.as_slice(),
        b"not json",
    ] {
        fs::write(&stamp, junk).unwrap();
        assert!(valid(verify_for_launch(&f.root, &package).unwrap()));
        assert_ne!(fs::read(&stamp).unwrap(), junk);
    }
    let mut other =
        serde_json::from_slice::<serde_json::Value>(&fs::read(&stamp).unwrap()).unwrap();
    other["treeReceiptSha256"] = serde_json::json!("ab".repeat(32));
    let other = serde_json::to_vec(&other).unwrap();
    fs::write(&stamp, &other).unwrap();
    rewrite_unseen(&installed.dir.join("lib/libjli.so"), b"tampered!!!!!!");
    assert_eq!(
        verify_for_launch(&f.root, &package).unwrap(),
        Verification::Invalid("lib/libjli.so".into()),
        "a stamp of another tree is not trusted"
    );
}

#[tokio::test]
async fn wrong_hash_or_escaping_link_never_executes() {
    let f = Fixture::new().await;
    let good = f.package_bytes(21, &[]);
    let mut package = f.package("/jre.tar.gz", &good, ImageType::Jre);
    // The server returns other bytes of the same size.
    let mut other = good.clone();
    let last = other.len() - 1;
    other[last] ^= 1;
    f.server.serve("/jre.tar.gz", &other);
    assert_eq!(
        f.ensure(&package).await.unwrap_err().code,
        ErrorCode::HashMismatch
    );

    let evil = f.package_bytes(21, &[E::Link("jdk-21-jre/lib/evil", "../../../../outside")]);
    package = f.package("/evil.tar.gz", &evil, ImageType::Jre);
    assert_eq!(
        f.ensure(&package).await.unwrap_err().code,
        ErrorCode::UnsafeArchive
    );

    assert!(!f.marker().exists(), "java never ran");
    assert!(!f.root.runtime_dir(&package.key).exists());
    assert!(f.leftovers().is_empty(), "{:?}", f.leftovers());
}

#[tokio::test]
async fn wrong_major_or_missing_java_is_not_installed() {
    let f = Fixture::new().await;
    let package = f.package("/17.tar.gz", &f.package_bytes(17, &[]), ImageType::Jre);
    assert_eq!(
        f.ensure(&package).await.unwrap_err().code,
        ErrorCode::RuntimeMismatch
    );
    assert!(
        !f.root.runtime_dir(&package.key).exists(),
        "a malformed runtime is never ready"
    );

    let no_java = tar_gz(&[E::File("jdk/lib/x", b"x", 0o644)]);
    let package = f.package("/nojava.tar.gz", &no_java, ImageType::Jre);
    assert_eq!(
        f.ensure(&package).await.unwrap_err().code,
        ErrorCode::UnsafeArchive
    );
    assert!(f.leftovers().is_empty());
}

#[tokio::test]
async fn jre_and_jdk_with_one_release_label_never_share_a_directory() {
    let f = Fixture::new().await;
    let jre = f.package("/jre.tar.gz", &f.package_bytes(21, &[]), ImageType::Jre);
    let jdk_bytes = f.package_bytes(
        21,
        &[E::File("jdk-21-jre/bin/javac", b"#!/bin/sh\n", 0o755)],
    );
    let jdk = f.package("/jdk.tar.gz", &jdk_bytes, ImageType::Jdk);
    let rebuilt_bytes = f.package_bytes(21, &[E::File("jdk-21-jre/release", b"rebuild", 0o644)]);
    let rebuilt = f.package("/jre2.tar.gz", &rebuilt_bytes, ImageType::Jre);
    let a = f.ensure(&jre).await.unwrap();
    let b = f.ensure(&jdk).await.unwrap();
    let c = f.ensure(&rebuilt).await.unwrap();
    assert_ne!(a.dir, b.dir);
    assert_ne!(a.dir, c.dir);
    for package in [&jre, &jdk, &rebuilt] {
        assert!(matches!(
            verify_installed(&f.root, package).unwrap(),
            Verification::Valid(_)
        ));
    }
    assert!(b.receipt.package.jdk_fallback);
}

#[tokio::test]
async fn mojang_runtime_installs_from_its_manifest_and_is_reused() {
    let f = Fixture::new().await;
    let script = f.java_script(21);
    let package = f.mojang_package(
        "delta",
        &[
            ("bin/java", script.as_bytes(), true),
            ("lib/libjli.so", b"native library", false),
            // Same bytes under two names: downloaded once.
            ("legal/NOTICE", b"license notices", false),
            ("legal/COPY", b"license notices", false),
        ],
        &[("lib/alias.so", "libjli.so")],
        &[],
    );
    let installed = f.ensure(&package).await.unwrap();
    assert!(!installed.reused);
    assert_eq!(installed.dir, f.root.runtime_dir(&package.key));
    assert!(
        installed
            .dir
            .starts_with(f.root.runtimes_dir().join("mojang"))
    );
    assert!(installed.java.ends_with("bin/java"));
    assert!(extract::is_executable(&installed.java));
    assert!(!extract::is_executable(
        &installed.dir.join("lib/libjli.so")
    ));
    assert_eq!(
        fs::read_link(installed.dir.join("lib/alias.so")).unwrap(),
        Path::new("libjli.so")
    );
    assert_eq!(installed.receipt.probe.specification_version, 21);
    assert!(f.marker().exists());
    let notices = format!(
        "/delta/objects/{}",
        hex::encode(sha1::Sha1::digest(b"license notices"))
    );
    assert_eq!(f.server.hits(&notices), 1);
    let report = installed.archive_download.clone().unwrap();
    assert_eq!(
        report.downloaded_files, 4,
        "manifest and three unique files"
    );
    assert!(f.leftovers().is_empty(), "{:?}", f.leftovers());
    let locked = installed.locked();
    assert_eq!(locked.provider, "mojang");
    assert_eq!(locked.archive_url, package.archive_url);

    fs::remove_file(f.marker()).unwrap();
    let again = f.ensure(&package).await.unwrap();
    assert!(again.reused);
    assert_eq!(f.server.hits("/delta/manifest.json"), 1);
    assert!(f.marker().exists());

    // Damage is found by the tree receipt and rebuilt from the same manifest.
    fs::write(installed.dir.join("lib/libjli.so"), b"tampered!!!!!!").unwrap();
    assert_eq!(
        verify_installed(&f.root, &package).unwrap(),
        Verification::Invalid("lib/libjli.so".into())
    );
    let repaired = f.ensure(&package).await.unwrap();
    assert!(!repaired.reused);
    assert_eq!(
        fs::read(installed.dir.join("lib/libjli.so")).unwrap(),
        b"native library"
    );
}

#[tokio::test]
async fn mojang_files_with_wrong_bytes_or_escaping_links_never_execute() {
    let f = Fixture::new().await;
    let script = f.java_script(21);
    let tampered = f.mojang_package(
        "tampered",
        &[
            ("bin/java", script.as_bytes(), true),
            ("lib/libjli.so", b"native library", false),
        ],
        &[],
        &[("lib/libjli.so", b"native librarX")],
    );
    assert_eq!(
        f.ensure(&tampered).await.unwrap_err().code,
        ErrorCode::HashMismatch
    );
    let escaping = f.mojang_package(
        "escaping",
        &[("bin/java", script.as_bytes(), true)],
        &[("lib/evil", "../../../outside")],
        &[],
    );
    assert_eq!(
        f.ensure(&escaping).await.unwrap_err().code,
        ErrorCode::UnsafeArchive
    );
    let no_java = f.mojang_package("nojava", &[("lib/x", b"x", false)], &[], &[]);
    assert_eq!(
        f.ensure(&no_java).await.unwrap_err().code,
        ErrorCode::UnsafeArchive
    );
    assert!(!f.marker().exists(), "java never ran");
    for package in [&tampered, &escaping, &no_java] {
        assert!(!f.root.runtime_dir(&package.key).exists());
    }
    assert!(f.leftovers().is_empty(), "{:?}", f.leftovers());
}
