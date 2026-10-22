//! Whether a set of jars starts on a game version, loader and Java, as the
//! loader would see it.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::{Loader, Manifest, Side, allows, compare};

/// Where the jars go.
#[derive(Debug, Clone, Copy)]
pub struct Setup<'a> {
    pub game: &'a str,
    pub loader: Loader,
    /// `None`: not known, and what mods ask of it is not checked.
    pub loader_version: Option<&'a str>,
    /// Java's major version, if known.
    pub java: Option<u32>,
    /// `Client` for a game, `Server` for a world's server.
    pub side: Side,
}

/// A jar of the set: its file name and what [`crate::read`] found in it.
#[derive(Debug, Clone, Copy)]
pub struct Jar<'a> {
    pub file: &'a str,
    pub manifests: &'a [Manifest],
}

/// Why a set does not start. `file` is the jar that says so; `from` the jar
/// that gives the mod found (`None`: the game, the loader or Java).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Problem {
    /// `by` needs `id`, which is not there.
    Missing {
        file: String,
        by: String,
        id: String,
        versions: Vec<String>,
    },
    /// `by` needs `id` in `versions`, and `found` is there.
    Wrong {
        file: String,
        by: String,
        id: String,
        versions: Vec<String>,
        found: String,
        from: Option<String>,
    },
    /// `by` does not start with `id`, which is there.
    Breaks {
        file: String,
        by: String,
        id: String,
        found: Option<String>,
        from: Option<String>,
    },
    /// Two files are the same mod.
    Twice { id: String, files: Vec<String> },
    /// A jar made for another loader only.
    OtherLoader { file: String, loader: Loader },
}

/// The descriptor of a jar that `loader` reads: Quilt also reads Fabric's,
/// and NeoForge for 1.20.1 still Forge's.
pub fn for_loader<'m>(
    manifests: &'m [Manifest],
    loader: Loader,
    game: &str,
) -> Option<&'m Manifest> {
    let find = |l: Loader| manifests.iter().find(|m| m.loader == l);
    match loader {
        Loader::Quilt => find(Loader::Quilt).or_else(|| find(Loader::Fabric)),
        Loader::NeoForge => find(Loader::NeoForge)
            .or_else(|| (game == "1.20.1").then(|| find(Loader::Forge)).flatten()),
        other => find(other),
    }
}

struct Giver<'a> {
    version: Option<&'a str>,
    file: Option<&'a str>,
    nested: bool,
}

/// Everything in the way of the set starting; none when it should.
pub fn check(setup: &Setup<'_>, jars: &[Jar<'_>]) -> Vec<Problem> {
    let java = setup.java.map(|j| j.to_string());
    let mut given: BTreeMap<&str, Vec<Giver<'_>>> = BTreeMap::new();
    // What the game, the loader and Java give; loaders of today carry
    // MixinExtras.
    let mut own: Vec<(&str, Option<&str>)> = vec![
        ("minecraft", Some(setup.game)),
        (setup.loader.id(), setup.loader_version),
        ("java", java.as_deref()),
        ("mixinextras", None),
    ];
    match setup.loader {
        Loader::Quilt => own.push(("fabricloader", None)),
        Loader::NeoForge if setup.game == "1.20.1" => own.push(("forge", setup.loader_version)),
        _ => {}
    }
    for (id, version) in own {
        given.entry(id).or_default().push(Giver {
            version,
            file: None,
            nested: false,
        });
    }

    let mut problems = Vec::new();
    let mut chosen: Vec<(&str, &Manifest)> = Vec::new();
    for jar in jars {
        if jar.manifests.is_empty() {
            continue;
        }
        match for_loader(jar.manifests, setup.loader, setup.game) {
            Some(m) => chosen.push((jar.file, m)),
            None => problems.push(Problem::OtherLoader {
                file: jar.file.to_owned(),
                loader: jar.manifests[0].loader,
            }),
        }
    }
    for (file, m) in &chosen {
        for p in m.mods.iter().filter(|p| p.side.applies(setup.side)) {
            given.entry(&p.id).or_default().push(Giver {
                version: p.version.as_deref(),
                file: Some(file),
                nested: p.nested,
            });
        }
    }

    for (id, givers) in &given {
        let files: BTreeSet<&str> = givers
            .iter()
            .filter(|g| !g.nested)
            .filter_map(|g| g.file)
            .collect();
        if files.len() > 1 {
            problems.push(Problem::Twice {
                id: (*id).to_owned(),
                files: files.into_iter().map(str::to_owned).collect(),
            });
        }
    }

    for (file, m) in &chosen {
        // A mod of the set runs only where its side is.
        for need in m.needs.iter().filter(|n| n.side.applies(setup.side)) {
            let Some(givers) = given.get(need.id.as_str()) else {
                problems.push(Problem::Missing {
                    file: (*file).to_owned(),
                    by: need.by.clone(),
                    id: need.id.clone(),
                    versions: need.versions.clone(),
                });
                continue;
            };
            // Fine when any copy fits, or when that cannot be told.
            let mut wrong: Option<&Giver<'_>> = None;
            let fits = givers.iter().any(|g| match g.version {
                None => true,
                Some(version) => match allows(m.loader, &need.versions, version) {
                    Some(false) => {
                        if wrong.is_none_or(|w| {
                            w.version.and_then(|w| compare(version, w)) == Some(Ordering::Greater)
                        }) {
                            wrong = Some(g);
                        }
                        false
                    }
                    _ => true,
                },
            });
            if !fits && let Some(w) = wrong {
                problems.push(Problem::Wrong {
                    file: (*file).to_owned(),
                    by: need.by.clone(),
                    id: need.id.clone(),
                    versions: need.versions.clone(),
                    found: w.version.unwrap_or_default().to_owned(),
                    from: w.file.map(str::to_owned),
                });
            }
        }
        for breaks in m.breaks.iter().filter(|n| n.side.applies(setup.side)) {
            let Some(givers) = given.get(breaks.id.as_str()) else {
                continue;
            };
            let hit = givers
                .iter()
                .filter(|g| g.file != Some(file))
                .find(|g| match g.version {
                    None => breaks.versions.is_empty(),
                    Some(version) => allows(m.loader, &breaks.versions, version) == Some(true),
                });
            if let Some(g) = hit {
                problems.push(Problem::Breaks {
                    file: (*file).to_owned(),
                    by: breaks.by.clone(),
                    id: breaks.id.clone(),
                    found: g.version.map(str::to_owned),
                    from: g.file.map(str::to_owned),
                });
            }
        }
    }
    problems.sort();
    problems.dedup();
    problems
}
