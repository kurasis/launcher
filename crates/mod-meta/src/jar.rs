//! Reading a jar's descriptors, and those of the jars inside it.

use std::io::{Cursor, Read, Seek};

use serde_json::Value;

use crate::{Loader, Manifest, Need, Provided, Side};

/// Largest descriptor read (`fabric.mod.json`, `mods.toml`, the manifest).
const MAX_TEXT: u64 = 1024 * 1024;
/// Largest jar inside a jar that is opened, how many are opened in all, how
/// many bytes they unpack to together, and how deep.
const MAX_NESTED_SIZE: u64 = 32 * 1024 * 1024;
const MAX_NESTED: usize = 256;
const MAX_NESTED_TOTAL: u64 = 256 * 1024 * 1024;
const MAX_DEPTH: usize = 3;
/// Most mods, needs and ranges kept from one jar.
const MAX_MODS: usize = 512;
const MAX_NEEDS: usize = 4096;
const MAX_RANGES: usize = 16;
const MAX_RANGE: usize = 512;
const MAX_VERSION: usize = 128;
const MAX_PATH: usize = 512;

/// [`read`] of a jar in memory.
pub fn read_bytes(jar: &[u8]) -> Vec<Manifest> {
    read(Cursor::new(jar))
}

/// A jar's descriptors, one for each loader it is made for; none for what is
/// not a mod (a library, junk).
pub fn read<R: Read + Seek>(jar: R) -> Vec<Manifest> {
    let Ok(mut zip) = zip::ZipArchive::new(jar) else {
        return Vec::new();
    };
    let mut budget = Budget {
        jars: MAX_NESTED,
        bytes: MAX_NESTED_TOTAL,
    };
    let mut out: Vec<Manifest> = Vec::new();
    for descriptor in [
        "quilt.mod.json",
        "fabric.mod.json",
        "META-INF/neoforge.mods.toml",
        "META-INF/mods.toml",
        "mcmod.info",
    ] {
        let mut manifest = Manifest {
            loader: Loader::Forge,
            mods: Vec::new(),
            needs: Vec::new(),
            breaks: Vec::new(),
        };
        let mut reader = Reader {
            budget: &mut budget,
            manifest: &mut manifest,
        };
        let loader = match descriptor {
            "quilt.mod.json" => reader.quilt(&mut zip, false, 0).then_some(Loader::Quilt),
            "fabric.mod.json" => reader.fabric(&mut zip, false, 0).then_some(Loader::Fabric),
            "mcmod.info" => reader.mcmod(&mut zip).then_some(Loader::Forge),
            toml => reader.mods_toml(&mut zip, toml, false, 0),
        };
        // `mods.toml` beside `neoforge.mods.toml` counts for Forge only, and
        // `mcmod.info` only without either.
        if let Some(loader) = loader
            && !out.iter().any(|m| m.loader == loader && m.loader.maven())
        {
            manifest.loader = loader;
            out.push(manifest);
        }
    }
    // OptiFine names itself nowhere a loader reads, but other mods do not
    // start with it: it goes by `optifine` for Forge and, through
    // OptiFabric, for Fabric.
    if out.is_empty() && optifine(&zip) {
        for loader in [Loader::Forge, Loader::Fabric] {
            out.push(Manifest {
                loader,
                mods: vec![Provided {
                    id: OPTIFINE.to_owned(),
                    version: None,
                    nested: false,
                    side: Side::Client,
                }],
                needs: Vec::new(),
                breaks: Vec::new(),
            });
        }
    }
    out
}

/// The ID OptiFine's jar goes by here.
pub const OPTIFINE: &str = "optifine";

/// Whether a jar with no descriptor is OptiFine, by its classes.
fn optifine<R: Read + Seek>(zip: &zip::ZipArchive<R>) -> bool {
    zip.file_names()
        .any(|name| name.starts_with("net/optifine/") || name.starts_with("optifine/"))
}

/// Descriptors read together: a Quilt jar may hold Fabric jars, a Forge jar
/// NeoForge's, never across.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Family {
    Quilt,
    Fabric,
    Forge,
}

/// What is left for the jars inside one jar.
struct Budget {
    jars: usize,
    bytes: u64,
}

struct Reader<'a> {
    budget: &'a mut Budget,
    manifest: &'a mut Manifest,
}

fn text<R: Read + Seek>(zip: &mut zip::ZipArchive<R>, name: &str) -> Option<String> {
    let entry = zip.by_name(name).ok()?;
    if entry.size() > MAX_TEXT {
        return None;
    }
    let mut out = Vec::new();
    entry.take(MAX_TEXT).read_to_end(&mut out).ok()?;
    // Some descriptors start with a byte order mark.
    let text = String::from_utf8_lossy(&out);
    Some(text.trim_start_matches('\u{feff}').to_owned())
}

fn bytes<R: Read + Seek>(zip: &mut zip::ZipArchive<R>, name: &str, most: u64) -> Option<Vec<u8>> {
    if name.len() > MAX_PATH {
        return None;
    }
    let entry = zip.by_name(name).ok()?;
    if entry.size() > most {
        return None;
    }
    let mut out = Vec::new();
    entry.take(most + 1).read_to_end(&mut out).ok()?;
    (out.len() as u64 <= most).then_some(out)
}

/// A mod ID as the loaders accept it.
pub(crate) fn clean_id(id: &str) -> Option<String> {
    let id = id.trim();
    let ok = (1..=64).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"_-.".contains(&b));
    ok.then(|| id.to_owned())
}

fn clean_version(version: &str) -> Option<String> {
    let version = version.trim();
    let ok = !version.is_empty()
        && version.len() <= MAX_VERSION
        && !version.contains("${")
        && !version.chars().any(char::is_control);
    ok.then(|| version.to_owned())
}

/// Whether a manifest is one [`read`] could give: IDs, versions and ranges
/// as it cleans them, within its bounds.
pub(crate) fn clean(manifest: &Manifest) -> bool {
    let id = |id: &str| clean_id(id).as_deref() == Some(id);
    let version = |v: &Option<String>| {
        v.as_deref()
            .is_none_or(|v| clean_version(v).as_deref() == Some(v))
    };
    let need = |n: &Need| {
        id(&n.by)
            && id(&n.id)
            && n.versions.len() <= MAX_RANGES
            && n.versions
                .iter()
                .all(|r| r.len() <= MAX_RANGE && !r.chars().any(char::is_control))
    };
    (1..=MAX_MODS).contains(&manifest.mods.len())
        && manifest.needs.len() <= MAX_NEEDS
        && manifest.breaks.len() <= MAX_NEEDS
        && manifest
            .mods
            .iter()
            .all(|p| id(&p.id) && version(&p.version))
        && manifest.needs.iter().chain(&manifest.breaks).all(need)
}

/// The ranges of a need: a string, a list of them (any one), or Quilt's
/// `{"any": […]}` and `{"all": […]}`. `None` for anything else.
fn ranges(value: &Value) -> Option<Vec<String>> {
    let list: Vec<String> = match value {
        Value::Null => Vec::new(),
        Value::String(s) => vec![s.clone()],
        Value::Array(items) => items
            .iter()
            .map(|v| v.as_str().map(str::to_owned))
            .collect::<Option<_>>()?,
        Value::Object(o) if o.len() == 1 && o.contains_key("any") => ranges(&o["any"])?,
        Value::Object(o) if o.len() == 1 && o.contains_key("all") => {
            vec![ranges(&o["all"])?.join(" ")]
        }
        _ => return None,
    };
    let ok = list.len() <= MAX_RANGES
        && list
            .iter()
            .all(|r| r.len() <= MAX_RANGE && !r.chars().any(char::is_control));
    // `*` alone is any version, as no range at all.
    let list = if list.iter().any(|r| r.trim() == "*") {
        Vec::new()
    } else {
        list
    };
    ok.then_some(list)
}

fn side_of(value: &Value) -> Side {
    match value.as_str() {
        Some("client") => Side::Client,
        Some("server" | "dedicated_server") => Side::Server,
        _ => Side::Both,
    }
}

impl Reader<'_> {
    fn add_mod(&mut self, id: String, version: Option<String>, nested: bool, side: Side) {
        if self.manifest.mods.len() < MAX_MODS
            && !self
                .manifest
                .mods
                .iter()
                .any(|m| m.id == id && m.version == version)
        {
            self.manifest.mods.push(Provided {
                id,
                version,
                nested,
                side,
            });
        }
    }

    fn add_need(&mut self, breaks: bool, need: Need) {
        let list = if breaks {
            &mut self.manifest.breaks
        } else {
            &mut self.manifest.needs
        };
        if list.len() < MAX_NEEDS && !list.contains(&need) {
            list.push(need);
        }
    }

    /// Jars inside, read as `family` too.
    fn nested<R: Read + Seek>(
        &mut self,
        zip: &mut zip::ZipArchive<R>,
        paths: Vec<String>,
        family: Family,
        depth: usize,
    ) {
        if depth >= MAX_DEPTH {
            return;
        }
        for path in paths {
            if self.budget.jars == 0 || self.budget.bytes == 0 {
                return;
            }
            self.budget.jars -= 1;
            let Some(inner) = bytes(zip, &path, MAX_NESTED_SIZE.min(self.budget.bytes)) else {
                continue;
            };
            self.budget.bytes -= inner.len() as u64;
            let Ok(mut inner) = zip::ZipArchive::new(Cursor::new(inner)) else {
                continue;
            };
            match family {
                Family::Quilt => {
                    if !self.quilt(&mut inner, true, depth + 1) {
                        self.fabric(&mut inner, true, depth + 1);
                    }
                }
                Family::Fabric => {
                    self.fabric(&mut inner, true, depth + 1);
                }
                Family::Forge => {
                    if self
                        .mods_toml(&mut inner, "META-INF/neoforge.mods.toml", true, depth + 1)
                        .is_none()
                    {
                        self.mods_toml(&mut inner, "META-INF/mods.toml", true, depth + 1);
                    }
                }
            }
        }
    }

    fn fabric<R: Read + Seek>(
        &mut self,
        zip: &mut zip::ZipArchive<R>,
        nested: bool,
        depth: usize,
    ) -> bool {
        let Some(json) = text(zip, "fabric.mod.json") else {
            return false;
        };
        let Ok(value) = serde_json::from_str::<Value>(&json) else {
            return false;
        };
        let Some(id) = value["id"].as_str().and_then(clean_id) else {
            return false;
        };
        let version = value["version"].as_str().and_then(clean_version);
        let side = side_of(&value["environment"]);
        self.add_mod(id.clone(), version.clone(), nested, side);
        for other in value["provides"].as_array().into_iter().flatten() {
            if let Some(other) = other.as_str().and_then(clean_id) {
                self.add_mod(other, version.clone(), nested, side);
            }
        }
        for (key, breaks) in [("depends", false), ("breaks", true)] {
            for (other, range) in value[key].as_object().into_iter().flatten() {
                if let (Some(other), Some(versions)) = (clean_id(other), ranges(range)) {
                    self.add_need(
                        breaks,
                        Need {
                            by: id.clone(),
                            id: other,
                            versions,
                            side,
                        },
                    );
                }
            }
        }
        let paths = value["jars"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|j| j["file"].as_str().map(str::to_owned))
            .take(MAX_NESTED)
            .collect();
        self.nested(zip, paths, Family::Fabric, depth);
        true
    }

    fn quilt<R: Read + Seek>(
        &mut self,
        zip: &mut zip::ZipArchive<R>,
        nested: bool,
        depth: usize,
    ) -> bool {
        let Some(json) = text(zip, "quilt.mod.json") else {
            return false;
        };
        let Ok(value) = serde_json::from_str::<Value>(&json) else {
            return false;
        };
        let meta = &value["quilt_loader"];
        let Some(id) = meta["id"].as_str().and_then(clean_id) else {
            return false;
        };
        let version = meta["version"].as_str().and_then(clean_version);
        let side = side_of(&value["minecraft"]["environment"]);
        self.add_mod(id.clone(), version.clone(), nested, side);
        for other in meta["provides"].as_array().into_iter().flatten() {
            let (other, given) = match other {
                Value::String(s) => (clean_id(s), version.clone()),
                o => (
                    o["id"].as_str().and_then(clean_id),
                    o["version"]
                        .as_str()
                        .and_then(clean_version)
                        .or_else(|| version.clone()),
                ),
            };
            if let Some(other) = other {
                self.add_mod(other, given, nested, side);
            }
        }
        for (key, breaks) in [("depends", false), ("breaks", true)] {
            for need in meta[key].as_array().into_iter().flatten() {
                let (other, versions) = match need {
                    Value::String(s) => (s.as_str(), Some(Vec::new())),
                    o => {
                        // Optional needs and those with conditions are not checked.
                        if o["optional"].as_bool() == Some(true) || !o["unless"].is_null() {
                            continue;
                        }
                        (o["id"].as_str().unwrap_or_default(), ranges(&o["versions"]))
                    }
                };
                // `group:id` names the mod by its ID.
                let other = other.rsplit(':').next().and_then(clean_id);
                if let (Some(other), Some(versions)) = (other, versions) {
                    self.add_need(
                        breaks,
                        Need {
                            by: id.clone(),
                            id: other,
                            versions,
                            side,
                        },
                    );
                }
            }
        }
        let paths = meta["jars"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|j| j.as_str().map(str::to_owned))
            .take(MAX_NESTED)
            .collect();
        self.nested(zip, paths, Family::Quilt, depth);
        true
    }

    /// A `mods.toml` or `neoforge.mods.toml`: the loader it is for, NeoForge
    /// when it names NeoForge (or is NeoForge's own file name).
    fn mods_toml<R: Read + Seek>(
        &mut self,
        zip: &mut zip::ZipArchive<R>,
        name: &str,
        nested: bool,
        depth: usize,
    ) -> Option<Loader> {
        let table: toml::Table = toml::from_str(&text(zip, name)?).ok()?;
        let mut jar_version: Option<Option<String>> = None;
        let mut ids = Vec::new();
        for entry in table.get("mods")?.as_array()?.iter().take(MAX_MODS) {
            let Some(id) = entry
                .get("modId")
                .and_then(toml::Value::as_str)
                .and_then(clean_id)
            else {
                continue;
            };
            let version = match entry.get("version").and_then(toml::Value::as_str) {
                Some(v) if v.contains("${file.jarVersion}") => jar_version
                    .get_or_insert_with(|| text_of_manifest(zip))
                    .clone(),
                Some(v) => clean_version(v),
                None => None,
            };
            self.add_mod(id.clone(), version, nested, Side::Both);
            ids.push(id);
        }
        if ids.is_empty() {
            return None;
        }
        let mut neo = name.ends_with("neoforge.mods.toml");
        if let Some(deps) = table.get("dependencies").and_then(toml::Value::as_table) {
            for (owner, list) in deps.iter().take(MAX_MODS) {
                let by = clean_id(owner).unwrap_or_else(|| ids[0].clone());
                for dep in list.as_array().into_iter().flatten().take(MAX_NEEDS) {
                    let Some(other) = dep
                        .get("modId")
                        .and_then(toml::Value::as_str)
                        .and_then(clean_id)
                    else {
                        continue;
                    };
                    neo |= other == "neoforge";
                    let kind = match dep.get("type").and_then(toml::Value::as_str) {
                        Some(t) => t.to_ascii_lowercase(),
                        None if dep.get("mandatory").and_then(toml::Value::as_bool)
                            == Some(false) =>
                        {
                            "optional".to_owned()
                        }
                        None => "required".to_owned(),
                    };
                    let breaks = match kind.as_str() {
                        "required" => false,
                        "incompatible" => true,
                        _ => continue,
                    };
                    let side = match dep.get("side").and_then(toml::Value::as_str) {
                        Some(s) if s.eq_ignore_ascii_case("client") => Side::Client,
                        Some(s) if s.eq_ignore_ascii_case("server") => Side::Server,
                        _ => Side::Both,
                    };
                    let versions = match dep.get("versionRange").and_then(toml::Value::as_str) {
                        None => Vec::new(),
                        Some(r) if r.len() > MAX_RANGE || r.chars().any(char::is_control) => {
                            continue;
                        }
                        Some(r) if r.trim().is_empty() || r.trim() == "*" => Vec::new(),
                        Some(r) => vec![r.trim().to_owned()],
                    };
                    self.add_need(
                        breaks,
                        Need {
                            by: by.clone(),
                            id: other,
                            versions,
                            side,
                        },
                    );
                }
            }
        }
        if let Some(meta) = text(zip, "META-INF/jarjar/metadata.json")
            && let Ok(value) = serde_json::from_str::<Value>(&meta)
        {
            let paths = value["jars"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|j| j["path"].as_str().map(str::to_owned))
                .take(MAX_NESTED)
                .collect();
            self.nested(zip, paths, Family::Forge, depth);
        }
        Some(if neo { Loader::NeoForge } else { Loader::Forge })
    }

    /// `mcmod.info` of Forge before 1.13: the mods only, as what they need is
    /// written in their code.
    fn mcmod<R: Read + Seek>(&mut self, zip: &mut zip::ZipArchive<R>) -> bool {
        let Some(json) = text(zip, "mcmod.info") else {
            return false;
        };
        let Ok(value) = serde_json::from_str::<Value>(&json) else {
            return false;
        };
        let list = value
            .as_array()
            .or_else(|| value["modList"].as_array())
            .cloned()
            .unwrap_or_default();
        let before = self.manifest.mods.len();
        for entry in list.iter().take(MAX_MODS) {
            if let Some(id) = entry["modid"]
                .as_str()
                .map(str::to_ascii_lowercase)
                .as_deref()
                .and_then(clean_id)
            {
                let version = entry["version"].as_str().and_then(clean_version);
                // Named, not required: a bare Maven version takes any.
                if let Some(game) = entry["mcversion"].as_str().and_then(clean_version) {
                    self.add_need(
                        false,
                        Need {
                            by: id.clone(),
                            id: "minecraft".to_owned(),
                            versions: vec![game],
                            side: Side::Both,
                        },
                    );
                }
                self.add_mod(id, version, false, Side::Both);
            }
        }
        self.manifest.mods.len() > before
    }
}

/// `Implementation-Version` of the jar's manifest, which `${file.jarVersion}`
/// stands for.
fn text_of_manifest<R: Read + Seek>(zip: &mut zip::ZipArchive<R>) -> Option<String> {
    let manifest = text(zip, "META-INF/MANIFEST.MF")?;
    manifest
        .lines()
        .find_map(|l| l.strip_prefix("Implementation-Version:"))
        .and_then(clean_version)
}
