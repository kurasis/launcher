//! What people type: messages, room names and report reasons. Control
//! characters and the ones that reorder text are taken out, so a message
//! cannot hide or flip what is around it; lengths are bounded.

pub const MESSAGE_MAX: usize = 2000;
pub const NAME_MAX: usize = 40;
pub const REASON_MAX: usize = 500;

fn reorders(c: char) -> bool {
    matches!(c, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{200E}' | '\u{200F}')
}

/// A message: text with line breaks (at most two empty lines in a row),
/// 1–2000 characters after trimming.
pub fn message(text: &str) -> Option<String> {
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
    (!trimmed.is_empty() && trimmed.chars().count() <= MESSAGE_MAX).then(|| trimmed.to_owned())
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

/// A nick as the account service has them: 3–16 of `A-Z a-z 0-9 _`.
pub fn nick(text: &str) -> Option<&str> {
    let nick = text.trim();
    ((3..=16).contains(&nick.len()) && nick.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_'))
        .then_some(nick)
}

pub fn account_id(text: &str) -> bool {
    uuid::Uuid::parse_str(text).is_ok_and(|u| u.hyphenated().to_string() == text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_keep_text_and_lose_tricks() {
        assert_eq!(message("  hi\n there  "), Some("hi\n there".into()));
        assert_eq!(message("a\u{0}b\u{7}c\u{1b}[31md"), Some("abc[31md".into()));
        assert_eq!(message("x\u{202E}gnp.exe"), Some("xgnp.exe".into()));
        assert_eq!(message("a\n\n\n\n\n\nb"), Some("a\n\n\nb".into()));
        assert_eq!(message("   \n\t "), None);
        assert_eq!(message(""), None);
        assert!(message(&"я".repeat(2000)).is_some());
        assert!(message(&"я".repeat(2001)).is_none());
        // Markup is text: the launcher shows it as text.
        assert_eq!(
            message("<script>alert(1)</script>"),
            Some("<script>alert(1)</script>".into())
        );
    }

    #[test]
    fn names_nicks_and_ids() {
        assert_eq!(line("  Our\tgroup\n ", NAME_MAX), Some("Our group".into()));
        assert_eq!(line("\u{202E}", NAME_MAX), None);
        assert!(line(&"x".repeat(41), NAME_MAX).is_none());
        assert_eq!(nick(" Steve_2 "), Some("Steve_2"));
        assert_eq!(nick("Ст"), None);
        assert_eq!(nick("a' OR 1=1"), None);
        assert!(account_id("00000000-0000-4000-8000-000000000001"));
        assert!(!account_id("00000000-0000-4000-8000-00000000000Z"));
        assert!(!account_id("../../etc"));
        assert!(!account_id("00000000000040008000000000000001"));
    }
}
