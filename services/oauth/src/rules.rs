//! What a nick, an email address and a password must be (D-064).
//!
//! Everything a person types is checked here before it is stored or compared;
//! the rest of the service only sees values that passed.

use std::collections::HashSet;
use std::sync::OnceLock;

/// Nick length, as in Minecraft.
pub const NICK_MIN: usize = 3;
pub const NICK_MAX: usize = 16;
pub const PASSWORD_MIN: usize = 10;
/// Longer passwords only cost hashing time.
pub const PASSWORD_MAX: usize = 128;
pub const EMAIL_MAX: usize = 254;

/// Why a value was refused; the pages say it in the player's language.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Problem {
    NickLength,
    NickCharacters,
    NickReserved,
    NickTaken,
    Email,
    EmailTaken,
    PasswordShort,
    PasswordLong,
    PasswordCommon,
    PasswordLikeNick,
    PasswordsDiffer,
}

/// Names nobody may take: they could pass for the service or its staff.
const RESERVED: &[&str] = &[
    "admin",
    "administrator",
    "root",
    "system",
    "support",
    "help",
    "moderator",
    "mod",
    "staff",
    "official",
    "launcher",
    "sferum",
    "lapi",
    "security",
    "null",
    "undefined",
    "server",
    "owner",
    "team",
];

/// A nick: 3–16 of `A-Z a-z 0-9 _`. ASCII only, so no two nicks look alike
/// through letters of other alphabets.
pub fn nick(text: &str) -> Result<String, Problem> {
    let nick = text.trim();
    if !(NICK_MIN..=NICK_MAX).contains(&nick.len()) {
        return Err(Problem::NickLength);
    }
    if !nick.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
        return Err(Problem::NickCharacters);
    }
    let key = nick_key(nick);
    if RESERVED
        .iter()
        .any(|r| key == *r || key.trim_matches('_') == *r)
    {
        return Err(Problem::NickReserved);
    }
    Ok(nick.to_owned())
}

/// Nicks are unique without regard to case.
pub fn nick_key(nick: &str) -> String {
    nick.to_ascii_lowercase()
}

/// A plain address `local@domain.tld`: no spaces, quotes, angle brackets or
/// control characters, so it can go into a mail header and a page as it is.
pub fn email(text: &str) -> Result<String, Problem> {
    let email = text.trim();
    if email.is_empty() || email.len() > EMAIL_MAX {
        return Err(Problem::Email);
    }
    let Some((local, domain)) = email.rsplit_once('@') else {
        return Err(Problem::Email);
    };
    let local_ok = !local.is_empty()
        && local.len() <= 64
        && !local.starts_with('.')
        && !local.ends_with('.')
        && !local.contains("..")
        && local
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-/=?^_`{|}~.".contains(&b));
    let labels: Vec<&str> = domain.split('.').collect();
    let domain_ok = labels.len() >= 2
        && labels.iter().all(|l| {
            !l.is_empty()
                && l.len() <= 63
                && !l.starts_with('-')
                && !l.ends_with('-')
                && l.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        })
        && labels
            .last()
            .is_some_and(|tld| tld.len() >= 2 && tld.bytes().all(|b| b.is_ascii_alphabetic()));
    if local_ok && domain_ok {
        Ok(format!("{local}@{}", domain.to_ascii_lowercase()))
    } else {
        Err(Problem::Email)
    }
}

/// Addresses are unique without regard to case.
pub fn email_key(email: &str) -> String {
    email.to_ascii_lowercase()
}

fn common() -> &'static HashSet<&'static str> {
    static COMMON: OnceLock<HashSet<&'static str>> = OnceLock::new();
    // The 20 000 most used passwords of 10 characters or more, in lower case
    // (SecLists, MIT licence).
    COMMON.get_or_init(|| {
        include_str!("../data/common-passwords.txt")
            .lines()
            .collect()
    })
}

/// A password: 10–128 characters, not one of the most used, not the nick or
/// the address, not one character repeated.
pub fn password(text: &str, nick: &str, email: &str) -> Result<(), Problem> {
    let chars = text.chars().count();
    if chars < PASSWORD_MIN {
        return Err(Problem::PasswordShort);
    }
    if chars > PASSWORD_MAX {
        return Err(Problem::PasswordLong);
    }
    let lower = text.to_lowercase();
    if common().contains(lower.as_str()) || lower.chars().all(|c| lower.starts_with(c)) {
        return Err(Problem::PasswordCommon);
    }
    let local = email.split('@').next().unwrap_or_default().to_lowercase();
    let nick = nick.to_lowercase();
    if (!nick.is_empty() && lower.contains(&nick)) || (local.len() >= 4 && lower.contains(&local)) {
        return Err(Problem::PasswordLikeNick);
    }
    Ok(())
}

/// A two-letter country code from Cloudflare (`XX` unknown, `T1` Tor).
pub fn country(text: &str) -> String {
    let code = text.trim();
    if code.len() == 2 && code.bytes().all(|b| b.is_ascii_alphanumeric()) {
        code.to_ascii_uppercase()
    } else {
        "XX".to_owned()
    }
}

/// What the launcher says it runs on, for the sessions list.
pub fn device(text: &str) -> String {
    text.chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, ' ' | '.' | '-' | '_' | '(' | ')'))
        .take(40)
        .collect::<String>()
        .trim()
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nicks_are_short_ascii_and_not_reserved() {
        assert_eq!(nick(" Steve_2 "), Ok("Steve_2".to_owned()));
        assert_eq!(nick("ab"), Err(Problem::NickLength));
        assert_eq!(nick("a_very_long_nick_x"), Err(Problem::NickLength));
        for bad in [
            "Stеve", // Cyrillic е
            "st eve",
            "<script>",
            "a'; DROP--",
            "nick\u{0}",
            "ник_игрока",
            "x\u{202E}yz",
        ] {
            assert!(nick(bad).is_err(), "{bad:?}");
        }
        assert_eq!(nick("Admin"), Err(Problem::NickReserved));
        assert_eq!(nick("_support_"), Err(Problem::NickReserved));
        assert_eq!(nick_key("StEvE"), "steve");
    }

    #[test]
    fn addresses_are_plain() {
        assert_eq!(
            email("Player.One+mc@Example.COM"),
            Ok("Player.One+mc@example.com".to_owned())
        );
        for bad in [
            "",
            "no-at.example.com",
            "a@b",
            "a@-b.com",
            "a b@c.com",
            "a@b.c",
            "\"quoted\"@x.com",
            "a@x.com\r\nBcc: victim@y.com",
            "<a@x.com>",
            "a@x..com",
            ".a@x.com",
            "a@xn--p1ai.123",
            "a@exa_mple.com",
        ] {
            assert!(email(bad).is_err(), "{bad:?}");
        }
        assert!(email(&format!("{}@x.com", "a".repeat(65))).is_err());
    }

    #[test]
    fn passwords_are_long_and_not_common() {
        assert!(password("correct horse battery", "Steve", "s@x.com").is_ok());
        assert_eq!(
            password("short", "Steve", "s@x.com"),
            Err(Problem::PasswordShort)
        );
        assert_eq!(
            password(&"x".repeat(129), "Steve", "s@x.com"),
            Err(Problem::PasswordLong)
        );
        assert_eq!(
            password("QwertyUiop", "Steve", "s@x.com"),
            Err(Problem::PasswordCommon)
        );
        assert_eq!(
            password("aaaaaaaaaaaa", "Steve", "s@x.com"),
            Err(Problem::PasswordCommon)
        );
        assert_eq!(
            password("my-steve-password", "Steve", "s@x.com"),
            Err(Problem::PasswordLikeNick)
        );
        assert_eq!(
            password("player1234!!xyz", "Steve", "player1234@x.com"),
            Err(Problem::PasswordLikeNick)
        );
        // Any characters count, measured in characters, not bytes.
        assert!(password("пароль-для-игры", "Steve", "s@x.com").is_ok());
    }

    #[test]
    fn countries_and_devices_are_cleaned() {
        assert_eq!(country("ru"), "RU");
        assert_eq!(country("<x>"), "XX");
        assert_eq!(country(""), "XX");
        assert_eq!(device("Windows 11 (x64)"), "Windows 11 (x64)");
        assert_eq!(
            device("<img src=x onerror=alert(1)>"),
            "img srcx onerroralert(1)"
        );
        assert_eq!(device(&"a".repeat(100)).len(), 40);
    }
}
