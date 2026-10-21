//! What the owner's commands do to ads, and what the launcher is sent. The
//! commands and the service share these, so an ad is checked the same way
//! wherever it comes from.

use std::collections::BTreeMap;

use serde_json::{Value, json};

use crate::icons::{self, Icons};
use crate::ping::Pinger;
use crate::store::{Ad, Ping, Store};
use crate::text;

/// `--key value` flags and bare `--switch`es of a command.
#[derive(Debug, Default)]
pub struct Flags {
    values: BTreeMap<String, String>,
    switches: Vec<String>,
}

const SWITCHES: [&str; 6] = [
    "--quiet",
    "--if-new",
    "--all",
    "--no-ping",
    "--clear-erid",
    "--auto",
];

impl Flags {
    pub fn parse(args: &[String]) -> Result<(Vec<String>, Self), String> {
        let mut flags = Self::default();
        let mut plain = Vec::new();
        let mut args = args.iter();
        while let Some(arg) = args.next() {
            if SWITCHES.contains(&arg.as_str()) {
                flags.switches.push(arg.clone());
            } else if let Some(key) = arg.strip_prefix("--") {
                let value = args
                    .next()
                    .ok_or_else(|| format!("--{key} needs a value"))?;
                if flags.values.insert(key.to_owned(), value.clone()).is_some() {
                    return Err(format!("--{key} is given twice"));
                }
            } else {
                plain.push(arg.clone());
            }
        }
        Ok((plain, flags))
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.values.get(key).map(String::as_str)
    }

    pub fn has(&self, switch: &str) -> bool {
        self.switches.iter().any(|s| s == switch)
    }

    /// Fails on a flag the command does not know, so a typo is not lost.
    pub fn only(&self, known: &[&str]) -> Result<(), String> {
        for key in self.values.keys() {
            let text_key = key.starts_with("text");
            if !known.contains(&key.as_str()) && !(text_key && known.contains(&"text")) {
                return Err(format!("unknown flag --{key}"));
            }
        }
        Ok(())
    }
}

pub const AD_FLAGS: [&str; 12] = [
    "name", "address", "version", "from", "to", "text", "start", "for", "until", "weight", "place",
    "erid",
];

/// Texts from `--text`, `--text-file`, `--text-ru`, `--text-ru-file` and so on.
fn texts(flags: &Flags, into: &mut BTreeMap<String, String>) -> Result<(), String> {
    for (key, value) in &flags.values {
        let Some(rest) = key.strip_prefix("text") else {
            continue;
        };
        let (lang, file) = match rest.strip_suffix("-file") {
            Some(lang) => (lang, true),
            None => (rest, false),
        };
        let lang = match lang.strip_prefix('-') {
            None if lang.is_empty() => "*",
            Some(lang) if text::LANGS.contains(&lang) => lang,
            _ => {
                return Err(format!(
                    "unknown flag --{key} (languages: {})",
                    text::LANGS.join(", ")
                ));
            }
        };
        let raw = if file && value == "-" {
            // `--text-file -`: standard input, where a file in the owner's
            // home is not readable by the service's user.
            use std::io::Read;
            let mut raw = String::new();
            std::io::stdin()
                .take(4097)
                .read_to_string(&mut raw)
                .map_err(|e| format!("standard input: {e}"))?;
            if raw.len() > 4096 {
                return Err("standard input is larger than 4 KB".to_owned());
            }
            raw
        } else if file {
            let meta = std::fs::metadata(value).map_err(|e| format!("{value}: {e}"))?;
            if meta.len() > 4096 {
                return Err(format!("{value} is larger than 4 KB"));
            }
            std::fs::read_to_string(value).map_err(|e| format!("{value}: {e}"))?
        } else {
            value.clone()
        };
        into.insert(lang.to_owned(), text::ad_text(&raw)?);
    }
    Ok(())
}

/// Checks the versions against Mojang's releases.
fn versions(store: &Store, ad: &Ad) -> Result<(), String> {
    for id in [&ad.version, &ad.oldest, &ad.newest] {
        if !text::version_id(id) {
            return Err(format!("{id:?} is not a version ID"));
        }
        if store.rank(id)?.is_none() {
            return Err(format!("{id} is not a Minecraft release"));
        }
    }
    let rank = |id: &str| store.rank(id).map(Option::unwrap_or_default);
    if rank(&ad.oldest)? < rank(&ad.newest)? {
        return Err(format!(
            "--from {} is newer than --to {}",
            ad.oldest, ad.newest
        ));
    }
    let version = rank(&ad.version)?;
    if version > rank(&ad.oldest)? || version < rank(&ad.newest)? {
        return Err(format!(
            "--version {} is outside {} to {}",
            ad.version, ad.oldest, ad.newest
        ));
    }
    Ok(())
}

/// A new ad, or `base` changed, from the flags.
pub fn build(
    store: &Store,
    base: Option<&Ad>,
    flags: &Flags,
    zone: chrono::FixedOffset,
    now: i64,
) -> Result<Ad, String> {
    flags.only(&AD_FLAGS)?;
    let required = |key: &str| flags.get(key).ok_or_else(|| format!("--{key} is required"));
    let mut ad = match base {
        Some(ad) => ad.clone(),
        None => Ad {
            id: 0,
            name: text::name(required("name")?)?,
            address: text::address(required("address")?)?,
            version: required("version")?.trim().to_owned(),
            oldest: String::new(),
            newest: String::new(),
            texts: BTreeMap::new(),
            starts: now,
            ends: 0,
            weight: 10,
            place: "feed".to_owned(),
            erid: None,
            paused: false,
            icon: None,
        },
    };
    if let Some(name) = flags.get("name") {
        ad.name = text::name(name)?;
    }
    if let Some(address) = flags.get("address") {
        ad.address = text::address(address)?;
    }
    if let Some(version) = flags.get("version") {
        ad.version = version.trim().to_owned();
    }
    if base.is_none() {
        ad.oldest = flags.get("from").unwrap_or(&ad.version).trim().to_owned();
        ad.newest = flags.get("to").unwrap_or(&ad.version).trim().to_owned();
    } else {
        if let Some(from) = flags.get("from") {
            ad.oldest = from.trim().to_owned();
        }
        if let Some(to) = flags.get("to") {
            ad.newest = to.trim().to_owned();
        }
    }
    versions(store, &ad)?;
    texts(flags, &mut ad.texts)?;
    if ad.texts.is_empty() {
        return Err("--text or --text-file is required".to_owned());
    }
    if let Some(start) = flags.get("start") {
        ad.starts = text::time(start, zone, now)?;
    }
    match (flags.get("for"), flags.get("until")) {
        (Some(_), Some(_)) => return Err("give --for or --until, not both".to_owned()),
        (Some(run), None) => ad.ends = ad.starts.max(now) + text::duration(run)?,
        (None, Some(until)) => ad.ends = text::time(until, zone, now)?,
        (None, None) if base.is_none() => {
            return Err("--for (such as 7d) or --until is required".to_owned());
        }
        (None, None) => {}
    }
    check_run(&ad, now)?;
    if let Some(weight) = flags.get("weight") {
        ad.weight = text::weight(weight)?;
    }
    if let Some(place) = flags.get("place") {
        ad.place = text::place(place)?.to_owned();
    }
    if let Some(erid) = flags.get("erid") {
        ad.erid = Some(text::erid(erid)?);
    }
    if flags.has("--clear-erid") {
        ad.erid = None;
    }
    Ok(ad)
}

/// From a minute to five years of showing, counted from when it shows.
pub fn check_run(ad: &Ad, now: i64) -> Result<(), String> {
    let from = ad.starts.max(now);
    if ad.ends - from < text::MIN_RUN {
        return Err("the ad must show for at least a minute from now".to_owned());
    }
    if ad.ends - from > text::MAX_RUN {
        return Err("an ad runs at most 5 years".to_owned());
    }
    if ad.starts > now + text::MAX_RUN {
        return Err("an ad starts within 5 years".to_owned());
    }
    Ok(())
}

/// Pings the ad's server and keeps the result and its icon.
pub async fn ping(
    store: &Store,
    icons: &Icons,
    pinger: &Pinger,
    ad: &Ad,
    now: i64,
) -> Result<Ping, String> {
    let result = pinger.ping(&ad.address).await;
    let ping = match result {
        Ok(status) => {
            let icon = status
                .favicon
                .as_deref()
                .and_then(|f| icons::from_status(f).ok())
                .and_then(|png| icons.keep(&png).ok());
            Ping {
                at: now,
                up: true,
                online: status.online,
                max: status.max,
                protocol: status.protocol,
                server_version: status.version,
                icon,
            }
        }
        Err(_) => Ping {
            at: now,
            up: false,
            ..Ping::default()
        },
    };
    store.save_ping(ad.id, &ping)?;
    Ok(store.ping(ad.id)?.unwrap_or(ping))
}

/// The language a launcher asked for, English otherwise.
pub fn lang(asked: Option<&str>) -> &'static str {
    text::LANGS
        .iter()
        .find(|l| Some(**l) == asked)
        .copied()
        .unwrap_or("en")
}

pub fn text_for<'a>(ad: &'a Ad, lang: &str) -> &'a str {
    ad.texts
        .get(lang)
        .or_else(|| ad.texts.get("*"))
        .or_else(|| ad.texts.get("en"))
        .or_else(|| ad.texts.values().next())
        .map(String::as_str)
        .unwrap_or_default()
}

/// What a launcher gets of an ad.
pub fn public(store: &Store, ad: &Ad, lang: &str) -> Result<Value, String> {
    let mut versions = store.range(&ad.oldest, &ad.newest)?;
    if !versions.contains(&ad.version) {
        versions.insert(0, ad.version.clone());
    }
    let ping = store.ping(ad.id)?;
    let icon = ad
        .icon
        .clone()
        .or_else(|| ping.as_ref().and_then(|p| p.icon.clone()));
    let up = ping.as_ref().map(|p| p.up);
    let live = ping.as_ref().filter(|p| p.up);
    Ok(json!({
        "id": ad.id.to_string(),
        "name": ad.name,
        "address": ad.address,
        "text": text_for(ad, lang),
        "version": ad.version,
        "versions": versions,
        "icon": icon,
        "up": up,
        "online": live.and_then(|p| p.online),
        "max": live.and_then(|p| p.max),
        "pingedAt": ping.as_ref().map(|p| p.at),
        "endsAt": ad.ends,
        "weight": ad.weight,
        "erid": ad.erid,
    }))
}

/// One line about an ad for the owner's terminal.
pub fn line(ad: &Ad, now: i64, zone: chrono::FixedOffset) -> String {
    format!(
        "{:>4}  {:<9}  {} → {}  w{:<3}  {}  {}",
        ad.id,
        crate::store::state(ad, now),
        local(ad.starts, zone),
        local(ad.ends, zone),
        ad.weight,
        ad.name,
        ad.address
    )
}

pub fn local(at: i64, zone: chrono::FixedOffset) -> String {
    chrono::DateTime::from_timestamp(at, 0)
        .map(|t| t.with_timezone(&zone).format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_else(|| at.to_string())
}
