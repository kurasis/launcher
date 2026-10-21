//! Which game versions each loader supports and which loader versions exist for a
//! game version (D-039).
//!
//! The lists come from each project's own service: Fabric and Quilt meta, the
//! Forge Maven metadata and promotions, the NeoForge Maven API. They only fill the
//! version pickers; installing re-checks everything against pinned hashes.

use std::collections::BTreeSet;

use serde::Serialize;
use serde_json::Value;

use super::{LoaderKind, compare_versions, valid_version};
use crate::catalog::source::Fetch;
use crate::error::{ErrorCode, LauncherError, LauncherResult};

/// Bound for the version lists (the NeoForge list is the largest, ~100 KB).
pub const MAX_LIST_BYTES: usize = 8 * 1024 * 1024;

pub const FABRIC_META: &str = "https://meta.fabricmc.net/v2";
pub const QUILT_META: &str = "https://meta.quiltmc.org/v3";
pub const FORGE_MAVEN: &str = "https://maven.minecraftforge.net";
/// Game version → builds; the files site serves it, the Maven repository does not.
const FORGE_METADATA: &str =
    "https://files.minecraftforge.net/net/minecraftforge/forge/maven-metadata.json";
pub const FORGE_PROMOTIONS: &str =
    "https://files.minecraftforge.net/net/minecraftforge/forge/promotions_slim.json";
pub const NEOFORGE_MAVEN: &str = "https://maven.neoforged.net/releases";
const NEOFORGE_VERSIONS: &str =
    "https://maven.neoforged.net/api/maven/versions/releases/net/neoforged/neoforge";
/// NeoForge's first release, for Minecraft 1.20.1, kept Forge's artifact name.
const NEOFORGE_LEGACY_VERSIONS: &str =
    "https://maven.neoforged.net/api/maven/versions/releases/net/neoforged/forge";
/// The only game version NeoForge published under the `forge` artifact.
pub const NEOFORGE_LEGACY_GAME: &str = "1.20.1";

/// One loader version for the picker.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoaderVersion {
    /// The id to install (see [`super::LoaderChoice::version`]).
    pub id: String,
    /// What people call it: Forge's `52.1.0` for the id `1.21.1-52.1.0`.
    pub label: String,
    /// False for betas and other builds the project marks unstable.
    pub stable: bool,
    /// The project's recommended build for this game version.
    pub recommended: bool,
}

fn bad_list(kind: LoaderKind) -> LauncherError {
    LauncherError::templated(
        ErrorCode::ServiceUnavailable,
        "The {loader} version list could not be read. Try again later.",
        crate::error::params([("loader", kind.name().to_owned().into())]),
    )
}

async fn json(fetcher: &dyn Fetch, url: &str, kind: LoaderKind) -> LauncherResult<Value> {
    let fetched = fetcher.fetch(url, None, None, MAX_LIST_BYTES).await?;
    serde_json::from_slice(&fetched.body).map_err(|_| bad_list(kind))
}

/// Game versions the loader can be installed on, in no particular order. The
/// picker keeps Mojang's order and only hides what is not listed here.
pub async fn game_versions(fetcher: &dyn Fetch, kind: LoaderKind) -> LauncherResult<Vec<String>> {
    let versions: BTreeSet<String> = match kind {
        // From the mods catalog (`super::optifine`).
        LoaderKind::OptiFine => return Err(bad_list(kind)),
        LoaderKind::Fabric | LoaderKind::Quilt => {
            let base = if kind == LoaderKind::Fabric {
                FABRIC_META
            } else {
                QUILT_META
            };
            json(fetcher, &format!("{base}/versions/game"), kind)
                .await?
                .as_array()
                .ok_or_else(|| bad_list(kind))?
                .iter()
                .filter_map(|v| v.get("version").and_then(Value::as_str))
                .map(str::to_owned)
                .collect()
        }
        LoaderKind::Forge => forge_metadata(fetcher)
            .await?
            .iter()
            .filter(|(game, builds)| builds.iter().any(|b| forge_supported(game, b)))
            .map(|(game, _)| game.clone())
            .collect(),
        LoaderKind::NeoForge => {
            let mut games: BTreeSet<String> = neoforge_versions(fetcher, NEOFORGE_VERSIONS)
                .await?
                .iter()
                .filter_map(|v| neoforge_game(v))
                .collect();
            if !neoforge_versions(fetcher, NEOFORGE_LEGACY_VERSIONS)
                .await?
                .is_empty()
            {
                games.insert(NEOFORGE_LEGACY_GAME.to_owned());
            }
            games
        }
    };
    Ok(versions.into_iter().filter(|v| valid_version(v)).collect())
}

/// Loader versions for one game version, newest first.
pub async fn loader_versions(
    fetcher: &dyn Fetch,
    kind: LoaderKind,
    game: &str,
) -> LauncherResult<Vec<LoaderVersion>> {
    if !valid_version(game) {
        return Err(LauncherError::invalid_input("Choose a Minecraft version."));
    }
    let mut list = match kind {
        LoaderKind::OptiFine => return Err(bad_list(kind)),
        LoaderKind::Fabric | LoaderKind::Quilt => {
            let base = if kind == LoaderKind::Fabric {
                FABRIC_META
            } else {
                QUILT_META
            };
            json(fetcher, &format!("{base}/versions/loader/{game}"), kind)
                .await?
                .as_array()
                .ok_or_else(|| bad_list(kind))?
                .iter()
                .filter_map(|entry| {
                    let loader = entry.get("loader")?;
                    let id = loader.get("version")?.as_str()?.to_owned();
                    // Quilt marks no stability; its pre-releases carry a suffix.
                    let stable = loader
                        .get("stable")
                        .and_then(Value::as_bool)
                        .unwrap_or(!id.contains('-'));
                    Some(LoaderVersion {
                        label: id.clone(),
                        id,
                        stable,
                        recommended: false,
                    })
                })
                .collect::<Vec<_>>()
        }
        LoaderKind::Forge => {
            let metadata = forge_metadata(fetcher).await?;
            let builds = metadata
                .iter()
                .find(|(g, _)| g == game)
                .map(|(_, builds)| builds.clone())
                .unwrap_or_default();
            let promotions = json(fetcher, FORGE_PROMOTIONS, kind).await.ok();
            let promoted = |suffix: &str| {
                promotions
                    .as_ref()
                    .and_then(|p| p.pointer(&format!("/promos/{game}-{suffix}")))
                    .and_then(Value::as_str)
                    .map(|v| format!("{game}-{v}"))
            };
            let first_recommended = promoted("recommended");
            let recommended = first_recommended.clone().or_else(|| promoted("latest"));
            builds
                .into_iter()
                .filter(|b| forge_supported(game, b))
                .map(|id| LoaderVersion {
                    label: id
                        .strip_prefix(&format!("{game}-"))
                        .unwrap_or(&id)
                        .to_owned(),
                    recommended: recommended.as_deref() == Some(id.as_str()),
                    stable: forge_stable(&id, first_recommended.as_deref()),
                    id,
                })
                .collect()
        }
        LoaderKind::NeoForge => {
            let legacy = game == NEOFORGE_LEGACY_GAME;
            let url = if legacy {
                NEOFORGE_LEGACY_VERSIONS
            } else {
                NEOFORGE_VERSIONS
            };
            neoforge_versions(fetcher, url)
                .await?
                .into_iter()
                .filter(|v| {
                    if legacy {
                        v.starts_with("1.20.1-")
                    } else {
                        neoforge_game(v).as_deref() == Some(game)
                    }
                })
                .map(|id| LoaderVersion {
                    label: id
                        .strip_prefix(&format!("{NEOFORGE_LEGACY_GAME}-"))
                        .unwrap_or(&id)
                        .to_owned(),
                    stable: !id.contains("-beta") && !id.contains("-alpha"),
                    recommended: false,
                    id,
                })
                .collect()
        }
    };
    list.retain(|v| valid_version(&v.id));
    list.sort_by(|a, b| compare_versions(&b.id, &a.id));
    list.dedup_by(|a, b| a.id == b.id);
    // Without a promotion the newest stable build is the suggestion.
    if !list.iter().any(|v| v.recommended)
        && let Some(first) = list.iter_mut().find(|v| v.stable)
    {
        first.recommended = true;
    }
    Ok(list)
}

/// Forge's Maven metadata: game version → builds, oldest first.
async fn forge_metadata(fetcher: &dyn Fetch) -> LauncherResult<Vec<(String, Vec<String>)>> {
    let value = json(fetcher, FORGE_METADATA, LoaderKind::Forge).await?;
    let map = value
        .as_object()
        .ok_or_else(|| bad_list(LoaderKind::Forge))?;
    Ok(map
        .iter()
        .map(|(game, builds)| {
            let builds = builds
                .as_array()
                .map(|b| {
                    b.iter()
                        .filter_map(Value::as_str)
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_default();
            (game.clone(), builds)
        })
        .collect())
}

/// Forge calls every build before a game version's recommended build a beta
/// (its title screen says "Forge Beta"); without a recommended build, all are.
pub fn forge_stable(build: &str, recommended: Option<&str>) -> bool {
    recommended.is_some_and(|r| compare_versions(build, r) != std::cmp::Ordering::Less)
}

/// Forge builds whose installer holds a version profile: Minecraft 1.13 and
/// newer, and 1.12.2 from build 14.23.5.2851. Older installers use a format
/// the launcher does not read (D-039).
pub fn forge_supported(game: &str, build: &str) -> bool {
    let Some(version) = build.strip_prefix(&format!("{game}-")) else {
        return false;
    };
    if game == "1.12.2" {
        return compare_versions(version, "14.23.5.2851") != std::cmp::Ordering::Less;
    }
    let mut parts = game.split('.').map(str::parse::<u32>);
    match (parts.next(), parts.next()) {
        (Some(Ok(1)), Some(Ok(minor))) => minor >= 13,
        // Year-numbered releases (26.1 and later).
        (Some(Ok(major)), _) => major >= 25,
        _ => false,
    }
}

async fn neoforge_versions(fetcher: &dyn Fetch, url: &str) -> LauncherResult<Vec<String>> {
    Ok(json(fetcher, url, LoaderKind::NeoForge)
        .await?
        .get("versions")
        .and_then(Value::as_array)
        .ok_or_else(|| bad_list(LoaderKind::NeoForge))?
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect())
}

/// The game version a NeoForge version is for. NeoForge numbers its builds after
/// the game: `21.1.262` is for 1.21.1 and `21.0.167` for 1.21; from the
/// year-numbered releases on, `26.1.2.122` is for 26.1.2 and `26.3.0.88` for 26.3.
/// Snapshot builds (`+snapshot`) and April Fools builds (`0.…`) have none.
pub fn neoforge_game(version: &str) -> Option<String> {
    if version.contains('+') {
        return None;
    }
    let base = version.split('-').next()?;
    let parts: Vec<u32> = base
        .split('.')
        .map(str::parse)
        .collect::<Result<_, _>>()
        .ok()?;
    match parts.as_slice() {
        [major @ 20..=24, minor, _build] => Some(if *minor == 0 {
            format!("1.{major}")
        } else {
            format!("1.{major}.{minor}")
        }),
        [year @ 25..=99, drop, patch, _build] => Some(if *patch == 0 {
            format!("{year}.{drop}")
        } else {
            format!("{year}.{drop}.{patch}")
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn neoforge_versions_name_their_game() {
        assert_eq!(neoforge_game("21.1.262").as_deref(), Some("1.21.1"));
        assert_eq!(neoforge_game("21.0.167").as_deref(), Some("1.21"));
        assert_eq!(neoforge_game("20.2.3-beta").as_deref(), Some("1.20.2"));
        assert_eq!(neoforge_game("26.1.2.122").as_deref(), Some("26.1.2"));
        assert_eq!(neoforge_game("26.3.0.88").as_deref(), Some("26.3"));
        assert_eq!(neoforge_game("26.1.0.0-alpha.1+snapshot-1"), None);
        assert_eq!(neoforge_game("0.25w14craftmine.3-beta"), None);
        assert_eq!(neoforge_game("1.20.1-47.1.5"), None);
    }

    #[test]
    fn forge_betas_come_before_the_recommended_build() {
        let recommended = Some("1.21.1-52.1.0");
        assert!(forge_stable("1.21.1-52.1.0", recommended));
        assert!(forge_stable("1.21.1-52.1.20", recommended));
        assert!(!forge_stable("1.21.1-52.0.57", recommended));
        assert!(!forge_stable("26.3-64.0.1", None));
    }

    #[test]
    fn forge_builds_with_a_version_profile() {
        assert!(forge_supported("1.21.1", "1.21.1-52.1.0"));
        assert!(forge_supported("1.13.2", "1.13.2-25.0.223"));
        assert!(forge_supported("26.3", "26.3-64.0.1"));
        assert!(forge_supported("1.12.2", "1.12.2-14.23.5.2851"));
        assert!(forge_supported("1.12.2", "1.12.2-14.23.5.2859"));
        assert!(!forge_supported("1.12.2", "1.12.2-14.23.5.2847"));
        assert!(!forge_supported("1.7.10", "1.7.10-10.13.4.1614-1.7.10"));
        assert!(!forge_supported("1.21.1", "1.20.1-47.4.10"));
    }
}
