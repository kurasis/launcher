use std::io::Write;

use super::*;

struct Keys {
    pk: minisign::PublicKey,
    sk: minisign::SecretKey,
}

fn keys() -> Keys {
    let pair = minisign::KeyPair::generate_unencrypted_keypair().unwrap();
    Keys {
        pk: pair.pk,
        sk: pair.sk,
    }
}

fn b64(text: &str) -> String {
    base64::engine::general_purpose::STANDARD.encode(text)
}

impl Keys {
    /// The public key as Tauri writes it (base64 of the minisign file).
    fn public(&self) -> String {
        b64(&self.pk.to_box().unwrap().to_string())
    }

    /// A `.sig` as the Tauri CLI writes it, naming `version`.
    fn sign(&self, data: &[u8], version: &str) -> String {
        let comment = format!("timestamp:1760000000\tfile:x\tversion:{version}");
        let signature =
            minisign::sign(Some(&self.pk), &self.sk, data, Some(&comment), None).unwrap();
        b64(&signature.to_string())
    }
}

struct Setup {
    _dir: tempfile::TempDir,
    root: PathBuf,
    server: Server,
    keys: Keys,
}

fn setup() -> Setup {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("upd");
    let keys = keys();
    fs::create_dir_all(root.join("incoming")).unwrap();
    fs::write(root.join("update-key.pub"), keys.public()).unwrap();
    let server = Server::new(
        root.clone(),
        "https://upd.example".into(),
        root.join("update-key.pub"),
    );
    Setup {
        _dir: dir,
        root,
        server,
        keys,
    }
}

/// One platform's file name and content.
fn file_for(target: &str, bundle: &str, version: &str) -> (String, Vec<u8>) {
    (
        format!("Launcher_{version}_{target}.{bundle}"),
        format!("{target} {bundle} {version}").into_bytes(),
    )
}

impl Setup {
    /// Builds `incoming/launcher-<version>-release.tar.gz`; `edit` may change
    /// the release or the files before they are packed.
    fn archive(
        &self,
        version: &str,
        platforms: &[(&str, &str)],
        edit: impl FnOnce(&mut Release, &mut Vec<(String, Vec<u8>)>),
    ) {
        let mut files = Vec::new();
        let mut release = Release {
            version: version.into(),
            pub_date: Some("2026-10-18T00:00:00Z".into()),
            notes: [
                ("en".into(), "Fixes".into()),
                ("ru".into(), "Исправления".into()),
            ]
            .into(),
            platforms: Vec::new(),
        };
        for (target, bundle) in platforms {
            let (name, bytes) = file_for(target, bundle, version);
            release.platforms.push(Platform {
                target: (*target).into(),
                bundle: (*bundle).into(),
                file: name.clone(),
                sha256: hex::encode(sha2::Sha256::digest(&bytes)),
                signature: if *bundle == "deb" {
                    String::new()
                } else {
                    self.keys.sign(&bytes, version)
                },
            });
            files.push((name, bytes));
        }
        edit(&mut release, &mut files);
        files.push(("release.json".into(), serde_json::to_vec(&release).unwrap()));
        let path = self
            .root
            .join("incoming")
            .join(format!("launcher-{version}-release.tar.gz"));
        let gz = flate2::write::GzEncoder::new(
            fs::File::create(path).unwrap(),
            flate2::Compression::default(),
        );
        let mut tar = tar::Builder::new(gz);
        for (name, bytes) in files {
            let mut header = tar::Header::new_gnu();
            header.set_size(bytes.len() as u64);
            header.set_mode(0o644);
            header.as_gnu_mut().unwrap().name[..name.len()].copy_from_slice(name.as_bytes());
            header.set_cksum();
            tar.append(&header, bytes.as_slice()).unwrap();
        }
        tar.into_inner().unwrap().finish().unwrap().flush().unwrap();
    }

    fn manifest(&self, channel: &str, name: &str) -> serde_json::Value {
        serde_json::from_slice(
            &fs::read(self.root.join("channels").join(channel).join(name)).unwrap(),
        )
        .unwrap()
    }
}

fn options(rollout: u8) -> Options {
    Options {
        channel: "stable".into(),
        rollout,
        mandatory: false,
        partial: false,
    }
}

#[test]
fn a_complete_signed_release_becomes_current_with_one_manifest_per_platform() {
    let s = setup();
    s.archive("0.2.0", REQUIRED, |_, _| {});
    s.server.publish("0.2.0", &options(10)).unwrap();
    let nsis = s.manifest("stable", "windows-x86_64-nsis.json");
    assert_eq!(nsis["version"], "0.2.0");
    assert_eq!(nsis["rollout"], 10);
    assert_eq!(nsis["installable"], true);
    assert_eq!(nsis["notesByLanguage"]["ru"], "Исправления");
    assert_eq!(
        nsis["url"],
        "https://upd.example/releases/0.2.0/Launcher_0.2.0_windows-x86_64.nsis"
    );
    // The signature in the manifest verifies the published file.
    let bytes = fs::read(
        s.root
            .join("releases/0.2.0/Launcher_0.2.0_windows-x86_64.nsis"),
    )
    .unwrap();
    verify_signature(
        &s.keys.public(),
        &bytes,
        nsis["signature"].as_str().unwrap(),
        "0.2.0",
    )
    .unwrap();
    let deb = s.manifest("stable", "linux-x86_64-deb.json");
    assert_eq!(
        (deb["installable"].clone(), deb["signature"].clone()),
        (json!(false), json!(""))
    );
    assert_eq!(deb["download"], deb["url"]);
    assert!(
        fs::read_to_string(s.root.join("download/index.html"))
            .unwrap()
            .contains("Launcher 0.2.0")
    );
    assert!(
        s.server
            .status()
            .unwrap()
            .starts_with("stable: 0.2.0, 10% of installs")
    );
}

#[test]
fn broken_or_foreign_releases_are_refused_and_nothing_changes() {
    let s = setup();
    s.archive("0.2.0", REQUIRED, |_, files| files[0].1.push(b'!'));
    assert!(
        s.server
            .publish("0.2.0", &options(100))
            .unwrap_err()
            .contains("checksum")
    );

    let other = keys();
    s.archive("0.2.1", REQUIRED, |release, files| {
        release.platforms[0].signature = other.sign(&files[0].1, "0.2.1");
    });
    assert!(
        s.server
            .publish("0.2.1", &options(100))
            .unwrap_err()
            .contains("signature")
    );

    let keys = &s.keys;
    s.archive("0.2.2", REQUIRED, |release, files| {
        release.platforms[0].signature = keys.sign(&files[0].1, "0.1.0");
    });
    assert!(
        s.server
            .publish("0.2.2", &options(100))
            .unwrap_err()
            .contains("signed for version 0.1.0")
    );

    s.archive("0.2.3", &REQUIRED[..4], |_, _| {});
    assert!(
        s.server
            .publish("0.2.3", &options(100))
            .unwrap_err()
            .contains("missing linux-x86_64/appimage")
    );

    s.archive("0.2.4", REQUIRED, |_, files| {
        files.push(("../escape".into(), b"x".to_vec()));
    });
    assert!(
        s.server
            .publish("0.2.4", &options(100))
            .unwrap_err()
            .contains("not a plain file")
    );

    assert!(
        s.server
            .publish("0.9.0", &options(100))
            .unwrap_err()
            .contains("no ")
    );
    assert!(
        s.server
            .status()
            .unwrap()
            .starts_with("stable: nothing published")
    );
    assert!(!s.root.join("releases/0.2.0").exists());
    // A partial release is accepted when asked for.
    s.server
        .publish(
            "0.2.3",
            &Options {
                partial: true,
                ..options(100)
            },
        )
        .unwrap();
    assert!(
        !s.root
            .join("channels/stable/linux-x86_64-appimage.json")
            .exists()
    );
}

#[test]
fn versions_only_go_forward_and_revoking_goes_back() {
    let s = setup();
    s.archive("0.2.0", REQUIRED, |_, _| {});
    s.server.publish("0.2.0", &options(100)).unwrap();
    s.archive("0.1.9", REQUIRED, |_, _| {});
    assert!(
        s.server
            .publish("0.1.9", &options(100))
            .unwrap_err()
            .contains("newer")
    );
    assert!(s.server.publish("0.2.0", &options(100)).is_err());

    s.archive("0.3.0", REQUIRED, |_, _| {});
    s.server
        .publish(
            "0.3.0",
            &Options {
                mandatory: true,
                ..options(5)
            },
        )
        .unwrap();
    s.server.set_rollout("stable", 50).unwrap();
    assert_eq!(
        s.manifest("stable", "darwin-aarch64-app.json")["rollout"],
        50
    );
    s.server.pause("stable", true).unwrap();
    assert_eq!(
        s.manifest("stable", "darwin-aarch64-app.json")["rollout"],
        0
    );
    s.server.pause("stable", false).unwrap();
    assert_eq!(
        s.manifest("stable", "darwin-aarch64-app.json")["rollout"],
        50
    );

    s.server.revoke("stable", "0.3.0").unwrap();
    let back = s.manifest("stable", "darwin-aarch64-app.json");
    assert_eq!(back["version"], "0.2.0");
    assert_eq!(back["revoked"], json!(["0.3.0"]));
    assert_eq!(
        back["mandatory"], true,
        "leaving a broken version is not optional"
    );
    assert!(
        s.server
            .revoke("stable", "0.2.0")
            .unwrap_err()
            .contains("no earlier")
    );
    assert!(
        s.server
            .publish(
                "0.3.1",
                &Options {
                    channel: "nightly".into(),
                    ..options(1)
                }
            )
            .unwrap_err()
            .contains("unknown channel")
    );
}

#[test]
fn packed_releases_are_signed_for_their_version_and_publish() {
    let s = setup();
    // A password-protected key in Tauri's form, as `tauri signer generate` writes it.
    let pair = minisign::KeyPair::generate_encrypted_keypair(Some("pw".into())).unwrap();
    let secret = b64(&pair.sk.to_box(None).unwrap().to_string());
    fs::write(
        s.root.join("update-key.pub"),
        b64(&pair.pk.to_box().unwrap().to_string()),
    )
    .unwrap();
    let key = crate::pack::secret_key(&secret, "pw").unwrap();
    assert!(crate::pack::secret_key(&secret, "wrong").is_err());
    let built = s.root.join("built");
    fs::create_dir_all(&built).unwrap();
    let items: Vec<String> = REQUIRED
        .iter()
        .map(|(target, bundle)| {
            let (name, bytes) = file_for(target, bundle, "1.0.0");
            fs::write(built.join(&name), bytes).unwrap();
            format!("{target}:{bundle}:{}", built.join(&name).display())
        })
        .collect();
    let out = s.root.join("incoming/launcher-1.0.0-release.tar.gz");
    crate::pack::pack(
        "1.0.0",
        &out,
        [("en".into(), "New".into())].into(),
        &items,
        &key,
    )
    .unwrap();
    s.server.publish("1.0.0", &options(100)).unwrap();
    let manifest = s.manifest("stable", "linux-x86_64-appimage.json");
    assert_eq!(manifest["notes"], "New");
    let file = fs::read(
        s.root
            .join("releases/1.0.0/Launcher_1.0.0_linux-x86_64.appimage"),
    )
    .unwrap();
    let public = fs::read_to_string(s.root.join("update-key.pub")).unwrap();
    verify_signature(
        &public,
        &file,
        manifest["signature"].as_str().unwrap(),
        "1.0.0",
    )
    .unwrap();
    assert!(
        verify_signature(
            &public,
            &file,
            manifest["signature"].as_str().unwrap(),
            "1.0.1"
        )
        .is_err()
    );
}
