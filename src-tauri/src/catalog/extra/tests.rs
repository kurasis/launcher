use base64::Engine as _;
use serde_json::{Value, json};

use super::*;
use crate::catalog::manifest::Catalog;
use crate::install::plan::plan_game;
use crate::install::rules::{MetaOs, RuleContext};

/// Our list as CI publishes it: every version of the repository's data.
const DATA: &[u8] = include_bytes!("../../../../data/extra-versions.json");

pub(crate) struct Keys {
    pub public: String,
    secret: minisign::SecretKey,
}

pub(crate) fn keys() -> Keys {
    let pair = minisign::KeyPair::generate_unencrypted_keypair().unwrap();
    Keys {
        public: base64::engine::general_purpose::STANDARD
            .encode(pair.pk.to_box().unwrap().to_string()),
        secret: pair.sk,
    }
}

/// `list` signed as CI signs it, with this trusted comment.
pub(crate) fn signed_as(keys: &Keys, list: &Value, trusted: &str) -> Vec<u8> {
    let text = list.to_string();
    let signature = minisign::sign(
        None,
        &keys.secret,
        text.as_bytes(),
        Some(trusted),
        Some("signature from tauri secret key"),
    )
    .unwrap();
    json!({
        "list": text,
        "signature": base64::engine::general_purpose::STANDARD.encode(signature.to_string()),
    })
    .to_string()
    .into_bytes()
}

pub(crate) fn signed(keys: &Keys, list: &Value) -> Vec<u8> {
    let serial = list["serial"].as_u64().unwrap();
    signed_as(
        keys,
        list,
        &format!("timestamp:1\tfile:manifest.json\tkind:versions\tserial:{serial}"),
    )
}

pub(crate) fn listed(id: &str, sha1: &str) -> Value {
    json!({
        "id": id, "name": id, "group": "joke", "type": "snapshot",
        "releaseTime": "2024-04-01T12:00:00+00:00", "server": true,
        "sha1": sha1, "size": 100,
    })
}

pub(crate) fn list(serial: u64, versions: Vec<Value>) -> Value {
    json!({"kind": "versions", "serial": serial, "versions": versions})
}

fn data() -> Vec<Value> {
    let data: Value = serde_json::from_slice(DATA).unwrap();
    data["versions"].as_array().unwrap().clone()
}

/// The repository's data as the list CI signs (services/upd `versions-list`).
fn repository_list() -> Value {
    let versions = data()
        .iter()
        .map(|v| {
            let mut entry = json!({
                "id": v["id"], "name": v["name"], "group": v["group"], "type": v["type"],
                "releaseTime": v["releaseTime"], "server": v["server"], "sha1": v["sha1"],
                "size": v["descriptor"].as_str().unwrap().len(),
            });
            for key in ["title", "variant", "note"] {
                if !v[key].is_null() {
                    entry[key] = v[key].clone();
                }
            }
            entry
        })
        .collect();
    list(1_800_000_000, versions)
}

#[test]
fn the_repository_list_verifies_in_its_order() {
    let keys = keys();
    let verified = verify(&keys.public, &signed(&keys, &repository_list())).unwrap();
    assert_eq!(verified.versions.len(), data().len());
    let ids: Vec<_> = verified.versions.iter().map(|v| v.id.as_str()).collect();
    // The April Fools versions first, newest first, then the rest.
    assert_eq!(&ids[..3], ["26w14a", "25w14craftmine", "24w14potato"]);
    assert!(ids.contains(&"2.0-blue") && ids.last() == Some(&"c0.0.12a_03"));
    let jokes = verified
        .versions
        .iter()
        .take_while(|v| v.group == "joke")
        .count();
    assert!(verified.versions[jokes..].iter().all(|v| v.group != "joke"));
}

#[test]
fn every_version_file_of_the_repository_plans_with_its_jars_on_our_server() {
    let targets = [
        (MetaOs::Windows, "x86_64", "10.0"),
        (MetaOs::Windows, "arm64", "10.0"),
        (MetaOs::Linux, "x86_64", "6.8.0"),
        (MetaOs::Osx, "arm64", "14.5"),
    ];
    let mut failures = Vec::new();
    for version in data() {
        let id = version["id"].as_str().unwrap();
        let bytes = version["descriptor"].as_str().unwrap().as_bytes();
        let sha1 = Sha1::parse(version["sha1"].as_str().unwrap()).unwrap();
        let url = format!("{META_URL}{sha1}.json");
        for (os, arch, os_version) in targets {
            let ctx = RuleContext::with_default_features(os, arch, Some(os_version.to_owned()));
            match plan_game(bytes, &url, &sha1, id, &ctx) {
                Ok(plan) => {
                    let (source, _) = SourcePolicy::for_artifact(&plan.client.url).unwrap();
                    assert_eq!(source, SourcePolicy::ExtraVersions, "{id}");
                    assert!(
                        plan.libraries
                            .iter()
                            .all(|l| { SourcePolicy::MojangArtifacts.check(&l.url).is_ok() })
                    );
                }
                // Apple silicon cannot run Intel-only natives, as for Mojang's
                // own versions of those years.
                Err(e)
                    if os == MetaOs::Osx
                        && e.code == crate::error::ErrorCode::UnsupportedPlatform => {}
                Err(e) => failures.push(format!("{id} {os:?} {arch}: {:?}", e.code)),
            }
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

#[test]
fn a_list_verifies_only_as_itself() {
    let keys = keys();
    let good = list(5, vec![listed("2.0-blue", &"a".repeat(40))]);
    assert_eq!(
        verify(&keys.public, &signed(&keys, &good)).unwrap().serial,
        5
    );
    // Another key, another kind or serial in the signature, a changed list.
    let other_keys = self::keys();
    assert!(verify(&other_keys.public, &signed(&keys, &good)).is_none());
    for trusted in [
        "timestamp:1\tkind:skins-mod\tserial:5",
        "timestamp:1\tkind:versions\tserial:6",
        "timestamp:1\tserial:5",
        "timestamp:1\tkind:versions",
    ] {
        assert!(
            verify(&keys.public, &signed_as(&keys, &good, trusted)).is_none(),
            "{trusted}"
        );
    }
    let mut envelope: Value = serde_json::from_slice(&signed(&keys, &good)).unwrap();
    envelope["list"] = Value::String(envelope["list"].as_str().unwrap().replace("blue", "red"));
    assert!(verify(&keys.public, envelope.to_string().as_bytes()).is_none());
    let mut other = good.clone();
    other["kind"] = json!("skins-mod");
    assert!(verify(&keys.public, &signed(&keys, &other)).is_none());
    for junk in [
        &b""[..],
        b"{}",
        b"[]",
        b"\xff\xfe",
        b"{\"list\":1,\"signature\":2}",
    ] {
        assert!(verify(&keys.public, junk).is_none());
    }
    assert!(verify(&keys.public, &vec![b' '; MAX_BYTES + 1]).is_none());
    assert!(verify("not a key", &signed(&keys, &good)).is_none());
}

#[test]
fn entries_out_of_bounds_refuse_the_whole_list() {
    let keys = keys();
    let sha = "b".repeat(40);
    let changed = |key: &str, value: Value| {
        let mut entry = listed("24w14potato", &sha);
        entry[key] = value;
        entry
    };
    let bad = [
        changed("id", json!("CON")),
        changed("id", json!("../x")),
        changed("id", json!("a/b")),
        changed("id", json!(".hidden")),
        changed("id", json!("x".repeat(65))),
        changed("name", json!("a\u{202E}b")),
        changed("name", json!("a\u{2066}b")),
        changed("name", json!("a\u{200B}b")),
        changed("name", json!("a\nb")),
        changed("name", json!("")),
        changed("name", json!(" a")),
        changed("name", json!("x".repeat(65))),
        changed("title", json!("<script>\u{0}")),
        changed("group", json!("other")),
        changed("variant", json!("second-build")),
        changed("note", json!("works")),
        changed("note", json!("Crashes at start")),
        changed("type", json!("pending")),
        changed("releaseTime", json!("yesterday")),
        changed("sha1", json!("B".repeat(40))),
        changed("sha1", json!("../../x")),
        changed("size", json!(0)),
        changed("size", json!(MAX_DESCRIPTOR_BYTES as u64 + 1)),
    ];
    for entry in bad {
        let bytes = signed(&keys, &list(1, vec![entry.clone()]));
        assert!(verify(&keys.public, &bytes).is_none(), "{entry}");
    }
    // The same folder twice, in any case; no versions; too many.
    let twice = list(1, vec![listed("2.0-Blue", &sha), listed("2.0-blue", &sha)]);
    assert!(verify(&keys.public, &signed(&keys, &twice)).is_none());
    assert!(verify(&keys.public, &signed(&keys, &list(1, vec![]))).is_none());
    let many = (0..=MAX_VERSIONS)
        .map(|n| listed(&format!("v{n}"), &sha))
        .collect();
    assert!(verify(&keys.public, &signed(&keys, &list(1, many))).is_none());
    // Text in other scripts is fine.
    let named = changed("name", json!("20w14\u{221E}"));
    assert!(verify(&keys.public, &signed(&keys, &list(1, vec![named]))).is_some());
}

#[test]
fn our_versions_follow_mojangs_and_take_their_place() {
    let keys = keys();
    let sha = "c".repeat(40);
    let mut potato = listed("24w14potato", &sha);
    potato["title"] = json!("Poisonous Potato Update");
    let extra = verify(
        &keys.public,
        &signed(&keys, &list(1, vec![potato, listed("2.0-blue", &sha)])),
    )
    .unwrap();
    let mojang = |id: &str| CatalogEntry {
        id: id.to_owned(),
        version_type: "snapshot".to_owned(),
        url: format!("https://piston-meta.mojang.com/v1/packages/{sha}/{id}.json"),
        sha1: Sha1::parse(&sha).unwrap(),
        release_time: None,
        special: None,
    };
    let mut catalog = Catalog {
        latest_release: None,
        latest_snapshot: None,
        versions: vec![mojang("26.3"), mojang("24w14potato"), mojang("1.21.1")],
    };
    catalog.add_extra(&extra);
    let ids: Vec<_> = catalog.versions.iter().map(|v| v.id.as_str()).collect();
    assert_eq!(ids, ["26.3", "1.21.1", "24w14potato", "2.0-blue"]);
    let potato = catalog.find("24w14potato").unwrap();
    assert_eq!(
        potato.url,
        format!("https://upd.lapi.sh/versions/meta/{sha}.json")
    );
    assert!(ours(&potato.url));
    let special = potato.special.as_ref().unwrap();
    assert_eq!(special.title.as_deref(), Some("Poisonous Potato Update"));
    assert_eq!(special.group, "joke");
    assert!(catalog.find("26.3").unwrap().special.is_none());
}

#[test]
fn only_our_servers_versions_path_is_ours() {
    assert!(ours(&format!("{META_URL}{}.json", "a".repeat(40))));
    for url in [
        "https://piston-meta.mojang.com/v1/packages/a/b.json",
        "https://upd.lapi.sh/v1/stable/skins-mod-list.json",
        "http://upd.lapi.sh/versions/meta/a.json",
        "https://upd.lapi.sh:8443/versions/meta/a.json",
        "https://user@upd.lapi.sh/versions/meta/a.json",
        "https://mirror.lapi.sh/versions/meta/a.json",
    ] {
        assert!(!ours(url), "{url}");
    }
}
