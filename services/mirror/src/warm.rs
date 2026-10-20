//! `lapi-mirror warm`: fetches what players will need into the mirror before
//! they ask (D-062), so an origin that goes down finds its files already kept.
//!
//! It asks the running service for everything (the service keeps files and
//! metadata, applies the quota and fetches each file once), and skips files
//! already kept. What it covers, as the launcher requests it:
//! - Mojang: every release's version file, client jar, server jar (worlds with
//!   friends), libraries, logging config, asset index and asset objects, and
//!   Mojang's Java runtimes;
//! - Adoptium: the JRE (or JDK where there is none) of every Java major those
//!   releases need, for Windows, Linux and macOS on x64 and arm64;
//! - Fabric and Quilt: the newest stable loader's profile and libraries for
//!   every release they support;
//! - Forge and NeoForge: the recommended and newest installer per release.
//!
//! Snapshots and older loader versions are kept when first asked for.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use futures_util::StreamExt as _;
use serde_json::Value;

use crate::origin::{Class, parse};
use crate::store::Store;

const MANIFEST: &str = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";
const JAVA_INDEX: &str = "https://piston-meta.mojang.com/v1/products/java-runtime/2ec0cc96c44e5a76b9c8b7c39df7210883d12871/all.json";
const ADOPTIUM: &str = "https://api.adoptium.net/v3/assets/latest";
const FABRIC_META: &str = "https://meta.fabricmc.net/v2";
const QUILT_META: &str = "https://meta.quiltmc.org/v3";
const FORGE_PROMOTIONS: &str =
    "https://files.minecraftforge.net/net/minecraftforge/forge/promotions_slim.json";
const FORGE_MAVEN: &str = "https://maven.minecraftforge.net";
const NEOFORGE_VERSIONS: &str =
    "https://maven.neoforged.net/api/maven/versions/releases/net/neoforged/neoforge";
const NEOFORGE_MAVEN: &str = "https://maven.neoforged.net/releases";
/// Files fetched at once: polite to the origins.
const AT_ONCE: usize = 8;

#[derive(Default)]
struct Counts {
    kept: AtomicU64,
    fetched: AtomicU64,
    failed: AtomicU64,
}

pub struct Warm<'a> {
    http: reqwest::Client,
    service: String,
    store: &'a Store,
    seen: Mutex<HashSet<String>>,
    counts: Counts,
}

/// `https://<host>/<path>` as a request to the service.
fn local(service: &str, url: &str) -> Option<String> {
    url.strip_prefix("https://")
        .map(|rest| format!("{service}/{rest}"))
}

/// `group:artifact:version[:classifier][@ext]` as a Maven path.
fn maven_path(name: &str) -> Option<String> {
    let (coordinate, ext) = name.split_once('@').unwrap_or((name, "jar"));
    let parts: Vec<&str> = coordinate.split(':').collect();
    let (group, artifact, version) = (parts.first()?, parts.get(1)?, parts.get(2)?);
    let file = match parts.get(3) {
        Some(classifier) => format!("{artifact}-{version}-{classifier}.{ext}"),
        None => format!("{artifact}-{version}.{ext}"),
    };
    Some(format!(
        "{}/{artifact}/{version}/{file}",
        group.replace('.', "/")
    ))
}

fn text(value: &Value, pointer: &str) -> Option<String> {
    value
        .pointer(pointer)
        .and_then(Value::as_str)
        .map(str::to_owned)
}

impl<'a> Warm<'a> {
    pub fn new(store: &'a Store, service: &str) -> Self {
        Self {
            http: reqwest::Client::builder()
                .no_proxy()
                .build()
                .unwrap_or_default(),
            service: service.trim_end_matches('/').to_owned(),
            store,
            seen: Mutex::new(HashSet::new()),
            counts: Counts::default(),
        }
    }

    async fn json(&self, url: &str) -> Option<Value> {
        let response = self
            .http
            .get(local(&self.service, url)?)
            .send()
            .await
            .ok()?;
        if !response.status().is_success() {
            return None;
        }
        serde_json::from_slice(&response.bytes().await.ok()?).ok()
    }

    /// Makes sure the mirror keeps one file.
    async fn file(&self, url: String) {
        let Some(origin) = url
            .strip_prefix("https://")
            .and_then(|rest| rest.split_once('/'))
            .and_then(|(host, path)| {
                let (path, query) = path
                    .split_once('?')
                    .map_or((path, None), |(p, q)| (p, Some(q)));
                parse(&format!("/{host}/{path}"), query)
            })
        else {
            return;
        };
        if let Class::File(relative) = &origin.class
            && self.store.file(relative).exists()
        {
            self.counts.kept.fetch_add(1, Ordering::Relaxed);
            return;
        }
        let ok = match local(&self.service, &url) {
            Some(request) => match self.http.get(request).send().await {
                Ok(response) if response.status().is_success() => {
                    let mut body = response.bytes_stream();
                    let mut complete = true;
                    while let Some(chunk) = body.next().await {
                        if chunk.is_err() {
                            complete = false;
                            break;
                        }
                    }
                    complete
                }
                _ => false,
            },
            None => false,
        };
        let count = if ok {
            &self.counts.fetched
        } else {
            &self.counts.failed
        };
        count.fetch_add(1, Ordering::Relaxed);
    }

    /// Fetches the files not seen yet in this run, [`AT_ONCE`] at a time.
    async fn files(&self, urls: impl IntoIterator<Item = String>) {
        let new: Vec<String> = {
            let mut seen = self
                .seen
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            urls.into_iter()
                .filter(|u| seen.insert(u.clone()))
                .collect()
        };
        futures_util::stream::iter(new)
            .map(|url| self.file(url))
            .buffer_unordered(AT_ONCE)
            .collect::<Vec<()>>()
            .await;
    }

    /// Everything above; returns a one-line summary.
    pub async fn run(&self) -> String {
        let releases = self.mojang().await;
        self.mojang_java().await;
        self.adoptium(&releases).await;
        let games: Vec<String> = releases.keys().cloned().collect();
        self.fabric_like(FABRIC_META, &games).await;
        self.fabric_like(QUILT_META, &games).await;
        self.forge(&games).await;
        self.neoforge().await;
        format!(
            "{} releases; files already kept {}, fetched {}, failed {}",
            releases.len(),
            self.counts.kept.load(Ordering::Relaxed),
            self.counts.fetched.load(Ordering::Relaxed),
            self.counts.failed.load(Ordering::Relaxed)
        )
    }

    /// Mojang's releases; returns each release's Java major.
    async fn mojang(&self) -> BTreeMap<String, u64> {
        let mut releases = BTreeMap::new();
        let Some(manifest) = self.json(MANIFEST).await else {
            eprintln!("warm: no version list");
            return releases;
        };
        let versions = manifest["versions"].as_array().cloned().unwrap_or_default();
        for version in versions.iter().filter(|v| v["type"] == "release") {
            let (Some(id), Some(url)) = (text(version, "/id"), text(version, "/url")) else {
                continue;
            };
            let Some(descriptor) = self.json(&url).await else {
                continue;
            };
            eprintln!("warm: Minecraft {id}");
            releases.insert(
                id,
                descriptor
                    .pointer("/javaVersion/majorVersion")
                    .and_then(Value::as_u64)
                    .unwrap_or(8),
            );
            let mut urls = vec![url];
            urls.extend(text(&descriptor, "/downloads/client/url"));
            urls.extend(text(&descriptor, "/downloads/server/url"));
            urls.extend(text(&descriptor, "/logging/client/file/url"));
            for library in descriptor["libraries"].as_array().into_iter().flatten() {
                urls.extend(text(library, "/downloads/artifact/url"));
                if let Some(classifiers) = library
                    .pointer("/downloads/classifiers")
                    .and_then(Value::as_object)
                {
                    urls.extend(classifiers.values().filter_map(|c| text(c, "/url")));
                }
            }
            if let Some(index_url) = text(&descriptor, "/assetIndex/url") {
                urls.push(index_url.clone());
                if let Some(index) = self.json(&index_url).await {
                    for object in index["objects"]
                        .as_object()
                        .into_iter()
                        .flatten()
                        .map(|(_, o)| o)
                    {
                        if let Some(hash) = text(object, "/hash").filter(|h| h.len() > 2) {
                            urls.push(format!(
                                "https://resources.download.minecraft.net/{}/{hash}",
                                &hash[..2]
                            ));
                        }
                    }
                }
            }
            self.files(urls).await;
        }
        releases
    }

    async fn mojang_java(&self) {
        eprintln!("warm: Mojang's Java runtimes");
        let Some(index) = self.json(JAVA_INDEX).await else {
            return;
        };
        let mut manifests = Vec::new();
        for components in index.as_object().into_iter().flatten().map(|(_, c)| c) {
            for entries in components.as_object().into_iter().flatten().map(|(_, e)| e) {
                manifests.extend(entries.get(0).and_then(|e| text(e, "/manifest/url")));
            }
        }
        for manifest_url in manifests {
            let Some(manifest) = self.json(&manifest_url).await else {
                continue;
            };
            let urls: Vec<String> = manifest["files"]
                .as_object()
                .into_iter()
                .flatten()
                .filter_map(|(_, f)| text(f, "/downloads/raw/url"))
                .collect();
            self.files(std::iter::once(manifest_url).chain(urls)).await;
        }
    }

    async fn adoptium(&self, releases: &BTreeMap<String, u64>) {
        eprintln!("warm: Adoptium");
        let majors: BTreeSet<u64> = releases.values().copied().chain([8, 17, 21, 25]).collect();
        let mut links = Vec::new();
        for major in majors {
            for os in ["windows", "linux", "mac"] {
                for arch in ["x64", "aarch64"] {
                    for image in ["jre", "jdk"] {
                        let query = format!(
                            "{ADOPTIUM}/{major}/hotspot?architecture={arch}&image_type={image}&os={os}&vendor=eclipse"
                        );
                        let link = self
                            .json(&query)
                            .await
                            .and_then(|v| v.get(0).and_then(|a| text(a, "/binary/package/link")));
                        if let Some(link) = link {
                            links.push(link);
                            break;
                        }
                    }
                }
            }
        }
        self.files(links).await;
    }

    async fn fabric_like(&self, base: &str, releases: &[String]) {
        eprintln!("warm: {base}");
        let Some(supported) = self.json(&format!("{base}/versions/game")).await else {
            return;
        };
        let supported: HashSet<String> = supported
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|v| text(v, "/version"))
            .collect();
        for game in releases.iter().filter(|g| supported.contains(*g)) {
            let Some(loaders) = self.json(&format!("{base}/versions/loader/{game}")).await else {
                continue;
            };
            let list = loaders.as_array().cloned().unwrap_or_default();
            let chosen = list
                .iter()
                .find(|l| l.pointer("/loader/stable").and_then(Value::as_bool) != Some(false))
                .and_then(|l| text(l, "/loader/version"));
            let Some(loader) = chosen else {
                continue;
            };
            let Some(profile) = self
                .json(&format!(
                    "{base}/versions/loader/{game}/{loader}/profile/json"
                ))
                .await
            else {
                continue;
            };
            let urls = profile["libraries"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|library| {
                    let repository = text(library, "/url")?;
                    let path = maven_path(&text(library, "/name")?)?;
                    Some(format!("{}/{path}", repository.trim_end_matches('/')))
                });
            self.files(urls.collect::<Vec<_>>()).await;
        }
    }

    async fn forge(&self, releases: &[String]) {
        eprintln!("warm: Forge");
        let Some(promotions) = self.json(FORGE_PROMOTIONS).await else {
            return;
        };
        let mut urls = Vec::new();
        for (key, version) in promotions["promos"].as_object().into_iter().flatten() {
            let Some((game, _)) = key.rsplit_once('-') else {
                continue;
            };
            let Some(version) = version.as_str() else {
                continue;
            };
            if !releases.iter().any(|r| r == game) {
                continue;
            }
            let file = format!(
                "{FORGE_MAVEN}/net/minecraftforge/forge/{game}-{version}/forge-{game}-{version}-installer.jar"
            );
            urls.push(format!("{file}.sha1"));
            urls.push(file);
        }
        self.files(urls).await;
    }

    async fn neoforge(&self) {
        eprintln!("warm: NeoForge");
        let Some(list) = self.json(NEOFORGE_VERSIONS).await else {
            return;
        };
        // The newest version per game version (21.1.x is for 1.21.1), releases
        // before betas.
        let mut newest: BTreeMap<String, String> = BTreeMap::new();
        for version in list["versions"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            let mut parts = version.split('.');
            let (Some(a), Some(b)) = (parts.next(), parts.next()) else {
                continue;
            };
            let key = format!("{a}.{b}");
            let beta = version.contains("beta");
            match newest.get(&key) {
                Some(current) if beta && !current.contains("beta") => {}
                _ => {
                    newest.insert(key, version.to_owned());
                }
            }
        }
        let mut urls = Vec::new();
        for version in newest.values() {
            let file = format!(
                "{NEOFORGE_MAVEN}/net/neoforged/neoforge/{version}/neoforge-{version}-installer.jar"
            );
            urls.push(format!("{file}.sha1"));
            urls.push(file);
        }
        self.files(urls).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maven_names_become_paths() {
        assert_eq!(
            maven_path("net.fabricmc:fabric-loader:0.16.9").unwrap(),
            "net/fabricmc/fabric-loader/0.16.9/fabric-loader-0.16.9.jar"
        );
        assert_eq!(
            maven_path("org.lwjgl:lwjgl:3.3.3:natives-linux").unwrap(),
            "org/lwjgl/lwjgl/3.3.3/lwjgl-3.3.3-natives-linux.jar"
        );
        assert_eq!(maven_path("a.b:c:1@zip").unwrap(), "a/b/c/1/c-1.zip");
        assert!(maven_path("broken").is_none());
    }

    #[test]
    fn urls_become_service_requests() {
        assert_eq!(
            local(
                "http://127.0.0.1:8782",
                "https://libraries.minecraft.net/a/b.jar"
            )
            .unwrap(),
            "http://127.0.0.1:8782/libraries.minecraft.net/a/b.jar"
        );
        assert!(local("http://127.0.0.1:8782", "http://x/y").is_none());
    }
}
