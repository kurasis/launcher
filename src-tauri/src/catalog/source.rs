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

use super::manifest::{self, Catalog, CatalogEntry};
use super::{compatibility, extra};
use crate::error::{ErrorCode, LauncherError, LauncherResult};
use crate::install::descriptor::{self, MAX_DESCRIPTOR_BYTES};
use crate::install::plan::{self, GamePlan, LibraryLayout, MAX_ASSET_INDEX_BYTES, PlannedArtifact};
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
            let conditional = |mut request: reqwest::RequestBuilder| {
                if let Some(etag) = etag {
                    request = request.header(reqwest::header::IF_NONE_MATCH, etag);
                }
                if let Some(last_modified) = last_modified {
                    request = request.header(reqwest::header::IF_MODIFIED_SINCE, last_modified);
                }
                request
            };
            let (status, headers, body) = self.fetch_bounded(&url, conditional, max_bytes).await?;
            if status == reqwest::StatusCode::NOT_MODIFIED {
                return Ok(Fetched {
                    not_modified: true,
                    etag: etag.map(str::to_owned),
                    last_modified: last_modified.map(str::to_owned),
                    body: Vec::new(),
                });
            }
            if !status.is_success() {
                return Err(if self.policy() == SourcePolicy::ModLoaders {
                    net::status_error(status, "mod loader metadata")
                } else {
                    net::status_error(status, "Minecraft metadata")
                });
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

/// Whether a version can be installed now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Availability {
    /// Listed in the catalog; its descriptor hash is the trust anchor.
    Available,
    NotInCatalog,
    CatalogUnavailable,
}

/// One version for the picker (D-035).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionView {
    pub id: String,
    /// `release`, `snapshot`, `old_beta` or `old_alpha`.
    pub version_type: String,
    pub release_time: Option<String>,
    /// On the shipped list with this exact descriptor hash: launched and
    /// checked by this project (doc 01 "Supported release policy").
    pub verified: bool,
    pub availability: Availability,
    /// A version of our own list, not Mojang's (D-073): shown on a tab of its
    /// own, with its name.
    pub special: Option<extra::Special>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogView {
    /// Newest first, as Mojang lists them, then the versions of our own list in
    /// its order (D-073).
    pub versions: Vec<VersionView>,
    pub fetched_at: Option<DateTime<Utc>>,
    /// True when shown from cache because the refresh failed.
    pub stale: bool,
    pub error: Option<LauncherError>,
    /// Informational; never installed automatically.
    pub latest_release: Option<String>,
    pub latest_snapshot: Option<String>,
}

fn verified(id: &str, sha1: &Sha1) -> bool {
    compatibility::find(id).is_some_and(|v| v.accepted_descriptor_sha1.contains(&sha1.as_str()))
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
    extra: Option<Extra>,
    root: DataRoot,
    catalog_lock: Mutex<()>,
}

/// Our update server, for the versions Mojang's list does not have (D-073).
struct Extra {
    fetcher: Box<dyn Fetch>,
    /// The update key built in; without one no list is taken, but version
    /// files already pinned (an installed version's) still are.
    public_key: Option<String>,
}

/// The list kept, checked again.
struct KeptList {
    receipt: Option<CatalogReceipt>,
    bytes: Vec<u8>,
    list: extra::List,
}

fn sha1_hex(bytes: &[u8]) -> String {
    hex::encode(sha1::Sha1::digest(bytes))
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::Digest as _;
    hex::encode(sha2::Sha256::digest(bytes))
}

fn hash_mismatch(what: &str) -> LauncherError {
    LauncherError::templated(
        ErrorCode::HashMismatch,
        "Downloaded {what} did not match its expected checksum.",
        crate::error::params([("what", what.into())]),
    )
}

fn storage_error(error: &std::io::Error) -> LauncherError {
    LauncherError::from_io(error, "Saving Minecraft metadata")
}

impl<F: Fetch> MetadataService<F> {
    pub fn new(fetcher: F, root: DataRoot) -> Self {
        Self {
            fetcher,
            extra: None,
            root,
            catalog_lock: Mutex::new(()),
        }
    }

    /// With the versions Mojang's list does not have, from `fetcher` (our
    /// update server) and checked with `public_key` (D-073).
    #[must_use]
    pub fn with_extra(mut self, fetcher: Box<dyn Fetch>, public_key: Option<&str>) -> Self {
        self.extra = Some(Extra {
            fetcher,
            public_key: public_key.map(str::to_owned),
        });
        self
    }

    fn cached_catalog(&self) -> Option<(CatalogReceipt, Vec<u8>)> {
        let receipt = match document::load::<CatalogReceipt>(&self.root.catalog_receipt(), None) {
            Ok(Loaded::Valid(receipt)) => receipt,
            _ => return None,
        };
        let bytes = std::fs::read(self.root.catalog()).ok()?;
        (sha256_hex(&bytes) == receipt.sha256).then_some((receipt, bytes))
    }

    /// Returns the catalog, refreshing it when requested or stale, with the
    /// versions of our own list after Mojang's (D-073). Concurrent callers are
    /// coalesced. A failed refresh falls back to the cache; our list never
    /// stops the catalog.
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
        let (mut catalog, fetched_at, stale, error) = self.mojang_catalog(refresh).await;
        if let Some(catalog) = &mut catalog
            && let Some(list) = self.extra_list(refresh).await
        {
            catalog.add_extra(&list);
        }
        (catalog, fetched_at, stale, error)
    }

    async fn mojang_catalog(
        &self,
        refresh: bool,
    ) -> (
        Option<Catalog>,
        Option<DateTime<Utc>>,
        bool,
        Option<LauncherError>,
    ) {
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

    fn kept_list(&self, public_key: &str) -> Option<KeptList> {
        use std::io::Read as _;
        let mut bytes = Vec::new();
        std::fs::File::open(self.root.extra_versions())
            .ok()?
            .take(extra::MAX_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .ok()?;
        let list = extra::verify(public_key, &bytes)?;
        let receipt =
            match document::load::<CatalogReceipt>(&self.root.extra_versions_receipt(), None) {
                Ok(Loaded::Valid(receipt)) if receipt.sha256 == sha256_hex(&bytes) => Some(receipt),
                _ => None,
            };
        Some(KeptList {
            receipt,
            bytes,
            list,
        })
    }

    /// Our list of versions Mojang's does not have: the one kept while it is
    /// fresh, else asked again. Only a list signed with the update key and not
    /// older than the one kept is taken; anything else keeps the old one.
    async fn extra_list(&self, refresh: bool) -> Option<extra::List> {
        let extra = self.extra.as_ref()?;
        let key = extra.public_key.as_deref()?;
        let kept = self.kept_list(key);
        let fresh = kept
            .as_ref()
            .and_then(|k| k.receipt.as_ref())
            .is_some_and(|r| Utc::now() - r.fetched_at < Duration::hours(CATALOG_MAX_AGE_HOURS));
        if !refresh && fresh {
            return kept.map(|k| k.list);
        }
        match self.refresh_extra(extra, key, kept.as_ref()).await {
            Ok(list) => Some(list),
            Err(_) => kept.map(|k| k.list),
        }
    }

    async fn refresh_extra(
        &self,
        extra: &Extra,
        key: &str,
        kept: Option<&KeptList>,
    ) -> LauncherResult<extra::List> {
        let receipt = kept.and_then(|k| k.receipt.as_ref());
        let fetched = extra
            .fetcher
            .fetch(
                extra::URL,
                receipt.and_then(|r| r.etag.as_deref()),
                receipt.and_then(|r| r.last_modified.as_deref()),
                extra::MAX_BYTES,
            )
            .await?;
        let refused = || {
            LauncherError::new(
                ErrorCode::UnsupportedVersionMetadata,
                "The list of other Minecraft versions was refused.",
            )
        };
        let bytes = if fetched.not_modified {
            kept.map(|k| k.bytes.clone()).ok_or_else(refused)?
        } else {
            fetched.body
        };
        let list = extra::verify(key, &bytes).ok_or_else(refused)?;
        // An older list than the one kept: someone serves an old file.
        if kept.is_some_and(|k| k.list.serial > list.serial) {
            return Err(refused());
        }
        if let Some(parent) = self.root.extra_versions().parent() {
            std::fs::create_dir_all(parent).map_err(|e| storage_error(&e))?;
        }
        if kept.is_none_or(|k| k.bytes != bytes) {
            atomic::write(&self.root.extra_versions(), &bytes).map_err(|e| storage_error(&e))?;
        }
        document::save(
            &self.root.extra_versions_receipt(),
            &CatalogReceipt {
                schema_version: SCHEMA_VERSION,
                fetched_at: Utc::now(),
                etag: fetched.etag,
                last_modified: fetched.last_modified,
                sha256: sha256_hex(&bytes),
            },
            None,
        )
        .map_err(|e| storage_error(&e))?;
        Ok(list)
    }

    /// Every vanilla version in the catalog, newest first. Without a catalog the
    /// shipped verified versions are listed as unavailable.
    pub async fn supported_versions(&self, refresh: bool) -> CatalogView {
        let (catalog, fetched_at, stale, error) = self.catalog(refresh).await;
        let versions = match &catalog {
            Some(catalog) => catalog
                .versions
                .iter()
                .filter(|e| descriptor::VERSION_TYPES.contains(&e.version_type.as_str()))
                .map(|entry| VersionView {
                    id: entry.id.clone(),
                    version_type: entry.version_type.clone(),
                    release_time: entry.release_time.clone(),
                    verified: entry.special.is_none() && verified(&entry.id, &entry.sha1),
                    availability: Availability::Available,
                    special: entry.special.clone(),
                })
                .collect(),
            None => compatibility::SUPPORTED_VERSIONS
                .iter()
                .map(|v| VersionView {
                    id: v.id.to_owned(),
                    version_type: "release".to_owned(),
                    release_time: None,
                    verified: true,
                    availability: Availability::CatalogUnavailable,
                    special: None,
                })
                .collect(),
        };
        let (latest_release, latest_snapshot) = catalog
            .map(|c| (c.latest_release, c.latest_snapshot))
            .unwrap_or_default();
        CatalogView {
            versions,
            fetched_at,
            stale,
            latest_release,
            latest_snapshot,
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
        // Version files of our own list come from our server (D-073), every
        // other one from Mojang's.
        let fetcher: &dyn Fetch = if extra::ours(&entry.url) {
            self.extra
                .as_ref()
                .map(|e| e.fetcher.as_ref())
                .ok_or_else(net::untrusted_url)?
        } else {
            SourcePolicy::MojangMetadata.check(&entry.url)?;
            &self.fetcher
        };
        let fetched = fetcher
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

    /// Resolves a complete game plan for a catalog version; the catalog's
    /// descriptor hash is verified on download. Never selects `latest.release` on
    /// its own.
    pub async fn plan(&self, version_id: &str, ctx: &RuleContext) -> LauncherResult<GamePlan> {
        self.plan_in(version_id, ctx, LibraryLayout::Shared).await
    }

    pub fn root(&self) -> &DataRoot {
        &self.root
    }

    /// [`Self::plan`] in a chosen library layout (Maven for mod loaders).
    pub async fn plan_in(
        &self,
        version_id: &str,
        ctx: &RuleContext,
        layout: LibraryLayout,
    ) -> LauncherResult<GamePlan> {
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
        if !descriptor::VERSION_TYPES.contains(&entry.version_type.as_str()) {
            return Err(descriptor::unsupported("version type"));
        }
        let bytes = self.descriptor(entry).await?;
        let mut plan =
            plan::plan_game_in(&bytes, &entry.url, &entry.sha1, version_id, ctx, layout)?;
        let index = self.asset_index(&plan.asset_index.artifact).await?;
        plan::plan_assets(&mut plan, &index)?;
        Ok(plan)
    }

    /// Rebuilds the plan of an installed version from its locked descriptor URL and
    /// hash, without consulting the current catalog (Repair, launch preflight).
    pub async fn plan_locked(
        &self,
        version_id: &str,
        descriptor_url: &str,
        descriptor_sha1: &Sha1,
        ctx: &RuleContext,
        layout: LibraryLayout,
    ) -> LauncherResult<GamePlan> {
        let entry = CatalogEntry {
            id: version_id.to_owned(),
            // Not used for planning; the descriptor states its own type.
            version_type: String::new(),
            url: descriptor_url.to_owned(),
            sha1: descriptor_sha1.clone(),
            release_time: None,
            special: None,
        };
        let bytes = self.descriptor(&entry).await?;
        let mut plan =
            plan::plan_game_in(&bytes, &entry.url, &entry.sha1, version_id, ctx, layout)?;
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
