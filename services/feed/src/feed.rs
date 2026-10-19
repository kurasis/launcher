//! What the service knows, how it keeps it and how the feed is made from it.
//!
//! Patch notes and news live in `data/notes/<id>.json`, their translations
//! in `data/notes/<id>.<lang>.json`; lists, videos and streams in
//! `data/state.json`. Everything is fetched again from the sources, so a
//! lost folder only means a slow first minute.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, RwLock};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::deepl::{self, DeepL};
use crate::editorial::{Editorial, LANGS};
use crate::html::{self, Block};
use crate::images::{self, Images};
use crate::mojang::{self, Note, Version};
use crate::twitch::{Stream, Twitch};
use crate::youtube::{self, Channel, Video};

/// Feed format; the launcher drops what it does not know.
pub const FORMAT: u32 = 1;
/// Videos in a feed and in one answer for a player's own channels.
pub const VIDEOS: usize = 40;
/// Channels of players kept in memory at once.
pub const OWN_CHANNELS: usize = 2000;
/// A channel's videos are fetched again after this.
pub const CHANNEL_TTL: Duration = Duration::from_secs(15 * 60);
/// Patch notes and news translated: the newest ones only.
const TRANSLATED: usize = 30;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Saved {
    #[serde(default)]
    pub versions: Vec<Version>,
    #[serde(default)]
    pub patch_notes: Vec<String>,
    #[serde(default)]
    pub news: Vec<String>,
    /// Our channels' videos, by channel id.
    #[serde(default)]
    pub channels: HashMap<String, Channel>,
    /// Streams by language.
    #[serde(default)]
    pub streams: HashMap<String, Vec<Stream>>,
    /// When each source last answered: mojang, youtube, twitch.
    #[serde(default)]
    pub fetched: HashMap<String, String>,
}

/// A note's translation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Translated {
    pub title: String,
    pub summary: String,
    pub blocks: Vec<Block>,
}

struct OwnChannel {
    channel: Channel,
    fetched: Instant,
    used: Instant,
}

pub struct Feed {
    dir: PathBuf,
    pub editorial: Editorial,
    pub images: Images,
    pub http: reqwest::Client,
    saved: RwLock<Saved>,
    notes: RwLock<HashMap<String, Note>>,
    translations: RwLock<HashMap<(String, String), Translated>>,
    own: Mutex<HashMap<String, OwnChannel>>,
    /// Videos whose preview may be fetched: those of any channel we know.
    videos: RwLock<HashSet<String>>,
    pub twitch: tokio::sync::Mutex<Option<Twitch>>,
    pub deepl: Option<DeepL>,
}

pub fn now_text() -> String {
    chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, serde_json::to_vec(value).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Option<T> {
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

fn lock<T>(lock: &RwLock<T>) -> std::sync::RwLockReadGuard<'_, T> {
    lock.read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn lock_mut<T>(lock: &RwLock<T>) -> std::sync::RwLockWriteGuard<'_, T> {
    lock.write()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

pub fn lang(lang: Option<&str>) -> &'static str {
    match lang {
        Some("ru") => "ru",
        Some("ar") => "ar",
        _ => "en",
    }
}

impl Feed {
    pub fn open(
        dir: &Path,
        editorial: Editorial,
        http: reqwest::Client,
        twitch: Option<Twitch>,
        deepl: Option<DeepL>,
    ) -> Result<Self, String> {
        std::fs::create_dir_all(dir.join("notes")).map_err(|e| e.to_string())?;
        let saved: Saved = read_json(&dir.join("state.json")).unwrap_or_default();
        let mut notes = HashMap::new();
        let mut translations = HashMap::new();
        for id in saved.patch_notes.iter().chain(&saved.news) {
            if !mojang::note_id(id) {
                continue;
            }
            if let Some(note) = read_json::<Note>(&dir.join("notes").join(format!("{id}.json"))) {
                notes.insert(id.clone(), note);
            }
            for lang in ["ru", "ar"] {
                let path = dir.join("notes").join(format!("{id}.{lang}.json"));
                if let Some(t) = read_json::<Translated>(&path) {
                    translations.insert((id.clone(), lang.to_owned()), t);
                }
            }
        }
        let videos = saved
            .channels
            .values()
            .flat_map(|c| c.videos.iter().map(|v| v.id.clone()))
            .collect();
        Ok(Self {
            images: Images::new(dir, http.clone())?,
            dir: dir.to_owned(),
            editorial,
            http,
            saved: RwLock::new(saved),
            notes: RwLock::new(notes),
            translations: RwLock::new(translations),
            own: Mutex::new(HashMap::new()),
            videos: RwLock::new(videos),
            twitch: tokio::sync::Mutex::new(twitch),
            deepl,
        })
    }

    fn save(&self) {
        let saved = lock(&self.saved).clone();
        if let Err(e) = write_json(&self.dir.join("state.json"), &saved) {
            eprintln!("saving the state failed: {e}");
        }
    }

    fn mark(&self, source: &str) {
        lock_mut(&self.saved)
            .fetched
            .insert(source.to_owned(), now_text());
    }

    /// Whether a note is fetched (its body is never fetched again).
    pub fn has_note(&self, id: &str) -> bool {
        lock(&self.notes).contains_key(id)
    }

    /// Stores a new note with its pictures fetched.
    async fn add_note(&self, found: mojang::Found) {
        let mut note = found.note;
        if let Some(url) = found.image_url {
            note.image = self.images.article(&url, images::WIDE).await.ok();
        }
        let mut wanted = Vec::new();
        html::images(&note.blocks, &mut wanted);
        let mut stored = HashMap::new();
        for url in wanted.into_iter().take(40) {
            if let Ok(name) = self.images.article(&url, images::WIDE).await {
                stored.insert(url, name);
            }
        }
        note.blocks = html::replace_images(note.blocks, &|src| stored.get(src).cloned());
        let path = self.dir.join("notes").join(format!("{}.json", note.id));
        if let Err(e) = write_json(&path, &note) {
            eprintln!("saving a note failed: {e}");
        }
        lock_mut(&self.notes).insert(note.id.clone(), note);
    }

    /// Mojang's versions, patch notes and news.
    pub async fn refresh_mojang(&self) -> Result<(), String> {
        let versions = mojang::versions(&self.http).await?;
        let known = |id: &str| self.has_note(id);
        let (patch_order, patch_found) = mojang::patch_notes(&self.http, &known).await?;
        for found in patch_found {
            self.add_note(found).await;
        }
        let (news_order, news_found) = mojang::news(&self.http, &known).await?;
        for found in news_found {
            self.add_note(found).await;
        }
        {
            let mut saved = lock_mut(&self.saved);
            saved.versions = versions;
            saved.patch_notes = patch_order;
            saved.news = news_order;
        }
        self.mark("mojang");
        self.forget_old_notes();
        self.save();
        Ok(())
    }

    /// Notes no list names any more leave memory and disk.
    fn forget_old_notes(&self) {
        let keep: HashSet<String> = {
            let saved = lock(&self.saved);
            saved
                .patch_notes
                .iter()
                .chain(&saved.news)
                .cloned()
                .collect()
        };
        let gone: Vec<String> = lock(&self.notes)
            .keys()
            .filter(|id| !keep.contains(*id))
            .cloned()
            .collect();
        for id in gone {
            lock_mut(&self.notes).remove(&id);
            lock_mut(&self.translations).retain(|(note, _), _| note != &id);
            let _ = std::fs::remove_file(self.dir.join("notes").join(format!("{id}.json")));
            for lang in ["ru", "ar"] {
                let _ =
                    std::fs::remove_file(self.dir.join("notes").join(format!("{id}.{lang}.json")));
            }
        }
    }

    /// Our channels' videos; a channel that fails keeps its videos before.
    pub async fn refresh_youtube(&self) -> Result<(), String> {
        let mut answered = 0;
        for channel in &self.editorial.channels {
            match youtube::fetch(&self.http, &channel.id).await {
                Ok(mut fetched) => {
                    // Our name for it, not whatever the channel calls itself today.
                    fetched.title.clone_from(&channel.name);
                    for video in &mut fetched.videos {
                        video.channel.clone_from(&channel.name);
                    }
                    self.know_videos(&fetched.videos);
                    lock_mut(&self.saved)
                        .channels
                        .insert(channel.id.clone(), fetched);
                    answered += 1;
                }
                Err(e) => eprintln!("a channel's videos could not be fetched: {e}"),
            }
            tokio::time::sleep(Duration::from_millis(300)).await;
        }
        let ours: HashSet<&str> = self
            .editorial
            .channels
            .iter()
            .map(|c| c.id.as_str())
            .collect();
        lock_mut(&self.saved)
            .channels
            .retain(|id, _| ours.contains(id.as_str()));
        if answered == 0 && !self.editorial.channels.is_empty() {
            return Err("no channel answered".to_owned());
        }
        self.mark("youtube");
        self.save();
        Ok(())
    }

    fn know_videos(&self, videos: &[Video]) {
        let mut known = lock_mut(&self.videos);
        if known.len() > 100_000 {
            known.clear();
        }
        known.extend(videos.iter().map(|v| v.id.clone()));
    }

    pub fn video_known(&self, id: &str) -> bool {
        lock(&self.videos).contains(id)
    }

    pub async fn refresh_twitch(&self) -> Result<(), String> {
        let mut twitch = self.twitch.lock().await;
        let Some(twitch) = twitch.as_mut() else {
            return Ok(());
        };
        for lang in LANGS {
            let streams = twitch
                .streams(&self.http, lang, &self.editorial.twitch_blocked)
                .await?;
            lock_mut(&self.saved)
                .streams
                .insert(lang.to_owned(), streams);
        }
        self.mark("twitch");
        self.save();
        Ok(())
    }

    /// The preview address of a stream the feed lists.
    pub fn stream_preview(&self, login: &str) -> Option<String> {
        lock(&self.saved)
            .streams
            .values()
            .flatten()
            .find(|s| s.login == login)
            .map(|s| s.preview.clone())
    }

    /// Translates the newest notes that have no translation yet, one at a
    /// time, while DeepL's month allows.
    pub async fn translate(&self) -> Result<usize, String> {
        let Some(deepl) = &self.deepl else {
            return Ok(0);
        };
        let ids: Vec<String> = {
            let saved = lock(&self.saved);
            let mut ids: Vec<String> = saved.patch_notes.iter().take(TRANSLATED).cloned().collect();
            ids.extend(saved.news.iter().take(TRANSLATED).cloned());
            ids
        };
        let mut done = 0;
        for lang in ["ru", "ar"] {
            for id in &ids {
                if lock(&self.translations).contains_key(&(id.clone(), lang.to_owned())) {
                    continue;
                }
                let Some(note) = lock(&self.notes).get(id).cloned() else {
                    continue;
                };
                if !deepl.room(&self.http).await? {
                    return Ok(done);
                }
                let translated = self.translate_note(deepl, &note, lang).await?;
                let path = self.dir.join("notes").join(format!("{id}.{lang}.json"));
                write_json(&path, &translated)?;
                lock_mut(&self.translations).insert((id.clone(), lang.to_owned()), translated);
                done += 1;
            }
        }
        Ok(done)
    }

    async fn translate_note(
        &self,
        deepl: &DeepL,
        note: &Note,
        lang: &str,
    ) -> Result<Translated, String> {
        let mut texts = vec![html::escaped(&note.title), html::escaped(&note.summary)];
        let pieces = html::chunks(&note.blocks, deepl::PIECE);
        texts.extend(pieces.iter().map(|p| html::to_html(p)));
        let mut out = Vec::new();
        // Several pieces of a long note, a few at a time.
        for batch in texts.chunks(4) {
            out.extend(deepl.translate(&self.http, batch, lang).await?);
        }
        let plain = |t: &str, max| {
            html::shorten(
                &html::summary(&html::parse(&format!("<p>{t}</p>"), None), usize::MAX),
                max,
            )
        };
        let blocks = out[2..]
            .iter()
            .flat_map(|piece| html::parse(piece, None))
            .collect();
        Ok(Translated {
            title: plain(&out[0], 200),
            summary: plain(&out[1], 600),
            blocks,
        })
    }

    fn note_card(&self, note: &Note, lang: &str) -> Value {
        let translated = lock(&self.translations)
            .get(&(note.id.clone(), lang.to_owned()))
            .map(|t| (t.title.clone(), t.summary.clone()));
        let (title, summary, is_translated) = match translated {
            Some((title, summary)) => (title, summary, true),
            None => (note.title.clone(), note.summary.clone(), false),
        };
        let mut card = json!({
            "id": note.id,
            "title": title,
            "date": note.date,
            "summary": summary,
            "translated": is_translated,
        });
        for (key, value) in [
            ("version", &note.version),
            ("kind", &note.kind),
            ("image", &note.image),
            ("url", &note.url),
        ] {
            if let Some(value) = value {
                card[key] = json!(value);
            }
        }
        card
    }

    /// The whole feed for one interface language.
    pub fn build(&self, lang: &str) -> Value {
        let now = chrono::Utc::now();
        let saved = lock(&self.saved);
        let notes = lock(&self.notes);
        let cards = |ids: &[String]| -> Vec<Value> {
            ids.iter()
                .filter_map(|id| notes.get(id))
                .map(|note| self.note_card(note, lang))
                .collect()
        };
        let mut videos: Vec<&Video> = self
            .editorial
            .channels_for(lang)
            .filter_map(|c| saved.channels.get(&c.id))
            .flat_map(|c| c.videos.iter())
            .collect();
        videos.sort_by(|a, b| b.date.cmp(&a.date));
        videos.truncate(VIDEOS);
        let videos: Vec<Value> = videos.into_iter().map(video_card).collect();
        let streams: Vec<Value> = saved
            .streams
            .get(lang)
            .into_iter()
            .flatten()
            .map(|s| {
                json!({
                    "login": s.login,
                    "name": s.name,
                    "title": s.title,
                    "viewers": s.viewers,
                    "started": s.started,
                    "image": format!("tw/{}.jpg", s.login),
                })
            })
            .collect();
        json!({
            "format": FORMAT,
            "generated": now_text(),
            "lang": lang,
            "versions": cards(&saved.patch_notes),
            "news": cards(&saved.news),
            "videos": videos,
            "streams": streams,
            "cycle": mojang::cycle(&saved.versions),
            "latest": latest(&saved.versions),
            "events": self.editorial.events(lang, now),
            "announcements": self.editorial.announcements(lang, now),
            "tip": self.editorial.tip(lang, now),
            "fetched": saved.fetched,
        })
    }

    /// A whole note in a language (the original when there is no
    /// translation, or for `en`).
    pub fn note(&self, id: &str, lang: &str) -> Option<Value> {
        let note = lock(&self.notes).get(id).cloned()?;
        let mut card = self.note_card(&note, lang);
        let blocks = lock(&self.translations)
            .get(&(id.to_owned(), lang.to_owned()))
            .map(|t| t.blocks.clone())
            .unwrap_or(note.blocks);
        card["blocks"] = json!(blocks);
        Some(card)
    }

    /// Videos of a player's own channels, fetched when not known or old.
    pub async fn own_videos(&self, ids: &[String]) -> Vec<Value> {
        let stale: Vec<String> = {
            let own = self
                .own
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            ids.iter()
                .filter(|id| {
                    own.get(*id)
                        .is_none_or(|c| c.fetched.elapsed() > CHANNEL_TTL)
                })
                .cloned()
                .collect()
        };
        let fetched = futures_util::future::join_all(
            stale
                .iter()
                .map(|id| async move { (id.clone(), youtube::fetch(&self.http, id).await) }),
        )
        .await;
        let mut own = self
            .own
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for (id, result) in fetched {
            if let Ok(channel) = result {
                self.know_videos(&channel.videos);
                let now = Instant::now();
                own.insert(
                    id,
                    OwnChannel {
                        channel,
                        fetched: now,
                        used: now,
                    },
                );
            }
        }
        let mut videos: Vec<Video> = Vec::new();
        for id in ids {
            if let Some(entry) = own.get_mut(id) {
                entry.used = Instant::now();
                videos.extend(entry.channel.videos.iter().cloned());
            }
        }
        if own.len() > OWN_CHANNELS {
            let mut by_use: Vec<(String, Instant)> =
                own.iter().map(|(k, v)| (k.clone(), v.used)).collect();
            by_use.sort_by_key(|(_, used)| *used);
            for (id, _) in by_use.into_iter().take(own.len() - OWN_CHANNELS) {
                own.remove(&id);
            }
        }
        videos.sort_by(|a, b| b.date.cmp(&a.date));
        videos.truncate(VIDEOS);
        videos.iter().map(video_card).collect()
    }

    /// A channel a player named, checked to exist; kept for their videos.
    pub fn keep_own(&self, id: &str, channel: Channel) {
        self.know_videos(&channel.videos);
        let now = Instant::now();
        self.own
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(
                id.to_owned(),
                OwnChannel {
                    channel,
                    fetched: now,
                    used: now,
                },
            );
    }

    /// Pictures the feed or a note refers to.
    pub fn used_images(&self) -> HashSet<String> {
        let mut used = HashSet::new();
        for note in lock(&self.notes).values() {
            used.extend(note.image.clone());
            let mut found = Vec::new();
            html::images(&note.blocks, &mut found);
            used.extend(found);
        }
        for t in lock(&self.translations).values() {
            let mut found = Vec::new();
            html::images(&t.blocks, &mut found);
            used.extend(found);
        }
        used
    }

    pub fn counts(&self) -> Value {
        let saved = lock(&self.saved);
        json!({
            "versions": saved.versions.len(),
            "patchNotes": saved.patch_notes.len(),
            "news": saved.news.len(),
            "channels": saved.channels.len(),
            "videos": saved.channels.values().map(|c| c.videos.len()).sum::<usize>(),
            "streams": saved.streams.values().map(Vec::len).sum::<usize>(),
            "translations": lock(&self.translations).len(),
            "ownChannels": self.own.lock().map(|o| o.len()).unwrap_or(0),
            "fetched": saved.fetched,
        })
    }
}

fn video_card(video: &Video) -> Value {
    json!({
        "id": video.id,
        "channelId": video.channel_id,
        "channel": video.channel,
        "title": video.title,
        "date": video.date,
        "short": video.short,
        "image": format!("yt/{}.jpg", video.id),
    })
}

/// The newest release and the newest snapshot (or pre-release, release
/// candidate) of all, for the "new version" card.
fn latest(versions: &[Version]) -> Value {
    json!({
        "release": versions.iter().find(|v| v.kind == "release"),
        "snapshot": versions.iter().find(|v| v.kind != "release"),
    })
}
