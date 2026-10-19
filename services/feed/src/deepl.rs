//! Machine translation of patch notes and news through DeepL. Only the
//! service calls it, with the key from `LAPI_FEED_DEEPL_KEY`; it stops at
//! `STOP_AT` of the month's characters and the original is shown instead.

use serde::Deserialize;
use serde_json::json;

use crate::net;

/// Share of the month's characters at which translating stops.
pub const STOP_AT: f64 = 0.9;
/// The largest piece of HTML in one request (DeepL takes 128 KiB a request).
pub const PIECE: usize = 60_000;

pub struct DeepL {
    key: String,
    base: String,
}

#[derive(Deserialize)]
struct Translations {
    translations: Vec<Translation>,
}

#[derive(Deserialize)]
struct Translation {
    text: String,
}

#[derive(Deserialize)]
struct Usage {
    character_count: u64,
    character_limit: u64,
}

/// DeepL's name of a language of ours.
pub fn target(lang: &str) -> Option<&'static str> {
    match lang {
        "ru" => Some("RU"),
        "ar" => Some("AR"),
        _ => None,
    }
}

impl DeepL {
    pub fn new(key: String) -> Option<Self> {
        let key = key.trim().to_owned();
        if key.is_empty() {
            return None;
        }
        // Keys of the free plan end in ":fx" and have their own address.
        let base = if key.ends_with(":fx") {
            "https://api-free.deepl.com"
        } else {
            "https://api.deepl.com"
        };
        Some(Self {
            key,
            base: base.to_owned(),
        })
    }

    fn auth(&self) -> String {
        format!("DeepL-Auth-Key {}", self.key)
    }

    /// Whether this month's characters allow more translating.
    pub async fn room(&self, http: &reqwest::Client) -> Result<bool, String> {
        let response = http
            .get(format!("{}/v2/usage", self.base))
            .header("Authorization", self.auth())
            .send()
            .await
            .map_err(|e| e.without_url().to_string())?;
        let usage: Usage = serde_json::from_slice(&net::read(response, 64 * 1024).await?)
            .map_err(|_| "bad usage answer".to_owned())?;
        Ok(usage.character_limit == 0
            || (usage.character_count as f64) < usage.character_limit as f64 * STOP_AT)
    }

    /// Translates pieces of English HTML (or plain text) into `lang`.
    pub async fn translate(
        &self,
        http: &reqwest::Client,
        texts: &[String],
        lang: &str,
    ) -> Result<Vec<String>, String> {
        let target = target(lang).ok_or("no such language")?;
        let response = http
            .post(format!("{}/v2/translate", self.base))
            .header("Authorization", self.auth())
            .json(&json!({
                "text": texts,
                "source_lang": "EN",
                "target_lang": target,
                "tag_handling": "html",
                "ignore_tags": ["code"],
                "preserve_formatting": true,
            }))
            .send()
            .await
            .map_err(|e| e.without_url().to_string())?;
        let answer: Translations = serde_json::from_slice(&net::read(response, 4 << 20).await?)
            .map_err(|_| "bad translation answer".to_owned())?;
        if answer.translations.len() != texts.len() {
            return Err("translation count differs".to_owned());
        }
        Ok(answer.translations.into_iter().map(|t| t.text).collect())
    }
}
