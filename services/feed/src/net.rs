//! Requests to the sources: one client, a time limit on each request and a
//! limit on how much of an answer is ever read.

use std::time::Duration;

use futures_util::StreamExt;

/// A browser's name: YouTube answers a bare client with a consent page.
const AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/141.0.0.0 Safari/537.36";

pub fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .user_agent(AGENT)
        .timeout(Duration::from_secs(20))
        .connect_timeout(Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()
        .map_err(|e| e.to_string())
}

/// The body of a successful answer, at most `max` bytes (more is an error).
pub async fn read(response: reqwest::Response, max: usize) -> Result<Vec<u8>, String> {
    let status = response.status();
    if !status.is_success() {
        return Err(format!("HTTP {}", status.as_u16()));
    }
    if response
        .content_length()
        .is_some_and(|len| len > max as u64)
    {
        return Err("answer too large".to_owned());
    }
    let mut body = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| e.without_url().to_string())?;
        if body.len() + chunk.len() > max {
            return Err("answer too large".to_owned());
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

pub async fn get(http: &reqwest::Client, url: &str, max: usize) -> Result<Vec<u8>, String> {
    let response = http
        .get(url)
        // YouTube's consent page for visitors from the EU: declined.
        .header("Cookie", "SOCS=CAI")
        .header("Accept-Language", "en")
        .send()
        .await
        .map_err(|e| e.without_url().to_string())?;
    read(response, max).await
}

pub async fn get_text(http: &reqwest::Client, url: &str, max: usize) -> Result<String, String> {
    String::from_utf8(get(http, url, max).await?).map_err(|_| "not UTF-8".to_owned())
}

pub async fn get_json<T: serde::de::DeserializeOwned>(
    http: &reqwest::Client,
    url: &str,
    max: usize,
) -> Result<T, String> {
    serde_json::from_slice(&get(http, url, max).await?).map_err(|e| format!("bad JSON: {e}"))
}
