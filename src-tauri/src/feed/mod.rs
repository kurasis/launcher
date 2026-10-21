//! The start page feed from feed.lapi.sh (D-067): patch notes, news, videos,
//! streams and our own events and tips.
//!
//! The launcher asks only our service, never minecraft.net, YouTube or Twitch,
//! until the player chooses to watch something. Every answer is read through
//! [`model`], which keeps only what passes its rules. The last good feed of
//! each language, the notes opened and the pictures shown are kept in
//! `cache/feed`, so the feed shows without a connection (marked as such). The
//! YouTube and Twitch channels a player adds stay on this computer, in
//! `feed-channels.json`, and are sent only to ask for their videos and live
//! streams. Nothing
//! about a player goes to the launcher's log.

pub mod ads;
pub mod model;
pub mod player;

#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime};

use serde::Serialize;
use serde_json::{Value, json};
use url::Url;

use crate::error::{ErrorCode, LauncherResult};
use crate::net::{PublicClient, SourcePolicy};
use crate::storage::atomic;
use crate::storage::models::{FeedBlock, FeedSettings, VideoLanguage};
use crate::storage::paths::DataRoot;
use crate::sync::lock;
use model::{
    ChannelMark, Feed, FeedItem, FeedPage, Lang, MarkKind, Note, OwnChannel, OwnStreamer,
    PictureKind, Stream, Video,
};

/// The service.
pub const BASE: &str = "https://feed.lapi.sh";
/// The protocol the webview loads the feed's pictures from.
pub const PROTOCOL: &str = "feed";
/// A player's own YouTube channels, at most (the service's limit), and as
/// many Twitch channels.
pub const MAX_OWN_CHANNELS: usize = 30;
/// The longest channel link, @handle or id a player may type.
pub const MAX_CHANNEL_INPUT: usize = 200;

/// A feed fetched this recently is not fetched again (the service's own
/// cache time).
const FEED_FRESH: Duration = Duration::from_secs(60);
/// A note opened this recently is shown from disk.
const NOTE_FRESH: Duration = Duration::from_secs(10 * 60);
/// A player's videos fetched this recently are not fetched again.
const OWN_FRESH: Duration = Duration::from_secs(5 * 60);
/// A player's live streams fetched this recently are not fetched again (the
/// service's own cache time).
const OWN_STREAMS_FRESH: Duration = Duration::from_secs(60);
/// Notes kept on disk; the oldest go first.
const MAX_NOTE_FILES: usize = 60;
/// The largest picture read.
const MAX_PICTURE_BYTES: usize = 2 * 1024 * 1024;
/// Pictures kept on disk, in all; the oldest go first down to `PICTURES_KEPT`.
const MAX_PICTURES_TOTAL: u64 = 64 * 1024 * 1024;
const PICTURES_KEPT: u64 = 48 * 1024 * 1024;
/// The largest copy on disk read back (a feed or note with its wrapper).
const MAX_FILE_BYTES: u64 = 2 * model::MAX_NOTE_BYTES as u64;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);

/// What went wrong, for the interface to say in the player's language.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FeedProblem {
    /// The feed is switched off in Settings.
    Disabled,
    /// feed.lapi.sh could not be reached.
    Offline,
    /// It answered with something the launcher cannot use.
    Server,
    NotFound,
    /// Not a channel link, @handle or channel id.
    BadInput,
    /// Too many lookups for now.
    TooMany,
    /// The list of own channels is full.
    Full,
    /// No data folder to keep anything in.
    Unavailable,
}

pub type FeedResult<T> = Result<T, FeedProblem>;

/// The feed as the interface shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedView {
    pub feed: Feed,
    /// A copy kept on disk is shown: the service did not answer.
    pub stale: bool,
    /// Why it is stale.
    pub problem: Option<FeedProblem>,
}

/// A note as the interface shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteView {
    pub note: Note,
    pub stale: bool,
}

/// The languages the parts of the feed are read in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Languages {
    /// Events, announcements and the tip.
    pub interface: Lang,
    /// Patch notes and news: the interface's, or English without machine
    /// translation.
    pub notes: Lang,
    /// Videos and streams.
    pub videos: Lang,
}

impl Languages {
    pub fn of(interface_tag: &str, settings: &FeedSettings) -> Self {
        let interface = Lang::of_interface(interface_tag);
        Self {
            interface,
            notes: if settings.machine_translation {
                interface
            } else {
                Lang::En
            },
            videos: match settings.video_language {
                VideoLanguage::Ui => interface,
                VideoLanguage::En => Lang::En,
                VideoLanguage::Ru => Lang::Ru,
                VideoLanguage::Ar => Lang::Ar,
            },
        }
    }
}

/// The feed of the interface language with its patch notes and news from
/// `notes` and its videos and streams from `videos`, when given.
pub fn compose(mut base: Feed, notes: Option<Feed>, videos: Option<Feed>) -> Feed {
    if let Some(notes) = notes {
        base.versions = notes.versions;
        base.news = notes.news;
    }
    if let Some(videos) = videos {
        base.videos = videos.videos;
        base.streams = videos.streams;
    }
    base
}

/// The parts as the service names them, comma separated.
fn kind_list(kinds: &[FeedBlock]) -> String {
    kinds
        .iter()
        .map(|k| match k {
            FeedBlock::Versions => "versions",
            FeedBlock::News => "news",
            FeedBlock::Videos => "videos",
            FeedBlock::Streams => "streams",
        })
        .collect::<Vec<_>>()
        .join(",")
}

/// Parses a `feed:` request path: `/img/<sha>.jpg`, `/yt/<id>.jpg` or
/// `/tw/<login>.jpg`, nothing else.
pub fn parse_picture_request(path: &str) -> Option<(PictureKind, String)> {
    let (kind, name) = path.strip_prefix('/')?.split_once('/')?;
    let kind = PictureKind::parse(kind)?;
    model::picture_name(kind, name).then(|| (kind, name.to_owned()))
}

/// How long a picture of `kind` is shown from disk before it is fetched again.
fn picture_fresh(kind: PictureKind) -> Duration {
    match kind {
        // Named by their content: never change.
        PictureKind::Img => Duration::from_secs(365 * 24 * 3600),
        PictureKind::Yt => Duration::from_secs(24 * 3600),
        PictureKind::Tw => Duration::from_secs(5 * 60),
    }
}

/// Checks a channel link, @handle or id a player typed, before anything is
/// sent: the service reads it the same way.
pub fn channel_input(input: &str) -> Option<&str> {
    let input = input.trim();
    (!input.is_empty() && input.len() <= MAX_CHANNEL_INPUT && !input.chars().any(char::is_control))
        .then_some(input)
}

fn age(path: &Path) -> Option<Duration> {
    let modified = fs::metadata(path).ok()?.modified().ok()?;
    Some(
        SystemTime::now()
            .duration_since(modified)
            .unwrap_or_default(),
    )
}

/// A small file read back whole, if it is not too big.
fn read_small(path: &Path, max: u64) -> Option<Vec<u8>> {
    let meta = fs::symlink_metadata(path).ok()?;
    if !meta.is_file() || meta.len() > max {
        return None;
    }
    fs::read(path).ok()
}

fn read_json(path: &Path) -> Option<Value> {
    serde_json::from_slice(&read_small(path, MAX_FILE_BYTES)?).ok()
}

/// Writes a copy; failing to keep it only costs a request later.
fn keep(path: &Path, bytes: &[u8]) {
    if let Some(dir) = path.parent()
        && fs::create_dir_all(dir).is_ok()
    {
        let _ = atomic::write(path, bytes);
    }
}

/// The files of `dir` that are not unfinished writes, oldest first, with
/// their sizes.
fn files_by_age(dir: &Path) -> Vec<(PathBuf, SystemTime, u64)> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<(PathBuf, SystemTime, u64)> = entries
        .filter_map(Result::ok)
        .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
        .filter_map(|e| {
            let meta = e.metadata().ok()?;
            meta.is_file().then(|| {
                (
                    e.path(),
                    meta.modified().unwrap_or(SystemTime::UNIX_EPOCH),
                    meta.len(),
                )
            })
        })
        .collect();
    files.sort_by_key(|(_, modified, _)| *modified);
    files
}

/// Keeps at most `max_files` files and `max_bytes` bytes in `dir` (down to
/// `keep_bytes` once over), removing the oldest.
fn trim_dir(dir: &Path, max_files: usize, max_bytes: u64, keep_bytes: u64) {
    let files = files_by_age(dir);
    let mut total: u64 = files.iter().map(|(_, _, size)| size).sum();
    let mut count = files.len();
    let over_bytes = total > max_bytes;
    for (path, _, size) in files {
        let too_many = count > max_files;
        let too_big = over_bytes && total > keep_bytes;
        if !too_many && !too_big {
            break;
        }
        if fs::remove_file(&path).is_ok() {
            total = total.saturating_sub(size);
            count -= 1;
        }
    }
}

fn feed_file(root: &DataRoot, lang: Lang) -> PathBuf {
    root.feed_cache_dir()
        .join(format!("feed-{}.json", lang.as_str()))
}

fn note_file(root: &DataRoot, id: &str, lang: Lang) -> PathBuf {
    root.feed_cache_dir()
        .join("notes")
        .join(format!("{id}.{}.json", lang.as_str()))
}

fn picture_file(root: &DataRoot, kind: PictureKind, name: &str) -> PathBuf {
    root.feed_cache_dir()
        .join("pictures")
        .join(format!("{}-{name}", kind.as_str()))
}

fn own_videos_file(root: &DataRoot) -> PathBuf {
    root.feed_cache_dir().join("own-videos.json")
}

/// The feed kept on disk for `lang`.
pub fn cached_feed(root: &DataRoot, lang: Lang) -> Option<Feed> {
    model::feed(read_json(&feed_file(root, lang))?.get("feed")?, lang)
}

/// The note kept on disk.
pub fn cached_note(root: &DataRoot, id: &str, lang: Lang) -> Option<Note> {
    if !model::note_id(id) {
        return None;
    }
    model::note(
        read_json(&note_file(root, id, lang))?.get("note")?,
        id,
        lang,
    )
}

/// The most channels the player can hide.
pub const MAX_HIDDEN: usize = 200;
/// The largest `feed-channels.json` read.
const MAX_CHANNELS_FILE: u64 = 128 * 1024;
/// The lists `feed-channels.json` holds; anything else in it is dropped.
const CHANNEL_LISTS: [&str; 4] = ["channels", "twitch", "hidden", "favorite"];

fn channels_file(root: &DataRoot) -> Option<Value> {
    read_small(&root.feed_channels(), MAX_CHANNELS_FILE)
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
}

/// One list kept in `feed-channels.json` under `key`: checked, each once, at
/// most `max`.
fn own_list<T>(
    root: &DataRoot,
    key: &str,
    max: usize,
    item: impl Fn(&Value) -> Option<T>,
    same: impl Fn(&T, &T) -> bool,
) -> Vec<T> {
    let Some(value) = channels_file(root) else {
        return Vec::new();
    };
    let mut out: Vec<T> = Vec::new();
    for found in value
        .get(key)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(item)
    {
        if out.len() == max {
            break;
        }
        if !out.iter().any(|kept| same(kept, &found)) {
            out.push(found);
        }
    }
    out
}

/// The player's own YouTube channels, as kept.
pub fn own_channels(root: &DataRoot) -> Vec<OwnChannel> {
    own_list(
        root,
        "channels",
        MAX_OWN_CHANNELS,
        model::own_channel,
        |a, b| a.channel_id == b.channel_id,
    )
}

/// The player's own Twitch channels, as kept.
pub fn own_streamers(root: &DataRoot) -> Vec<OwnStreamer> {
    own_list(
        root,
        "twitch",
        MAX_OWN_CHANNELS,
        model::own_streamer,
        |a, b| a.login == b.login,
    )
}

/// Writes one list of `feed-channels.json`, keeping the others.
fn save_list(root: &DataRoot, key: &str, list: Value) -> FeedResult<()> {
    let kept = channels_file(root);
    let mut all = serde_json::Map::new();
    for name in CHANNEL_LISTS {
        if name == key {
            all.insert(name.to_owned(), list.clone());
        } else if let Some(value) = kept.as_ref().and_then(|k| k.get(name)) {
            all.insert(name.to_owned(), value.clone());
        }
    }
    let bytes = serde_json::to_vec_pretty(&Value::Object(all)).map_err(|_| FeedProblem::Server)?;
    atomic::write(&root.feed_channels(), &bytes).map_err(|_| FeedProblem::Unavailable)
}

fn save_own_channels(root: &DataRoot, channels: &[OwnChannel]) -> FeedResult<()> {
    save_list(root, "channels", json!(channels))
}

fn save_own_streamers(root: &DataRoot, streamers: &[OwnStreamer]) -> FeedResult<()> {
    save_list(root, "twitch", json!(streamers))
}

/// The channels the player hid from the feed and the ones they put first.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct FeedMarks {
    pub hidden: Vec<ChannelMark>,
    pub favorite: Vec<ChannelMark>,
}

/// Which list a mark puts a channel on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkList {
    Hidden,
    Favorite,
}

impl MarkList {
    pub fn parse(list: &str) -> Option<Self> {
        match list {
            "hidden" => Some(Self::Hidden),
            "favorite" => Some(Self::Favorite),
            _ => None,
        }
    }
}

fn same_channel(a: &ChannelMark, b: &ChannelMark) -> bool {
    a.kind == b.kind && a.id == b.id
}

/// The marks as kept: at most [`MAX_HIDDEN`] hidden and
/// [`MAX_OWN_CHANNELS`] favourites of each place.
pub fn marks(root: &DataRoot) -> FeedMarks {
    let hidden = own_list(
        root,
        "hidden",
        MAX_HIDDEN,
        model::channel_mark,
        same_channel,
    );
    let kept = own_list(
        root,
        "favorite",
        MAX_HIDDEN,
        model::channel_mark,
        same_channel,
    );
    let mut favorite: Vec<ChannelMark> = Vec::new();
    for kind in [MarkKind::Youtube, MarkKind::Twitch] {
        favorite.extend(
            kept.iter()
                .filter(|m| m.kind == kind)
                .take(MAX_OWN_CHANNELS)
                .cloned(),
        );
    }
    FeedMarks { hidden, favorite }
}

/// Puts a channel on a list, taking it off the other one, or (`None`) takes
/// it off both; the marks after.
pub fn mark(
    root: &DataRoot,
    channel: ChannelMark,
    list: Option<MarkList>,
) -> FeedResult<FeedMarks> {
    let FeedMarks {
        mut hidden,
        mut favorite,
    } = marks(root);
    hidden.retain(|m| !same_channel(m, &channel));
    favorite.retain(|m| !same_channel(m, &channel));
    match list {
        Some(MarkList::Hidden) => {
            if hidden.len() >= MAX_HIDDEN {
                return Err(FeedProblem::Full);
            }
            hidden.push(channel);
        }
        Some(MarkList::Favorite) => {
            if favorite.iter().filter(|m| m.kind == channel.kind).count() >= MAX_OWN_CHANNELS {
                return Err(FeedProblem::Full);
            }
            favorite.push(channel);
        }
        None => {}
    }
    save_list(root, "hidden", json!(hidden))?;
    save_list(root, "favorite", json!(favorite))?;
    Ok(FeedMarks { hidden, favorite })
}

/// Removes one of the player's Twitch channels; the list after.
pub fn remove_own_streamer(root: &DataRoot, login: &str) -> FeedResult<Vec<OwnStreamer>> {
    let mut streamers = own_streamers(root);
    let before = streamers.len();
    streamers.retain(|s| s.login != login);
    if streamers.len() != before {
        save_own_streamers(root, &streamers)?;
    }
    Ok(streamers)
}

/// Removes one of the player's channels; the list after.
pub fn remove_own_channel(root: &DataRoot, id: &str) -> FeedResult<Vec<OwnChannel>> {
    let mut channels = own_channels(root);
    let before = channels.len();
    channels.retain(|c| c.channel_id != id);
    if channels.len() != before {
        save_own_channels(root, &channels)?;
    }
    Ok(channels)
}

/// The answer to a request: its status and the JSON it carried, if any.
struct Answer {
    status: u16,
    body: Option<Value>,
}

fn problem_of(answer: &Answer) -> FeedProblem {
    match answer.status {
        404 => FeedProblem::NotFound,
        429 => FeedProblem::TooMany,
        400 if answer
            .body
            .as_ref()
            .and_then(|b| b.get("error"))
            .and_then(Value::as_str)
            == Some("bad_input") =>
        {
            FeedProblem::BadInput
        }
        _ => FeedProblem::Server,
    }
}

/// A player's videos fetched at a time, for these channels.
struct OwnVideos {
    at: Instant,
    channels: Vec<String>,
    videos: Vec<Video>,
}

/// A player's live streams fetched at a time, for these channels.
struct OwnStreams {
    at: Instant,
    logins: Vec<String>,
    streams: Vec<Stream>,
}

/// What the favourite channels showed at a time, for these marks.
struct Favorites {
    at: Instant,
    marks: Vec<ChannelMark>,
    found: FavoriteFeed,
}

/// The newest videos and the live streams of the favourite channels.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct FavoriteFeed {
    pub videos: Vec<Video>,
    pub streams: Vec<Stream>,
}

/// The feed's client and what it keeps in memory.
pub struct FeedService {
    client: PublicClient,
    /// The feeds fetched lately, by language.
    fresh: Mutex<HashMap<Lang, (Instant, Feed)>>,
    /// The player's videos fetched lately, for these channels.
    own: Mutex<Option<OwnVideos>>,
    /// The player's live streams fetched lately, for these channels.
    own_streams: Mutex<Option<OwnStreams>>,
    /// The favourite channels' videos and streams fetched lately.
    favorites: Mutex<Option<Favorites>>,
}

impl FeedService {
    pub fn new(app_version: &str) -> LauncherResult<Self> {
        Ok(Self {
            client: PublicClient::new(SourcePolicy::Feed, app_version)?,
            fresh: Mutex::new(HashMap::new()),
            own: Mutex::new(None),
            own_streams: Mutex::new(None),
            favorites: Mutex::new(None),
        })
    }

    async fn send(&self, request: reqwest::RequestBuilder, max: usize) -> FeedResult<Answer> {
        let request = request
            .header(reqwest::header::ACCEPT, "application/json")
            .timeout(REQUEST_TIMEOUT);
        let (status, _, body) = self.client.get_bounded(request, max).await.map_err(|e| {
            if e.code == ErrorCode::NetworkError {
                FeedProblem::Offline
            } else {
                FeedProblem::Server
            }
        })?;
        Ok(Answer {
            status: status.as_u16(),
            body: serde_json::from_slice(&body).ok(),
        })
    }

    async fn get(&self, path: &str, max: usize) -> FeedResult<Answer> {
        let url = Url::parse(&format!("{BASE}{path}")).map_err(|_| FeedProblem::Server)?;
        let request = self.client.get(&url).map_err(|_| FeedProblem::Server)?;
        self.send(request, max).await
    }

    /// A 200 answer's JSON, or what went wrong.
    async fn get_ok(&self, path: &str, max: usize) -> FeedResult<Value> {
        let answer = self.get(path, max).await?;
        match (answer.status, answer.body) {
            (200, Some(body)) => Ok(body),
            (status, body) => Err(problem_of(&Answer { status, body })),
        }
    }

    /// The feed of one language: fetched, or the copy on disk with why.
    pub async fn feed(
        &self,
        root: Option<&DataRoot>,
        lang: Lang,
        refresh: bool,
    ) -> FeedResult<(Feed, Option<FeedProblem>)> {
        if !refresh
            && let Some((at, feed)) = lock(&self.fresh).get(&lang)
            && at.elapsed() < FEED_FRESH
        {
            return Ok((feed.clone(), None));
        }
        let fetched = self
            .get_ok(
                &format!("/v1/feed?lang={}", lang.as_str()),
                model::MAX_FEED_BYTES,
            )
            .await
            .and_then(|value| model::feed(&value, lang).ok_or(FeedProblem::Server));
        match fetched {
            Ok(feed) => {
                lock(&self.fresh).insert(lang, (Instant::now(), feed.clone()));
                if let Some(root) = root
                    && let Ok(bytes) = serde_json::to_vec(&json!({ "feed": feed }))
                {
                    keep(&feed_file(root, lang), &bytes);
                }
                Ok((feed, None))
            }
            Err(problem) => match root.and_then(|root| cached_feed(root, lang)) {
                Some(feed) => Ok((feed, Some(problem))),
                None => Err(problem),
            },
        }
    }

    /// The feed the interface shows, in the languages the settings ask for.
    pub async fn view(
        &self,
        root: Option<&DataRoot>,
        languages: Languages,
        refresh: bool,
    ) -> FeedResult<FeedView> {
        let (base, mut problem) = self.feed(root, languages.interface, refresh).await?;
        let other = |lang: Lang| async move {
            if lang == languages.interface {
                return (None, None);
            }
            match self.feed(root, lang, refresh).await {
                Ok((feed, problem)) => (Some(feed), problem),
                Err(problem) => (None, Some(problem)),
            }
        };
        let (notes, notes_problem) = other(languages.notes).await;
        let (videos, videos_problem) = other(languages.videos).await;
        problem = problem.or(notes_problem).or(videos_problem);
        Ok(FeedView {
            feed: compose(base, notes, videos),
            stale: problem.is_some(),
            problem,
        })
    }

    /// A page of the endless list: `filter` one part or all, as the settings
    /// show them, or with `shorts_only` the Shorts alone (when the settings
    /// show Shorts); `before` the cursor of the page before. Pages are not
    /// kept: without the service the interface shows the feed kept on disk.
    pub async fn page(
        &self,
        languages: Languages,
        settings: &FeedSettings,
        filter: Option<FeedBlock>,
        shorts_only: bool,
        before: Option<&str>,
    ) -> FeedResult<FeedPage> {
        if before.is_some_and(|b| !model::cursor(b)) {
            return Err(FeedProblem::NotFound);
        }
        let filter = if shorts_only {
            Some(FeedBlock::Videos)
        } else {
            filter
        };
        let kinds: Vec<FeedBlock> = FeedBlock::ALL
            .into_iter()
            .filter(|k| filter.is_none_or(|f| f == *k) && !settings.hidden.contains(k))
            .filter(|_| !shorts_only || settings.shorts)
            .collect();
        if kinds.is_empty() {
            return Ok(FeedPage {
                items: Vec::new(),
                next: None,
            });
        }
        let mut url = Url::parse(&format!("{BASE}/v1/page")).map_err(|_| FeedProblem::Server)?;
        {
            let mut query = url.query_pairs_mut();
            query
                .append_pair("notes", languages.notes.as_str())
                .append_pair("videos", languages.videos.as_str())
                .append_pair("kinds", &kind_list(&kinds))
                .append_pair("snapshots", if settings.snapshots { "1" } else { "0" })
                .append_pair(
                    "shorts",
                    match (shorts_only, settings.shorts) {
                        (true, _) => "only",
                        (false, true) => "1",
                        (false, false) => "0",
                    },
                );
            if let Some(before) = before {
                query.append_pair("before", before);
            }
        }
        let request = self.client.get(&url).map_err(|_| FeedProblem::Server)?;
        let answer = self.send(request, model::MAX_FEED_BYTES).await?;
        match (answer.status, answer.body) {
            (200, Some(body)) => {
                let mut page = model::page(&body, &kinds).ok_or(FeedProblem::Server)?;
                if shorts_only {
                    page.items
                        .retain(|item| matches!(item, FeedItem::Video { video } if video.short));
                }
                Ok(page)
            }
            (status, body) => Err(problem_of(&Answer { status, body })),
        }
    }

    /// A whole note: from disk when opened lately, else fetched (the copy
    /// on disk when that fails).
    pub async fn note(
        &self,
        root: Option<&DataRoot>,
        id: &str,
        lang: Lang,
    ) -> FeedResult<NoteView> {
        if !model::note_id(id) {
            return Err(FeedProblem::NotFound);
        }
        let file = root.map(|root| note_file(root, id, lang));
        let cached = || root.and_then(|root| cached_note(root, id, lang));
        if file
            .as_deref()
            .and_then(age)
            .is_some_and(|age| age < NOTE_FRESH)
            && let Some(note) = cached()
        {
            return Ok(NoteView { note, stale: false });
        }
        let fetched = self
            .get_ok(
                &format!("/v1/notes/{id}?lang={}", lang.as_str()),
                model::MAX_NOTE_BYTES,
            )
            .await
            .and_then(|value| model::note(&value, id, lang).ok_or(FeedProblem::Server));
        match fetched {
            Ok(note) => {
                if let Some(file) = &file
                    && let Ok(bytes) = serde_json::to_vec(&json!({ "note": note }))
                {
                    keep(file, &bytes);
                    if let Some(dir) = file.parent() {
                        trim_dir(dir, MAX_NOTE_FILES, u64::MAX, u64::MAX);
                    }
                }
                Ok(NoteView { note, stale: false })
            }
            Err(FeedProblem::NotFound) => Err(FeedProblem::NotFound),
            Err(problem) => cached()
                .map(|note| NoteView { note, stale: true })
                .ok_or(problem),
        }
    }

    /// Looks a channel up and adds it to the player's list; the list after.
    pub async fn add_channel(&self, root: &DataRoot, input: &str) -> FeedResult<Vec<OwnChannel>> {
        let input = channel_input(input).ok_or(FeedProblem::BadInput)?;
        let mut channels = own_channels(root);
        if channels.len() >= MAX_OWN_CHANNELS {
            return Err(FeedProblem::Full);
        }
        let url =
            Url::parse(&format!("{BASE}/v1/channels/resolve")).map_err(|_| FeedProblem::Server)?;
        let request = self
            .client
            .post(&url)
            .map_err(|_| FeedProblem::Server)?
            .json(&json!({ "input": input }));
        let answer = self.send(request, model::MAX_SMALL_BYTES).await?;
        if answer.status != 200 {
            return Err(problem_of(&answer));
        }
        let found = answer
            .body
            .as_ref()
            .and_then(model::own_channel)
            .ok_or(FeedProblem::Server)?;
        if !channels.iter().any(|c| c.channel_id == found.channel_id) {
            channels.push(found);
            save_own_channels(root, &channels)?;
            *lock(&self.own) = None;
        }
        Ok(channels)
    }

    /// Looks a Twitch channel up by its link or login and adds it; the list
    /// after.
    pub async fn add_streamer(&self, root: &DataRoot, input: &str) -> FeedResult<Vec<OwnStreamer>> {
        let input = channel_input(input).ok_or(FeedProblem::BadInput)?;
        let mut streamers = own_streamers(root);
        if streamers.len() >= MAX_OWN_CHANNELS {
            return Err(FeedProblem::Full);
        }
        let url =
            Url::parse(&format!("{BASE}/v1/channels/resolve")).map_err(|_| FeedProblem::Server)?;
        let request = self
            .client
            .post(&url)
            .map_err(|_| FeedProblem::Server)?
            .json(&json!({ "input": input, "kind": "twitch" }));
        let answer = self.send(request, model::MAX_SMALL_BYTES).await?;
        if answer.status != 200 {
            return Err(problem_of(&answer));
        }
        let found = answer
            .body
            .as_ref()
            .and_then(model::own_streamer)
            .ok_or(FeedProblem::Server)?;
        if !streamers.iter().any(|s| s.login == found.login) {
            streamers.push(found);
            save_own_streamers(root, &streamers)?;
            *lock(&self.own_streams) = None;
        }
        Ok(streamers)
    }

    /// The live streams of the player's own Twitch channels. Nothing is kept
    /// on disk: an old stream is no longer live.
    pub async fn own_streams(&self, root: &DataRoot) -> FeedResult<Vec<Stream>> {
        let logins: Vec<String> = own_streamers(root).into_iter().map(|s| s.login).collect();
        if logins.is_empty() {
            return Ok(Vec::new());
        }
        if let Some(own) = lock(&self.own_streams).as_ref()
            && own.logins == logins
            && own.at.elapsed() < OWN_STREAMS_FRESH
        {
            return Ok(own.streams.clone());
        }
        let streams = self
            .get_ok(
                &format!("/v1/streams?logins={}", logins.join(",")),
                model::MAX_SMALL_BYTES,
            )
            .await
            .and_then(|value| model::streams(&value).ok_or(FeedProblem::Server))?;
        *lock(&self.own_streams) = Some(OwnStreams {
            at: Instant::now(),
            logins,
            streams: streams.clone(),
        });
        Ok(streams)
    }

    /// The newest videos and the live streams of the favourite channels,
    /// shown before everything else. A part that cannot be fetched is left
    /// empty; both failing is a problem.
    pub async fn favorites(&self, root: &DataRoot) -> FeedResult<FavoriteFeed> {
        let marks = marks(root).favorite;
        if marks.is_empty() {
            return Ok(FavoriteFeed::default());
        }
        if let Some(kept) = lock(&self.favorites).as_ref()
            && kept.marks == marks
            && kept.at.elapsed() < OWN_STREAMS_FRESH
        {
            return Ok(kept.found.clone());
        }
        let ids = |kind: MarkKind| -> Vec<&str> {
            marks
                .iter()
                .filter(|m| m.kind == kind)
                .map(|m| m.id.as_str())
                .collect()
        };
        let (channels, logins) = (ids(MarkKind::Youtube), ids(MarkKind::Twitch));
        let videos = if channels.is_empty() {
            Ok(Vec::new())
        } else {
            self.get_ok(
                &format!("/v1/videos?channels={}", channels.join(",")),
                model::MAX_SMALL_BYTES,
            )
            .await
            .and_then(|value| model::videos(&value).ok_or(FeedProblem::Server))
        };
        let streams = if logins.is_empty() {
            Ok(Vec::new())
        } else {
            self.get_ok(
                &format!("/v1/streams?logins={}", logins.join(",")),
                model::MAX_SMALL_BYTES,
            )
            .await
            .and_then(|value| model::streams(&value).ok_or(FeedProblem::Server))
        };
        let found = match (videos, streams) {
            (Err(problem), Err(_)) => return Err(problem),
            (videos, streams) => FavoriteFeed {
                videos: videos.unwrap_or_default(),
                streams: streams.unwrap_or_default(),
            },
        };
        *lock(&self.favorites) = Some(Favorites {
            at: Instant::now(),
            marks,
            found: found.clone(),
        });
        Ok(found)
    }

    /// The newest videos of the player's own channels.
    pub async fn own_videos(&self, root: &DataRoot) -> FeedResult<Vec<Video>> {
        let ids: Vec<String> = own_channels(root)
            .into_iter()
            .map(|c| c.channel_id)
            .collect();
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        if let Some(own) = lock(&self.own).as_ref()
            && own.channels == ids
            && own.at.elapsed() < OWN_FRESH
        {
            return Ok(own.videos.clone());
        }
        let fetched = self
            .get_ok(
                &format!("/v1/videos?channels={}", ids.join(",")),
                model::MAX_SMALL_BYTES,
            )
            .await
            .and_then(|value| model::videos(&value).ok_or(FeedProblem::Server));
        let file = own_videos_file(root);
        match fetched {
            Ok(videos) => {
                *lock(&self.own) = Some(OwnVideos {
                    at: Instant::now(),
                    channels: ids.clone(),
                    videos: videos.clone(),
                });
                if let Ok(bytes) = serde_json::to_vec(&json!({ "channels": ids, "videos": videos }))
                {
                    keep(&file, &bytes);
                }
                Ok(videos)
            }
            Err(problem) => {
                let kept = read_json(&file).filter(|kept| {
                    kept.get("channels")
                        .and_then(Value::as_array)
                        .is_some_and(|kept| {
                            kept.iter()
                                .map(Value::as_str)
                                .eq(ids.iter().map(|i| Some(i.as_str())))
                        })
                });
                kept.as_ref().and_then(model::videos).ok_or(problem)
            }
        }
    }

    /// A picture of the feed as JPEG bytes: from disk while fresh, else
    /// fetched and kept (the copy on disk when that fails).
    pub async fn picture(&self, root: &DataRoot, kind: PictureKind, name: &str) -> Option<Vec<u8>> {
        if !model::picture_name(kind, name) {
            return None;
        }
        let file = picture_file(root, kind, name);
        let kept = || read_small(&file, MAX_PICTURE_BYTES as u64).filter(|b| model::jpeg(b));
        if age(&file).is_some_and(|age| age < picture_fresh(kind))
            && let Some(bytes) = kept()
        {
            return Some(bytes);
        }
        match self.picture_bytes(kind, name).await {
            Some(bytes) => {
                keep(&file, &bytes);
                if let Some(dir) = file.parent() {
                    trim_dir(dir, usize::MAX, MAX_PICTURES_TOTAL, PICTURES_KEPT);
                }
                Some(bytes)
            }
            None => kept(),
        }
    }

    async fn picture_bytes(&self, kind: PictureKind, name: &str) -> Option<Vec<u8>> {
        let url = Url::parse(&format!("{BASE}/v1/{}/{name}", kind.as_str())).ok()?;
        let request = self
            .client
            .get(&url)
            .ok()?
            .header(reqwest::header::ACCEPT, "image/jpeg")
            .timeout(REQUEST_TIMEOUT);
        let (status, headers, bytes) = self
            .client
            .get_bounded(request, MAX_PICTURE_BYTES)
            .await
            .ok()?;
        let jpeg_type = headers
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.trim().eq_ignore_ascii_case("image/jpeg"));
        (status.as_u16() == 200 && jpeg_type && model::jpeg(&bytes)).then_some(bytes)
    }
}
