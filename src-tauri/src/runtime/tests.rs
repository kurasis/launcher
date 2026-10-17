//! End-to-end runtime installation against the local fixture server with a fake
//! `java` script (Unix only: the script relies on `/bin/sh`).

use sha2::Digest as _;

use super::extract::tests::{E, tar_gz};
use super::*;
use crate::download::NoProgress;
use crate::download::tests::{Server, fast_config};
use crate::storage::paths::{ImageType, RuntimeKey, TargetArch, TargetOs};

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
                if name != "temurin" {
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
        (3, ".kurasis-tree.json"),
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
