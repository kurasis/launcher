//! Unauthenticated HTTPS access to public metadata and artifact sources
//! (docs/spec/07 "Trust boundaries", "Network host categories").
//!
//! - HTTPS only; every request URL and every redirect hop must match the exact host
//!   allowlist of its source category; at most five redirects.
//! - Names on the allowlist must resolve to public addresses: the client's DNS
//!   resolver rejects loopback, private, link-local and other special-purpose ranges
//!   for them. Requests can only target allowlisted names, so any other lookup is the
//!   user's configured proxy, which is resolved normally (the proxy then resolves the
//!   destination itself).
//! - Never carries credentials; it is a separate client from the auth client.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use url::Url;

use crate::error::{ErrorCode, LauncherError, LauncherResult};

/// Source categories with their exact allowed hosts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SourcePolicy {
    /// Version catalog, descriptors and asset indexes.
    MojangMetadata,
    /// Client JARs, libraries, logging configs and asset objects.
    MojangArtifacts,
    /// Adoptium API and runtime archives (including the GitHub release redirect).
    Adoptium,
    /// Mod loader metadata, installers and libraries: Fabric, Quilt, Forge and
    /// NeoForge (D-039).
    ModLoaders,
    /// Our crash report service; reports leave only when the player sends them
    /// (D-061).
    CrashReports,
    /// Files of the catalog of mods (D-066), checked against the SHA-256 the
    /// catalog gives. Not on the download mirror.
    ModsCatalog,
}

// `launchermeta` is Mojang's previous metadata host; older descriptors still
// point their asset indexes there.
const MOJANG_METADATA_HOSTS: &[&str] = &["piston-meta.mojang.com", "launchermeta.mojang.com"];
const MOJANG_ARTIFACT_HOSTS: &[&str] = &[
    "piston-data.mojang.com",
    "launchermeta.mojang.com",
    // Mojang's previous artifact host; descriptors of older versions still use it.
    "launcher.mojang.com",
    "piston-meta.mojang.com",
    "libraries.minecraft.net",
    "resources.download.minecraft.net",
];
const ADOPTIUM_HOSTS: &[&str] = &[
    "api.adoptium.net",
    "github.com",
    "objects.githubusercontent.com",
    "release-assets.githubusercontent.com",
];
const MOD_LOADER_HOSTS: &[&str] = &[
    "meta.fabricmc.net",
    "maven.fabricmc.net",
    "meta.quiltmc.org",
    "maven.quiltmc.org",
    "maven.minecraftforge.net",
    "files.minecraftforge.net",
    "maven.neoforged.net",
];

/// Every host any policy allows; these names must resolve to public addresses.
const ALL_SOURCE_HOSTS: &[&str] = &[
    "piston-meta.mojang.com",
    "piston-data.mojang.com",
    "launcher.mojang.com",
    "launchermeta.mojang.com",
    "libraries.minecraft.net",
    "resources.download.minecraft.net",
    "api.adoptium.net",
    "github.com",
    "objects.githubusercontent.com",
    "release-assets.githubusercontent.com",
    "meta.fabricmc.net",
    "maven.fabricmc.net",
    "meta.quiltmc.org",
    "maven.quiltmc.org",
    "maven.minecraftforge.net",
    "files.minecraftforge.net",
    "maven.neoforged.net",
    crate::mirror::MIRROR_HOST,
    "crash.lapi.sh",
    crate::mods::FILES_HOST,
];

const CRASH_REPORT_HOSTS: &[&str] = &["crash.lapi.sh"];
const MODS_CATALOG_HOSTS: &[&str] = &[crate::mods::FILES_HOST];

pub const MAX_REDIRECTS: usize = 5;

impl SourcePolicy {
    pub fn hosts(self) -> &'static [&'static str] {
        match self {
            Self::MojangMetadata => MOJANG_METADATA_HOSTS,
            Self::MojangArtifacts => MOJANG_ARTIFACT_HOSTS,
            Self::Adoptium => ADOPTIUM_HOSTS,
            Self::ModLoaders => MOD_LOADER_HOSTS,
            Self::CrashReports => CRASH_REPORT_HOSTS,
            Self::ModsCatalog => MODS_CATALOG_HOSTS,
        }
    }

    /// Whether the download mirror (D-062) copies this source's files.
    pub fn mirrored(self) -> bool {
        !matches!(self, Self::CrashReports | Self::ModsCatalog)
    }

    /// The artifact source whose allowlist names this URL's host: Mojang's
    /// artifact hosts or a mod loader's. Loader profiles list both.
    pub fn for_artifact(raw: &str) -> LauncherResult<(Self, Url)> {
        let url = Url::parse(raw).map_err(|_| untrusted_url())?;
        match [Self::MojangArtifacts, Self::ModLoaders]
            .into_iter()
            .find(|policy| policy.allows(&url))
        {
            Some(policy) => Ok((policy, url)),
            None => {
                Err(untrusted_url().with_detail("host", url.host_str().unwrap_or("").to_owned()))
            }
        }
    }

    /// Exact-host check: HTTPS, default port, no credentials, host on the list.
    pub fn allows(self, url: &Url) -> bool {
        url.scheme() == "https"
            && url.port().is_none()
            && url.username().is_empty()
            && url.password().is_none()
            && url
                .host_str()
                .is_some_and(|host| self.hosts().contains(&host))
    }

    /// Parses and checks a URL that came from metadata.
    pub fn check(self, raw: &str) -> LauncherResult<Url> {
        let url = Url::parse(raw).map_err(|_| untrusted_url())?;
        if self.allows(&url) {
            Ok(url)
        } else {
            Err(untrusted_url().with_detail("host", url.host_str().unwrap_or("").to_owned()))
        }
    }
}

fn untrusted_url() -> LauncherError {
    LauncherError::new(
        ErrorCode::UnsupportedVersionMetadata,
        "Metadata points to a download location the launcher does not trust.",
    )
}

/// True for globally routable unicast addresses.
pub fn is_public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_public_v4(v4),
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_public_v4(v4);
            }
            let s = v6.segments();
            !(v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                || (s[0] & 0xfe00) == 0xfc00 // unique local fc00::/7
                || (s[0] & 0xffc0) == 0xfe80 // link-local fe80::/10
                || (s[0] == 0x2001 && s[1] == 0x0db8) // documentation
                || (s[0] == 0x0064 && s[1] == 0xff9b) // NAT64 well-known prefix
                || v6 == Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 1))
        }
    }
}

fn is_public_v4(v4: Ipv4Addr) -> bool {
    let [a, b, c, _] = v4.octets();
    !(v4.is_private()
        || v4.is_loopback()
        || v4.is_link_local()
        || v4.is_unspecified()
        || v4.is_broadcast()
        || v4.is_multicast()
        || v4.is_documentation()
        || a == 0
        || (a == 100 && (64..128).contains(&b)) // CGNAT 100.64/10
        || (a == 192 && b == 0 && c == 0) // IETF protocol assignments
        || (a == 198 && (b == 18 || b == 19)) // benchmarking
        || a >= 240) // reserved
}

/// DNS resolver that refuses non-public answers for source hosts.
#[derive(Debug, Default)]
struct PublicOnlyResolver;

impl Resolve for PublicOnlyResolver {
    fn resolve(&self, name: Name) -> Resolving {
        let host = name.as_str().to_owned();
        Box::pin(async move {
            let addrs: Vec<SocketAddr> =
                tokio::net::lookup_host((host.as_str(), 0)).await?.collect();
            let addrs: Vec<SocketAddr> = if ALL_SOURCE_HOSTS.contains(&host.as_str()) {
                addrs.into_iter().filter(|a| is_public_ip(a.ip())).collect()
            } else {
                addrs
            };
            if addrs.is_empty() {
                return Err(Box::new(std::io::Error::new(
                    std::io::ErrorKind::AddrNotAvailable,
                    "no public address",
                ))
                    as Box<dyn std::error::Error + Send + Sync>);
            }
            Ok(Box::new(addrs.into_iter()) as Addrs)
        })
    }
}

/// Follows at most [`MAX_REDIRECTS`] hops, each of which must pass `allows`.
pub fn redirect_policy(
    allows: impl Fn(&Url) -> bool + Send + Sync + 'static,
) -> reqwest::redirect::Policy {
    reqwest::redirect::Policy::custom(move |attempt| {
        // `previous` includes the original request, as in reqwest's own limit.
        if attempt.previous().len() > MAX_REDIRECTS {
            attempt.error("too many redirects")
        } else if allows(attempt.url()) {
            attempt.follow()
        } else {
            attempt.error("redirect to an untrusted host")
        }
    })
}

/// A public-source HTTPS client bound to one source policy, with the
/// download mirror (D-062) as the fallback.
#[derive(Clone)]
pub struct PublicClient {
    client: reqwest::Client,
    policy: SourcePolicy,
    /// HTTPS to mirror.lapi.sh only, no redirects.
    mirror: reqwest::Client,
    routing: Arc<crate::mirror::Routing>,
}

/// Worth asking again or asking the mirror: the request did not get an answer
/// that means anything about the document.
fn unanswered(
    result: &LauncherResult<(reqwest::StatusCode, reqwest::header::HeaderMap, Vec<u8>)>,
) -> bool {
    match result {
        Ok((status, ..)) => matches!(status.as_u16(), 408 | 429 | 500..=599),
        Err(error) => error.code == ErrorCode::NetworkError,
    }
}

fn missing(
    result: &LauncherResult<(reqwest::StatusCode, reqwest::header::HeaderMap, Vec<u8>)>,
) -> bool {
    matches!(result, Ok((status, ..)) if matches!(status.as_u16(), 404 | 410))
}

/// Network errors with a stable classification and no URLs in the message.
pub fn map_transport_error(error: &reqwest::Error) -> LauncherError {
    let kind = if error.is_timeout() {
        "timeout"
    } else if error.is_connect() {
        "connect"
    } else if error.is_redirect() {
        "redirect"
    } else if error.is_body() || error.is_decode() {
        "body"
    } else {
        "request"
    };
    if error.is_redirect() {
        return LauncherError::new(
            ErrorCode::NetworkError,
            "A download was redirected to a location the launcher does not trust.",
        )
        .with_detail("transport", kind);
    }
    LauncherError::new(
        ErrorCode::NetworkError,
        "A download source could not be reached. Check the internet connection and try again.",
    )
    .with_detail("transport", kind)
}

impl PublicClient {
    pub fn new(policy: SourcePolicy, app_version: &str) -> LauncherResult<Self> {
        Self::with_routing(policy, app_version, crate::mirror::shared())
    }

    pub fn with_routing(
        policy: SourcePolicy,
        app_version: &str,
        routing: Arc<crate::mirror::Routing>,
    ) -> LauncherResult<Self> {
        let failed = |_| {
            LauncherError::internal(
                "The HTTPS client could not be created. Check that the system CA certificates are installed.",
            )
        };
        let mirror = reqwest::Client::builder()
            .https_only(true)
            .redirect(reqwest::redirect::Policy::none())
            .dns_resolver(Arc::new(PublicOnlyResolver))
            .connect_timeout(Duration::from_secs(15))
            .read_timeout(Duration::from_secs(60))
            .user_agent(format!("Launcher/{app_version}"))
            .build()
            .map_err(failed)?;
        let client = reqwest::Client::builder()
            .https_only(true)
            .redirect(redirect_policy(move |url| policy.allows(url)))
            .dns_resolver(Arc::new(PublicOnlyResolver))
            .connect_timeout(Duration::from_secs(15))
            .read_timeout(Duration::from_secs(60))
            .user_agent(format!("Launcher/{app_version}"))
            .build()
            .map_err(failed)?;
        Ok(Self {
            client,
            policy,
            mirror,
            routing,
        })
    }

    pub fn routing(&self) -> &Arc<crate::mirror::Routing> {
        &self.routing
    }

    /// Starts a GET of `url`'s copy on the mirror.
    pub fn mirror_get(&self, url: &Url) -> LauncherResult<reqwest::RequestBuilder> {
        if !self.policy.allows(url) {
            return Err(untrusted_url());
        }
        let copy = self.routing.mirror_url(url).ok_or_else(untrusted_url)?;
        Ok(self.mirror.get(copy))
    }

    /// A small document with the mirror as the fallback (D-062): the origin
    /// (twice when unanswered), then the mirror when the origin did not
    /// answer or does not have it; in "mirror first" the other way round.
    /// `prepare` adds headers to each request.
    pub async fn fetch_bounded(
        &self,
        url: &Url,
        prepare: impl Fn(reqwest::RequestBuilder) -> reqwest::RequestBuilder,
        max_bytes: usize,
    ) -> LauncherResult<(reqwest::StatusCode, reqwest::header::HeaderMap, Vec<u8>)> {
        use crate::mirror::{ORIGIN_ATTEMPTS, Route};
        let host = url.host_str().unwrap_or_default().to_owned();
        let from_origin = || async {
            let mut result = self.get_bounded(prepare(self.get(url)?), max_bytes).await;
            for _ in 1..ORIGIN_ATTEMPTS {
                if !unanswered(&result) {
                    break;
                }
                tokio::time::sleep(Duration::from_secs(1)).await;
                result = self.get_bounded(prepare(self.get(url)?), max_bytes).await;
            }
            result
        };
        let from_mirror = || async {
            self.get_bounded(prepare(self.mirror_get(url)?), max_bytes)
                .await
        };
        match self.routing.first(&host) {
            Route::Origin => {
                let result = from_origin().await;
                if !(unanswered(&result) || missing(&result)) {
                    self.routing.origin_worked(&host);
                    return result;
                }
                if !self.routing.mirror_allowed() {
                    return result;
                }
                self.routing.origin_failed(&host);
                let copy = from_mirror().await;
                if unanswered(&copy) || missing(&copy) {
                    result
                } else {
                    copy
                }
            }
            Route::Mirror => {
                let copy = from_mirror().await;
                if !(unanswered(&copy) || missing(&copy)) {
                    return copy;
                }
                let result = from_origin().await;
                if !(unanswered(&result) || missing(&result)) {
                    self.routing.origin_worked(&host);
                }
                result
            }
        }
    }

    pub fn policy(&self) -> SourcePolicy {
        self.policy
    }

    pub fn inner(&self) -> &reqwest::Client {
        &self.client
    }

    /// Starts a GET after checking the URL against the policy.
    pub fn get(&self, url: &Url) -> LauncherResult<reqwest::RequestBuilder> {
        if !self.policy.allows(url) {
            return Err(untrusted_url());
        }
        Ok(self.client.get(url.clone()))
    }

    /// Starts a POST after checking the URL against the policy.
    pub fn post(&self, url: &Url) -> LauncherResult<reqwest::RequestBuilder> {
        if !self.policy.allows(url) {
            return Err(untrusted_url());
        }
        Ok(self.client.post(url.clone()))
    }

    /// Starts a HEAD request (headers only) after checking the URL against the policy.
    pub fn head(&self, url: &Url) -> LauncherResult<reqwest::RequestBuilder> {
        if !self.policy.allows(url) {
            return Err(untrusted_url());
        }
        Ok(self.client.head(url.clone()))
    }

    /// Fetches a small document fully into memory with a hard size limit.
    pub async fn get_bounded(
        &self,
        request: reqwest::RequestBuilder,
        max_bytes: usize,
    ) -> LauncherResult<(reqwest::StatusCode, reqwest::header::HeaderMap, Vec<u8>)> {
        let mut response = request.send().await.map_err(|e| map_transport_error(&e))?;
        let status = response.status();
        let headers = response.headers().clone();
        if response
            .content_length()
            .is_some_and(|l| l > max_bytes as u64)
        {
            return Err(too_large());
        }
        let mut body = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|e| map_transport_error(&e))?
        {
            crate::traffic::record(self.policy, chunk.len() as u64);
            if body.len() + chunk.len() > max_bytes {
                return Err(too_large());
            }
            body.extend_from_slice(&chunk);
        }
        Ok((status, headers, body))
    }
}

fn too_large() -> LauncherError {
    LauncherError::new(
        ErrorCode::UnsupportedVersionMetadata,
        "A metadata document exceeded the launcher's size limit.",
    )
}

/// Maps an HTTP error status from a public source.
pub fn status_error(status: reqwest::StatusCode, what: &str) -> LauncherError {
    let code = match status.as_u16() {
        429 => ErrorCode::RateLimited,
        500..=599 => ErrorCode::ServiceUnavailable,
        _ => ErrorCode::NetworkError,
    };
    LauncherError::templated(
        code,
        "Downloading {what} failed (HTTP {status}).",
        crate::error::params([("what", what.into()), ("status", status.as_u16().into())]),
    )
    .with_detail("httpStatus", i64::from(status.as_u16()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_hosts_per_policy() {
        let ok = |p: SourcePolicy, u: &str| p.allows(&Url::parse(u).unwrap());
        assert!(ok(
            SourcePolicy::MojangMetadata,
            "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json"
        ));
        assert!(!ok(
            SourcePolicy::MojangMetadata,
            "https://piston-data.mojang.com/x"
        ));
        assert!(ok(
            SourcePolicy::MojangArtifacts,
            "https://libraries.minecraft.net/a/b.jar"
        ));
        assert!(ok(
            SourcePolicy::MojangArtifacts,
            "https://resources.download.minecraft.net/ab/abcd"
        ));
        assert!(ok(
            SourcePolicy::Adoptium,
            "https://github.com/adoptium/temurin21-binaries/releases/download/x/y.zip"
        ));
        for bad in [
            "http://piston-data.mojang.com/x",
            "https://piston-data.mojang.com:8443/x",
            "https://user@piston-data.mojang.com/x",
            "https://piston-data.mojang.com.evil.example/x",
            "https://evil.example/piston-data.mojang.com",
            "https://mojang.com/x",
            "https://127.0.0.1/x",
            "file:///etc/passwd",
        ] {
            assert!(!ok(SourcePolicy::MojangArtifacts, bad), "{bad}");
        }
        assert!(!ok(
            SourcePolicy::Adoptium,
            "https://libraries.minecraft.net/x"
        ));
        assert!(ok(
            SourcePolicy::ModLoaders,
            "https://maven.neoforged.net/releases/net/neoforged/neoforge/21.1.262/x.jar"
        ));
        assert!(!ok(
            SourcePolicy::ModLoaders,
            "https://maven.fabricmc.net.evil.example/x"
        ));
        let (policy, _) =
            SourcePolicy::for_artifact("https://libraries.minecraft.net/a/b.jar").unwrap();
        assert_eq!(policy, SourcePolicy::MojangArtifacts);
        let (policy, _) = SourcePolicy::for_artifact("https://maven.fabricmc.net/a/b.jar").unwrap();
        assert_eq!(policy, SourcePolicy::ModLoaders);
        assert!(SourcePolicy::for_artifact("https://api.adoptium.net/x").is_err());
        assert!(SourcePolicy::MojangArtifacts.check("not a url").is_err());
    }

    #[test]
    fn special_purpose_addresses_are_not_public() {
        for private in [
            "127.0.0.1",
            "10.1.2.3",
            "172.16.0.1",
            "192.168.1.1",
            "169.254.169.254",
            "0.0.0.0",
            "100.64.0.1",
            "192.0.0.8",
            "198.18.0.1",
            "224.0.0.1",
            "255.255.255.255",
            "240.0.0.1",
            "192.0.2.1",
            "::1",
            "::",
            "fc00::1",
            "fd12::1",
            "fe80::1",
            "2001:db8::1",
            "::ffff:10.0.0.1",
            "::ffff:127.0.0.1",
            "64:ff9b::a00:1",
        ] {
            assert!(!is_public_ip(private.parse().unwrap()), "{private}");
        }
        for public in [
            "13.107.4.52",
            "140.82.112.3",
            "2606:4700::6810:84e5",
            "::ffff:8.8.8.8",
        ] {
            assert!(is_public_ip(public.parse().unwrap()), "{public}");
        }
    }

    #[test]
    fn source_hosts_cover_all_policies() {
        for policy in [
            SourcePolicy::MojangMetadata,
            SourcePolicy::MojangArtifacts,
            SourcePolicy::Adoptium,
            SourcePolicy::ModLoaders,
            SourcePolicy::CrashReports,
            SourcePolicy::ModsCatalog,
        ] {
            for host in policy.hosts() {
                assert!(ALL_SOURCE_HOSTS.contains(host), "{host}");
            }
        }
    }
}
