use base64::Engine as _;

use super::*;
use crate::loader::LoaderKind;
use crate::skins_mod::{JARS, MOD_VERSION, OPTIFINE_FROM, choose};

struct Keys {
    pk: minisign::PublicKey,
    sk: minisign::SecretKey,
}

fn b64(text: &str) -> String {
    base64::engine::general_purpose::STANDARD.encode(text)
}

fn keys() -> Keys {
    let pair = minisign::KeyPair::generate_unencrypted_keypair().unwrap();
    Keys {
        pk: pair.pk,
        sk: pair.sk,
    }
}

const H: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn list_json(channel: &str, serial: u64, version: &str, rollback: bool) -> String {
    serde_json::json!({
        "kind": "skins-mod",
        "channel": channel,
        "serial": serial,
        "version": version,
        "rollback": rollback,
        "jars": [{
            "target": "fabric",
            "games": ["26.3", "99.1"],
            "file": format!("launcherskins-fabric-26.1-26.3-{version}.jar"),
            "sha256": H,
            "size": 100,
        }, {
            "target": "agent",
            "games": ["1.20.1"],
            "file": format!("launcherskins-agent-1.20.1-{version}.jar"),
            "sha256": H,
            "size": 100,
        }],
    })
    .to_string()
}

impl Keys {
    fn public(&self) -> String {
        b64(&self.pk.to_box().unwrap().to_string())
    }

    /// An envelope as CI makes it; `comment` is the trusted comment.
    fn envelope(&self, list: &str, comment: &str) -> Vec<u8> {
        let signature = minisign::sign(
            Some(&self.pk),
            &self.sk,
            list.as_bytes(),
            Some(comment),
            None,
        )
        .unwrap();
        serde_json::to_vec(&serde_json::json!({
            "list": list,
            "signature": b64(&signature.to_string()),
        }))
        .unwrap()
    }

    fn signed(&self, channel: &str, serial: u64, version: &str, rollback: bool) -> Vec<u8> {
        self.envelope(
            &list_json(channel, serial, version, rollback),
            &format!(
                "timestamp:1\tfile:{FILE}\tkind:skins-mod\tchannel:{channel}\tserial:{serial}"
            ),
        )
    }
}

#[test]
fn a_list_signed_for_this_channel_verifies() {
    let k = keys();
    let signed = k.signed("beta", 5, "0.7.0", false);
    let list = verify(&k.public(), &signed, UpdateChannel::Beta).unwrap();
    assert_eq!(list.serial, 5);
    assert_eq!(list.version, "0.7.0");
    let jar = list.find("26.3", Target::Fabric).unwrap();
    assert_eq!(
        list.url(jar),
        "https://upd.lapi.sh/skins-mod/0.7.0/launcherskins-fabric-26.1-26.3-0.7.0.jar"
    );
    assert!(list.find("26.3", Target::Forge).is_none());
    assert!(list.find("26.2", Target::Fabric).is_none());
}

#[test]
fn other_lists_signatures_and_junk_are_refused() {
    let k = keys();
    let key = k.public();
    let beta = UpdateChannel::Beta;
    // Another channel's list, or a list the comment does not name.
    assert!(verify(&key, &k.signed("stable", 5, "0.7.0", false), beta).is_none());
    let list = list_json("beta", 5, "0.7.0", false);
    for comment in [
        "timestamp:1\tfile:x\tversion:0.7.0",
        "timestamp:1\tkind:skins-mod\tchannel:stable\tserial:5",
        "timestamp:1\tkind:skins-mod\tchannel:beta\tserial:6",
        "timestamp:1\tkind:launcher\tchannel:beta\tserial:5",
    ] {
        assert!(
            verify(&key, &k.envelope(&list, comment), beta).is_none(),
            "{comment}"
        );
    }
    // Another key.
    assert!(verify(&keys().public(), &k.signed("beta", 5, "0.7.0", false), beta).is_none());
    // Changed after signing.
    let mut envelope: serde_json::Value =
        serde_json::from_slice(&k.signed("beta", 5, "0.7.0", false)).unwrap();
    envelope["list"] = list_json("beta", 5, "0.7.1", false).into();
    assert!(verify(&key, &serde_json::to_vec(&envelope).unwrap(), beta).is_none());
    // Entries out of shape, though signed.
    let comment = "timestamp:1\tkind:skins-mod\tchannel:beta\tserial:5";
    for (from, to) in [
        ("\"0.7.0\"", "\"../../x\""),
        ("\"file\":\"launcherskins-", "\"file\":\"../launcherskins-"),
        ("\"target\":\"fabric\"", "\"target\":\"optifine\""),
        ("\"size\":100", "\"size\":999999999"),
        ("\"size\":100", "\"size\":0"),
        (H, "PLACEHOLDER"),
        ("\"26.3\"", "\"26.3; rm -rf\""),
        ("\"kind\":\"skins-mod\"", "\"kind\":\"launcher\""),
    ] {
        let bad = list.replace(from, to);
        assert_ne!(bad, list, "{from}");
        assert!(
            verify(&key, &k.envelope(&bad, comment), beta).is_none(),
            "{to}"
        );
    }
    // Junk, and a key that is not one.
    for junk in [
        &b""[..],
        b"{}",
        b"[]",
        b"{\"list\":\"{}\",\"signature\":\"\"}",
    ] {
        assert!(verify(&key, junk, beta).is_none());
    }
    assert!(verify("not a key", &k.signed("beta", 5, "0.7.0", false), beta).is_none());
    let huge = vec![b' '; MAX_BYTES as usize + 1];
    assert!(verify(&key, &huge, beta).is_none());
}

#[test]
fn only_a_list_as_new_or_newer_is_kept() {
    let k = keys();
    let key = k.public();
    let dir = tempfile::tempdir().unwrap();
    let root = DataRoot::new(dir.path().join("data")).unwrap();
    let beta = UpdateChannel::Beta;
    assert!(kept(&root, &key, beta).is_none());
    assert_eq!(
        take(&root, &key, beta, &k.signed("beta", 5, "0.7.0", false)),
        Taken::New("0.7.0".into())
    );
    assert_eq!(kept(&root, &key, beta).unwrap().serial, 5);
    assert_eq!(
        take(&root, &key, beta, &k.signed("beta", 5, "0.7.0", false)),
        Taken::Same
    );
    // An old list served again does not go back.
    assert_eq!(
        take(&root, &key, beta, &k.signed("beta", 4, "0.6.9", false)),
        Taken::Older
    );
    assert_eq!(kept(&root, &key, beta).unwrap().version, "0.7.0");
    assert_eq!(take(&root, &key, beta, b"junk"), Taken::Refused);
    // A rollback is a newer list of an older version.
    assert_eq!(
        take(&root, &key, beta, &k.signed("beta", 6, "0.6.9", true)),
        Taken::New("0.6.9".into())
    );
    // Stable keeps its own list.
    assert!(kept(&root, &key, UpdateChannel::Stable).is_none());
    // A damaged kept list is no list.
    let path = kept_path(&root, beta).unwrap();
    std::fs::write(&path, b"{").unwrap();
    assert!(kept(&root, &key, beta).is_none());
}

#[test]
fn the_list_is_asked_of_the_update_server_only() {
    assert_eq!(
        address("https://upd.lapi.sh", UpdateChannel::Stable)
            .unwrap()
            .as_str(),
        "https://upd.lapi.sh/v1/stable/skins-mod-list.json"
    );
    assert_eq!(
        address("https://upd.lapi.sh/", UpdateChannel::Beta)
            .unwrap()
            .as_str(),
        "https://upd.lapi.sh/v1/beta/skins-mod-list.json"
    );
    assert!(address("http://127.0.0.1:8080", UpdateChannel::Beta).is_none());
    assert!(address("https://upd.lapi.sh.evil.example", UpdateChannel::Beta).is_none());
}

/// A list of `version` with one Fabric jar for 26.3 and the unreleased 99.1.
fn listed(version: &str, rollback: bool) -> List {
    let k = keys();
    let signed = k.signed("stable", 1, version, rollback);
    verify(&k.public(), &signed, UpdateChannel::Stable).unwrap()
}

#[test]
fn a_newer_list_wins_and_the_built_in_jars_fill_its_gaps() {
    let newer = listed("99.0.0", false);
    let fabric = Some(LoaderKind::Fabric);
    let pick = choose(JARS, Some(&newer), "26.3", fabric).unwrap();
    assert_eq!(pick.file, "launcherskins-fabric-26.1-26.3-99.0.0.jar");
    assert!(pick.url.contains("/skins-mod/99.0.0/"));
    // A version only the list knows.
    assert!(choose(JARS, Some(&newer), "99.1", fabric).is_some());
    assert!(choose(JARS, None, "99.1", fabric).is_none());
    // The list has no Forge jar: the built-in one.
    let forge = choose(JARS, Some(&newer), "26.3", Some(LoaderKind::Forge)).unwrap();
    assert!(
        forge.file.ends_with(&format!("-{MOD_VERSION}.jar")),
        "{}",
        forge.file
    );
}

#[test]
fn an_older_list_gives_way_unless_it_goes_back_on_purpose() {
    let fabric = Some(LoaderKind::Fabric);
    let older = listed("0.0.1", false);
    let pick = choose(JARS, Some(&older), "26.3", fabric).unwrap();
    assert!(
        pick.file.ends_with(&format!("-{MOD_VERSION}.jar")),
        "{}",
        pick.file
    );
    let back = listed("0.0.1", true);
    let pick = choose(JARS, Some(&back), "26.3", fabric).unwrap();
    assert_eq!(pick.file, "launcherskins-fabric-26.1-26.3-0.0.1.jar");
    // Going back gives only its own jars.
    assert!(choose(JARS, Some(&back), "26.3", Some(LoaderKind::Forge)).is_none());
}

#[test]
fn optifine_gets_the_agent_only_from_a_mod_that_reaches_launchwrapper() {
    let optifine = Some(LoaderKind::OptiFine);
    // The built-in jars are older than OPTIFINE_FROM: none for OptiFine, the agent
    // for the game with no loader.
    assert!(
        semver::Version::parse(MOD_VERSION).unwrap()
            < semver::Version::parse(OPTIFINE_FROM).unwrap()
    );
    assert!(choose(JARS, None, "1.20.1", optifine).is_none());
    assert!(choose(JARS, None, "1.20.1", None).is_some());
    let reaching = listed(OPTIFINE_FROM, false);
    let pick = choose(JARS, Some(&reaching), "1.20.1", optifine).unwrap();
    assert_eq!(
        pick.file,
        format!("launcherskins-agent-1.20.1-{OPTIFINE_FROM}.jar")
    );
    // A list from before it, even one going back on purpose, gives OptiFine nothing.
    let back = listed("0.7.0", true);
    assert!(choose(JARS, Some(&back), "1.20.1", optifine).is_none());
    assert!(choose(JARS, Some(&back), "1.20.1", None).is_some());
}
