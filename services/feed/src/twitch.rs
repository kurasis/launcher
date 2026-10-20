//! Live streams from Twitch's Helix API: the most watched Minecraft streams
//! per language, and those of players' own channels; with an app access
//! token (client credentials: no player signs in anywhere).

use std::collections::HashSet;
use std::time::{Duration, Instant};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::html;
use crate::images::twitch_login;
use crate::net;

/// Twitch's id of the game Minecraft.
pub const MINECRAFT: &str = "27471";
/// Streams kept per language.
pub const STREAMS: usize = 6;
/// Channels one Helix request may name.
pub const HELIX_LOGINS: usize = 100;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Stream {
    pub login: String,
    pub name: String,
    pub title: String,
    pub viewers: u64,
    pub started: String,
    /// Where its preview is fetched (440×248), not given to players.
    #[serde(skip_serializing, default)]
    pub preview: String,
}

pub struct Twitch {
    client_id: String,
    secret: String,
    token: Option<(String, Instant)>,
}

#[derive(Deserialize)]
struct Token {
    access_token: String,
    expires_in: u64,
}

#[derive(Deserialize)]
struct Streams {
    data: Vec<RawStream>,
}

#[derive(Deserialize)]
struct Users {
    data: Vec<User>,
}

/// A channel as Helix gives it.
#[derive(Deserialize)]
struct User {
    login: String,
    #[serde(default)]
    display_name: String,
}

/// A stream as Helix gives it.
#[derive(Deserialize)]
pub struct RawStream {
    pub user_login: String,
    pub user_name: String,
    #[serde(default)]
    pub game_id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub viewer_count: u64,
    #[serde(default)]
    pub started_at: String,
    #[serde(default)]
    pub thumbnail_url: String,
    #[serde(default)]
    pub is_mature: bool,
}

/// What keeps a stream out of the automatic list (`editorial.toml`).
#[derive(Debug, Clone, Copy, Default)]
pub struct Blocked<'a> {
    /// Found anywhere in the login or the shown name, `_` aside, so the
    /// copies of a channel (`name_8225539`) go too.
    pub names: &'a [String],
    /// Found anywhere in the title: gambling streams under Minecraft.
    pub words: &'a [String],
}

/// Lower case without `_`, as blocked names are matched.
fn plain(text: &str) -> String {
    text.to_lowercase().replace('_', "")
}

impl Blocked<'_> {
    pub fn stream(&self, login: &str, name: &str, title: &str) -> bool {
        let (login, name) = (plain(login), plain(name));
        let title = title.to_lowercase();
        self.names
            .iter()
            .map(|b| plain(b))
            .any(|b| login.contains(&b) || name.contains(&b))
            || self.words.iter().any(|w| title.contains(&w.to_lowercase()))
    }
}

/// Streams fit for the feed: not marked for adults, not blocked by us, of
/// `game` when one is given, with a sound login and a Twitch preview address;
/// each login once, the most watched first.
pub fn pick(data: Vec<RawStream>, blocked: Blocked<'_>, game: Option<&str>) -> Vec<Stream> {
    let mut out: Vec<Stream> = data
        .into_iter()
        .filter(|s| !s.is_mature)
        .filter(|s| game.is_none_or(|g| s.game_id == g))
        .filter_map(|s| {
            let login = s.user_login.to_ascii_lowercase();
            if !twitch_login(&login) || blocked.stream(&login, &s.user_name, &s.title) {
                return None;
            }
            let preview = s
                .thumbnail_url
                .replace("{width}", "440")
                .replace("{height}", "248");
            let url = url::Url::parse(&preview).ok()?;
            if url.scheme() != "https"
                || !url.host_str().is_some_and(|h| h == "static-cdn.jtvnw.net")
            {
                return None;
            }
            Some(Stream {
                name: html::clean(&s.user_name, 40),
                title: html::shorten(&s.title, 140),
                viewers: s.viewer_count,
                started: chrono::DateTime::parse_from_rfc3339(&s.started_at)
                    .map(|t| {
                        t.with_timezone(&chrono::Utc)
                            .format("%Y-%m-%dT%H:%M:%SZ")
                            .to_string()
                    })
                    .unwrap_or_default(),
                preview,
                login,
            })
        })
        .collect();
    out.sort_by_key(|s| std::cmp::Reverse(s.viewers));
    let mut seen = HashSet::new();
    out.retain(|s| seen.insert(s.login.clone()));
    out
}

/// The feed's streams in one language: Minecraft only, at most `STREAMS`.
pub fn top(data: Vec<RawStream>, blocked: Blocked<'_>) -> Vec<Stream> {
    let mut streams = pick(data, blocked, Some(MINECRAFT));
    streams.truncate(STREAMS);
    streams
}

/// A player's own channels live now: any game (they chose them), not for
/// adults, only those asked; the most watched first.
pub fn own(data: Vec<RawStream>, logins: &[String]) -> Vec<Stream> {
    let mut streams = pick(data, Blocked::default(), None);
    streams.retain(|s| logins.contains(&s.login));
    streams.truncate(logins.len());
    streams
}

/// Reads the channel a player typed: a login, or a link to it on twitch.tv
/// (with or without scheme, `www.`, a trailing slash or a query). Anything
/// else, or any other site, is refused before a request is made.
pub fn wanted(input: &str) -> Option<String> {
    let input = input.trim();
    if input.is_empty() || input.len() > 200 || input.chars().any(char::is_control) {
        return None;
    }
    let lower = input.to_ascii_lowercase();
    if twitch_login(&lower) {
        return Some(lower);
    }
    let with_scheme = if input.contains("://") {
        input.to_owned()
    } else {
        format!("https://{input}")
    };
    let url = url::Url::parse(&with_scheme).ok()?;
    if !matches!(url.scheme(), "https" | "http")
        || url.port().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
        || !matches!(
            url.host_str()?,
            "twitch.tv" | "www.twitch.tv" | "m.twitch.tv"
        )
    {
        return None;
    }
    let path = url.path().strip_prefix('/')?;
    let login = path.strip_suffix('/').unwrap_or(path).to_ascii_lowercase();
    twitch_login(&login).then_some(login)
}

impl Twitch {
    pub fn new(client_id: String, secret: String) -> Option<Self> {
        (!client_id.trim().is_empty() && !secret.trim().is_empty()).then_some(Self {
            client_id: client_id.trim().to_owned(),
            secret: secret.trim().to_owned(),
            token: None,
        })
    }

    async fn token(&mut self, http: &reqwest::Client) -> Result<String, String> {
        if let Some((token, until)) = &self.token
            && Instant::now() < *until
        {
            return Ok(token.clone());
        }
        let response = http
            .post("https://id.twitch.tv/oauth2/token")
            .form(&[
                ("client_id", self.client_id.as_str()),
                ("client_secret", self.secret.as_str()),
                ("grant_type", "client_credentials"),
            ])
            .send()
            .await
            .map_err(|e| e.without_url().to_string())?;
        let token: Token = serde_json::from_slice(&net::read(response, 64 * 1024).await?)
            .map_err(|_| "bad token answer".to_owned())?;
        // A little before it runs out.
        let until =
            Instant::now() + Duration::from_secs(token.expires_in.saturating_sub(300).max(60));
        self.token = Some((token.access_token.clone(), until));
        Ok(token.access_token)
    }

    /// One Helix request with our app token; at most `max` bytes are read.
    async fn helix<T: DeserializeOwned>(
        &mut self,
        http: &reqwest::Client,
        url: url::Url,
        max: usize,
    ) -> Result<T, String> {
        let token = self.token(http).await?;
        let response = http
            .get(url)
            .header("Client-Id", &self.client_id)
            .bearer_auth(&token)
            .send()
            .await
            .map_err(|e| e.without_url().to_string())?;
        if response.status() == reqwest::StatusCode::UNAUTHORIZED {
            self.token = None;
        }
        serde_json::from_slice(&net::read(response, max).await?)
            .map_err(|_| "bad Helix answer".to_owned())
    }

    /// The most watched Minecraft streams in one language.
    pub async fn streams(
        &mut self,
        http: &reqwest::Client,
        lang: &str,
        blocked: Blocked<'_>,
    ) -> Result<Vec<Stream>, String> {
        let url = url::Url::parse_with_params(
            "https://api.twitch.tv/helix/streams",
            [
                ("game_id", MINECRAFT),
                ("language", lang),
                ("first", "40"),
                ("type", "live"),
            ],
        )
        .map_err(|e| e.to_string())?;
        let streams: Streams = self.helix(http, url, 1 << 20).await?;
        Ok(top(streams.data, blocked))
    }

    /// The live streams of these channels, any game; no request for none.
    /// Helix takes at most `HELIX_LOGINS` at once.
    pub async fn live(
        &mut self,
        http: &reqwest::Client,
        logins: &[String],
    ) -> Result<Vec<RawStream>, String> {
        if logins.is_empty() {
            return Ok(Vec::new());
        }
        let url = url::Url::parse_with_params(
            "https://api.twitch.tv/helix/streams",
            logins
                .iter()
                .take(HELIX_LOGINS)
                .map(|l| ("user_login", l.as_str()))
                .chain([("type", "live"), ("first", "100")]),
        )
        .map_err(|e| e.to_string())?;
        let streams: Streams = self.helix(http, url, 1 << 20).await?;
        Ok(streams.data)
    }

    /// The display name of a channel, when Twitch has it.
    pub async fn user(
        &mut self,
        http: &reqwest::Client,
        login: &str,
    ) -> Result<Option<String>, String> {
        let url =
            url::Url::parse_with_params("https://api.twitch.tv/helix/users", [("login", login)])
                .map_err(|e| e.to_string())?;
        let users: Users = self.helix(http, url, 64 * 1024).await?;
        Ok(users
            .data
            .into_iter()
            .find(|u| u.login.eq_ignore_ascii_case(login))
            .map(|u| {
                let name = html::clean(&u.display_name, 40).trim().to_owned();
                if name.is_empty() {
                    login.to_owned()
                } else {
                    name
                }
            }))
    }
}
