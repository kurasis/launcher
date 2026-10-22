//! The skins of the player's own Microsoft accounts, for their heads in the
//! rail and in Accounts. Mojang's public session server says which texture a
//! profile wears; the core fetches it from textures.minecraft.net (the
//! window's CSP loads no remote image) and cuts the head as for our skins.
//! Only the profile IDs of accounts signed in on this computer are asked, and
//! no token goes anywhere.

use base64::Engine as _;
use serde::Deserialize;

/// Mojang's session server: a profile's textures, no sign-in needed.
pub const SESSION: &str = "https://sessionserver.mojang.com";
/// Where Mojang serves skins.
pub const TEXTURES: &str = "https://textures.minecraft.net";
const TEXTURES_HOST: &str = "textures.minecraft.net";
/// Most of a session answer read: a name and one signed property.
pub const MAX_SESSION_BYTES: usize = 32 * 1024;
/// Most of a Mojang skin read: it takes 64×64 PNGs of up to 24 KB.
pub const MAX_SKIN_BYTES: usize = 256 * 1024;
/// The longest texture property taken, in base64.
const MAX_PROPERTY_CHARS: usize = 16 * 1024;

/// A profile ID as the launcher keeps it: 32 lowercase hex digits.
pub fn is_profile_id(text: &str) -> bool {
    text.len() == 32 && text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

#[derive(Deserialize)]
struct SessionAnswer {
    id: String,
    #[serde(default)]
    properties: Vec<Property>,
}

#[derive(Deserialize)]
struct Property {
    name: String,
    value: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TexturesValue {
    profile_id: String,
    #[serde(default)]
    textures: Textures,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
struct Textures {
    skin: Option<Url>,
}

#[derive(Deserialize)]
struct Url {
    url: String,
}

/// What the session server says a profile wears.
#[derive(Debug, PartialEq, Eq)]
pub enum Wears {
    /// The ID of a texture at textures.minecraft.net.
    Skin(String),
    /// Mojang's default skin, which the interface draws itself.
    Default,
}

/// Reads the session server's answer for `profile_id`. `None` for an answer
/// that is not about that profile or not one Mojang would send; a skin only
/// at `textures.minecraft.net/texture/<hex>`, whatever else the URL says.
pub fn wears(profile_id: &str, body: &[u8]) -> Option<Wears> {
    let answer: SessionAnswer = serde_json::from_slice(body).ok()?;
    if answer.id.to_ascii_lowercase() != profile_id {
        return None;
    }
    let Some(property) = answer.properties.iter().find(|p| p.name == "textures") else {
        return Some(Wears::Default);
    };
    if property.value.len() > MAX_PROPERTY_CHARS {
        return None;
    }
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(property.value.as_bytes())
        .ok()?;
    let value: TexturesValue = serde_json::from_slice(&decoded).ok()?;
    if value.profile_id.to_ascii_lowercase() != profile_id {
        return None;
    }
    let Some(skin) = value.textures.skin else {
        return Some(Wears::Default);
    };
    texture_id(&skin.url).map(Wears::Skin)
}

/// The texture ID in a skin's URL: only `http(s)://textures.minecraft.net/
/// texture/<hex>` with nothing after it.
fn texture_id(url: &str) -> Option<String> {
    let url = url::Url::parse(url).ok()?;
    let id = url.path().strip_prefix("/texture/")?;
    let plain = matches!(url.scheme(), "http" | "https")
        && url.host_str() == Some(TEXTURES_HOST)
        && url.port().is_none()
        && url.username().is_empty()
        && url.password().is_none()
        && url.query().is_none()
        && url.fragment().is_none()
        && (1..=64).contains(&id.len())
        && id.bytes().all(|b| b.is_ascii_hexdigit());
    plain.then(|| id.to_ascii_lowercase())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use serde_json::json;

    const ID: &str = "0123456789abcdef0123456789abcdef";

    pub(crate) fn answer(id: &str, textures: serde_json::Value) -> Vec<u8> {
        let value = base64::engine::general_purpose::STANDARD.encode(
            json!({ "timestamp": 1, "profileId": id, "profileName": "Steve", "textures": textures })
                .to_string(),
        );
        json!({ "id": id, "name": "Steve", "properties": [{ "name": "textures", "value": value }] })
            .to_string()
            .into_bytes()
    }

    #[test]
    fn a_skin_is_read_from_the_session_answer() {
        let skin = |url: &str| answer(ID, json!({ "SKIN": { "url": url } }));
        assert_eq!(
            wears(ID, &skin("http://textures.minecraft.net/texture/AbC123")),
            Some(Wears::Skin("abc123".into()))
        );
        assert_eq!(
            wears(ID, &skin("https://textures.minecraft.net/texture/abc")),
            Some(Wears::Skin("abc".into()))
        );
        // No skin of their own, or no textures at all: the default one.
        assert_eq!(wears(ID, &answer(ID, json!({}))), Some(Wears::Default));
        let bare = json!({ "id": ID, "name": "Steve", "properties": [] }).to_string();
        assert_eq!(wears(ID, bare.as_bytes()), Some(Wears::Default));
        // A capital ID from the server is the same profile.
        assert!(wears(ID, &answer(&ID.to_uppercase(), json!({}))).is_some());
    }

    #[test]
    fn only_mojangs_texture_host_is_taken() {
        for url in [
            "http://evil.example/texture/abc",
            "http://textures.minecraft.net.evil.example/texture/abc",
            "http://textures.minecraft.net:8080/texture/abc",
            "http://user@textures.minecraft.net/texture/abc",
            "http://textures.minecraft.net/texture/../../v1/me",
            "http://textures.minecraft.net/texture/abc?x=1",
            "http://textures.minecraft.net/texture/abc#x",
            "http://textures.minecraft.net/texture/",
            "http://textures.minecraft.net/skin/abc",
            "file:///etc/passwd",
            "javascript:alert(1)",
            &format!("http://textures.minecraft.net/texture/{}", "a".repeat(65)),
        ] {
            let body = answer(ID, json!({ "SKIN": { "url": url } }));
            assert_eq!(wears(ID, &body), None, "{url}");
        }
    }

    #[test]
    fn an_answer_about_someone_else_or_junk_is_refused() {
        let other = "f".repeat(32);
        let skin = json!({ "SKIN": { "url": "http://textures.minecraft.net/texture/abc" } });
        assert_eq!(wears(ID, &answer(&other, skin.clone())), None);
        // The signed property names the profile too.
        let value = base64::engine::general_purpose::STANDARD
            .encode(json!({ "profileId": other, "textures": skin }).to_string());
        let mixed = json!({ "id": ID, "properties": [{ "name": "textures", "value": value }] });
        assert_eq!(wears(ID, mixed.to_string().as_bytes()), None);
        for junk in [
            b"".to_vec(),
            b"<html>".to_vec(),
            json!({ "id": ID, "properties": [{ "name": "textures", "value": "%%%" }] })
                .to_string()
                .into_bytes(),
            json!({ "id": ID, "properties": [{ "name": "textures", "value": "a".repeat(20_000) }] })
                .to_string()
                .into_bytes(),
        ] {
            assert_eq!(wears(ID, &junk), None);
        }
        assert!(is_profile_id(ID));
        for bad in ["", "../x", &ID.to_uppercase(), &format!("{ID}0")] {
            assert!(!is_profile_id(bad));
        }
    }
}
