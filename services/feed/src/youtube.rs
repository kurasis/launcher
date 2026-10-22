//! YouTube without an API key: a channel's RSS feed lists its 15 newest
//! videos, and a channel is found by its id, its @handle or one of its
//! videos (through YouTube's oEmbed).

use quick_xml::events::Event;
use serde::{Deserialize, Serialize};

use crate::html;
use crate::images::video_id;
use crate::net;

const MAX_RSS: usize = 512 * 1024;
pub const MAX_PAGE: usize = 3 * 1024 * 1024;
/// The longest address of a channel's picture taken from its page.
const MAX_AVATAR_URL: usize = 400;
/// Characters of one element's text kept while reading a feed.
const MAX_TEXT: usize = 4096;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Video {
    pub id: String,
    pub channel_id: String,
    pub channel: String,
    pub title: String,
    pub date: String,
    /// A YouTube Short.
    pub short: bool,
    /// Its length in seconds, from the Data API (RSS has none).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seconds: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Channel {
    pub title: String,
    pub videos: Vec<Video>,
}

pub fn channel_id(id: &str) -> bool {
    id.len() == 24
        && id.starts_with("UC")
        && id[2..]
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// A YouTube handle: 3 to 30 letters (of any script), digits, `.`, `_`, `-`.
fn handle(handle: &str) -> bool {
    (3..=30).contains(&handle.chars().count())
        && handle
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

/// A path segment of a link, percent-decoded.
fn segment(raw: &str) -> Option<String> {
    percent_encoding::percent_decode_str(raw)
        .decode_utf8()
        .ok()
        .map(|s| s.into_owned())
}

/// Reads a channel's RSS feed.
pub fn parse_rss(xml: &str, channel_id: &str) -> Result<Channel, String> {
    let mut reader = quick_xml::Reader::from_str(xml);
    let mut path: Vec<String> = Vec::new();
    // The text of the element being read: quick-xml gives text and each
    // `&amp;` or `&#…;` in it as separate events.
    let mut text = String::new();
    let mut title = String::new();
    let mut videos = Vec::new();
    let mut current: Option<(String, String, String, bool)> = None;
    loop {
        match reader.read_event().map_err(|e| e.to_string())? {
            Event::Start(e) => {
                let name = e.name().as_ref().to_owned();
                if name == "entry" {
                    current = Some(Default::default());
                }
                text.clear();
                path.push(name);
                if path.len() > 16 {
                    return Err("feed nested too deep".to_owned());
                }
            }
            Event::Empty(e) => {
                let name = e.name();
                if name.as_ref() == "link"
                    && let Some(entry) = current.as_mut()
                {
                    for attr in e.attributes().flatten() {
                        if attr.key.as_ref() == "href" {
                            let href = attr
                                .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                                .map_err(|e| e.to_string())?;
                            entry.3 = href.contains("/shorts/");
                        }
                    }
                }
            }
            Event::Text(t) => {
                if text.len() < MAX_TEXT {
                    text.push_str(&t.xml10_content());
                }
            }
            Event::CData(t) => {
                if text.len() < MAX_TEXT {
                    text.push_str(&t.xml10_content());
                }
            }
            Event::GeneralRef(r) => {
                let name = r.xml10_content();
                // Only XML's own five and character references exist here.
                if text.len() < MAX_TEXT
                    && let Ok(c) = quick_xml::escape::unescape(&format!("&{name};"))
                {
                    text.push_str(&c);
                }
            }
            Event::End(e) => {
                let parent = path.iter().rev().nth(1).map(String::as_str);
                let value = std::mem::take(&mut text);
                match (path.last().map(String::as_str), parent, current.as_mut()) {
                    (Some("title"), Some("feed"), _) => {
                        title = html::clean(&value, 100).trim().to_owned()
                    }
                    (Some("yt:videoId"), Some("entry"), Some(entry)) => {
                        entry.0 = value.trim().to_owned()
                    }
                    (Some("title"), Some("entry"), Some(entry)) => {
                        entry.1 = html::clean(&value, 200).trim().to_owned()
                    }
                    (Some("published"), Some("entry"), Some(entry)) => {
                        entry.2 = value.trim().to_owned()
                    }
                    _ => {}
                }
                if e.name().as_ref() == "entry"
                    && let Some((id, video_title, published, short)) = current.take()
                    && video_id(&id)
                    && !video_title.is_empty()
                    && let Ok(date) = chrono::DateTime::parse_from_rfc3339(&published)
                {
                    videos.push(Video {
                        id,
                        channel_id: channel_id.to_owned(),
                        channel: String::new(),
                        title: video_title,
                        date: date
                            .with_timezone(&chrono::Utc)
                            .format("%Y-%m-%dT%H:%M:%SZ")
                            .to_string(),
                        short,
                        seconds: None,
                    });
                }
                path.pop();
            }
            // Entities beyond XML's own five are never expanded: quick-xml
            // has no DTD support.
            Event::DocType(_) => return Err("feed with a DOCTYPE".to_owned()),
            Event::Eof => break,
            _ => {}
        }
    }
    for video in &mut videos {
        video.channel.clone_from(&title);
    }
    videos.truncate(15);
    Ok(Channel { title, videos })
}

pub async fn fetch(http: &reqwest::Client, id: &str) -> Result<Channel, String> {
    if !channel_id(id) {
        return Err("bad channel id".to_owned());
    }
    let xml = net::get_text(
        http,
        &format!("https://www.youtube.com/feeds/videos.xml?channel_id={id}"),
        MAX_RSS,
    )
    .await?;
    parse_rss(&xml, id)
}

/// What a player typed to name a channel.
#[derive(Debug, PartialEq, Eq)]
pub enum Wanted {
    Channel(String),
    Handle(String),
    Video(String),
}

/// Reads a channel link, a video link or an @handle. Anything else, or any
/// other site, is refused before a request is made.
pub fn wanted(input: &str) -> Option<Wanted> {
    let input = input.trim();
    if input.is_empty() || input.len() > 200 || input.chars().any(char::is_control) {
        return None;
    }
    if channel_id(input) {
        return Some(Wanted::Channel(input.to_owned()));
    }
    if let Some(h) = input.strip_prefix('@') {
        return handle(h).then(|| Wanted::Handle(h.to_owned()));
    }
    let with_scheme = if input.contains("://") {
        input.to_owned()
    } else {
        format!("https://{input}")
    };
    let url = url::Url::parse(&with_scheme).ok()?;
    if !matches!(url.scheme(), "https" | "http") || url.port().is_some() {
        return None;
    }
    let host = url.host_str()?.to_ascii_lowercase();
    let segments: Vec<String> = url
        .path_segments()?
        .filter(|s| !s.is_empty())
        .map(segment)
        .collect::<Option<_>>()?;
    let segments: Vec<&str> = segments.iter().map(String::as_str).collect();
    if host == "youtu.be" {
        let id = segments.first()?;
        return video_id(id).then(|| Wanted::Video((*id).to_owned()));
    }
    if !matches!(
        host.as_str(),
        "youtube.com" | "www.youtube.com" | "m.youtube.com" | "music.youtube.com"
    ) {
        return None;
    }
    match segments.as_slice() {
        [first, ..] if first.starts_with('@') => {
            let h = &first[1..];
            handle(h).then(|| Wanted::Handle(h.to_owned()))
        }
        ["channel", id, ..] if channel_id(id) => Some(Wanted::Channel((*id).to_owned())),
        ["watch"] => url
            .query_pairs()
            .find(|(k, _)| k == "v")
            .map(|(_, v)| v.into_owned())
            .filter(|v| video_id(v))
            .map(Wanted::Video),
        ["shorts" | "live" | "embed", id, ..] if video_id(id) => {
            Some(Wanted::Video((*id).to_owned()))
        }
        _ => None,
    }
}

/// The channel id in a channel page.
pub fn id_in_page(page: &str) -> Option<String> {
    for marker in [
        "\"externalId\":\"",
        "<link rel=\"canonical\" href=\"https://www.youtube.com/channel/",
        "\"channelId\":\"",
    ] {
        if let Some(at) = page.find(marker) {
            let id: String = page[at + marker.len()..].chars().take(24).collect();
            if channel_id(&id) {
                return Some(id);
            }
        }
    }
    None
}

/// A channel picture's address: on YouTube's picture hosts only, over
/// https, plain (no escapes, entities, queries or user names).
pub fn picture_url(raw: &str) -> Option<String> {
    if raw.len() > MAX_AVATAR_URL
        || !raw
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-._~/:=".contains(&b))
    {
        return None;
    }
    let url = url::Url::parse(raw).ok()?;
    (url.scheme() == "https"
        && url.port().is_none()
        && matches!(
            url.host_str(),
            Some("yt3.googleusercontent.com" | "yt3.ggpht.com")
        ))
    .then(|| url.into())
}

/// The address of the channel's picture in a channel page.
pub fn avatar_in_page(page: &str) -> Option<String> {
    for marker in [
        "<meta property=\"og:image\" content=\"",
        "\"avatar\":{\"thumbnails\":[{\"url\":\"",
    ] {
        let Some(at) = page.find(marker) else {
            continue;
        };
        let rest = &page[at + marker.len()..];
        let found = rest
            .find('"')
            .filter(|end| *end <= MAX_AVATAR_URL)
            .and_then(|end| picture_url(&rest[..end]));
        if found.is_some() {
            return found;
        }
    }
    None
}

/// The address of a channel's picture, from its page.
pub async fn avatar(http: &reqwest::Client, id: &str) -> Result<String, String> {
    if !channel_id(id) {
        return Err("bad channel id".to_owned());
    }
    let page = net::get_text(
        http,
        &format!("https://www.youtube.com/channel/{id}"),
        MAX_PAGE,
    )
    .await?;
    avatar_in_page(&page).ok_or_else(|| "no picture on the page".to_owned())
}

#[derive(Deserialize)]
struct OEmbed {
    author_url: String,
}

/// Finds the channel a player named; its RSS answers, so it exists.
pub async fn resolve(http: &reqwest::Client, wanted: Wanted) -> Result<(String, Channel), String> {
    // `join` percent-encodes a handle in another script.
    let handle_page = |h: String| {
        url::Url::parse("https://www.youtube.com/")
            .and_then(|u| u.join(&format!("@{h}")))
            .map(|u| u.to_string())
            .unwrap_or_default()
    };
    let page = match wanted {
        Wanted::Channel(id) => {
            let channel = fetch(http, &id).await?;
            return Ok((id, channel));
        }
        Wanted::Handle(h) => handle_page(h),
        Wanted::Video(id) => {
            let oembed: OEmbed = net::get_json(
                http,
                &format!(
                    "https://www.youtube.com/oembed?format=json&url=https://www.youtube.com/watch?v={id}"
                ),
                64 * 1024,
            )
            .await?;
            let author = url::Url::parse(&oembed.author_url).map_err(|e| e.to_string())?;
            let segments: Vec<String> = author
                .path_segments()
                .map(|s| s.filter(|s| !s.is_empty()).filter_map(segment).collect())
                .unwrap_or_default();
            let segments: Vec<&str> = segments.iter().map(String::as_str).collect();
            match segments.as_slice() {
                ["channel", id] if channel_id(id) => {
                    let channel = fetch(http, id).await?;
                    return Ok(((*id).to_owned(), channel));
                }
                [first] if first.starts_with('@') && handle(&first[1..]) => {
                    handle_page(first[1..].to_owned())
                }
                _ => return Err("the video's channel was not found".to_owned()),
            }
        }
    };
    let html = net::get_text(http, &page, MAX_PAGE).await?;
    let id = id_in_page(&html).ok_or("no channel id on the page")?;
    let channel = fetch(http, &id).await?;
    Ok((id, channel))
}
