use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde_json::json;

use super::*;
use crate::install::rules::MetaOs;
use crate::storage::models::ArtifactRole;

const V1_21_1: &[u8] = include_bytes!("../../../../tests/fixtures/mojang/1.21.1.json");
const V26_3: &[u8] = include_bytes!("../../../../tests/fixtures/mojang/26.3.json");
const SHA_1_21_1: &str = "499680d659b7a0ccb2fa9a809c345c5baf2ace7d";
const SHA_26_3: &str = "11f1aefe473eefba0af62e3286c33b692807b2eb";

/// Body and ETag served for a URL.
type Response = LauncherResult<(Vec<u8>, Option<String>)>;
/// Requested URL and the ETag sent with it.
type Call = (String, Option<String>);

#[derive(Clone, Default)]
struct FixtureFetcher {
    responses: Arc<Mutex<HashMap<String, Response>>>,
    calls: Arc<Mutex<Vec<Call>>>,
}

impl FixtureFetcher {
    fn set(&self, url: &str, response: Response) {
        self.responses.lock().unwrap().insert(url.into(), response);
    }
    fn calls_to(&self, url: &str) -> usize {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|(u, _)| u == url)
            .count()
    }
}

impl Fetch for FixtureFetcher {
    fn fetch<'a>(
        &'a self,
        url: &'a str,
        etag: Option<&'a str>,
        _last_modified: Option<&'a str>,
        _max_bytes: usize,
    ) -> FetchFuture<'a> {
        Box::pin(async move {
            self.calls
                .lock()
                .unwrap()
                .push((url.to_owned(), etag.map(str::to_owned)));
            let response = self
                .responses
                .lock()
                .unwrap()
                .get(url)
                .cloned()
                .unwrap_or_else(|| Err(LauncherError::new(ErrorCode::NetworkError, "offline")));
            let (body, served_etag) = response?;
            if etag.is_some() && etag == served_etag.as_deref() {
                return Ok(Fetched {
                    not_modified: true,
                    etag: served_etag,
                    last_modified: None,
                    body: Vec::new(),
                });
            }
            Ok(Fetched {
                not_modified: false,
                etag: served_etag,
                last_modified: None,
                body,
            })
        })
    }
}

fn descriptor_url(sha: &str, id: &str) -> String {
    format!("https://piston-meta.mojang.com/v1/packages/{sha}/{id}.json")
}

fn catalog_bytes(sha_26_3: &str) -> Vec<u8> {
    json!({
        "latest": {"release": "26.3", "snapshot": "26.4-snapshot-4"},
        "versions": [
            {"id": "26.4-snapshot-4", "type": "snapshot", "url": descriptor_url("0000000000000000000000000000000000000001", "x"), "sha1": "0000000000000000000000000000000000000001"},
            {"id": "26.3", "type": "release", "url": descriptor_url(sha_26_3, "26.3"), "sha1": sha_26_3},
            {"id": "1.21.1", "type": "release", "url": descriptor_url(SHA_1_21_1, "1.21.1"), "sha1": SHA_1_21_1},
        ]
    })
    .to_string()
    .into_bytes()
}

fn service() -> (
    tempfile::TempDir,
    FixtureFetcher,
    MetadataService<FixtureFetcher>,
) {
    let dir = tempfile::tempdir().unwrap();
    let root = DataRoot::new(dir.path().to_path_buf()).unwrap();
    let fetcher = FixtureFetcher::default();
    fetcher.set(
        manifest::CATALOG_URL,
        Ok((catalog_bytes(SHA_26_3), Some("\"v1\"".into()))),
    );
    fetcher.set(
        &descriptor_url(SHA_1_21_1, "1.21.1"),
        Ok((V1_21_1.to_vec(), None)),
    );
    fetcher.set(
        &descriptor_url(SHA_26_3, "26.3"),
        Ok((V26_3.to_vec(), None)),
    );
    let service = MetadataService::new(fetcher.clone(), root);
    (dir, fetcher, service)
}

#[tokio::test]
async fn catalog_is_cached_and_revalidated_with_etag() {
    let (_dir, fetcher, service) = service();
    let view = service.supported_versions(false).await;
    assert!(view.error.is_none());
    assert_eq!(view.latest_release.as_deref(), Some("26.3"));
    assert!(
        view.versions
            .iter()
            .all(|v| v.availability == Availability::Available)
    );
    assert_eq!(fetcher.calls_to(manifest::CATALOG_URL), 1);

    // Fresh cache: no request.
    service.supported_versions(false).await;
    assert_eq!(fetcher.calls_to(manifest::CATALOG_URL), 1);

    // Forced refresh revalidates with the stored ETag and accepts 304.
    let view = service.supported_versions(true).await;
    assert!(view.error.is_none() && !view.stale);
    let calls = fetcher.calls.lock().unwrap().clone();
    assert_eq!(calls.last().unwrap().1.as_deref(), Some("\"v1\""));
}

#[tokio::test]
async fn outage_uses_the_cache_without_fabricating_versions() {
    let (_dir, fetcher, service) = service();
    service.supported_versions(false).await;
    fetcher.set(
        manifest::CATALOG_URL,
        Err(LauncherError::new(ErrorCode::NetworkError, "down")),
    );
    let view = service.supported_versions(true).await;
    assert!(view.stale);
    assert_eq!(view.error.unwrap().code, ErrorCode::NetworkError);
    assert!(
        view.versions
            .iter()
            .all(|v| v.availability == Availability::Available)
    );

    let (_dir, fetcher, service) = self::service();
    fetcher.set(
        manifest::CATALOG_URL,
        Err(LauncherError::new(ErrorCode::NetworkError, "down")),
    );
    let view = service.supported_versions(false).await;
    assert!(
        view.versions
            .iter()
            .all(|v| v.availability == Availability::CatalogUnavailable)
    );
    assert!(view.latest_release.is_none());
}

#[tokio::test]
async fn every_catalog_version_is_listed_and_only_pinned_ones_are_verified() {
    let (_dir, fetcher, service) = service();
    let view = service.supported_versions(false).await;
    let ids: Vec<_> = view.versions.iter().map(|v| v.id.as_str()).collect();
    assert_eq!(ids, ["26.4-snapshot-4", "26.3", "1.21.1"], "catalog order");
    let snapshot = &view.versions[0];
    assert_eq!(snapshot.version_type, "snapshot");
    assert!(!snapshot.verified);
    assert_eq!(snapshot.availability, Availability::Available);
    assert!(view.versions[1].verified && view.versions[2].verified);
    assert_eq!(view.latest_snapshot.as_deref(), Some("26.4-snapshot-4"));

    // A changed descriptor hash is an ordinary catalog version, no longer verified.
    fetcher.set(
        manifest::CATALOG_URL,
        Ok((
            catalog_bytes("2222222222222222222222222222222222222222"),
            None,
        )),
    );
    let view = service.supported_versions(true).await;
    let v26 = view.versions.iter().find(|v| v.id == "26.3").unwrap();
    assert!(!v26.verified);
    assert_eq!(v26.availability, Availability::Available);

    // Unknown version types (none exist today) are not offered.
    let odd = json!({"latest": {"release": "27.0"}, "versions": [
        {"id": "27.0", "type": "release", "url": descriptor_url(SHA_26_3, "27.0"), "sha1": SHA_26_3},
        {"id": "x1", "type": "experiment", "url": descriptor_url(SHA_26_3, "x1"), "sha1": SHA_26_3}]});
    fetcher.set(
        manifest::CATALOG_URL,
        Ok((odd.to_string().into_bytes(), None)),
    );
    let view = service.supported_versions(true).await;
    let ids: Vec<_> = view.versions.iter().map(|v| v.id.as_str()).collect();
    assert_eq!(ids, ["27.0"]);
    assert!(!view.versions[0].verified);
}

#[tokio::test]
async fn descriptors_are_verified_stored_unchanged_and_reused() {
    let (dir, fetcher, service) = service();
    let (catalog, ..) = service.catalog(false).await;
    let entry = catalog.unwrap().find("1.21.1").unwrap().clone();
    let bytes = service.descriptor(&entry).await.unwrap();
    assert_eq!(bytes, V1_21_1);
    let stored = dir
        .path()
        .join("metadata/versions/1.21.1")
        .join(format!("{SHA_1_21_1}.json"));
    assert_eq!(std::fs::read(&stored).unwrap(), V1_21_1, "byte-for-byte");
    service.descriptor(&entry).await.unwrap();
    assert_eq!(
        fetcher.calls_to(&entry.url),
        1,
        "second use comes from the cache"
    );

    // A corrupted cache file is detected and replaced from the source.
    std::fs::write(&stored, b"tampered").unwrap();
    assert_eq!(service.descriptor(&entry).await.unwrap(), V1_21_1);
    assert_eq!(fetcher.calls_to(&entry.url), 2);
}

#[tokio::test]
async fn wrong_descriptor_bytes_are_rejected_and_not_stored() {
    let (dir, fetcher, service) = service();
    fetcher.set(
        &descriptor_url(SHA_1_21_1, "1.21.1"),
        Ok((V26_3.to_vec(), None)),
    );
    let (catalog, ..) = service.catalog(false).await;
    let entry = catalog.unwrap().find("1.21.1").unwrap().clone();
    assert_eq!(
        service.descriptor(&entry).await.unwrap_err().code,
        ErrorCode::HashMismatch
    );
    assert!(
        !dir.path()
            .join("metadata/versions/1.21.1")
            .join(format!("{SHA_1_21_1}.json"))
            .exists()
    );
}

#[tokio::test]
async fn asset_index_is_hash_and_size_checked() {
    let (_dir, fetcher, service) = service();
    let body = br#"{"objects":{}}"#.to_vec();
    let artifact = PlannedArtifact {
        role: ArtifactRole::AssetIndex,
        path: "metadata/asset-indexes/x.json".into(),
        url: "https://piston-meta.mojang.com/v1/packages/x/test.json".into(),
        sha1: Sha1::parse(&sha1_hex(&body)).unwrap(),
        size: body.len() as u64,
        provenance: None,
    };
    fetcher.set(&artifact.url, Ok((body.clone(), None)));
    assert_eq!(service.asset_index(&artifact).await.unwrap(), body);
    let wrong_size = PlannedArtifact {
        size: 1,
        ..artifact.clone()
    };
    std::fs::remove_file(service.root.asset_index_source(&artifact.sha1)).unwrap();
    assert_eq!(
        service.asset_index(&wrong_size).await.unwrap_err().code,
        ErrorCode::HashMismatch
    );
}

#[tokio::test]
async fn plan_requires_a_verified_asset_index() {
    let (_dir, _fetcher, service) = service();
    // The real index URL is not served by the fixture: planning fails cleanly.
    let error = service
        .plan(
            "1.21.1",
            &RuleContext::with_default_features(MetaOs::Windows, "x86_64", None),
        )
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::NetworkError);
    assert!(
        service
            .plan(
                "1.20",
                &RuleContext::with_default_features(MetaOs::Windows, "x86_64", None)
            )
            .await
            .is_err()
    );
}
