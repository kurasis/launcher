//! Metadata source: catalog fetch/cache with HTTP validators, hash-verified version
//! descriptors and asset indexes stored byte-for-byte (docs/spec/04 "Metadata
//! resolution", doc 02 "Persistent models").
//!
//! Upstream bytes are verified against their catalog/descriptor SHA-1 before parsing
//! and stored unchanged under their hash; app-owned data lives in a separate receipt.

use std::future::Future;
use std::pin::Pin;

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use sha1::Digest;
use tokio::sync::Mutex;

use super::compatibility;
use super::manifest::{self, Catalog, CatalogEntry};
use crate::error::{ErrorCode, LauncherError, LauncherResult};
use crate::install::descriptor::MAX_DESCRIPTOR_BYTES;
use crate::install::plan::{self, GamePlan, MAX_ASSET_INDEX_BYTES, PlannedArtifact};
use crate::install::rules::RuleContext;
use crate::net::{self, PublicClient, SourcePolicy};
use crate::storage::atomic;
use crate::storage::document::{self, Loaded, SCHEMA_VERSION};
use crate::storage::paths::{DataRoot, SafeName, Sha1};

/// The cache is refreshed automatically when older than this.
pub const CATALOG_MAX_AGE_HOURS: i64 = 6;

/// Result of a conditional GET.
pub struct Fetched {
    pub not_modified: bool,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub body: Vec<u8>,
}

pub type FetchFuture<'a> = Pin<Box<dyn Future<Output = LauncherResult<Fetched>> + Send + 'a>>;

/// Abstraction over the public metadata client so tests can supply fixtures.
pub trait Fetch: Send + Sync {
    fn fetch<'a>(
        &'a self,
        url: &'a str,
        etag: Option<&'a str>,
        last_modified: Option<&'a str>,
        max_bytes: usize,
    ) -> FetchFuture<'a>;
}

impl Fetch for PublicClient {
    fn fetch<'a>(
        &'a self,
        url: &'a str,
        etag: Option<&'a str>,
        last_modified: Option<&'a str>,
        max_bytes: usize,
    ) -> FetchFuture<'a> {
        Box::pin(async move {
            let url = self.policy().check(url)?;
            let mut request = self.get(&url)?;
            if let Some(etag) = etag {
                request = request.header(reqwest::header::IF_NONE_MATCH, etag);
            }
            if let Some(last_modified) = last_modified {
                request = request.header(reqwest::header::IF_MODIFIED_SINCE, last_modified);
            }
            let (status, headers, body) = self.get_bounded(request, max_bytes).await?;
            if status == reqwest::StatusCode::NOT_MODIFIED {
                return Ok(Fetched {
                    not_modified: true,
                    etag: etag.map(str::to_owned),
                    last_modified: last_modified.map(str::to_owned),
                    body: Vec::new(),
                });
            }
            if !status.is_success() {
                return Err(net::status_error(status, "Minecraft metadata"));
            }
            let header = |name| {
                headers
                    .get(name)
                    .and_then(|v: &reqwest::header::HeaderValue| v.to_str().ok())
                    .map(str::to_owned)
            };
            Ok(Fetched {
                not_modified: false,
                etag: header(reqwest::header::ETAG),
                last_modified: header(reqwest::header::LAST_MODIFIED),
                body,
            })
        })
    }
}

/// App-owned receipt for the cached catalog bytes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogReceipt {
    pub schema_version: u64,
    pub fetched_at: DateTime<Utc>,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    /// SHA-256 of the cached catalog bytes.
    pub sha256: String,
}

/// Availability of one supported version for a new installation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Availability {
    /// Catalog descriptor hash is on the accepted list.
    Available,
    /// The catalog now lists another descriptor hash: a new candidate that is not
    /// trusted until qualified.
    DescriptorChanged,
    NotInCatalog,
    CatalogUnavailable,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SupportedVersionView {
    pub id: &'static str,
    pub status: compatibility::QualificationStatus,
    pub expected_java_major: u16,
    pub availability: Availability,
    pub catalog_sha1: Option<Sha1>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogView {
    pub versions: Vec<SupportedVersionView>,
    pub fetched_at: Option<DateTime<Utc>>,
    /// True when shown from cache because the refresh failed.
    pub stale: bool,
    pub error: Option<LauncherError>,
    /// Informational; never installed automatically.
    pub latest_release: Option<String>,
}

/// Summary for the UI before installation (P05 demonstration).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanSummary {
    pub version_id: String,
    pub descriptor_sha1: Sha1,
    pub java_major: u16,
    pub libraries: usize,
    pub asset_objects: usize,
    pub game_artifacts: usize,
    pub game_bytes: u64,
    /// Bytes of game artifacts not already present with the expected size.
    pub missing_game_bytes: u64,
    /// Always unknown here: the Java runtime package is resolved in P07.
    pub runtime_bytes: Option<u64>,
}

pub struct MetadataService<F: Fetch> {
    fetcher: F,
    root: DataRoot,
    catalog_lock: Mutex<()>,
}

fn sha1_hex(bytes: &[u8]) -> String {
    hex::encode(sha1::Sha1::digest(bytes))
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::Digest as _;
    hex::encode(sha2::Sha256::digest(bytes))
}

fn hash_mismatch(what: &str) -> LauncherError {
    LauncherError::new(
        ErrorCode::HashMismatch,
        format!("Downloaded {what} did not match its expected checksum."),
    )
}

fn storage_error(error: &std::io::Error) -> LauncherError {
    LauncherError::from_io(error, "Saving Minecraft metadata")
}

impl<F: Fetch> MetadataService<F> {
    pub fn new(fetcher: F, root: DataRoot) -> Self {
        Self {
            fetcher,
            root,
            catalog_lock: Mutex::new(()),
        }
    }

    fn cached_catalog(&self) -> Option<(CatalogReceipt, Vec<u8>)> {
        let receipt = match document::load::<CatalogReceipt>(&self.root.catalog_receipt(), None) {
            Ok(Loaded::Valid(receipt)) => receipt,
            _ => return None,
        };
        let bytes = std::fs::read(self.root.catalog()).ok()?;
        (sha256_hex(&bytes) == receipt.sha256).then_some((receipt, bytes))
    }

    /// Returns the catalog, refreshing it when requested or stale. Concurrent
    /// callers are coalesced. A failed refresh falls back to the cache.
    pub async fn catalog(
        &self,
        refresh: bool,
    ) -> (
        Option<Catalog>,
        Option<DateTime<Utc>>,
        bool,
        Option<LauncherError>,
    ) {
        let _guard = self.catalog_lock.lock().await;
        let cached = self.cached_catalog();
        let fresh_enough = cached.as_ref().is_some_and(|(r, _)| {
            Utc::now() - r.fetched_at < Duration::hours(CATALOG_MAX_AGE_HOURS)
        });
        if !refresh && fresh_enough {
            let (receipt, bytes) = cached.expect("checked");
            return match manifest::parse(&bytes) {
                Ok(catalog) => (Some(catalog), Some(receipt.fetched_at), false, None),
                Err(error) => (None, None, false, Some(error)),
            };
        }
        match self.refresh_catalog(cached.as_ref()).await {
            Ok((catalog, fetched_at)) => (Some(catalog), Some(fetched_at), false, None),
            Err(error) => match cached {
                Some((receipt, bytes)) => (
                    manifest::parse(&bytes).ok(),
                    Some(receipt.fetched_at),
                    true,
                    Some(error),
                ),
                None => (None, None, false, Some(error)),
            },
        }
    }

    async fn refresh_catalog(
        &self,
        cached: Option<&(CatalogReceipt, Vec<u8>)>,
    ) -> LauncherResult<(Catalog, DateTime<Utc>)> {
        let receipt = cached.map(|(r, _)| r);
        let fetched = self
            .fetcher
            .fetch(
                manifest::CATALOG_URL,
                receipt.and_then(|r| r.etag.as_deref()),
                receipt.and_then(|r| r.last_modified.as_deref()),
                manifest::MAX_CATALOG_BYTES,
            )
            .await?;
        let now = Utc::now();
        let bytes = if fetched.not_modified {
            cached.map(|(_, b)| b.clone()).ok_or_else(|| {
                LauncherError::new(ErrorCode::NetworkError, "The catalog cache is missing.")
            })?
        } else {
            fetched.body
        };
        let catalog = manifest::parse(&bytes)?;
        if let Some(parent) = self.root.catalog().parent() {
            std::fs::create_dir_all(parent).map_err(|e| storage_error(&e))?;
        }
        if !fetched.not_modified {
            atomic::write(&self.root.catalog(), &bytes).map_err(|e| storage_error(&e))?;
        }
        document::save(
            &self.root.catalog_receipt(),
            &CatalogReceipt {
                schema_version: SCHEMA_VERSION,
                fetched_at: now,
                etag: fetched.etag,
                last_modified: fetched.last_modified,
                sha256: sha256_hex(&bytes),
            },
            None,
        )
        .map_err(|e| storage_error(&e))?;
        Ok((catalog, now))
    }

    /// The supported versions intersected with the catalog.
    pub async fn supported_versions(&self, refresh: bool) -> CatalogView {
        let (catalog, fetched_at, stale, error) = self.catalog(refresh).await;
        let versions = compatibility::SUPPORTED_VERSIONS
            .iter()
            .map(|supported| {
                let entry = catalog.as_ref().and_then(|c| c.find(supported.id));
                let availability = match (&catalog, entry) {
                    (None, _) => Availability::CatalogUnavailable,
                    (Some(_), None) => Availability::NotInCatalog,
                    (Some(_), Some(entry))
                        if supported
                            .accepted_descriptor_sha1
                            .contains(&entry.sha1.as_str()) =>
                    {
                        Availability::Available
                    }
                    (Some(_), Some(_)) => Availability::DescriptorChanged,
                };
                SupportedVersionView {
                    id: supported.id,
                    status: supported.status,
                    expected_java_major: supported.expected_java_major,
                    availability,
                    catalog_sha1: entry.map(|e| e.sha1.clone()),
                }
            })
            .collect();
        CatalogView {
            versions,
            fetched_at,
            stale,
            latest_release: catalog.and_then(|c| c.latest_release),
            error,
        }
    }

    /// Returns verified descriptor bytes, from the hash-addressed cache when valid.
    pub async fn descriptor(&self, entry: &CatalogEntry) -> LauncherResult<Vec<u8>> {
        let id = SafeName::parse(&entry.id)?;
        let path = self.root.version_descriptor(&id, &entry.sha1);
        if let Ok(bytes) = std::fs::read(&path)
            && sha1_hex(&bytes) == entry.sha1.as_str()
        {
            return Ok(bytes);
        }
        SourcePolicy::MojangMetadata.check(&entry.url)?;
        let fetched = self
            .fetcher
            .fetch(&entry.url, None, None, MAX_DESCRIPTOR_BYTES)
            .await?;
        if sha1_hex(&fetched.body) != entry.sha1.as_str() {
            return Err(hash_mismatch("version metadata"));
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| storage_error(&e))?;
        }
        atomic::write(&path, &fetched.body).map_err(|e| storage_error(&e))?;
        Ok(fetched.body)
    }

    /// Returns verified asset-index bytes, from the hash-addressed cache when valid.
    pub async fn asset_index(&self, artifact: &PlannedArtifact) -> LauncherResult<Vec<u8>> {
        let path = self.root.asset_index_source(&artifact.sha1);
        if let Ok(bytes) = std::fs::read(&path)
            && sha1_hex(&bytes) == artifact.sha1.as_str()
        {
            return Ok(bytes);
        }
        let fetched = self
            .fetcher
            .fetch(&artifact.url, None, None, MAX_ASSET_INDEX_BYTES)
            .await?;
        if fetched.body.len() as u64 != artifact.size
            || sha1_hex(&fetched.body) != artifact.sha1.as_str()
        {
            return Err(hash_mismatch("asset index"));
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| storage_error(&e))?;
        }
        atomic::write(&path, &fetched.body).map_err(|e| storage_error(&e))?;
        Ok(fetched.body)
    }

    /// Resolves a complete game plan for a supported version whose catalog
    /// descriptor hash is accepted. Never selects `latest.release` on its own.
    pub async fn plan(&self, version_id: &str, ctx: &RuleContext) -> LauncherResult<GamePlan> {
        let supported = compatibility::find(version_id).ok_or_else(|| {
            LauncherError::invalid_input("This Minecraft version is not supported.")
        })?;
        let (catalog, _, _, error) = self.catalog(false).await;
        let catalog = catalog.ok_or_else(|| {
            error.unwrap_or_else(|| {
                LauncherError::new(
                    ErrorCode::NetworkError,
                    "The version catalog is unavailable.",
                )
            })
        })?;
        let entry = catalog.find(version_id).ok_or_else(|| {
            LauncherError::new(
                ErrorCode::UnsupportedVersionMetadata,
                "This version is no longer listed by Mojang.",
            )
        })?;
        if !supported
            .accepted_descriptor_sha1
            .contains(&entry.sha1.as_str())
        {
            return Err(LauncherError::new(
                ErrorCode::UnsupportedVersionMetadata,
                "Mojang changed this version's metadata. The new metadata has not been \
                 qualified for this launcher yet.",
            )
            .with_detail("catalogSha1", entry.sha1.to_string()));
        }
        let bytes = self.descriptor(entry).await?;
        let mut plan = plan::plan_game(&bytes, &entry.url, &entry.sha1, version_id, ctx)?;
        let index = self.asset_index(&plan.asset_index.artifact).await?;
        plan::plan_assets(&mut plan, &index)?;
        Ok(plan)
    }

    /// Summarizes a plan, estimating missing bytes from file presence and size.
    pub fn summarize(&self, plan: &GamePlan) -> PlanSummary {
        let missing = plan
            .artifacts()
            .filter(|a| {
                std::fs::metadata(self.root.path().join(&a.path))
                    .map(|m| m.len() != a.size)
                    .unwrap_or(true)
            })
            .map(|a| a.size)
            .sum();
        PlanSummary {
            version_id: plan.version_id.clone(),
            descriptor_sha1: plan.descriptor_sha1.clone(),
            java_major: plan.runtime.major,
            libraries: plan.libraries.len(),
            asset_objects: plan.assets.len(),
            game_artifacts: plan.artifacts().count(),
            game_bytes: plan.total_bytes(),
            missing_game_bytes: missing,
            runtime_bytes: None,
        }
    }
}

#[cfg(test)]
mod tests;
