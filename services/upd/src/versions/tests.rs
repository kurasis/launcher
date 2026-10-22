use super::*;

const DATA: &[u8] = include_bytes!("../../../../data/extra-versions.json");

fn keys() -> (String, minisign::SecretKey) {
    let pair = minisign::KeyPair::generate_unencrypted_keypair().unwrap();
    let public =
        base64::engine::general_purpose::STANDARD.encode(pair.pk.to_box().unwrap().to_string());
    (public, pair.sk)
}

/// A one-version data file whose client jar is `jar`.
fn data(jar: &[u8], library: &str) -> (Vec<u8>, String) {
    let sha1 = sha1_hex(jar);
    let descriptor = serde_json::json!({
        "id": "2.0-blue",
        "type": "release",
        "downloads": {"client": {"sha1": sha1, "size": jar.len(), "url": format!("{FILES_URL}{sha1}.jar")}},
        "libraries": [{"name": "a:b:1", "downloads": {"artifact": {"url": library}}}],
        "assetIndex": {"url": "https://launchermeta.mojang.com/v1/packages/x/pre-1.6.json"},
    })
    .to_string();
    let doc = serde_json::json!({
        "schemaVersion": 1,
        "versions": [{
            "id": "2.0-blue", "name": "2.0 Blue", "title": "Minecraft 2.0", "group": "joke",
            "type": "release", "releaseTime": "2013-04-01T17:40:00+00:00", "server": false,
            "sha1": sha1_hex(descriptor.as_bytes()),
            "files": [{"kind": "client", "sha1": sha1, "size": jar.len(),
                       "origin": "https://vault.omniarchive.uk/archive/java/client-april-fools/2.0-blue.jar"}],
            "descriptor": descriptor,
        }],
    });
    (serde_json::to_vec(&doc).unwrap(), sha1)
}

#[test]
fn the_repository_data_is_a_valid_list_in_order() {
    let (list, meta) = build(DATA, 1).unwrap();
    assert!(list.versions.len() >= 233);
    assert_eq!(meta.len(), list.versions.len());
    let jokes = list
        .versions
        .iter()
        .take_while(|v| v.group == "joke")
        .count();
    assert!(jokes >= 14);
    assert!(list.versions[jokes..].iter().all(|v| v.group != "joke"));
    let newest_first = |part: &[Listed]| {
        part.windows(2)
            .all(|w| w[0].release_time >= w[1].release_time)
    };
    assert!(newest_first(&list.versions[..jokes]) && newest_first(&list.versions[jokes..]));
    assert_eq!(list.versions[0].id, "26w14a");
    for id in [
        "2.0-blue",
        "2.0-red",
        "2.0-purple",
        "combat-8c",
        "1.21.11_unobfuscated",
    ] {
        assert!(list.versions.iter().any(|v| v.id == id), "{id}");
    }
    let sums = sums(&meta).unwrap();
    assert!(parse_sums(&sums).unwrap().len() > meta.len());
}

#[test]
fn foreign_hosts_and_mismatched_files_are_refused() {
    let (good, _) = data(b"jar", "https://libraries.minecraft.net/a/b/1/b-1.jar");
    assert!(build(&good, 1).is_ok());
    let (foreign, _) = data(b"jar", "https://example.com/a/b/1/b-1.jar");
    assert!(build(&foreign, 1).unwrap_err().contains("another host"));
    let mut value: Value = serde_json::from_slice(&good).unwrap();
    value["versions"][0]["sha1"] = Value::from("0".repeat(40));
    assert!(build(&serde_json::to_vec(&value).unwrap(), 1).is_err());
    let mut value: Value = serde_json::from_slice(&good).unwrap();
    value["versions"][0]["id"] = Value::from("../x");
    assert!(build(&serde_json::to_vec(&value).unwrap(), 1).is_err());
}

#[test]
fn a_download_elsewhere_is_not_ours() {
    let descriptor = serde_json::json!({"downloads": {"client": {
        "sha1": "a".repeat(40), "size": 1, "url": "https://piston-data.mojang.com/v1/objects/x/client.jar"}}});
    assert!(jars(&descriptor).is_err());
}

#[test]
fn signed_lists_verify_and_others_do_not() {
    let (public, secret) = keys();
    let (list, _) = build(DATA, 7).unwrap();
    let envelope = sign(&list, &secret).unwrap();
    let bytes = serde_json::to_vec(&envelope).unwrap();
    assert_eq!(verify(&public, &bytes).unwrap(), list);
    let mut changed = envelope.clone();
    changed.list = changed.list.replacen("26w14a", "26w14b", 1);
    assert!(verify(&public, &serde_json::to_vec(&changed).unwrap()).is_err());
    let (other, _) = keys();
    assert!(verify(&other, &bytes).is_err());
}

#[test]
fn sums_name_only_version_files_and_jars() {
    let sum = "a".repeat(40);
    assert!(parse_sums(&format!("{sum}  files/{sum}.jar\n")).is_ok());
    assert!(parse_sums(&format!("{sum}  ../files/{sum}.jar\n")).is_err());
    assert!(parse_sums(&format!("{sum}  files/{}.jar\n", "b".repeat(40))).is_err());
    assert!(parse_sums(&format!("{sum}  release/x\n")).is_err());
}

#[test]
fn files_then_list_go_in_place() {
    let dir = tempfile::tempdir().unwrap();
    let (public, secret) = keys();
    let key = dir.path().join("update-key.pub");
    fs::write(&key, &public).unwrap();
    let server = Server::new(
        dir.path().join("data"),
        "https://upd.lapi.sh".to_owned(),
        key,
    );
    let jar = b"a game jar".to_vec();
    let (data, jar_sha1) = data(&jar, "https://libraries.minecraft.net/a/b/1/b-1.jar");
    let (list, meta) = build(&data, 5).unwrap();
    let incoming = dir.path().join("data/incoming");
    let write = |path: PathBuf, bytes: &[u8]| {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    };
    write(
        incoming.join("versions.json"),
        &serde_json::to_vec(&sign(&list, &secret).unwrap()).unwrap(),
    );
    // The list waits for its files.
    assert!(
        server
            .install_versions()
            .unwrap_err()
            .contains("not in place")
    );
    write(
        incoming.join("versions/SHA1SUMS"),
        sums(&meta).unwrap().as_bytes(),
    );
    write(
        incoming.join(format!("versions/meta/{}.json", meta[0].0)),
        &meta[0].1,
    );
    write(
        incoming.join(format!("versions/files/{jar_sha1}.jar")),
        b"other bytes",
    );
    assert!(
        server
            .install_versions_files()
            .unwrap_err()
            .contains("does not match")
    );
    write(
        incoming.join(format!("versions/files/{jar_sha1}.jar")),
        &jar,
    );
    assert!(server.install_versions_files().unwrap().contains("2 new"));
    assert_eq!(server.versions_have().unwrap(), jar_sha1);
    assert!(server.install_versions().unwrap().contains("1 listed"));
    assert!(dir.path().join("data/versions/manifest.json").exists());
    // An older or the same list again is refused.
    let (older, _) = build(&data, 4).unwrap();
    write(
        incoming.join("versions.json"),
        &serde_json::to_vec(&sign(&older, &secret).unwrap()).unwrap(),
    );
    assert!(
        server
            .install_versions()
            .unwrap_err()
            .contains("as new or newer")
    );
    assert!(
        server
            .versions_status()
            .unwrap()
            .contains("0 files not in place")
    );
}
