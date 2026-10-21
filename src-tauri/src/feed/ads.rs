//! Ads for Minecraft servers from servers.lapi.sh (D-069), shown as tiles in
//! the feed. Each ad is read through the rules here: one that breaks any of
//! them is left out. Icons (PNG) and banners (JPEG) are fetched from our
//! service only and handed to the interface as data URLs; the advertised
//! servers are asked nothing until the player presses Play, which starts the
//! game straight into one.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use base64::Engine;
use serde::Serialize;
use serde_json::{Value, json};
use url::Url;

use super::model::Lang;
use super::{FeedProblem, FeedResult, keep, read_json, read_small, trim_dir};
use crate::error::ErrorCode;
use crate::net::{PublicClient, SourcePolicy};
use crate::storage::paths::DataRoot;
use crate::sync::lock;

pub const BASE: &str = "https://servers.lapi.sh";
/// Where ads can be shown; the feed only, for now.
pub const PLACES: [&str; 1] = ["feed"];
/// The service sends at most 50.
pub const MAX_ADS: usize = 50;
const MAX_ADS_BYTES: usize = 512 * 1024;
const MAX_ICON_BYTES: usize = 64 * 1024;
/// A 640×360 JPEG from the service is far smaller.
const MAX_BANNER_BYTES: usize = 256 * 1024;
const MAX_NAME: usize = 40;
const MAX_TEXT: usize = 200;
const MAX_LINES: usize = 3;
const MAX_VERSIONS: usize = 300;
/// The service's own cache time.
const FRESH: Duration = Duration::from_secs(30);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
/// Icons and banners kept on disk, in all.
const MAX_ICONS: usize = 200;

/// One ad as the interface shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerAd {
    pub id: String,
    pub name: String,
    /// `host`, `host:port` or `[IPv6]:port`, as Play joins it.
    pub address: String,
    pub text: String,
    /// The version Play starts by default.
    pub version: String,
    /// Every version the server lets in, newest first; `version` is one.
    pub versions: Vec<String>,
    /// A `data:image/png;base64,…` URL, when there is an icon.
    pub icon: Option<String>,
    /// Whether the server answered its last ping; None before the first.
    pub up: Option<bool>,
    pub online: Option<i64>,
    pub max: Option<i64>,
    /// Unix seconds; the interface hides the ad from then on.
    pub ends_at: i64,
    pub weight: i64,
    pub erid: Option<String>,
    /// A `data:image/jpeg;base64,…` URL of a 16:9 picture shown over the icon.
    pub banner: Option<String>,
    /// Whether the server wants a Microsoft sign-in (online mode); None when
    /// the service does not know.
    pub licensed: Option<bool>,
}

/// The pictures of an ad, by SHA-256, before they are fetched.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Pictures {
    pub icon: Option<String>,
    pub banner: Option<String>,
}

/// How a picture of the service is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Png,
    Jpeg,
}

impl Kind {
    fn ext(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpg",
        }
    }

    fn mime(self) -> &'static str {
        match self {
            Self::Png => "image/png",
            Self::Jpeg => "image/jpeg",
        }
    }

    fn max(self) -> usize {
        match self {
            Self::Png => MAX_ICON_BYTES,
            Self::Jpeg => MAX_BANNER_BYTES,
        }
    }

    fn holds(self, bytes: &[u8]) -> bool {
        match self {
            Self::Png => png(bytes),
            Self::Jpeg => jpeg(bytes),
        }
    }
}

/// The ads of a place.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerAdsView {
    pub ads: Vec<ServerAd>,
    /// The copy on disk is shown: the service did not answer.
    pub stale: bool,
}

/// Characters that change how text around them is laid out or hide it.
fn invisible(c: char) -> bool {
    matches!(c,
        '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2060}'..='\u{2069}'
        | '\u{FEFF}' | '\u{FFF9}'..='\u{FFFB}' | '\u{061C}' | '\u{180E}')
}

fn name(text: &str) -> Option<String> {
    let count = text.chars().count();
    (count > 0
        && count <= MAX_NAME
        && text.trim() == text
        && !text.chars().any(|c| c.is_control() || invisible(c)))
    .then(|| text.to_owned())
}

fn ad_text(text: &str) -> Option<String> {
    let count = text.chars().count();
    (count > 0
        && count <= MAX_TEXT
        && text.split('\n').count() <= MAX_LINES
        && !text
            .chars()
            .any(|c| (c.is_control() && c != '\n') || invisible(c))
        && !text.to_lowercase().contains("://"))
    .then(|| text.to_owned())
}

pub fn version_id(id: &str) -> bool {
    (1..=32).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        && id.bytes().next().is_some_and(|b| b.is_ascii_alphanumeric())
}

pub fn icon_name(sha: &str) -> bool {
    sha.len() == 64 && sha.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// An ad as the service sent it, before its pictures are fetched; None when
/// any part breaks the rules.
fn ad(value: &Value, now: i64) -> Option<(ServerAd, Pictures)> {
    let text = |key: &str| value.get(key).and_then(Value::as_str);
    let number = |key: &str| value.get(key).and_then(Value::as_i64);
    let count = |key: &str| match value.get(key) {
        None | Some(Value::Null) => Some(None),
        Some(v) => v
            .as_i64()
            .filter(|n| (0..=10_000_000).contains(n))
            .map(Some),
    };
    let id = text("id")?;
    if id.is_empty() || id.len() > 12 || !id.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let address = text("address")?;
    // The launcher joins only what it would accept from an invite.
    crate::launch::server::parse_address(address)?;
    if address.len() > 270 || address.trim() != address {
        return None;
    }
    let version = text("version")?;
    if !version_id(version) {
        return None;
    }
    let versions: Vec<String> = value
        .get("versions")?
        .as_array()?
        .iter()
        .map(|v| v.as_str().filter(|v| version_id(v)).map(str::to_owned))
        .collect::<Option<Vec<_>>>()?;
    let unique: HashSet<&String> = versions.iter().collect();
    if versions.is_empty()
        || versions.len() > MAX_VERSIONS
        || unique.len() != versions.len()
        || !versions.iter().any(|v| v == version)
    {
        return None;
    }
    let sha = |key: &str| match value.get(key) {
        None | Some(Value::Null) => Some(None),
        Some(v) => Some(Some(v.as_str().filter(|s| icon_name(s))?.to_owned())),
    };
    let flag = |key: &str| match value.get(key) {
        None | Some(Value::Null) => Some(None),
        Some(v) => Some(Some(v.as_bool()?)),
    };
    let pictures = Pictures {
        icon: sha("icon")?,
        banner: sha("banner")?,
    };
    let up = flag("up")?;
    let licensed = flag("licensed")?;
    let ends_at = number("endsAt")?;
    if ends_at <= now {
        return None;
    }
    let weight = number("weight").filter(|w| (1..=100).contains(w))?;
    let erid = match value.get("erid") {
        None | Some(Value::Null) => None,
        Some(v) => Some(
            v.as_str()
                .filter(|e| {
                    (1..=64).contains(&e.len()) && e.bytes().all(|b| b.is_ascii_alphanumeric())
                })?
                .to_owned(),
        ),
    };
    Some((
        ServerAd {
            id: id.to_owned(),
            name: name(text("name")?)?,
            address: address.to_owned(),
            text: ad_text(text("text")?)?,
            version: version.to_owned(),
            versions,
            icon: None,
            up,
            online: count("online")?,
            max: count("max")?,
            ends_at,
            weight,
            erid,
            banner: None,
            licensed,
        },
        pictures,
    ))
}

/// The ads of an answer that keep the rules, at most `MAX_ADS`, each once.
pub fn ads(value: &Value, now: i64) -> Option<Vec<(ServerAd, Pictures)>> {
    let list = value.get("ads")?.as_array()?;
    let mut seen = HashSet::new();
    Some(
        list.iter()
            .take(MAX_ADS)
            .filter_map(|v| ad(v, now))
            .filter(|(ad, _)| seen.insert(ad.id.clone()))
            .collect(),
    )
}

pub fn png(bytes: &[u8]) -> bool {
    bytes.len() > 32 && bytes.starts_with(b"\x89PNG\r\n\x1a\n")
}

pub fn jpeg(bytes: &[u8]) -> bool {
    bytes.len() > 32 && bytes.starts_with(&[0xff, 0xd8, 0xff]) && bytes.ends_with(&[0xff, 0xd9])
}

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

/// The client of servers.lapi.sh and the ads fetched lately.
pub struct ServerAds {
    client: PublicClient,
    fresh: Mutex<Option<(Instant, String, Vec<ServerAd>)>>,
}

fn ads_file(root: &DataRoot, place: &str, lang: Lang) -> PathBuf {
    root.feed_cache_dir()
        .join(format!("server-ads-{place}-{}.json", lang.as_str()))
}

fn picture_file(root: &DataRoot, sha: &str, kind: Kind) -> PathBuf {
    root.feed_cache_dir()
        .join("server-icons")
        .join(format!("{sha}.{}", kind.ext()))
}

impl ServerAds {
    pub fn new(app_version: &str) -> crate::error::LauncherResult<Self> {
        Ok(Self {
            client: PublicClient::new(SourcePolicy::ServerAds, app_version)?,
            fresh: Mutex::new(None),
        })
    }

    /// The status, the body and its content type.
    async fn fetch(&self, url: &Url, max: usize) -> FeedResult<(u16, Vec<u8>, String)> {
        let request = self
            .client
            .get(url)
            .map_err(|_| FeedProblem::Server)?
            .timeout(REQUEST_TIMEOUT);
        let (status, headers, body) = self.client.get_bounded(request, max).await.map_err(|e| {
            if e.code == ErrorCode::NetworkError {
                FeedProblem::Offline
            } else {
                FeedProblem::Server
            }
        })?;
        let kind = headers
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(|v| v.trim().to_ascii_lowercase())
            .unwrap_or_default();
        Ok((status.as_u16(), body, kind))
    }

    /// A picture from disk, else fetched and kept; never anything but `kind`.
    async fn picture(&self, root: Option<&DataRoot>, sha: &str, kind: Kind) -> Option<String> {
        if !icon_name(sha) {
            return None;
        }
        let kept = root
            .and_then(|root| read_small(&picture_file(root, sha, kind), kind.max() as u64))
            .filter(|b| kind.holds(b));
        let bytes = match kept {
            Some(bytes) => bytes,
            None => {
                let url = Url::parse(&format!("{BASE}/v1/icons/{sha}.{}", kind.ext())).ok()?;
                let (status, bytes, content_type) = self.fetch(&url, kind.max()).await.ok()?;
                if status != 200 || content_type != kind.mime() || !kind.holds(&bytes) {
                    return None;
                }
                if let Some(root) = root {
                    let file = picture_file(root, sha, kind);
                    keep(&file, &bytes);
                    if let Some(dir) = file.parent() {
                        trim_dir(dir, MAX_ICONS, u64::MAX, u64::MAX);
                    }
                }
                bytes
            }
        };
        Some(format!(
            "data:{};base64,{}",
            kind.mime(),
            base64::engine::general_purpose::STANDARD.encode(bytes)
        ))
    }

    /// The ads of `place` in `lang`: fetched, or the copy on disk (without
    /// the ads that ended since) when the service does not answer.
    pub async fn get(
        &self,
        root: Option<&DataRoot>,
        place: &str,
        lang: Lang,
        refresh: bool,
    ) -> FeedResult<ServerAdsView> {
        if !PLACES.contains(&place) {
            return Err(FeedProblem::NotFound);
        }
        let key = format!("{place} {}", lang.as_str());
        let now = now();
        if !refresh
            && let Some((at, fresh_key, ads)) = lock(&self.fresh).as_ref()
            && at.elapsed() < FRESH
            && *fresh_key == key
        {
            return Ok(ServerAdsView {
                ads: ads.iter().filter(|a| a.ends_at > now).cloned().collect(),
                stale: false,
            });
        }
        let url = Url::parse(&format!(
            "{BASE}/v1/ads?place={place}&lang={}",
            lang.as_str()
        ))
        .map_err(|_| FeedProblem::Server)?;
        let fetched = match self.fetch(&url, MAX_ADS_BYTES).await {
            Ok((200, body, _)) => serde_json::from_slice::<Value>(&body)
                .ok()
                .and_then(|value| ads(&value, now).map(|list| (value, list)))
                .ok_or(FeedProblem::Server),
            Ok(_) => Err(FeedProblem::Server),
            Err(problem) => Err(problem),
        };
        let (list, stale) = match fetched {
            Ok((value, list)) => {
                if let Some(root) = root
                    && let Ok(bytes) = serde_json::to_vec(&json!({ "ads": value["ads"] }))
                {
                    keep(&ads_file(root, place, lang), &bytes);
                }
                (list, false)
            }
            Err(problem) => {
                let kept = root
                    .and_then(|root| read_json(&ads_file(root, place, lang)))
                    .and_then(|value| ads(&value, now));
                match kept {
                    Some(list) => (list, true),
                    None => return Err(problem),
                }
            }
        };
        let mut shown = Vec::with_capacity(list.len());
        for (mut ad, pictures) in list {
            if let Some(sha) = pictures.icon {
                ad.icon = self.picture(root, &sha, Kind::Png).await;
            }
            if let Some(sha) = pictures.banner {
                ad.banner = self.picture(root, &sha, Kind::Jpeg).await;
            }
            shown.push(ad);
        }
        if !stale {
            *lock(&self.fresh) = Some((Instant::now(), key, shown.clone()));
        }
        Ok(ServerAdsView { ads: shown, stale })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_800_000_000;

    fn good() -> Value {
        json!({
            "id": "7",
            "name": "Test Server",
            "address": "play.example.net",
            "text": "Survival.\nWeekend events.",
            "version": "1.21.4",
            "versions": ["1.21.11", "1.21.4", "1.20.1"],
            "icon": "a".repeat(64),
            "up": true,
            "online": 12,
            "max": 100,
            "pingedAt": NOW - 60,
            "endsAt": NOW + 3600,
            "weight": 10,
            "erid": null,
            "banner": "b".repeat(64),
            "licensed": true,
        })
    }

    fn with(key: &str, value: Value) -> Value {
        let mut ad = good();
        ad[key] = value;
        ad
    }

    #[test]
    fn a_good_ad_is_read() {
        let (ad, pictures) = ad(&good(), NOW).unwrap();
        assert_eq!(ad.address, "play.example.net");
        assert_eq!(ad.versions.len(), 3);
        assert_eq!(pictures.icon.unwrap().len(), 64);
        assert_eq!(pictures.banner.unwrap(), "b".repeat(64));
        assert_eq!(ad.online, Some(12));
        assert_eq!(ad.licensed, Some(true));
        let quiet = with("up", Value::Null);
        assert_eq!(super::ad(&quiet, NOW).unwrap().0.up, None);
        // A service from before banners and the sign-in check.
        let mut old = good();
        old.as_object_mut().unwrap().remove("banner");
        old.as_object_mut().unwrap().remove("licensed");
        let (old, pictures) = super::ad(&old, NOW).unwrap();
        assert_eq!((old.licensed, pictures.banner), (None, None));
    }

    #[test]
    fn an_ad_that_breaks_a_rule_is_left_out() {
        for (key, value) in [
            ("id", json!("../1")),
            ("id", json!(7)),
            ("name", json!("")),
            ("name", json!("x".repeat(41))),
            ("name", json!("a\u{202E}b")),
            ("address", json!("--server evil")),
            ("address", json!("evil.example; rm -rf /")),
            ("address", json!(" play.example.net")),
            ("address", json!("play.example.net:0")),
            ("text", json!("see https://evil.example")),
            ("text", json!("a\nb\nc\nd")),
            ("text", json!("a\u{0007}")),
            ("text", json!("x".repeat(201))),
            ("version", json!("../../x")),
            ("version", json!("1.16.5")),
            ("versions", json!([])),
            ("versions", json!(["1.21.4", "1.21.4"])),
            ("versions", json!(["1.21.4", "bad id"])),
            ("icon", json!("../x")),
            ("icon", json!("A".repeat(64))),
            ("up", json!("yes")),
            ("online", json!(-1)),
            ("max", json!(1e12)),
            ("endsAt", json!(NOW)),
            ("endsAt", json!("soon")),
            ("weight", json!(0)),
            ("weight", json!(101)),
            ("erid", json!("a-b")),
            ("banner", json!("../../x.jpg")),
            ("banner", json!(1)),
            ("licensed", json!("yes")),
            ("licensed", json!(1)),
        ] {
            assert!(
                super::ad(&with(key, value.clone()), NOW).is_none(),
                "{key}: {value}"
            );
        }
    }

    #[test]
    fn a_list_is_bounded_and_each_ad_comes_once() {
        let list: Vec<Value> = (0..80).map(|i| with("id", json!(i.to_string()))).collect();
        assert_eq!(ads(&json!({ "ads": list }), NOW).unwrap().len(), MAX_ADS);
        let twice = json!({ "ads": [good(), good(), with("id", json!("x"))] });
        assert_eq!(ads(&twice, NOW).unwrap().len(), 1);
        assert!(ads(&json!({}), NOW).is_none());
        assert!(ads(&json!({ "ads": "x" }), NOW).is_none());
    }

    #[test]
    fn only_our_service_is_asked() {
        let ok = |url: &str| SourcePolicy::ServerAds.allows(&Url::parse(url).unwrap());
        assert!(ok("https://servers.lapi.sh/v1/ads?place=feed&lang=ru"));
        assert!(!ok("https://play.example.net/"));
        assert!(!ok("https://feed.lapi.sh/v1/feed"));
        assert!(!ok("http://servers.lapi.sh/v1/ads"));
        assert!(!ok("https://servers.lapi.sh.evil.example/v1/ads"));
        assert!(png(b"\x89PNG\r\n\x1a\n0123456789012345678901234567890123"));
        assert!(!png(b"<svg onload=alert(1)>"));
        let mut jpg = vec![0xff, 0xd8, 0xff, 0xe0];
        jpg.extend_from_slice(&[0; 40]);
        jpg.extend_from_slice(&[0xff, 0xd9]);
        assert!(jpeg(&jpg));
        assert!(!jpeg(&jpg[..jpg.len() - 1]), "cut short");
        assert!(!jpeg(
            b"\x89PNG\r\n\x1a\n0123456789012345678901234567890123"
        ));
        assert!(!Kind::Jpeg.holds(b"<svg onload=alert(1)>"));
    }
}
