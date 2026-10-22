//! One list of categories over Modrinth's and CurseForge's (D-072): a
//! category is `<kind>.<name>` and stands for the slugs either site gives
//! projects of that kind. Projects keep the sites' own slugs; this list maps
//! them. Slugs that mean different things on the two sites are told apart by
//! the kind (CurseForge's `vanilla` is Vanilla+ for modpacks, plain for
//! shaders). A Modrinth data pack is a mod whose only loader is `datapack`,
//! so data packs map mod slugs too.

/// (kind, name, the sites' slugs), in the order the launcher lists them.
pub const CATEGORIES: &[(&str, &str, &[&str])] = &[
    (
        "mod",
        "optimization",
        &["bug-fixes", "optimization", "performance"],
    ),
    ("mod", "utility", &["utility", "utility-qol"]),
    ("mod", "maps-info", &["map-information"]),
    ("mod", "library", &["library", "library-api"]),
    (
        "mod",
        "technology",
        &[
            "redstone",
            "technology",
            "technology-automation",
            "technology-energy",
            "technology-farming",
            "technology-genetics",
            "technology-item-fluid-energy-transport",
            "technology-processing",
        ],
    ),
    ("mod", "magic", &["magic"]),
    (
        "mod",
        "adventure",
        &["adventure", "adventure-rpg", "horror"],
    ),
    (
        "mod",
        "worldgen",
        &[
            "world-biomes",
            "world-dimensions",
            "world-gen",
            "world-ores-resources",
            "world-structures",
            "worldgen",
        ],
    ),
    ("mod", "mobs", &["mobs", "world-mobs"]),
    ("mod", "equipment", &["armor-weapons-tools", "equipment"]),
    (
        "mod",
        "decoration",
        &["cosmetic", "creativemode", "decoration"],
    ),
    ("mod", "food", &["food", "mc-food"]),
    ("mod", "storage", &["storage"]),
    (
        "mod",
        "transportation",
        &["technology-player-transport", "transportation"],
    ),
    ("mod", "game-mechanics", &["game-mechanics"]),
    (
        "mod",
        "server",
        &["economy", "management", "server-utility"],
    ),
    ("mod", "social", &["social", "twitch-integration"]),
    (
        "mod",
        "addons",
        &[
            "addons-buildcraft",
            "addons-forestry",
            "addons-industrialcraft",
            "addons-thaumcraft",
            "addons-thermalexpansion",
            "addons-tinkers-construct",
            "applied-energistics-2",
            "blood-magic",
            "crafttweaker",
            "create",
            "farmers-delight",
            "galacticraft",
            "integrated-dynamics",
            "kubejs",
            "mc-addons",
            "refined-storage",
            "skyblock",
            "twilight-forest",
        ],
    ),
    ("mod", "fun", &["cursed", "minigame"]),
    ("datapack", "adventure", &["adventure"]),
    ("datapack", "magic", &["fantasy", "magic"]),
    ("datapack", "game-mechanics", &["food", "game-mechanics"]),
    ("datapack", "worldgen", &["worldgen"]),
    ("datapack", "mobs", &["mobs"]),
    ("datapack", "equipment", &["equipment"]),
    ("datapack", "technology", &["tech", "technology"]),
    ("datapack", "decoration", &["decoration"]),
    (
        "datapack",
        "utility",
        &[
            "economy",
            "management",
            "social",
            "storage",
            "transportation",
            "utility",
        ],
    ),
    ("datapack", "optimization", &["optimization"]),
    ("datapack", "library", &["library"]),
    ("datapack", "fun", &["cursed", "minigame"]),
    ("datapack", "mod-support", &["mod-support"]),
    (
        "resourcepack",
        "vanilla-like",
        &["traditional", "vanilla-like"],
    ),
    ("resourcepack", "tweaks", &["tweaks"]),
    (
        "resourcepack",
        "realistic",
        &["photo-realistic", "realistic"],
    ),
    ("resourcepack", "simplistic", &["simplistic"]),
    (
        "resourcepack",
        "themed",
        &["medieval", "modern", "steampunk", "themed"],
    ),
    ("resourcepack", "decoration", &["decoration"]),
    ("resourcepack", "combat", &["combat"]),
    ("resourcepack", "utility", &["utility"]),
    ("resourcepack", "modded", &["mod-support", "modded"]),
    ("resourcepack", "animated", &["animated"]),
    ("resourcepack", "fonts", &["font-packs", "fonts"]),
    ("resourcepack", "cursed", &["cursed"]),
    ("shader", "vanilla-like", &["vanilla", "vanilla-like"]),
    ("shader", "semi-realistic", &["semi-realistic"]),
    ("shader", "realistic", &["realistic"]),
    ("shader", "fantasy", &["fantasy"]),
    ("shader", "cartoon", &["cartoon"]),
    ("shader", "cursed", &["cursed"]),
    (
        "modpack",
        "adventure",
        &["adventure", "adventure-and-rpg", "exploration", "horror"],
    ),
    ("modpack", "technology", &["sci-fi", "tech", "technology"]),
    ("modpack", "magic", &["magic"]),
    ("modpack", "quests", &["quests"]),
    (
        "modpack",
        "challenging",
        &["challenging", "expert", "hardcore", "rlcraft"],
    ),
    ("modpack", "combat", &["combat", "combat-pvp"]),
    ("modpack", "kitchen-sink", &["extra-large", "kitchen-sink"]),
    ("modpack", "lightweight", &["lightweight", "small-light"]),
    ("modpack", "vanilla-plus", &["vanilla"]),
    ("modpack", "optimization", &["optimization"]),
    ("modpack", "multiplayer", &["mini-game", "multiplayer"]),
    ("modpack", "skyblock", &["map-based", "skyblock"]),
    ("world", "adventure", &["adventure"]),
    ("world", "survival", &["survival"]),
    ("world", "creation", &["creation"]),
    ("world", "game-map", &["game-map"]),
    ("world", "parkour", &["parkour"]),
    ("world", "puzzle", &["puzzle"]),
    ("world", "modded", &["modded-world"]),
];

/// Categories asked for in one search at most.
pub const MAX_ASKED: usize = 8;

/// A category's slugs and kind, by its ID.
pub fn slugs(id: &str) -> Option<(&'static str, &'static [&'static str])> {
    let (kind, name) = id.split_once('.')?;
    CATEGORIES
        .iter()
        .find(|(k, n, _)| *k == kind && *n == name)
        .map(|(k, _, slugs)| (*k, *slugs))
}

/// The IDs of a kind's categories, in list order.
pub fn of_kind(kind: &str) -> impl Iterator<Item = (String, &'static [&'static str])> + '_ {
    CATEGORIES
        .iter()
        .filter(move |(k, _, _)| *k == kind)
        .map(|(k, n, slugs)| (format!("{k}.{n}"), *slugs))
}

/// The categories of `kind` a project with these slugs is in, in list order.
pub fn ids(kind: &str, project: &[String]) -> Vec<String> {
    of_kind(kind)
        .filter(|(_, slugs)| slugs.iter().any(|s| project.iter().any(|p| p == s)))
        .map(|(id, _)| id)
        .collect()
}

/// The slugs a search asks for: each word a category's ID or a site's own
/// slug. `None` when there are more than [`MAX_ASKED`].
pub fn asked(words: &[String]) -> Option<Vec<String>> {
    if words.len() > MAX_ASKED {
        return None;
    }
    let mut out: Vec<String> = Vec::new();
    for word in words {
        match slugs(word) {
            Some((_, slugs)) => out.extend(slugs.iter().map(|s| (*s).to_owned())),
            None => out.push(word.clone()),
        }
    }
    out.sort();
    out.dedup();
    Some(out)
}

/// Whether a category's ID or slug is one of libraries other projects need.
pub fn is_library(word: &str) -> bool {
    word.ends_with(".library") || crate::store::LIBRARIES.contains(&word)
}
