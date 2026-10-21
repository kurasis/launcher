//! Minecraft releases, newest first, from Mojang's version manifest: the
//! versions an ad's range can name and the list the launcher offers.

use crate::store::Store;
use crate::text;

const MANIFEST: &str = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";
const MAX_MANIFEST: usize = 8 * 1024 * 1024;
/// The list is fetched again after this long.
pub const REFRESH: i64 = 6 * 3600;

#[derive(serde::Deserialize)]
struct Manifest {
    versions: Vec<Entry>,
}

#[derive(serde::Deserialize)]
struct Entry {
    id: String,
    #[serde(rename = "type")]
    kind: String,
}

/// Release IDs in the manifest's order (newest first); snapshots and
/// anything with an odd ID are left out.
pub fn parse(bytes: &[u8]) -> Result<Vec<String>, String> {
    let manifest: Manifest =
        serde_json::from_slice(bytes).map_err(|e| format!("the version manifest: {e}"))?;
    let ids: Vec<String> = manifest
        .versions
        .into_iter()
        .filter(|v| v.kind == "release" && text::version_id(&v.id))
        .map(|v| v.id)
        .take(2000)
        .collect();
    if ids.is_empty() {
        return Err("the version manifest has no releases".to_owned());
    }
    Ok(ids)
}

pub fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .user_agent(concat!("lapi-servers/", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(30))
        .connect_timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| e.to_string())
}

async fn fetch(http: &reqwest::Client) -> Result<Vec<String>, String> {
    let response = http
        .get(MANIFEST)
        .send()
        .await
        .map_err(|e| format!("the version manifest: {}", e.without_url()))?;
    if !response.status().is_success() {
        return Err(format!(
            "the version manifest: HTTP {}",
            response.status().as_u16()
        ));
    }
    if response
        .content_length()
        .is_some_and(|n| n > MAX_MANIFEST as u64)
    {
        return Err("the version manifest is too large".to_owned());
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|e| format!("the version manifest: {}", e.without_url()))?;
    if bytes.len() > MAX_MANIFEST {
        return Err("the version manifest is too large".to_owned());
    }
    parse(&bytes)
}

/// Fetches the list when it is older than `REFRESH` (or `force`); a failure
/// keeps the list there was.
pub async fn refresh(
    store: &Store,
    http: &reqwest::Client,
    now: i64,
    force: bool,
) -> Result<(), String> {
    let fresh = store.releases_at()?.is_some_and(|at| now - at < REFRESH);
    if fresh && !force {
        return Ok(());
    }
    let ids = fetch(http).await?;
    store.set_releases(&ids, now)
}
