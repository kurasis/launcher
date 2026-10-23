//! What people type and what clients send: room names and descriptions,
//! device names, IDs and tunnel keys. Control characters and the ones that
//! reorder text are taken out; lengths are bounded.

pub const NAME_MAX: usize = 64;
pub const DESCRIPTION_MAX: usize = 1000;
pub const DEVICE_NAME_MAX: usize = 64;

/// A game version a room can be made for, as the launcher's world versions:
/// a release from 1.7.6 on (`1.7.10`, `1.21`, `26.1`), a snapshot from 14w17a
/// (`24w14a`), a pre-release or release candidate of such a release
/// (`1.21-pre1`, `1.21-rc1`, `26.1-snapshot-2`, `26.1-pre-1`, `26.1-rc-1`), or
/// a version of the launcher's own list with a server (D-073: `24w14potato`,
/// `combat-8c`).
pub fn minecraft(text: &str) -> bool {
    release(text) || snapshot(text) || listed(text)
}

/// The versions of the launcher's own list a world can be made for, one per
/// line, as scripts/extra-versions.mjs writes them.
const LISTED: &str = include_str!("../../../data/extra-world-versions.txt");

fn listed(text: &str) -> bool {
    !text.is_empty() && LISTED.lines().any(|id| id == text)
}

fn release(text: &str) -> bool {
    let number = |part: &str| -> Option<u16> {
        let plain = (1..=3).contains(&part.len())
            && part.bytes().all(|b| b.is_ascii_digit())
            && (part == "0" || !part.starts_with('0'));
        plain.then(|| part.parse().ok()).flatten()
    };
    let parts: Option<Vec<u16>> = text.split('.').map(number).collect();
    match parts.as_deref() {
        Some(&[major, minor]) => (major == 1 && minor >= 8) || major >= 26,
        Some(&[major, minor, patch]) => {
            (major == 1 && (minor >= 8 || (minor == 7 && patch >= 6))) || major >= 26
        }
        _ => false,
    }
}

fn snapshot(text: &str) -> bool {
    if let Some((year, rest)) = text.split_once('w') {
        let bytes = rest.as_bytes();
        let weekly = year.len() == 2
            && year.bytes().all(|b| b.is_ascii_digit())
            && bytes.len() == 3
            && bytes[..2].iter().all(u8::is_ascii_digit)
            && bytes[2].is_ascii_lowercase();
        return weekly
            && match (year.parse::<u8>(), rest[..2].parse::<u8>()) {
                (Ok(year), Ok(week)) => (1..=53).contains(&week) && (year, week) >= (14, 17),
                _ => false,
            };
    }
    let Some((base, tail)) = text.split_once('-') else {
        return false;
    };
    let small = |n: &str| {
        (1..=2).contains(&n.len()) && n.bytes().all(|b| b.is_ascii_digit()) && !n.starts_with('0')
    };
    let numbered = ["snapshot-", "pre-", "rc-", "pre", "rc"]
        .iter()
        .find_map(|prefix| tail.strip_prefix(prefix))
        .is_some_and(small);
    numbered && release(base)
}

fn reorders(c: char) -> bool {
    matches!(c, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{200E}' | '\u{200F}')
}

/// A line of text: no line breaks, 1–`max` characters.
pub fn line(text: &str, max: usize) -> Option<String> {
    let cleaned: String = text
        .chars()
        .map(|c| if c.is_whitespace() { ' ' } else { c })
        .filter(|c| !c.is_control() && !reorders(*c))
        .collect();
    let trimmed = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    (!trimmed.is_empty() && trimmed.chars().count() <= max).then_some(trimmed)
}

/// A description: text with line breaks (at most two empty lines in a row),
/// 0–1000 characters after trimming.
pub fn description(text: &str) -> Option<String> {
    let mut out = String::with_capacity(text.len());
    let mut newlines = 0;
    for c in text.chars() {
        if c == '\n' {
            newlines += 1;
            if newlines <= 3 {
                out.push('\n');
            }
            continue;
        }
        if c.is_control() || reorders(c) {
            continue;
        }
        newlines = 0;
        out.push(c);
    }
    let trimmed = out.trim();
    (trimmed.chars().count() <= DESCRIPTION_MAX).then(|| trimmed.to_owned())
}

/// A Minecraft player name as the account service has them: 3–16 of
/// `A-Z a-z 0-9 _`.
pub fn nick(text: &str) -> bool {
    (3..=16).contains(&text.len()) && text.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

/// A canonical (lowercase, hyphenated) UUID.
pub fn id(text: &str) -> bool {
    uuid::Uuid::parse_str(text).is_ok_and(|u| u.hyphenated().to_string() == text)
}

/// An iroh endpoint ID as it prints: 64 lowercase hex digits.
pub fn endpoint(text: &str) -> bool {
    text.len() == 64 && text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// A SHA-256 as the catalog writes it: 64 lowercase hex digits.
pub fn sha256(text: &str) -> bool {
    endpoint(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn releases_and_snapshots_are_game_versions() {
        for ok in [
            "1.7.6",
            "1.7.10",
            "1.8",
            "1.12.2",
            "1.20.1",
            "1.21.1",
            "26.1",
            "26.3.1",
            "14w17a",
            "24w14a",
            "1.21-pre1",
            "1.21.1-rc1",
            "26.1-snapshot-2",
            "26.1-pre-1",
            "26.1-rc-1",
        ] {
            assert!(minecraft(ok), "{ok}");
        }
        for bad in [
            "",
            "1.7",
            "1.7.5",
            "1.6.4",
            "b1.7.3",
            "14w02a",
            "24w54a",
            "20w14infinite ",
            "2.0-blue",
            "b1.7_01",
            "1.21-pre0",
            "1.21-pre",
            "1.7.2-pre1",
            "1.14 Pre-Release 1",
            " 1.21",
            "1.21 ",
            "1.21.1.1",
            "1..1",
            "1.021",
            "2.0",
            "25.4",
            "1.21\n",
            "1.21/x",
            "<script>",
        ] {
            assert!(!minecraft(bad), "{bad:?}");
        }
    }

    #[test]
    fn versions_of_the_launchers_list_with_a_server_are_game_versions() {
        let data: serde_json::Value = serde_json::from_slice(
            &std::fs::read(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../data/extra-versions.json"
            ))
            .unwrap(),
        )
        .unwrap();
        let from = chrono::DateTime::parse_from_rfc3339("2014-04-09T00:00:00+00:00").unwrap();
        let mut worlds = Vec::new();
        for v in data["versions"].as_array().unwrap() {
            let id = v["id"].as_str().unwrap();
            let time = chrono::DateTime::parse_from_rfc3339(v["releaseTime"].as_str().unwrap());
            if v["server"] == true && time.unwrap() >= from {
                worlds.push(id);
                assert!(minecraft(id), "{id}");
            } else {
                assert!(!minecraft(id), "{id}");
            }
        }
        assert_eq!(LISTED.lines().collect::<Vec<_>>(), worlds);
        for id in [
            "24w14potato",
            "3D Shareware v1.34",
            "combat-8c",
            "1.21.11_unobfuscated",
        ] {
            assert!(minecraft(id), "{id}");
        }
        for bad in [
            "24W14POTATO",
            "combat-8",
            "24w14potato\n",
            "\n",
            "combat-8c\ncombat-8b",
        ] {
            assert!(!minecraft(bad), "{bad:?}");
        }
    }

    #[test]
    fn names_and_descriptions_lose_tricks() {
        assert_eq!(line("  Our\tworld\n ", NAME_MAX), Some("Our world".into()));
        assert_eq!(line("\u{202E}", NAME_MAX), None);
        assert_eq!(line("a\u{0}b\u{1b}[31m", NAME_MAX), Some("ab[31m".into()));
        assert!(line(&"я".repeat(64), NAME_MAX).is_some());
        assert!(line(&"я".repeat(65), NAME_MAX).is_none());
        assert_eq!(description(""), Some(String::new()));
        assert_eq!(description("a\n\n\n\n\nb\u{7}"), Some("a\n\n\nb".into()));
        assert!(description(&"x".repeat(1001)).is_none());
        assert_eq!(
            description("<script>alert(1)</script>"),
            Some("<script>alert(1)</script>".into())
        );
    }

    #[test]
    fn ids_nicks_and_endpoints() {
        assert!(nick("Steve_2"));
        assert!(!nick("St"));
        assert!(!nick("Ст_ив"));
        assert!(!nick("a' OR 1=1"));
        assert!(id("00000000-0000-4000-8000-000000000001"));
        assert!(!id("00000000-0000-4000-8000-00000000000A"));
        assert!(!id("../../etc/passwd"));
        assert!(endpoint(&"ab".repeat(32)));
        assert!(!endpoint(&"AB".repeat(32)));
        assert!(!endpoint(&"ab".repeat(33)));
    }
}
