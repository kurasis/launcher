//! Checks of everything an ad is made of. The owner types these on the
//! server, and players' launchers show them, so each one is bounded and
//! cleaned the same way here and again in the launcher.

/// Names are one line of at most this many characters.
pub const MAX_NAME: usize = 40;
/// Ad texts are at most this many characters, with at most two line breaks.
pub const MAX_TEXT: usize = 200;
pub const MAX_LINES: usize = 3;
/// Languages a text can be given in; the first is the fallback.
pub const LANGS: [&str; 3] = ["en", "ru", "ar"];
/// Where an ad can be shown.
pub const PLACES: [&str; 1] = ["feed"];
/// The shortest and longest an ad may run, in seconds.
pub const MIN_RUN: i64 = 60;
pub const MAX_RUN: i64 = 5 * 365 * 86_400;
pub const MAX_WEIGHT: i64 = 100;
/// An erid (Russian ad register token) is letters and digits.
pub const MAX_ERID: usize = 64;

/// Characters that change how text around them is laid out or hide it.
fn invisible(c: char) -> bool {
    matches!(c,
        '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2060}'..='\u{2069}'
        | '\u{FEFF}' | '\u{FFF9}'..='\u{FFFB}' | '\u{061C}' | '\u{180E}')
}

/// One line: no control or layout characters, spaces collapsed.
pub fn name(text: &str) -> Result<String, String> {
    if text.chars().any(|c| c.is_control() || invisible(c)) {
        return Err("the name has control or invisible characters".to_owned());
    }
    let clean = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let count = clean.chars().count();
    if count == 0 || count > MAX_NAME {
        return Err(format!("the name must be 1 to {MAX_NAME} characters"));
    }
    Ok(clean)
}

/// Plain text: line breaks only, no links, bounded.
pub fn ad_text(text: &str) -> Result<String, String> {
    let text = text.replace("\r\n", "\n");
    if text
        .chars()
        .any(|c| (c.is_control() && c != '\n') || invisible(c))
    {
        return Err("the text has control or invisible characters".to_owned());
    }
    let lines: Vec<String> = text
        .trim()
        .split('\n')
        .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
        .collect();
    if lines.len() > MAX_LINES {
        return Err(format!("the text has more than {MAX_LINES} lines"));
    }
    let clean = lines.join("\n");
    let count = clean.chars().count();
    if count == 0 || count > MAX_TEXT {
        return Err(format!("the text must be 1 to {MAX_TEXT} characters"));
    }
    let lower = clean.to_lowercase();
    if lower.contains("://") || lower.contains("www.") {
        return Err("the text must not hold links; the address is shown apart".to_owned());
    }
    Ok(clean)
}

/// `play.example.com`, `play.example.com:25566`, `192.0.2.7` or
/// `[2001:db8::1]:25565`, as the launcher reads them (src-tauri
/// launch/server.rs). Kept as typed, lowercased, without the default port.
pub fn address(text: &str) -> Result<String, String> {
    let (host, port) =
        parse_address(text).ok_or_else(|| "that is not a server address".to_owned())?;
    Ok(if port == 25565 {
        host
    } else {
        format!("{host}:{port}")
    })
}

pub fn parse_address(text: &str) -> Option<(String, u16)> {
    let text = text.trim();
    if text.is_empty() || text.len() > 270 {
        return None;
    }
    let (host, port) = if let Some(rest) = text.strip_prefix('[') {
        let (inner, after) = rest.split_once(']')?;
        let port = match after {
            "" => 25565,
            _ => after.strip_prefix(':')?.parse().ok()?,
        };
        inner.parse::<std::net::Ipv6Addr>().ok()?;
        (format!("[{inner}]"), port)
    } else {
        let (host, port) = match text.rsplit_once(':') {
            Some((host, port)) => (host, port.parse().ok()?),
            None => (text, 25565),
        };
        if !valid_host(host) {
            return None;
        }
        (host.trim_end_matches('.').to_ascii_lowercase(), port)
    };
    (port != 0).then_some((host, port))
}

/// A DNS name (letters, digits, hyphens, dots; labels of 1 to 63) or an IPv4
/// address.
pub fn valid_host(host: &str) -> bool {
    if host.parse::<std::net::Ipv4Addr>().is_ok() {
        return true;
    }
    let host = host.strip_suffix('.').unwrap_or(host);
    !host.is_empty()
        && host.len() <= 253
        && host.split('.').all(|label| {
            (1..=63).contains(&label.len())
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        })
        // A name ends in letters; all digits would be a bad IPv4 address.
        && host
            .rsplit('.')
            .next()
            .is_some_and(|tld| tld.bytes().any(|b| b.is_ascii_alphabetic()))
}

/// A Minecraft version ID as Mojang's manifest has them: `1.21.4`, `26.3`.
pub fn version_id(id: &str) -> bool {
    (1..=32).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        && id.bytes().next().is_some_and(|b| b.is_ascii_alphanumeric())
}

pub fn erid(text: &str) -> Result<String, String> {
    let text = text.trim();
    if text.is_empty() || text.len() > MAX_ERID || !text.bytes().all(|b| b.is_ascii_alphanumeric())
    {
        return Err(format!("an erid is 1 to {MAX_ERID} letters and digits"));
    }
    Ok(text.to_owned())
}

pub fn weight(text: &str) -> Result<i64, String> {
    match text.trim().parse::<i64>() {
        Ok(w) if (1..=MAX_WEIGHT).contains(&w) => Ok(w),
        _ => Err(format!(
            "the weight is a whole number from 1 to {MAX_WEIGHT}"
        )),
    }
}

pub fn place(text: &str) -> Result<&'static str, String> {
    PLACES
        .iter()
        .find(|p| **p == text.trim())
        .copied()
        .ok_or_else(|| format!("places are: {}", PLACES.join(", ")))
}

/// `ru`, `ru,en` (the languages whose players see an ad, in `LANGS`
/// order) or `all` (everyone: an empty list).
pub fn langs(text: &str) -> Result<Vec<String>, String> {
    let text = text.trim().to_ascii_lowercase();
    if text == "all" {
        return Ok(Vec::new());
    }
    let mut given = Vec::new();
    for lang in text.split(',').map(str::trim) {
        if !LANGS.contains(&lang) {
            return Err(format!(
                "--langs takes all or some of: {}",
                LANGS.join(", ")
            ));
        }
        given.push(lang);
    }
    Ok(LANGS
        .iter()
        .filter(|l| given.contains(l))
        .map(|l| (*l).to_owned())
        .collect())
}

/// `90m`, `12h`, `7d`, `2w`, `3mo`, `1y` in seconds, from a minute to five
/// years.
pub fn duration(text: &str) -> Result<i64, String> {
    let text = text.trim().to_ascii_lowercase();
    let split = text
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(text.len());
    let (number, unit) = text.split_at(split);
    let number: i64 = number
        .parse()
        .map_err(|_| "a duration is a number and a unit: 90m, 12h, 7d, 2w, 3mo, 1y".to_owned())?;
    let unit = match unit {
        "m" | "min" => 60,
        "h" => 3600,
        "d" => 86_400,
        "w" => 7 * 86_400,
        "mo" => 30 * 86_400,
        "y" => 365 * 86_400,
        _ => return Err("units are m, h, d, w, mo, y".to_owned()),
    };
    let seconds = number
        .checked_mul(unit)
        .ok_or_else(|| "that duration is too long".to_owned())?;
    if !(MIN_RUN..=MAX_RUN).contains(&seconds) {
        return Err("an ad runs from 1 minute to 5 years".to_owned());
    }
    Ok(seconds)
}

/// `+03:00` and the like.
pub fn offset(text: &str) -> Result<chrono::FixedOffset, String> {
    let bad = || format!("{text:?} is not an offset such as +03:00");
    let (sign, rest) = match text.trim().split_at_checked(1) {
        Some(("+", rest)) => (1, rest),
        Some(("-", rest)) => (-1, rest),
        _ => return Err(bad()),
    };
    let (h, m) = rest.split_once(':').ok_or_else(bad)?;
    let (h, m): (i32, i32) = (h.parse().map_err(|_| bad())?, m.parse().map_err(|_| bad())?);
    if !(0..=14).contains(&h) || !(0..60).contains(&m) {
        return Err(bad());
    }
    chrono::FixedOffset::east_opt(sign * (h * 3600 + m * 60)).ok_or_else(bad)
}

/// `2026-10-21 14:00` in `zone`, or RFC 3339 with its own offset, or `now`;
/// as Unix seconds.
pub fn time(text: &str, zone: chrono::FixedOffset, now: i64) -> Result<i64, String> {
    let text = text.trim();
    if text == "now" {
        return Ok(now);
    }
    if let Ok(t) = chrono::DateTime::parse_from_rfc3339(text) {
        return Ok(t.timestamp());
    }
    for format in ["%Y-%m-%d %H:%M", "%Y-%m-%dT%H:%M", "%Y-%m-%d %H:%M:%S"] {
        if let Ok(t) = chrono::NaiveDateTime::parse_from_str(text, format) {
            return t
                .and_local_timezone(zone)
                .single()
                .map(|t| t.timestamp())
                .ok_or_else(|| "that time does not exist".to_owned());
        }
    }
    Err(format!(
        "{text:?} is not a time such as \"2026-10-21 14:00\" (zone {zone})"
    ))
}
