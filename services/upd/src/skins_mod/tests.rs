use std::path::PathBuf;

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
    fn public(&self) -> String {
        b64(&self.pk.to_box().unwrap().to_string())
    }
}

const BUILDS: &str = r#"[
  {"target": "fabric", "label": "26.1-26.3", "games": ["26.1", "26.3"]},
  {"target": "forge", "label": "1.12.2", "games": ["1.12.2"]}
]"#;

/// A release folder with both jars and their SHA256SUMS.
fn release(dir: &Path, version: &str) -> String {
    let mut sums = String::new();
    for (i, file) in [
        format!("launcherskins-fabric-26.1-26.3-{version}.jar"),
        format!("launcherskins-forge-1.12.2-{version}.jar"),
    ]
    .iter()
    .enumerate()
    {
        let bytes = vec![i as u8 + 1; 100 + i];
        fs::write(dir.join(file), &bytes).unwrap();
        sums.push_str(&format!(
            "{}  {file}\n",
            hex::encode(sha2::Sha256::digest(&bytes))
        ));
    }
    sums
}

fn list(dir: &Path, channel: &str, serial: u64) -> List {
    let sums = release(dir, "0.6.2");
    build(
        "0.6.2",
        channel,
        serial,
        false,
        BUILDS.as_bytes(),
        &sums,
        dir,
    )
    .unwrap()
}

#[test]
fn a_list_names_every_build_with_its_sum_and_size() {
    let dir = tempfile::tempdir().unwrap();
    let list = list(dir.path(), "beta", 7);
    assert_eq!(list.kind, KIND);
    assert_eq!(list.jars.len(), 2);
    assert_eq!(
        list.jars[0].file,
        "launcherskins-fabric-26.1-26.3-0.6.2.jar"
    );
    assert_eq!(list.jars[0].games, ["26.1", "26.3"]);
    assert_eq!(list.jars[1].size, 101);
}

#[test]
fn a_list_is_refused_when_the_release_and_the_builds_differ() {
    let dir = tempfile::tempdir().unwrap();
    let sums = release(dir.path(), "0.6.2");
    let p = dir.path();
    // A jar the builds name is missing from the sums.
    let short: String = sums.lines().take(1).map(|l| format!("{l}\n")).collect();
    assert!(build("0.6.2", "beta", 1, false, BUILDS.as_bytes(), &short, p).is_err());
    // A jar of the release the builds do not name.
    let more = format!(
        "{sums}{}  launcherskins-agent-1.21-0.6.2.jar\n",
        "a".repeat(64)
    );
    assert!(build("0.6.2", "beta", 1, false, BUILDS.as_bytes(), &more, p).is_err());
    // A jar whose bytes do not match.
    fs::write(p.join("launcherskins-forge-1.12.2-0.6.2.jar"), b"other").unwrap();
    assert!(build("0.6.2", "beta", 1, false, BUILDS.as_bytes(), &sums, p).is_err());
    // Junk.
    assert!(build("0.6.2", "beta", 1, false, b"{", &sums, p).is_err());
    assert!(build("../x", "beta", 1, false, BUILDS.as_bytes(), &sums, p).is_err());
    assert!(build("0.6.2", "nightly", 1, false, BUILDS.as_bytes(), &sums, p).is_err());
}

#[test]
fn a_signed_list_verifies_only_as_itself() {
    let dir = tempfile::tempdir().unwrap();
    let keys = keys();
    let list = list(dir.path(), "beta", 7);
    let envelope = sign(&list, &keys.sk).unwrap();
    let bytes = serde_json::to_vec(&envelope).unwrap();
    assert_eq!(verify(&keys.public(), &bytes, "beta").unwrap(), list);
    // Not for another channel.
    assert!(verify(&keys.public(), &bytes, "stable").is_err());
    // Not with another key.
    assert!(verify(&self::keys().public(), &bytes, "beta").is_err());
    // Not changed after signing.
    let mut changed = envelope.clone();
    changed.list = changed.list.replace("\"serial\":7", "\"serial\":8");
    assert!(
        verify(
            &keys.public(),
            &serde_json::to_vec(&changed).unwrap(),
            "beta"
        )
        .is_err()
    );
    // Not a launcher update's signature over the same bytes.
    let other = minisign::sign(
        None,
        &keys.sk,
        envelope.list.as_bytes(),
        Some("timestamp:1\tfile:x\tversion:0.6.2"),
        None,
    )
    .unwrap();
    let mut swapped = envelope;
    swapped.signature = b64(&other.to_string());
    assert!(
        verify(
            &keys.public(),
            &serde_json::to_vec(&swapped).unwrap(),
            "beta"
        )
        .is_err()
    );
    // Junk.
    for junk in [&b""[..], b"{}", b"null", b"{\"list\":1,\"signature\":2}"] {
        assert!(verify(&keys.public(), junk, "beta").is_err());
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

impl Setup {
    /// The release's jars and SHA256SUMS in `incoming/skins-mod-<version>/`, as
    /// CI uploads them.
    fn upload_jars(&self, version: &str) -> PathBuf {
        let dir = self
            .root
            .join("incoming")
            .join(format!("skins-mod-{version}"));
        fs::create_dir_all(&dir).unwrap();
        let sums = release(&dir, version);
        fs::write(dir.join("SHA256SUMS"), sums).unwrap();
        dir
    }

    fn upload(&self, channel: &str, serial: u64) {
        let jars = self.root.join("jars");
        fs::create_dir_all(&jars).unwrap();
        let envelope = sign(&list(&jars, channel, serial), &self.keys.sk).unwrap();
        fs::write(
            self.root
                .join("incoming")
                .join(format!("skins-mod-{channel}.json")),
            serde_json::to_vec(&envelope).unwrap(),
        )
        .unwrap();
    }
}

#[test]
fn the_server_takes_only_newer_lists() {
    let s = setup();
    assert!(
        s.server.install_skins_mod("beta").is_err(),
        "nothing uploaded"
    );
    s.upload("beta", 10);
    let refused = s.server.install_skins_mod("beta").unwrap_err();
    assert!(refused.contains("not in place"), "{refused}");
    assert!(!s.root.join("channels/beta").join(FILE).exists());
    s.upload_jars("0.6.2");
    s.server.install_skins_mod_jars("0.6.2").unwrap();
    assert!(
        s.server
            .install_skins_mod("beta")
            .unwrap()
            .contains("0.6.2")
    );
    assert!(
        s.server
            .skins_mod_status()
            .unwrap()
            .contains("2 of 2 jars in place")
    );
    let served = s.root.join("channels/beta").join(FILE);
    assert!(served.is_file());
    assert!(!s.root.join("incoming/skins-mod-beta.json").exists());
    // The same serial or an older one again: refused, the newer stays.
    s.upload("beta", 10);
    assert!(s.server.install_skins_mod("beta").is_err());
    s.upload("beta", 9);
    assert!(s.server.install_skins_mod("beta").is_err());
    s.upload("beta", 11);
    s.server.install_skins_mod("beta").unwrap();
    assert!(s.server.skins_mod_status().unwrap().contains("serial 11"));
    // A list for another channel under this channel's name.
    s.upload("stable", 12);
    fs::rename(
        s.root.join("incoming/skins-mod-stable.json"),
        s.root.join("incoming/skins-mod-beta.json"),
    )
    .unwrap();
    assert!(s.server.install_skins_mod("beta").is_err());
    assert!(s.server.install_skins_mod("../x").is_err());
}

#[test]
fn the_server_serves_a_release_s_jars_only_as_they_were_signed_off() {
    let s = setup();
    assert!(
        s.server.install_skins_mod_jars("0.6.2").is_err(),
        "nothing uploaded"
    );
    let dir = s.upload_jars("0.6.2");
    assert_eq!(
        s.server.install_skins_mod_jars("0.6.2").unwrap(),
        "skins mod 0.6.2: 2 jars, 2 new"
    );
    let served = s
        .root
        .join("skins-mod/0.6.2/launcherskins-fabric-26.1-26.3-0.6.2.jar");
    assert_eq!(fs::read(&served).unwrap(), vec![1u8; 100]);
    // Again: nothing changes.
    assert_eq!(
        s.server.install_skins_mod_jars("0.6.2").unwrap(),
        "skins mod 0.6.2: 2 jars, 0 new"
    );
    // A published jar never changes, even with a matching SHA256SUMS.
    let forge = "launcherskins-forge-1.12.2-0.6.2.jar";
    fs::write(dir.join(forge), b"other").unwrap();
    fs::write(
        dir.join("SHA256SUMS"),
        format!("{}  {forge}\n", sha256_hex(b"other")),
    )
    .unwrap();
    assert!(s.server.install_skins_mod_jars("0.6.2").is_err());
    assert_eq!(
        fs::read(s.root.join("skins-mod/0.6.2").join(forge)).unwrap(),
        vec![2u8; 101]
    );

    // A jar that does not match: none of the release is written.
    let dir = s.upload_jars("0.6.3");
    fs::write(dir.join(forge.replace("0.6.2", "0.6.3")), b"broken").unwrap();
    assert!(s.server.install_skins_mod_jars("0.6.3").is_err());
    assert!(!s.root.join("skins-mod/0.6.3").exists());

    // Names that are not jars of ours, and junk versions.
    for line in [
        format!("{}  ../launcherskins-x.jar", "a".repeat(64)),
        format!("{}  other.jar", "a".repeat(64)),
        format!("{}  launcherskins-x.jar", "z".repeat(64)),
    ] {
        fs::write(dir.join("SHA256SUMS"), format!("{line}\n")).unwrap();
        assert!(s.server.install_skins_mod_jars("0.6.3").is_err(), "{line}");
    }
    fs::write(dir.join("SHA256SUMS"), "").unwrap();
    assert!(s.server.install_skins_mod_jars("0.6.3").is_err());
    for version in ["../x", "0.6", "", "0.6.3/../../x"] {
        assert!(
            s.server.install_skins_mod_jars(version).is_err(),
            "{version}"
        );
    }
    assert!(!s.root.join("x").exists());
}
