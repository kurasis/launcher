//! Live Minecraft streams from Twitch's Helix API, with an app access token
//! (client credentials: no player signs in anywhere).

use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::html;
use crate::images::twitch_login;
use crate::net;

/// Twitch's id of the game Minecraft.
pub const MINECRAFT: &str = "27471";
/// Streams kept per language.
pub const STREAMS: usize = 6;

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

/// A stream as Helix gives it.
#[derive(Deserialize)]
pub struct RawStream {
    pub user_login: String,
    pub user_name: String,
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

/// Streams fit for the feed: not marked for adults, not blocked by us, with
/// a sound login and a Twitch preview address; the most watched first.
pub fn pick(data: Vec<RawStream>, blocked: &[String]) -> Vec<Stream> {
    let mut out: Vec<Stream> = data
        .into_iter()
        .filter(|s| !s.is_mature)
        .filter_map(|s| {
            let login = s.user_login.to_ascii_lowercase();
            if !twitch_login(&login) || blocked.iter().any(|b| b.eq_ignore_ascii_case(&login)) {
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
    out.truncate(STREAMS);
    out
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

    /// The most watched Minecraft streams in one language.
    pub async fn streams(
        &mut self,
        http: &reqwest::Client,
        lang: &str,
        blocked: &[String],
    ) -> Result<Vec<Stream>, String> {
        let token = self.token(http).await?;
        let response = http
            .get(format!(
                "https://api.twitch.tv/helix/streams?game_id={MINECRAFT}&language={lang}&first=40&type=live"
            ))
            .header("Client-Id", &self.client_id)
            .bearer_auth(&token)
            .send()
            .await
            .map_err(|e| e.without_url().to_string())?;
        if response.status() == reqwest::StatusCode::UNAUTHORIZED {
            self.token = None;
        }
        let streams: Streams = serde_json::from_slice(&net::read(response, 1 << 20).await?)
            .map_err(|_| "bad streams answer".to_owned())?;
        Ok(pick(streams.data, blocked))
    }
}
