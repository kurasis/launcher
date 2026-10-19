//! Our own part of the feed, from `editorial.toml` (built into the service).

use serde::{Deserialize, Serialize};

use crate::youtube::channel_id;

pub const LANGS: [&str; 3] = ["en", "ru", "ar"];

/// A text in English and, where written, Russian and Arabic.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Text {
    pub en: String,
    #[serde(default)]
    pub ru: Option<String>,
    #[serde(default)]
    pub ar: Option<String>,
}

impl Text {
    pub fn get(&self, lang: &str) -> &str {
        match lang {
            "ru" => self.ru.as_deref(),
            "ar" => self.ar.as_deref(),
            _ => None,
        }
        .unwrap_or(&self.en)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Channel {
    pub id: String,
    pub name: String,
    pub lang: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Event {
    pub id: String,
    pub starts: String,
    pub ends: String,
    #[serde(default)]
    pub url: Option<String>,
    pub title: Text,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Announcement {
    pub id: String,
    pub until: String,
    pub title: Text,
    pub text: Text,
    #[serde(default)]
    pub url: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Tip {
    pub text: Text,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Editorial {
    #[serde(default)]
    pub twitch_blocked: Vec<String>,
    #[serde(default, rename = "channel")]
    pub channels: Vec<Channel>,
    #[serde(default, rename = "event")]
    pub events: Vec<Event>,
    #[serde(default, rename = "announcement")]
    pub announcements: Vec<Announcement>,
    #[serde(default, rename = "tip")]
    pub tips: Vec<Tip>,
}

/// What a launcher may open from an event or announcement: https on a site
/// it allows.
pub fn link(url: &str) -> bool {
    url::Url::parse(url).is_ok_and(|u| {
        u.scheme() == "https"
            && u.port().is_none()
            && u.username().is_empty()
            && u.host_str().is_some_and(|h| {
                matches!(
                    h,
                    "www.youtube.com" | "youtube.com" | "www.twitch.tv" | "www.minecraft.net"
                ) || h.ends_with(".lapi.sh")
            })
    })
}

fn time(raw: &str) -> Result<chrono::DateTime<chrono::Utc>, String> {
    chrono::DateTime::parse_from_rfc3339(raw)
        .map(|t| t.with_timezone(&chrono::Utc))
        .map_err(|_| format!("bad date {raw}"))
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ShownEvent {
    pub id: String,
    pub title: String,
    pub starts: String,
    pub ends: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ShownAnnouncement {
    pub id: String,
    pub title: String,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

impl Editorial {
    pub fn load(source: &str) -> Result<Self, String> {
        let editorial: Self = toml::from_str(source).map_err(|e| e.to_string())?;
        for channel in &editorial.channels {
            if !channel_id(&channel.id) {
                return Err(format!("bad channel id {}", channel.id));
            }
            if channel.lang != "all" && !LANGS.contains(&channel.lang.as_str()) {
                return Err(format!("bad language {}", channel.lang));
            }
        }
        for event in &editorial.events {
            if time(&event.starts)? > time(&event.ends)? {
                return Err(format!("event {} ends before it starts", event.id));
            }
            if event.url.as_deref().is_some_and(|u| !link(u)) {
                return Err(format!("event {}: link not allowed", event.id));
            }
        }
        for a in &editorial.announcements {
            time(&a.until)?;
            if a.url.as_deref().is_some_and(|u| !link(u)) {
                return Err(format!("announcement {}: link not allowed", a.id));
            }
        }
        Ok(editorial)
    }

    /// The channels shown for a language: its own and those for all.
    pub fn channels_for(&self, lang: &str) -> impl Iterator<Item = &Channel> {
        self.channels
            .iter()
            .filter(move |c| c.lang == "all" || c.lang == lang)
    }

    /// Events from 14 days before they start until they end.
    pub fn events(&self, lang: &str, now: chrono::DateTime<chrono::Utc>) -> Vec<ShownEvent> {
        self.events
            .iter()
            .filter(|e| {
                let (Ok(starts), Ok(ends)) = (time(&e.starts), time(&e.ends)) else {
                    return false;
                };
                now < ends && starts - chrono::Duration::days(14) <= now
            })
            .map(|e| ShownEvent {
                id: e.id.clone(),
                title: e.title.get(lang).to_owned(),
                starts: e.starts.clone(),
                ends: e.ends.clone(),
                url: e.url.clone(),
            })
            .collect()
    }

    pub fn announcements(
        &self,
        lang: &str,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Vec<ShownAnnouncement> {
        self.announcements
            .iter()
            .filter(|a| time(&a.until).is_ok_and(|until| now < until))
            .map(|a| ShownAnnouncement {
                id: a.id.clone(),
                title: a.title.get(lang).to_owned(),
                text: a.text.get(lang).to_owned(),
                url: a.url.clone(),
            })
            .collect()
    }

    /// Today's tip: one a day, in turn.
    pub fn tip(&self, lang: &str, now: chrono::DateTime<chrono::Utc>) -> Option<String> {
        if self.tips.is_empty() {
            return None;
        }
        let day = now.timestamp().div_euclid(86_400) as usize;
        Some(self.tips[day % self.tips.len()].text.get(lang).to_owned())
    }
}
