//! What people type and what clients send: room names and descriptions,
//! device names, IDs and tunnel keys. Control characters and the ones that
//! reorder text are taken out; lengths are bounded.

pub const NAME_MAX: usize = 64;
pub const DESCRIPTION_MAX: usize = 1000;
pub const DEVICE_NAME_MAX: usize = 64;

/// A game version a room can be made for, as the launcher's world versions:
/// a release from 1.7.6 on (`1.7.10`, `1.21`, `26.1`), never a snapshot or a
/// pre-release.
pub fn minecraft(text: &str) -> bool {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn releases_from_1_7_6_are_game_versions() {
        for ok in [
            "1.7.6", "1.7.10", "1.8", "1.12.2", "1.20.1", "1.21.1", "26.1", "26.3.1",
        ] {
            assert!(minecraft(ok), "{ok}");
        }
        for bad in [
            "",
            "1.7",
            "1.7.5",
            "1.6.4",
            "b1.7.3",
            "24w14a",
            "1.21-pre1",
            "26.1-snapshot-2",
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
