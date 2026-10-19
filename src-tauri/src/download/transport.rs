//! HTTP transport for artifact downloads. Each request goes through the
//! [`PublicClient`] of its source category, so URLs and every redirect hop are
//! checked against that category's exact hosts and resolve only to public
//! addresses. No credentials are ever attached.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use url::Url;

use crate::error::{LauncherError, LauncherResult};
use crate::mirror::{Route, Routing};
use crate::net::{self, PublicClient, SourcePolicy};

/// Why one download attempt failed, which decides whether it is retried.
#[derive(Debug)]
pub enum Failure {
    /// Connection problems, timeouts, short reads, 5xx and 429.
    Transient {
        error: LauncherError,
        retry_after: Option<Duration>,
    },
    /// The received bytes did not match the expected size or digest.
    Integrity(LauncherError),
    /// The source answered 404 or 410: the mirror may still have it (D-062).
    Missing(LauncherError),
    /// The origin is very slow; the mirror may be faster (D-062).
    Slow(LauncherError),
    /// Retrying cannot help (policy violation, other 4xx, local storage failure).
    Fatal(LauncherError),
    Cancelled,
}

impl Failure {
    pub fn transient(error: LauncherError) -> Self {
        Self::Transient {
            error,
            retry_after: None,
        }
    }
}

enum Routes {
    Public(HashMap<SourcePolicy, PublicClient>),
    /// Plain HTTP to 127.0.0.1 for the fixture test suite only.
    #[cfg(test)]
    Loopback(reqwest::Client),
}

#[derive(Clone)]
pub struct HttpTransport {
    routes: Arc<Routes>,
    routing: Arc<Routing>,
}

#[cfg(test)]
fn is_loopback(url: &Url) -> bool {
    url.scheme() == "http" && url.host_str() == Some("127.0.0.1")
}

impl HttpTransport {
    /// One public HTTPS client per source category.
    pub fn public(app_version: &str) -> LauncherResult<Self> {
        let mut clients = HashMap::new();
        for policy in [
            SourcePolicy::MojangMetadata,
            SourcePolicy::MojangArtifacts,
            SourcePolicy::Adoptium,
            SourcePolicy::ModLoaders,
            SourcePolicy::ModsCatalog,
        ] {
            clients.insert(policy, PublicClient::new(policy, app_version)?);
        }
        Ok(Self {
            routes: Arc::new(Routes::Public(clients)),
            routing: crate::mirror::shared(),
        })
    }

    /// One source category, from its origin only: never the mirror, and
    /// apart from the launcher's shared routing (the skins mod, D-065).
    pub fn origin_only(source: SourcePolicy, app_version: &str) -> LauncherResult<Self> {
        let routing = crate::mirror::origin_only();
        let client = PublicClient::with_routing(source, app_version, Arc::clone(&routing))?;
        Ok(Self {
            routes: Arc::new(Routes::Public(HashMap::from([(source, client)]))),
            routing,
        })
    }

    /// Where downloads go first and how failures move them (D-062).
    pub fn routing(&self) -> &Arc<Routing> {
        &self.routing
    }

    /// A client for the local fixture server: plain HTTP, `127.0.0.1` only, the
    /// same redirect rules as production with the loopback host as the allowlist.
    /// Without the mirror, as before D-062.
    #[cfg(test)]
    pub fn loopback() -> Self {
        let routing = Routing::new("http://127.0.0.1/");
        routing.set_mode(crate::mirror::DownloadSource::OriginOnly);
        Self::loopback_with(Arc::new(routing))
    }

    /// With a mirror on the fixture server (`routing`'s base).
    #[cfg(test)]
    pub fn loopback_with(routing: Arc<Routing>) -> Self {
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(net::redirect_policy(is_loopback))
            .connect_timeout(Duration::from_secs(2))
            .build()
            .expect("loopback client");
        Self {
            routes: Arc::new(Routes::Loopback(client)),
            routing,
        }
    }

    /// Sends the request to the origin or its mirror copy and classifies
    /// anything but `200 OK`.
    pub async fn open(
        &self,
        source: SourcePolicy,
        url: &Url,
        route: Route,
    ) -> Result<reqwest::Response, Failure> {
        let request = match &*self.routes {
            Routes::Public(clients) => {
                let client = clients.get(&source).ok_or_else(|| {
                    Failure::Fatal(LauncherError::internal("No client for source."))
                })?;
                match route {
                    Route::Origin => client.get(url),
                    Route::Mirror => client.mirror_get(url),
                }
                .map_err(Failure::Fatal)?
            }
            #[cfg(test)]
            Routes::Loopback(client) => {
                let url = match route {
                    Route::Origin => url.clone(),
                    Route::Mirror => self
                        .routing
                        .mirror_url(url)
                        .ok_or_else(|| Failure::Fatal(LauncherError::internal("No mirror URL.")))?,
                };
                if !is_loopback(&url) {
                    return Err(Failure::Fatal(LauncherError::new(
                        crate::error::ErrorCode::NetworkError,
                        "Untrusted test URL.",
                    )));
                }
                client.get(url)
            }
        };
        let response = request.send().await.map_err(|e| classify(&e))?;
        let status = response.status();
        if status == reqwest::StatusCode::OK {
            return Ok(response);
        }
        let retry_after = response
            .headers()
            .get(reqwest::header::RETRY_AFTER)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| parse_retry_after(v, Utc::now()));
        let error = net::status_error(status, "a game file");
        Err(match status.as_u16() {
            408 | 429 | 500 | 502 | 503 | 504 => Failure::Transient { error, retry_after },
            404 | 410 => Failure::Missing(error),
            _ => Failure::Fatal(error),
        })
    }
}

/// Transport errors: a refused redirect is a policy violation, everything else
/// (connect, timeout, reset, truncated body) is worth another attempt.
pub fn classify(error: &reqwest::Error) -> Failure {
    let mapped = net::map_transport_error(error);
    if error.is_redirect() || error.is_builder() {
        Failure::Fatal(mapped)
    } else {
        Failure::transient(mapped)
    }
}

/// `Retry-After` as delta-seconds or an HTTP date; past dates mean "now".
pub fn parse_retry_after(value: &str, now: DateTime<Utc>) -> Option<Duration> {
    let value = value.trim();
    if let Ok(seconds) = value.parse::<u64>() {
        return Some(Duration::from_secs(seconds));
    }
    let date = DateTime::parse_from_rfc2822(value).ok()?;
    let delta = date.with_timezone(&Utc) - now;
    Some(delta.to_std().unwrap_or(Duration::ZERO))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retry_after_forms() {
        let now = DateTime::parse_from_rfc3339("2026-10-17T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        assert_eq!(parse_retry_after("7", now), Some(Duration::from_secs(7)));
        assert_eq!(
            parse_retry_after("Sat, 17 Oct 2026 00:00:30 GMT", now),
            Some(Duration::from_secs(30))
        );
        assert_eq!(
            parse_retry_after("Fri, 16 Oct 2026 00:00:00 GMT", now),
            Some(Duration::ZERO)
        );
        assert_eq!(parse_retry_after("soon", now), None);
        assert_eq!(parse_retry_after("-5", now), None);
    }

    #[tokio::test]
    async fn production_routes_refuse_urls_outside_the_source() {
        let transport = HttpTransport::public("test").unwrap();
        for (source, url) in [
            (SourcePolicy::MojangArtifacts, "https://api.adoptium.net/x"),
            (
                SourcePolicy::Adoptium,
                "https://libraries.minecraft.net/x.jar",
            ),
            (
                SourcePolicy::MojangArtifacts,
                "http://libraries.minecraft.net/x.jar",
            ),
            (SourcePolicy::MojangArtifacts, "https://127.0.0.1/x"),
        ] {
            let result = transport
                .open(source, &Url::parse(url).unwrap(), Route::Origin)
                .await;
            assert!(matches!(result, Err(Failure::Fatal(_))), "{url}");
        }
    }

    #[tokio::test]
    async fn an_origin_only_transport_has_one_source_and_no_mirror() {
        let transport = HttpTransport::origin_only(SourcePolicy::SkinsMod, "test").unwrap();
        assert!(!transport.routing().mirror_allowed());
        assert_eq!(transport.routing().first("github.com"), Route::Origin);
        for (source, url) in [
            (
                SourcePolicy::SkinsMod,
                "https://piston-data.mojang.com/x.jar",
            ),
            (SourcePolicy::Adoptium, "https://github.com/x/y.jar"),
        ] {
            let result = transport
                .open(source, &Url::parse(url).unwrap(), Route::Origin)
                .await;
            assert!(matches!(result, Err(Failure::Fatal(_))), "{url}");
        }
    }
}
