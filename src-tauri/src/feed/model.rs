//! The feed's documents as the launcher accepts them (D-067).
//!
//! Everything from feed.lapi.sh, and from the copies kept on disk, is read
//! here field by field: lists, texts and ids are bounded, ids follow the
//! service's own rules, pictures are only names of the service's pictures and
//! links only https addresses the launcher may open. An item that does not
//! pass is dropped; the rest of the feed stays. The interface receives only
//! these types, never the server's JSON.

use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::{Map, Value};

use crate::storage::models::FeedBlock;

/// The largest feed document read.
pub const MAX_FEED_BYTES: usize = 2 * 1024 * 1024;
/// The largest note (an article with its blocks) read.
pub const MAX_NOTE_BYTES: usize = 4 * 1024 * 1024;
/// The largest answer of a small request (a channel, a player's videos).
pub const MAX_SMALL_BYTES: usize = 256 * 1024;

/// Patch notes or news in one list.
const MAX_NOTES: usize = 100;
const MAX_VIDEOS: usize = 60;
const MAX_STREAMS: usize = 60;
const MAX_EDITORIAL: usize = 20;
const MAX_TITLE: usize = 300;
const MAX_SUMMARY: usize = 1200;
const MAX_NAME: usize = 100;
const MAX_TEXT: usize = 1000;
const MAX_URL: usize = 2000;
const MAX_VERSION: usize = 64;
const MAX_ALT: usize = 300;
/// Blocks of one note, list items included.
const MAX_BLOCKS: usize = 6000;
/// Characters of text in one note.
const MAX_NOTE_TEXT: usize = 1_000_000;
/// Runs of text in one block.
const MAX_RUNS: usize = 2000;
/// Characters of one run.
const MAX_RUN_TEXT: usize = 20_000;
/// Lists inside lists.
const MAX_DEPTH: usize = 8;
/// Viewers of a stream, at most.
const MAX_VIEWERS: u64 = 100_000_000;
/// Versions of an update cycle, at most.
const MAX_CYCLE_COUNT: u64 = 1000;
/// Items in one page of the endless list.
const MAX_PAGE_ITEMS: usize = 40;
/// The longest cursor the service makes.
const MAX_CURSOR: usize = 120;

/// The languages the feed is written in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Lang {
    En,
    Ru,
    Ar,
}

impl Lang {
    pub const ALL: [Self; 3] = [Self::En, Self::Ru, Self::Ar];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::En => "en",
            Self::Ru => "ru",
            Self::Ar => "ar",
        }
    }

    pub fn parse(tag: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|l| l.as_str() == tag)
    }

    /// The feed language of an interface language tag: Russian and Arabic
    /// (of any region), English for everything else.
    pub fn of_interface(tag: &str) -> Self {
        match tag.split('-').next() {
            Some("ru") => Self::Ru,
            Some("ar") => Self::Ar,
            _ => Self::En,
        }
    }
}

/// A version's kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum VersionKind {
    Release,
    Snapshot,
    Pre,
    Rc,
}

impl VersionKind {
    fn parse(value: &Value) -> Option<Self> {
        match value.as_str()? {
            "release" => Some(Self::Release),
            "snapshot" => Some(Self::Snapshot),
            "pre" => Some(Self::Pre),
            "rc" => Some(Self::Rc),
            _ => None,
        }
    }
}

/// A patch note or a news article, as a card.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteCard {
    pub id: String,
    pub title: String,
    pub date: String,
    pub summary: String,
    /// Machine-translated from English.
    pub translated: bool,
    /// The game version of a patch note.
    pub version: Option<String>,
    pub kind: Option<VersionKind>,
    /// `img/<sha256>.jpg`.
    pub image: Option<String>,
    /// The article on www.minecraft.net (news).
    pub url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Video {
    pub id: String,
    pub channel_id: String,
    pub channel: String,
    pub title: String,
    pub date: String,
    /// A YouTube Short.
    pub short: bool,
    /// `yt/<id>.jpg`.
    pub image: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Stream {
    pub login: String,
    pub name: String,
    pub title: String,
    pub viewers: u64,
    pub started: String,
    /// `tw/<login>.jpg`.
    pub image: Option<String>,
}

/// Where the current update cycle is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Cycle {
    pub target: String,
    pub snapshots: u64,
    pub pre: u64,
    pub rc: u64,
    pub released: bool,
    pub stage: VersionKind,
    pub latest: String,
    pub date: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LatestVersion {
    pub id: String,
    pub kind: VersionKind,
    pub date: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Latest {
    pub release: Option<LatestVersion>,
    pub snapshot: Option<LatestVersion>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Event {
    pub id: String,
    pub title: String,
    pub starts: String,
    pub ends: String,
    pub url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Announcement {
    pub id: String,
    pub title: String,
    pub text: String,
    pub url: Option<String>,
}

/// The feed for one language.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Feed {
    /// The service's format number; parts a newer format adds are dropped.
    pub format: u64,
    pub generated: String,
    pub lang: Lang,
    pub versions: Vec<NoteCard>,
    pub news: Vec<NoteCard>,
    pub videos: Vec<Video>,
    pub streams: Vec<Stream>,
    pub cycle: Option<Cycle>,
    pub latest: Latest,
    pub events: Vec<Event>,
    pub announcements: Vec<Announcement>,
    pub tip: Option<String>,
    /// When each source last answered the service: mojang, youtube, twitch.
    pub fetched: BTreeMap<String, String>,
}

/// One item of the endless list (D-067).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum FeedItem {
    Note { note: NoteCard },
    Video { video: Video },
    Stream { stream: Stream },
}

/// A page of the endless list, newest first; `next` asks for the page after.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedPage {
    pub items: Vec<FeedItem>,
    pub next: Option<String>,
}

/// A run of text: bold, italic, code, or a link the launcher may open.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Run {
    pub text: String,
    #[serde(skip_serializing_if = "is_false")]
    pub b: bool,
    #[serde(skip_serializing_if = "is_false")]
    pub i: bool,
    #[serde(skip_serializing_if = "is_false")]
    pub code: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub href: Option<String>,
}

// Serde's skip_serializing_if passes a reference.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_false(value: &bool) -> bool {
    !*value
}

/// A block of an article, in the service's own small format.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "t", rename_all = "lowercase")]
pub enum Block {
    H { level: u8, c: Vec<Run> },
    P { c: Vec<Run> },
    Ul { items: Vec<Vec<Block>> },
    Ol { items: Vec<Vec<Block>> },
    Img { src: String, alt: String },
}

/// A whole note.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Note {
    #[serde(flatten)]
    pub card: NoteCard,
    pub lang: Lang,
    pub blocks: Vec<Block>,
}

/// A YouTube channel a player follows in the feed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OwnChannel {
    pub channel_id: String,
    pub title: String,
}

/// A Twitch channel a player follows in the feed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OwnStreamer {
    pub login: String,
    pub name: String,
}

// ---- Ids, names and links --------------------------------------------------

/// A note id: `j-` (patch note) or `n-` (news) and lower case letters, digits
/// and dashes, 3 to 80 characters in all.
pub fn note_id(id: &str) -> bool {
    (3..=80).contains(&id.len())
        && (id.starts_with("j-") || id.starts_with("n-"))
        && id[2..]
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// A YouTube video id: 11 characters of `A-Z a-z 0-9 - _`.
pub fn video_id(id: &str) -> bool {
    id.len() == 11
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// A Twitch login: 3 to 25 lower case letters, digits or `_`.
pub fn twitch_login(login: &str) -> bool {
    (3..=25).contains(&login.len())
        && login
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

/// A YouTube channel id: `UC` and 22 characters of `A-Z a-z 0-9 - _`.
pub fn channel_id(id: &str) -> bool {
    id.len() == 24
        && id.starts_with("UC")
        && id[2..]
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// A game version's id: `26.4-snapshot-4`, `1.21.4-rc1`, `25w14a`.
fn version_id(id: &str) -> bool {
    (1..=MAX_VERSION).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
}

/// An editorial id (event, announcement).
fn editorial_id(id: &str) -> bool {
    (1..=64).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}

/// The kinds of the service's pictures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PictureKind {
    /// Article pictures, named by their SHA-256.
    Img,
    /// Video previews.
    Yt,
    /// Stream previews.
    Tw,
}

impl PictureKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Img => "img",
            Self::Yt => "yt",
            Self::Tw => "tw",
        }
    }

    pub fn parse(kind: &str) -> Option<Self> {
        match kind {
            "img" => Some(Self::Img),
            "yt" => Some(Self::Yt),
            "tw" => Some(Self::Tw),
            _ => None,
        }
    }
}

/// A picture's file name of `kind`: `<64 hex>.jpg`, `<video id>.jpg` or
/// `<login>.jpg`.
pub fn picture_name(kind: PictureKind, name: &str) -> bool {
    let Some(stem) = name.strip_suffix(".jpg") else {
        return false;
    };
    match kind {
        PictureKind::Img => {
            stem.len() == 64
                && stem
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        }
        PictureKind::Yt => video_id(stem),
        PictureKind::Tw => twitch_login(stem),
    }
}

/// A picture path as the feed names it: `img/<sha>.jpg`, `yt/<id>.jpg`,
/// `tw/<login>.jpg`.
pub fn picture_path(path: &str) -> Option<(PictureKind, &str)> {
    let (kind, name) = path.split_once('/')?;
    let kind = PictureKind::parse(kind)?;
    picture_name(kind, name).then_some((kind, name))
}

/// The sites a link of the feed may lead to: Minecraft's, YouTube, Twitch
/// and the feed's own pages. A subset of what the launcher may open at all
/// (`external::check_allowed`), so the feed can never send a player to, say,
/// a Microsoft sign-in page.
const LINK_HOSTS: &[&str] = &[
    "www.minecraft.net",
    "help.minecraft.net",
    "feed.lapi.sh",
    "www.youtube.com",
    "youtu.be",
    "www.twitch.tv",
];

/// A link the feed may show and open in the browser.
pub fn link(url: &str) -> bool {
    url.len() <= MAX_URL
        && crate::external::check_allowed(url)
            .is_ok_and(|u| u.host_str().is_some_and(|h| LINK_HOSTS.contains(&h)))
}

/// An article on www.minecraft.net.
fn article_link(url: &str) -> bool {
    link(url) && url::Url::parse(url).is_ok_and(|u| u.host_str() == Some("www.minecraft.net"))
}

// ---- Texts and dates ---------------------------------------------------------

/// Characters that change the direction of the text around them; a title
/// must not turn the rest of the line around.
fn direction_control(c: char) -> bool {
    matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
}

/// Text for the interface: control and direction characters go, spaces are
/// trimmed and a text longer than `max` characters is cut. Empty is `None`.
pub fn clean_text(raw: &str, max: usize) -> Option<String> {
    let mut out = String::with_capacity(raw.len().min(max * 4));
    let mut count = 0;
    for c in raw.chars() {
        if direction_control(c) {
            continue;
        }
        let c = if c.is_control() { ' ' } else { c };
        if count == max {
            out.push('…');
            break;
        }
        out.push(c);
        count += 1;
    }
    let trimmed = out.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

fn text(value: Option<&Value>, max: usize) -> Option<String> {
    clean_text(value?.as_str()?, max)
}

/// A date as `YYYY-MM-DDTHH:MM:SSZ`, from any RFC 3339 form.
fn date(value: Option<&Value>) -> Option<String> {
    let raw = value?.as_str()?;
    if raw.len() > 40 {
        return None;
    }
    let parsed = chrono::DateTime::parse_from_rfc3339(raw).ok()?;
    Some(
        parsed
            .with_timezone(&chrono::Utc)
            .format("%Y-%m-%dT%H:%M:%SZ")
            .to_string(),
    )
}

fn id_with(value: Option<&Value>, rule: impl Fn(&str) -> bool) -> Option<String> {
    let id = value?.as_str()?;
    rule(id).then(|| id.to_owned())
}

fn flag(value: Option<&Value>) -> bool {
    value.and_then(Value::as_bool).unwrap_or(false)
}

fn count(value: Option<&Value>, max: u64) -> Option<u64> {
    value?.as_u64().filter(|n| *n <= max)
}

fn picture(value: Option<&Value>, kind: PictureKind) -> Option<String> {
    let path = value?.as_str()?;
    picture_path(path)
        .filter(|(k, _)| *k == kind)
        .map(|_| path.to_owned())
}

fn optional_link(value: Option<&Value>, rule: fn(&str) -> bool) -> Option<String> {
    let url = value?.as_str()?;
    rule(url).then(|| url.to_owned())
}

/// The valid items of a list, at most `max` of them.
fn list<T>(
    value: Option<&Value>,
    max: usize,
    item: impl Fn(&Map<String, Value>) -> Option<T>,
) -> Vec<T> {
    value
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_object)
                .filter_map(item)
                .take(max)
                .collect()
        })
        .unwrap_or_default()
}

// ---- Documents ---------------------------------------------------------------

/// A note card; `prefix` is `j-` for patch notes, `n-` for news.
fn note_card(fields: &Map<String, Value>, prefix: Option<&str>) -> Option<NoteCard> {
    let id = id_with(fields.get("id"), note_id)?;
    if prefix.is_some_and(|p| !id.starts_with(p)) {
        return None;
    }
    Some(NoteCard {
        title: text(fields.get("title"), MAX_TITLE)?,
        date: date(fields.get("date"))?,
        summary: text(fields.get("summary"), MAX_SUMMARY).unwrap_or_default(),
        translated: flag(fields.get("translated")),
        version: id_with(fields.get("version"), version_id),
        kind: fields.get("kind").and_then(VersionKind::parse),
        image: picture(fields.get("image"), PictureKind::Img),
        url: optional_link(fields.get("url"), article_link),
        id,
    })
}

fn video(fields: &Map<String, Value>) -> Option<Video> {
    let id = id_with(fields.get("id"), video_id)?;
    Some(Video {
        channel_id: id_with(fields.get("channelId"), channel_id)?,
        channel: text(fields.get("channel"), MAX_NAME)?,
        title: text(fields.get("title"), MAX_TITLE)?,
        date: date(fields.get("date"))?,
        short: flag(fields.get("short")),
        image: picture(fields.get("image"), PictureKind::Yt)
            .filter(|path| *path == format!("yt/{id}.jpg")),
        id,
    })
}

fn stream(fields: &Map<String, Value>) -> Option<Stream> {
    let login = id_with(fields.get("login"), twitch_login)?;
    Some(Stream {
        name: text(fields.get("name"), MAX_NAME)?,
        title: text(fields.get("title"), MAX_TITLE).unwrap_or_default(),
        viewers: count(fields.get("viewers"), MAX_VIEWERS)?,
        started: date(fields.get("started"))?,
        image: picture(fields.get("image"), PictureKind::Tw)
            .filter(|path| *path == format!("tw/{login}.jpg")),
        login,
    })
}

fn cycle(value: Option<&Value>) -> Option<Cycle> {
    let fields = value?.as_object()?;
    Some(Cycle {
        target: id_with(fields.get("target"), version_id)?,
        snapshots: count(fields.get("snapshots"), MAX_CYCLE_COUNT)?,
        pre: count(fields.get("pre"), MAX_CYCLE_COUNT)?,
        rc: count(fields.get("rc"), MAX_CYCLE_COUNT)?,
        released: flag(fields.get("released")),
        stage: VersionKind::parse(fields.get("stage")?)?,
        latest: id_with(fields.get("latest"), version_id)?,
        date: date(fields.get("date"))?,
    })
}

fn latest_version(value: Option<&Value>, release: bool) -> Option<LatestVersion> {
    let fields = value?.as_object()?;
    let kind = VersionKind::parse(fields.get("kind")?)?;
    if (kind == VersionKind::Release) != release {
        return None;
    }
    Some(LatestVersion {
        id: id_with(fields.get("id"), version_id)?,
        kind,
        date: date(fields.get("date"))?,
    })
}

fn event(fields: &Map<String, Value>) -> Option<Event> {
    Some(Event {
        id: id_with(fields.get("id"), editorial_id)?,
        title: text(fields.get("title"), MAX_TITLE)?,
        starts: date(fields.get("starts"))?,
        ends: date(fields.get("ends"))?,
        url: optional_link(fields.get("url"), link),
    })
}

fn announcement(fields: &Map<String, Value>) -> Option<Announcement> {
    Some(Announcement {
        id: id_with(fields.get("id"), editorial_id)?,
        title: text(fields.get("title"), MAX_TITLE)?,
        text: text(fields.get("text"), MAX_TEXT).unwrap_or_default(),
        url: optional_link(fields.get("url"), link),
    })
}

/// Reads a feed. `None` when it is not a feed at all (no object, a language
/// we did not ask for, no format); bad items inside are dropped.
pub fn feed(value: &Value, wanted: Lang) -> Option<Feed> {
    let fields = value.as_object()?;
    let format = fields
        .get("format")
        .and_then(Value::as_u64)
        .filter(|f| *f > 0)?;
    let lang = Lang::parse(fields.get("lang")?.as_str()?)?;
    if lang != wanted {
        return None;
    }
    let latest = fields.get("latest").and_then(Value::as_object);
    let fetched = fields
        .get("fetched")
        .and_then(Value::as_object)
        .map(|sources| {
            sources
                .iter()
                .filter(|(source, _)| matches!(source.as_str(), "mojang" | "youtube" | "twitch"))
                .filter_map(|(source, time)| Some((source.clone(), date(Some(time))?)))
                .collect()
        })
        .unwrap_or_default();
    Some(Feed {
        format,
        generated: date(fields.get("generated"))?,
        lang,
        versions: list(fields.get("versions"), MAX_NOTES, |f| {
            note_card(f, Some("j-"))
        }),
        news: list(fields.get("news"), MAX_NOTES, |f| note_card(f, Some("n-"))),
        videos: list(fields.get("videos"), MAX_VIDEOS, video),
        streams: list(fields.get("streams"), MAX_STREAMS, stream),
        cycle: cycle(fields.get("cycle")),
        latest: Latest {
            release: latest_version(latest.and_then(|l| l.get("release")), true),
            snapshot: latest_version(latest.and_then(|l| l.get("snapshot")), false),
        },
        events: list(fields.get("events"), MAX_EDITORIAL, event),
        announcements: list(fields.get("announcements"), MAX_EDITORIAL, announcement),
        tip: text(fields.get("tip"), MAX_TEXT),
        fetched,
    })
}

/// A cursor of the endless list as the service makes it: `<time>~<key>`, the
/// time `YYYY-MM-DDTHH:MM:SSZ`, the key a note id, `yt:<id>` or `tw:<login>`.
pub fn cursor(raw: &str) -> bool {
    if raw.len() > MAX_CURSOR {
        return false;
    }
    let Some((time, key)) = raw.split_once('~') else {
        return false;
    };
    let time_ok = date(Some(&Value::String(time.to_owned()))).is_some_and(|t| t == time);
    let key_ok = note_id(key)
        || key.strip_prefix("yt:").is_some_and(video_id)
        || key.strip_prefix("tw:").is_some_and(twitch_login);
    time_ok && key_ok
}

/// Reads a page of the endless list, keeping only items of `kinds`.
pub fn page(value: &Value, kinds: &[FeedBlock]) -> Option<FeedPage> {
    let fields = value.as_object()?;
    fields
        .get("format")
        .and_then(Value::as_u64)
        .filter(|f| *f > 0)?;
    let items = fields
        .get("items")?
        .as_array()?
        .iter()
        .filter_map(Value::as_object)
        .filter_map(|item| {
            if let Some(note) = item.get("note").and_then(Value::as_object) {
                let card = note_card(note, None)?;
                let kind = if card.id.starts_with("j-") {
                    FeedBlock::Versions
                } else {
                    FeedBlock::News
                };
                kinds
                    .contains(&kind)
                    .then_some(FeedItem::Note { note: card })
            } else if let Some(fields) = item.get("video").and_then(Value::as_object) {
                let video = video(fields)?;
                kinds
                    .contains(&FeedBlock::Videos)
                    .then_some(FeedItem::Video { video })
            } else {
                let stream = stream(item.get("stream")?.as_object()?)?;
                kinds
                    .contains(&FeedBlock::Streams)
                    .then_some(FeedItem::Stream { stream })
            }
        })
        .take(MAX_PAGE_ITEMS)
        .collect();
    let next = match fields.get("next") {
        None | Some(Value::Null) => None,
        Some(next) => Some(next.as_str().filter(|n| cursor(n))?.to_owned()),
    };
    Some(FeedPage { items, next })
}

/// Reads a player's videos (`{videos: [...]}`).
pub fn videos(value: &Value) -> Option<Vec<Video>> {
    let fields = value.as_object()?;
    fields.get("videos")?.as_array()?;
    Some(list(fields.get("videos"), MAX_VIDEOS, video))
}

/// Reads the live streams of a player's channels (`{streams: [...]}`).
pub fn streams(value: &Value) -> Option<Vec<Stream>> {
    let fields = value.as_object()?;
    fields.get("streams")?.as_array()?;
    Some(list(fields.get("streams"), MAX_STREAMS, stream))
}

/// Reads a found Twitch channel (`{login, name}`).
pub fn own_streamer(value: &Value) -> Option<OwnStreamer> {
    let fields = value.as_object()?;
    Some(OwnStreamer {
        login: id_with(fields.get("login"), twitch_login)?,
        name: text(fields.get("name"), MAX_NAME)?,
    })
}

/// Reads a found channel (`{channelId, title}`).
pub fn own_channel(value: &Value) -> Option<OwnChannel> {
    let fields = value.as_object()?;
    Some(OwnChannel {
        channel_id: id_with(fields.get("channelId"), channel_id)?,
        title: text(fields.get("title"), MAX_NAME)?,
    })
}

/// Counts what a note holds, so no note can be endless.
struct Budget {
    blocks: usize,
    text: usize,
}

impl Budget {
    fn full(&self) -> bool {
        self.blocks >= MAX_BLOCKS || self.text >= MAX_NOTE_TEXT
    }
}

fn run(fields: &Map<String, Value>, budget: &mut Budget) -> Option<Run> {
    let raw = fields.get("text")?.as_str()?;
    // Spaces between runs matter: only controls and direction marks go.
    let text: String = raw
        .chars()
        .filter(|c| !direction_control(*c))
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(MAX_RUN_TEXT)
        .collect();
    if text.is_empty() {
        return None;
    }
    let size = text.chars().count();
    if budget.text + size > MAX_NOTE_TEXT {
        return None;
    }
    budget.text += size;
    Some(Run {
        text,
        b: flag(fields.get("b")),
        i: flag(fields.get("i")),
        code: flag(fields.get("code")),
        href: optional_link(fields.get("href"), link),
    })
}

fn runs(value: Option<&Value>, budget: &mut Budget) -> Vec<Run> {
    let Some(items) = value.and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for item in items.iter().take(MAX_RUNS) {
        if let Some(run) = item.as_object().and_then(|f| run(f, budget)) {
            out.push(run);
        }
    }
    out
}

fn blocks(value: Option<&Value>, depth: usize, budget: &mut Budget) -> Vec<Block> {
    let Some(items) = value.and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for item in items {
        if budget.full() {
            break;
        }
        if let Some(block) = item.as_object().and_then(|f| block(f, depth, budget)) {
            budget.blocks += 1;
            out.push(block);
        }
    }
    out
}

fn block(fields: &Map<String, Value>, depth: usize, budget: &mut Budget) -> Option<Block> {
    match fields.get("t")?.as_str()? {
        "h" => {
            let level = fields
                .get("level")?
                .as_u64()
                .and_then(|l| u8::try_from(l).ok())
                .filter(|l| (1..=4).contains(l))?;
            let c = runs(fields.get("c"), budget);
            (!c.is_empty()).then_some(Block::H { level, c })
        }
        "p" => {
            let c = runs(fields.get("c"), budget);
            (!c.is_empty()).then_some(Block::P { c })
        }
        kind @ ("ul" | "ol") => {
            if depth >= MAX_DEPTH {
                return None;
            }
            let mut items = Vec::new();
            for item in fields.get("items")?.as_array()? {
                if budget.full() {
                    break;
                }
                let inner = blocks(Some(item), depth + 1, budget);
                if !inner.is_empty() {
                    items.push(inner);
                }
            }
            if items.is_empty() {
                None
            } else if kind == "ul" {
                Some(Block::Ul { items })
            } else {
                Some(Block::Ol { items })
            }
        }
        "img" => Some(Block::Img {
            src: picture(fields.get("src"), PictureKind::Img)?,
            alt: text(fields.get("alt"), MAX_ALT).unwrap_or_default(),
        }),
        _ => None,
    }
}

/// Reads a whole note of `id` in `lang`; `None` when it is not that note.
pub fn note(value: &Value, id: &str, lang: Lang) -> Option<Note> {
    let fields = value.as_object()?;
    let card = note_card(fields, None).filter(|c| c.id == id)?;
    let mut budget = Budget { blocks: 0, text: 0 };
    Some(Note {
        blocks: blocks(fields.get("blocks"), 0, &mut budget),
        card,
        lang,
    })
}

/// Whether `bytes` look like a whole JPEG file: its start and end markers.
pub fn jpeg(bytes: &[u8]) -> bool {
    bytes.len() >= 128 && bytes.starts_with(&[0xff, 0xd8, 0xff]) && bytes.ends_with(&[0xff, 0xd9])
}
